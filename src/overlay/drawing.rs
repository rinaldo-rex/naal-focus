use super::App;
use crate::{
    process::boot_time,
    render,
    time::{system_ms, to_local},
};
use smithay_client_toolkit::shell::WaylandSurface;
use std::{error::Error, time::Duration};
use wayland_client::protocol::wl_shm;

impl App {
    pub(super) fn repaint(&mut self, now: Duration) -> Result<(), Box<dyn Error>> {
        if self
            .focus_snapshot
            .as_mut()
            .is_some_and(|snapshot| snapshot.day.advance_day(system_ms()))
        {
            self.chart_revision += 1;
            self.dirty = true;
        }
        if self.charts.kinds().next().is_some()
            && self.activity.recall.charts.advance(to_local(system_ms()))
        {
            self.chart_revision += 1;
            self.dirty = true;
        }
        let entrance = self.entrance_frame(now);
        if !entrance.active && self.entrance_frame(self.last_paint).active {
            self.dirty = true;
        }
        let interval = if entrance.active {
            Duration::from_millis(33)
        } else if self.activity.recall.animating(now)
            || self.activity.recall.animating(self.last_paint)
        {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(250)
        };
        if !self.dirty
            && ((!entrance.active && !self.reflections_enabled)
                || !self.activity.field_visible()
                || now.saturating_sub(self.last_paint) < interval)
        {
            return Ok(());
        }
        for index in 0..self.active.len() {
            if self.active[index].mapped {
                self.draw(index, now)?;
            }
        }
        self.last_paint = now;
        self.dirty =
            !entrance.active && self.active.iter().any(|a| a.entrance_dirty.contains(&true));
        Ok(())
    }
    pub(super) fn draw(&mut self, index: usize, now: Duration) -> Result<(), Box<dyn Error>> {
        let active = &self.active[index];
        let scale = active.scale.clamp(1, 4) as u32;
        let width = active.size.0.checked_mul(scale).ok_or("Width overflow")?;
        let height = active.size.1.checked_mul(scale).ok_or("Height overflow")?;
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 32_000_000 {
            return Err(format!("Unsupported size {width}x{height}").into());
        }
        let total: u64 = self
            .active
            .iter()
            .map(|a| {
                u64::from(a.size.0) * u64::from(a.size.1) * (a.scale.clamp(1, 4) as u64).pow(2)
            })
            .sum();
        // Two reusable buffers plus a temporary reveal cache per output. Keep
        // the same 512 MB aggregate pixel-storage ceiling as the old two-buffer path.
        let copies = if self.entrance.settings.enabled { 3 } else { 2 };
        if total.saturating_mul(copies) > 128_000_000 {
            return Err("Combined monitor buffers exceed the safety budget.".into());
        }
        if self.active[index].buffer_size != (width, height, scale) {
            self.active[index].buffers.clear();
            self.active[index].chart_versions.clear();
            self.active[index].entrance_dirty.clear();
            self.active[index].entrance_backdrops.clear();
            self.active[index].reveal_cache = None;
            self.active[index].buffer_size = (width, height, scale);
        }
        let entrance = self.entrance_frame(now);
        let active = &self.active[index];
        let available = active
            .buffers
            .iter()
            .enumerate()
            .filter(|(_, buffer)| self.pool.canvas(*buffer).is_some())
            // Restore both caches before returning to ordinary input updates.
            .min_by_key(|(i, _)| entrance.active || !active.entrance_dirty[*i])
            .map(|(i, _)| i);
        let (buffer_index, fresh) = if let Some(index) = available {
            (index, false)
        } else if self.active[index].buffers.len() < 2 {
            let (buffer, _) = self.pool.create_buffer(
                width as i32,
                height as i32,
                width as i32 * 4,
                wl_shm::Format::Argb8888,
            )?;
            self.active[index].buffers.push(buffer);
            self.active[index].chart_versions.push(0);
            self.active[index].entrance_dirty.push(false);
            self.active[index].entrance_backdrops.push([0; 4]);
            (self.active[index].buffers.len() - 1, true)
        } else {
            // Never allocate an unbounded queue of frames if the compositor is
            // slow or stops releasing buffers during a long writing session.
            return Ok(());
        };
        let restore = self.active[index].entrance_dirty[buffer_index];
        let full = fresh || restore;
        let charts_changed =
            full || self.active[index].chart_versions[buffer_index] != self.chart_revision;
        let background = render::breathing_background(entrance);
        let full_entrance =
            fresh || self.active[index].entrance_backdrops[buffer_index] != background;
        if !entrance.active {
            self.active[index].reveal_cache = None;
        }
        let content = render::focus_content(
            width as f32,
            height as f32,
            scale as f32,
            &self.focus_settings,
        );
        let mut reveal_cache = None;
        let buffer = &self.active[index].buffers[buffer_index];
        let canvas = self.pool.canvas(buffer).ok_or("Buffer is still in use")?;
        if entrance.active && entrance.reflection == 0.0 {
            render::render_breathing(
                canvas,
                (width as usize, height as usize),
                self.font.as_ref().ok_or("Font not loaded")?,
                scale as f32,
                render::BreathingView {
                    pattern: &self.entrance.pattern,
                    frame: entrance,
                    full: full_entrance,
                },
            );
        } else if entrance.active && self.active[index].reveal_cache.is_some() {
            render::render_reveal(
                canvas,
                self.active[index].reveal_cache.as_ref().unwrap(),
                entrance.reflection,
            );
        } else {
            if full {
                render::render_overlay(
                    canvas,
                    (width as usize, height as usize),
                    self.font.as_ref().ok_or("Font not loaded")?,
                    scale as f32,
                    render::OverlayView {
                        quote: &self.quote,
                        text_size: self.text_size as f32 * scale as f32,
                        reflections_enabled: self.reflections_enabled,
                        charts: &self.charts,
                        focus: &self.focus_settings,
                    },
                );
            }
            if charts_changed {
                render::render_charts_in(
                    canvas,
                    (width as usize, height as usize),
                    self.font.as_ref().ok_or("Font not loaded")?,
                    scale as f32,
                    content,
                    render::ChartView {
                        settings: &self.charts,
                        data: self
                            .pending_history
                            .is_none()
                            .then_some(&self.activity.recall.charts),
                        unavailable: self.history_unavailable,
                    },
                );
                render::render_focus(
                    canvas,
                    (width as usize, height as usize),
                    self.font.as_ref().ok_or("Font not loaded")?,
                    scale as f32,
                    &self.focus_settings,
                    self.focus_snapshot.as_ref(),
                );
            }
            if self.reflections_enabled {
                let saving = self
                    .save
                    .as_ref()
                    .is_some_and(|job| job.generation == self.activity.generation());
                render::render_input_in(
                    canvas,
                    (width as usize, height as usize),
                    self.font.as_ref().ok_or("Font not loaded")?,
                    scale as f32,
                    content,
                    render::InputView {
                        editor: &self.activity.editor,
                        suggestions: &self.activity.suggestions(),
                        suggestion_index: self.activity.suggestion_index(),
                        saved_text: self.activity.displayed_text(),
                        progress: self.activity.typing_fraction(now),
                        message: if saving {
                            Some("Saving…")
                        } else if let Some(error) = self.activity.error.as_deref() {
                            Some(error)
                        } else if self.history_unavailable && !self.activity.editing() {
                            Some("Previous reflections unavailable · type to reflect")
                        } else {
                            None
                        },
                        editing: self.activity.editing(),
                        caret_visible: self.activity.recall.selected().is_none()
                            && (now.as_millis() / 500).is_multiple_of(2),
                        history: Some(render::HistoryView {
                            recall: &self.activity.recall,
                            now,
                            now_ms: system_ms(),
                        }),
                    },
                );
            }
            if entrance.active {
                let cache = canvas.to_vec();
                render::render_reveal(canvas, &cache, entrance.reflection);
                reveal_cache = Some(cache);
            }
        }
        // Seed the second buffer now, not on the user's first keystroke. This
        // avoids another full-screen/font rasterization in the typing path.
        let seed = if full && !entrance.active && self.active[index].buffers.len() == 1 {
            Some(canvas.to_vec())
        } else {
            None
        };
        let active = &self.active[index];
        let surface = active.layer.wl_surface();
        surface.set_buffer_scale(scale as i32);
        if (entrance.active && (full_entrance || entrance.reflection > 0.0))
            || (!entrance.active && full)
        {
            surface.damage_buffer(0, 0, width as i32, height as i32);
        } else if entrance.active {
            let damage = render::breathing_damage(width as f32, height as f32, scale as f32);
            surface.damage_buffer(
                damage.x.floor() as i32,
                damage.y.floor() as i32,
                damage.width.ceil() as i32 + 1,
                damage.height.ceil() as i32 + 1,
            );
        } else {
            let damage = render::input_damage(content.width, content.height, scale as f32);
            surface.damage_buffer(
                content.x.floor() as i32,
                damage.y.floor() as i32,
                content.width.ceil() as i32,
                damage.height.ceil() as i32,
            );
            if charts_changed
                && let Some(band) = render::chart_damage_in(content, scale as f32, &self.charts)
            {
                surface.damage_buffer(
                    band.x.floor() as i32,
                    band.y.floor() as i32,
                    band.width.ceil() as i32 + 1,
                    band.height.ceil() as i32 + 1,
                );
            }
        }
        if !entrance.active
            && charts_changed
            && !full
            && let Some(sidebar) = render::focus_sidebar(
                width as f32,
                height as f32,
                scale as f32,
                &self.focus_settings,
            )
        {
            surface.damage_buffer(
                sidebar.x.floor() as i32,
                sidebar.y.floor() as i32,
                sidebar.width.ceil() as i32,
                sidebar.height.ceil() as i32,
            );
        }
        buffer.attach_to(surface)?;
        active.layer.commit();
        let active = &mut self.active[index];
        active.chart_versions[buffer_index] = if entrance.active {
            0
        } else {
            self.chart_revision
        };
        if let Some(cache) = reveal_cache {
            active.reveal_cache = Some(cache);
        }
        active.entrance_dirty[buffer_index] = entrance.active;
        active.entrance_backdrops[buffer_index] = background;
        if !active.mapped {
            active.mapped = true;
            if self.mapped_at.is_none() {
                let mapped_now = boot_time();
                self.mapped_at = Some(mapped_now);
                // The normal countdown starts only after the complete entrance;
                // early keyboard/pointer events are separately gated above.
                self.activity
                    .mapped(mapped_now + self.entrance.settings.duration());
            }
            let name = self
                .outputs
                .info(&active.output)
                .and_then(|i| i.name)
                .unwrap_or_else(|| "(unnamed)".into());
            println!("Buffer committed: {name} · {width}x{height} · scale {scale}");
        }
        if let Some(seed) = seed {
            let (buffer, canvas) = self.pool.create_buffer(
                width as i32,
                height as i32,
                width as i32 * 4,
                wl_shm::Format::Argb8888,
            )?;
            canvas.copy_from_slice(&seed);
            self.active[index].buffers.push(buffer);
            self.active[index].chart_versions.push(if entrance.active {
                0
            } else {
                self.chart_revision
            });
            self.active[index].entrance_dirty.push(entrance.active);
            self.active[index].entrance_backdrops.push(background);
        }
        Ok(())
    }
}
