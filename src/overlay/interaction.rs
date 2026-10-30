use super::{App, SaveJob};
use crate::{
    activity::Activity,
    activity_log::{Entry, PendingSave},
    process::boot_time,
    recall::PendingHistory,
    render,
    time::{system_ms, to_local},
};
use fontdue::Font;
use smithay_client_toolkit::seat::{
    keyboard::{KeyEvent, Keysym},
    pointer::PointerEventKind,
};
use std::time::Duration;
use wayland_client::protocol::wl_keyboard;

impl App {
    fn submit(&mut self, now: Duration) {
        // One in-flight submission: repeated Enter/monitor events cannot duplicate it.
        if self.save.is_some() || !self.reflection_input_ready(now) {
            return;
        }
        let Some((generation, text)) = self.activity.enter(now) else {
            return;
        };
        let candidate = if let Some(original) = &self.saved_entry {
            original.corrected(&text)
        } else if let Some((cached_generation, cached)) = &self.retry_entry
            && *cached_generation == generation
            && cached.text == text
        {
            Ok(cached.clone())
        } else {
            Entry::submitted(&text)
        };
        let entry = match candidate {
            Ok(entry) => entry,
            Err(error) => {
                self.activity.failed(generation, error);
                return;
            }
        };
        self.retry_entry = Some((generation, entry.clone()));
        let pending = if let Some(original) = &self.saved_entry {
            PendingSave::start_correction(original.clone(), entry.clone())
        } else {
            PendingSave::start(entry.clone())
        };
        match pending {
            Ok(result) => {
                self.save = Some(SaveJob {
                    generation,
                    entry,
                    result,
                })
            }
            Err(error) => self.activity.failed(generation, error),
        }
    }
    pub(super) fn poll_history(&mut self) {
        let Some(result) = self.pending_history.as_ref().and_then(PendingHistory::poll) else {
            return;
        };
        self.pending_history = None;
        self.chart_revision += 1;
        match result {
            Ok(mut history) => {
                // A slow history load must retain vocabulary saved meanwhile.
                if let Some(entry) = &self.saved_entry {
                    history.vocabulary.include(entry.metadata());
                    history.charts.upsert(entry, system_ms(), to_local);
                }
                self.activity.recall = history;
            }
            Err(error) => {
                self.history_unavailable = true;
                eprintln!("Previous reflections unavailable: {error}");
            }
        }
        self.dirty = true;
    }
    pub(super) fn poll_save(&mut self, now: Duration) {
        let Some(job) = &mut self.save else {
            return;
        };
        let Some(result) = job.result.poll() else {
            return;
        };
        let generation = job.generation;
        let entry = job.entry.clone();
        self.save = None;
        match result {
            Ok(()) => {
                self.activity.saved(generation, &entry.text, now);
                self.activity.recall.vocabulary.include(entry.metadata());
                self.activity
                    .recall
                    .charts
                    .upsert(&entry, system_ms(), to_local);
                self.chart_revision += 1;
                self.saved_entry = Some(entry);
                self.retry_entry = None;
            }
            Err(error) => self.activity.failed(generation, error),
        }
        self.dirty = true;
    }
    pub(super) fn key(
        &mut self,
        keyboard: &wl_keyboard::WlKeyboard,
        event: KeyEvent,
        repeated: bool,
    ) {
        let now = boot_time();
        if self.mapped_at.is_none() || self.reason.is_some() || self.activity.expired(now).is_some()
        {
            return;
        }
        if !self.reflection_input_ready(now) {
            if event.keysym == Keysym::Escape && !repeated {
                self.reason = Some("escape");
            }
            return;
        }
        if event.keysym == Keysym::Escape {
            if !repeated && !self.activity.dismiss_suggestions() && self.activity.cancel(now) {
                self.reason = Some("escape");
            }
            self.dirty = true;
            return;
        }
        if self
            .save
            .as_ref()
            .is_some_and(|job| job.generation == self.activity.generation())
        {
            return;
        }
        let modifiers = self.modifiers.get(keyboard).copied().unwrap_or_default();
        if matches!(event.keysym, Keysym::Return | Keysym::KP_Enter) {
            if !repeated {
                self.submit(now);
            }
        } else if !self.activity.field_visible() {
            return;
        } else if modifiers.ctrl && matches!(event.keysym, Keysym::a | Keysym::A) {
            self.activity.leave_history(now);
            self.activity.reset_completion();
            self.activity.editor.select_all();
        } else if !modifiers.ctrl && !modifiers.alt && !modifiers.logo {
            match event.keysym {
                Keysym::Up if !modifiers.shift => {
                    if !self.activity.move_suggestion(false) {
                        self.activity.browse_older(now);
                    }
                }
                Keysym::Down if !modifiers.shift => {
                    if !self.activity.move_suggestion(true) {
                        self.activity.browse_newer(now);
                    }
                }
                Keysym::Tab if !modifiers.shift => {
                    if !repeated {
                        self.activity.complete(now);
                    }
                }
                Keysym::BackSpace => self.activity.backspace(now),
                Keysym::Delete => self.activity.delete(now),
                Keysym::Left => {
                    self.activity.leave_history(now);
                    self.activity.reset_completion();
                    self.activity.editor.left(modifiers.shift);
                }
                Keysym::Right => {
                    self.activity.leave_history(now);
                    self.activity.reset_completion();
                    self.activity.editor.right(modifiers.shift);
                }
                Keysym::Home => {
                    self.activity.leave_history(now);
                    self.activity.reset_completion();
                    self.activity.editor.home(modifiers.shift);
                }
                Keysym::End => {
                    self.activity.leave_history(now);
                    self.activity.reset_completion();
                    self.activity.editor.end(modifiers.shift);
                }
                _ => {
                    if let Some(text) = &event.utf8 {
                        self.activity.insert(text, now);
                    }
                }
            }
        }
        self.dirty = true;
    }
}

/// Pointer input only edits the reflection field; it never cancels or dismisses.
pub(super) fn pointer_input(
    activity: &mut Activity,
    font: Option<&Font>,
    kind: &PointerEventKind,
    size: (u32, u32),
    scale: f32,
    position: (f64, f64),
    now: Duration,
) -> bool {
    if !matches!(kind, PointerEventKind::Press { .. }) {
        return false;
    }
    let field = render::input_rect(size.0 as f32 * scale, size.1 as f32 * scale, scale);
    if !field.contains(position.0 as f32 * scale, position.1 as f32 * scale) {
        return false;
    }
    let mut changed = activity.leave_history(now);
    changed |= activity.open_saved(now);
    if let Some(caret) =
        font.and_then(|font| render::input_caret(font, &activity.editor, size, scale, position))
    {
        activity.reset_completion();
        activity.editor.set_cursor(caret, false);
        changed = true;
    }
    changed
}
