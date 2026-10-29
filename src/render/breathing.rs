//! Small damaged-region geometry rendering; full surfaces change only during fades.
use super::{BACKGROUND, Image, Rect, premultiply};
use crate::breathing::{Frame, Orb, Pattern, Point};
use fontdue::Font;

fn layout(width: f32, height: f32, scale: f32) -> (Point, f32) {
    let radius = (220.0 * scale)
        .min(width / 4.0)
        .min(((height - 140.0 * scale) / 3.6).max(0.0));
    (
        Point(width / 2.0, (height - 80.0 * scale).max(0.0) / 2.0),
        radius,
    )
}
pub fn breathing_damage(width: f32, height: f32, scale: f32) -> Rect {
    let (center, radius) = layout(width, height, scale);
    let half = (radius * 1.8 + 4.0 * scale)
        .max(150.0 * scale)
        .min(width / 2.0);
    let top = (center.1 - radius * 1.8 - 4.0 * scale).max(0.0);
    Rect {
        x: center.0 - half,
        y: top,
        width: half * 2.0,
        height: height - top,
    }
}
pub fn breathing_background(frame: Frame) -> [u8; 4] {
    let mut color = BACKGROUND;
    color[3] = (f32::from(color[3]) * frame.backdrop).round() as u8;
    premultiply(color)
}
pub struct BreathingView<'a> {
    pub pattern: &'a Pattern,
    pub frame: Frame,
    pub full: bool,
}
pub fn render_breathing(
    canvas: &mut [u8],
    size: (usize, usize),
    font: &Font,
    scale: f32,
    view: BreathingView<'_>,
) {
    let (width, height) = size;
    let frame = view.frame;
    let bg = breathing_background(frame);
    if view.full {
        for pixel in canvas.chunks_exact_mut(4) {
            pixel.copy_from_slice(&bg);
        }
    }
    let damage = breathing_damage(width as f32, height as f32, scale);
    let mut image = Image {
        canvas,
        width,
        height,
        clip: damage,
    };
    if !view.full {
        let mut color = BACKGROUND;
        color[3] = (f32::from(color[3]) * frame.backdrop).round() as u8;
        image.rect(damage, color);
    }
    let (center, radius) = layout(width as f32, height as f32, scale);
    let transform = |p: Point| Point(center.0 + p.0 * radius, center.1 + p.1 * radius);
    let geometry = view.pattern.animated_geometry(frame.amount, frame.motion);
    for glow in geometry.glows {
        image.glow(
            transform(glow.center),
            glow.radius * radius,
            glow.color,
            glow.opacity * frame.pattern,
        );
    }
    for line in geometry.lines {
        image.line(
            transform(line.from),
            transform(line.to),
            scale * line.width,
            line.color.unwrap_or(view.pattern.color),
            line.opacity * frame.pattern,
        );
    }
    for orb in geometry.orbs {
        image.orb(
            transform(orb.center),
            orb.radius * radius,
            &orb,
            frame.pattern,
        );
    }
    for (point, radius, opacity) in geometry.dots {
        image.disc(
            transform(point),
            radius * scale,
            view.pattern.color,
            opacity * frame.pattern,
        );
    }
    let top = (height as f32 - 72.0 * scale).max(0.0);
    let text_size = (22.0 * scale).min(height as f32 / 6.0);
    let text_width = super::text_width(font, frame.cue, text_size);
    image.text_at_opacity(
        font,
        frame.cue,
        text_size,
        ((width as f32 - text_width) / 2.0, top),
        [224, 231, 224],
        frame.pattern,
    );
}
/// Fade cached reflection pixels from the same premultiplied backdrop. Never
/// fade an already-faded buffer: rounding must not accumulate across frames.
pub fn render_reveal(canvas: &mut [u8], cached: &[u8], opacity: f32) {
    let bg = premultiply(BACKGROUND);
    let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u16;
    for (pixel, source) in canvas.chunks_exact_mut(4).zip(cached.chunks_exact(4)) {
        for i in 0..4 {
            pixel[i] = ((u16::from(source[i]) * alpha + u16::from(bg[i]) * (255 - alpha) + 127)
                / 255) as u8;
        }
    }
}
fn source_over(destination: u32, source: u32) -> u32 {
    let inverse = 255 - (source >> 24);
    // Two separated byte channels fit in each product. This is exact floor / 255.
    let attenuate = |pair: u32| {
        let product = pair * inverse;
        ((product + 0x0001_0001 + ((product >> 8) & 0x00ff_00ff)) >> 8) & 0x00ff_00ff
    };
    source
        + attenuate(destination & 0x00ff_00ff)
        + (attenuate((destination >> 8) & 0x00ff_00ff) << 8)
}
impl Image<'_> {
    fn blend(&mut self, x: i32, y: i32, rgb: [u8; 3], opacity: f32) {
        if x < 0
            || y < 0
            || x >= self.width as i32
            || y >= self.height as i32
            || !self.clip.contains(x as f32, y as f32)
        {
            return;
        }
        let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u32;
        if alpha == 0 {
            return;
        }
        let offset = (y as usize * self.width + x as usize) * 4;
        let dest = &mut self.canvas[offset..offset + 4];
        let old = u32::from_ne_bytes(dest.try_into().unwrap());
        let mut color = (alpha + (old >> 24) * (255 - alpha) / 255) << 24;
        for (channel, shift) in rgb.into_iter().zip([16, 8, 0]) {
            color |= (u32::from(channel) * alpha / 255
                + ((old >> shift) & 255) * (255 - alpha) / 255)
                << shift;
        }
        dest.copy_from_slice(&color.to_ne_bytes());
    }
    fn glow(&mut self, center: Point, radius: f32, rgb: [u8; 3], opacity: f32) {
        if radius <= 0.0 || opacity <= 0.0 {
            return;
        }
        // Cache radial falloff and premultiplication; large 2× glows otherwise
        // repeat a square root and four channel conversions for every pixel.
        let samples: [u32; 1025] = std::array::from_fn(|i| {
            let distance = (i as f32 / 1024.0).sqrt();
            let falloff = if distance < 0.38 {
                1.0 - distance / 0.38 * 0.68
            } else {
                (1.0 - distance) / 0.62 * 0.32
            };
            let alpha = (opacity * falloff * 255.0).clamp(0.0, 255.0).round() as u32;
            (alpha << 24)
                | ((u32::from(rgb[0]) * alpha / 255) << 16)
                | ((u32::from(rgb[1]) * alpha / 255) << 8)
                | (u32::from(rgb[2]) * alpha / 255)
        });
        let left = (center.0 - radius).max(self.clip.x).max(0.0).ceil() as usize;
        let right = (center.0 + radius)
            .min(self.clip.x + self.clip.width)
            .min(self.width as f32)
            .ceil()
            .max(0.0) as usize;
        let top = (center.1 - radius).max(self.clip.y).max(0.0).ceil() as usize;
        let bottom = (center.1 + radius)
            .min(self.clip.y + self.clip.height)
            .min(self.height as f32)
            .ceil()
            .max(0.0) as usize;
        if left >= right || top >= bottom {
            return;
        }
        let factor = 1024.0 / (radius * radius);
        for y in top..bottom {
            let dy = y as f32 - center.1;
            let offset = (y * self.width + left) * 4;
            let row = &mut self.canvas[offset..offset + (right - left) * 4];
            for (column, pixel) in row.chunks_exact_mut(4).enumerate() {
                let dx = (left + column) as f32 - center.0;
                let index = ((dx * dx + dy * dy) * factor).round() as usize;
                let source = samples[index.min(1024)];
                if source != 0 {
                    let old = u32::from_ne_bytes(pixel.try_into().unwrap());
                    pixel.copy_from_slice(&source_over(old, source).to_ne_bytes());
                }
            }
        }
    }
    fn orb(&mut self, center: Point, radius: f32, orb: &Orb, opacity: f32) {
        if radius <= 0.0 || opacity <= 0.0 {
            return;
        }
        let stops = [
            (0.0, orb.palette[1], 0.94),
            (0.42, orb.palette[0], 0.65),
            (0.82, orb.palette[2], 0.22),
            (1.0, orb.palette[2], 0.03),
        ];
        for y in (center.1 - radius - 1.0).floor() as i32..=(center.1 + radius + 1.0).ceil() as i32
        {
            for x in
                (center.0 - radius - 1.0).floor() as i32..=(center.0 + radius + 1.0).ceil() as i32
            {
                let dx = x as f32 - center.0;
                let dy = y as f32 - center.1;
                let coverage = (radius + 0.5 - dx.hypot(dy)).clamp(0.0, 1.0);
                if coverage == 0.0 {
                    continue;
                }
                let distance = ((dx / radius + 0.33).hypot(dy / radius + 0.4) / 1.4).min(1.0);
                let pair = stops.windows(2).find(|pair| distance <= pair[1].0).unwrap();
                let amount = (distance - pair[0].0) / (pair[1].0 - pair[0].0);
                let color = std::array::from_fn(|i| {
                    (f32::from(pair[0].1[i]) * (1.0 - amount) + f32::from(pair[1].1[i]) * amount)
                        .round() as u8
                });
                let alpha = pair[0].2 * (1.0 - amount) + pair[1].2 * amount;
                self.blend(x, y, color, alpha * opacity * coverage);
            }
        }
    }
    fn disc(&mut self, center: Point, radius: f32, rgb: [u8; 3], opacity: f32) {
        let extent = radius + 1.0;
        for y in (center.1 - extent).floor() as i32..=(center.1 + extent).ceil() as i32 {
            for x in (center.0 - extent).floor() as i32..=(center.0 + extent).ceil() as i32 {
                let coverage = (radius + 0.5 - (x as f32 - center.0).hypot(y as f32 - center.1))
                    .clamp(0.0, 1.0);
                self.blend(x, y, rgb, opacity * coverage);
            }
        }
    }
    // Major-axis scan conversion keeps long diagonals O(length), not O(area).
    fn line(&mut self, mut a: Point, mut b: Point, width: f32, rgb: [u8; 3], opacity: f32) {
        if opacity <= 0.0 {
            return;
        }
        let steep = (b.1 - a.1).abs() > (b.0 - a.0).abs();
        if steep {
            a = Point(a.1, a.0);
            b = Point(b.1, b.0);
        }
        if a.0 > b.0 {
            std::mem::swap(&mut a, &mut b);
        }
        let dx = b.0 - a.0;
        if dx < 0.001 {
            return;
        }
        let gradient = (b.1 - a.1) / dx;
        let half = width * 0.5 * (1.0 + gradient * gradient).sqrt();
        for x in a.0.floor() as i32..=b.0.ceil() as i32 {
            let cap = ((x as f32 + 0.5).min(b.0) - (x as f32 - 0.5).max(a.0)).clamp(0.0, 1.0);
            let center = a.1 + (x as f32 - a.0) * gradient;
            for y in (center - half - 0.5).floor() as i32..=(center + half + 0.5).ceil() as i32 {
                let coverage = (half + 0.5 - (y as f32 - center).abs()).clamp(0.0, 1.0) * cap;
                let (px, py) = if steep { (y, x) } else { (x, y) };
                self.blend(px, py, rgb, opacity * coverage);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::breathing::{Entrance, Family, Settings};
    use std::time::Duration;
    #[test]
    fn packed_source_over_matches_each_premultiplied_channel_exactly() {
        for alpha in 0..=255u32 {
            for old_alpha in 0..=255u32 {
                let source = (alpha << 24) | ((alpha / 2) << 16) | ((alpha / 3) << 8) | alpha;
                let old = (old_alpha << 24)
                    | (old_alpha << 16)
                    | ((old_alpha / 2) << 8)
                    | (old_alpha / 3);
                let expected = [0, 8, 16, 24].into_iter().fold(0, |pixel, shift| {
                    pixel
                        | ((((source >> shift) & 255)
                            + ((old >> shift) & 255) * (255 - alpha) / 255)
                            << shift)
                });
                assert_eq!(source_over(old, source), expected);
            }
        }
    }
    #[test]
    fn geometry_is_clipped_premultiplied_and_replayable_at_desktop_scales() {
        let font = super::super::default_font().unwrap();
        for (width, height, scale) in [
            (800, 600, 1.0),
            (1024, 768, 1.0),
            (1920, 1080, 1.0),
            (1600, 1200, 2.0),
            (2560, 1080, 1.0),
        ] {
            for family in Family::ALL {
                let entrance = Entrance::new(
                    Settings {
                        families: vec![family],
                        ..Settings::default()
                    },
                    42,
                );
                let frame = entrance.frame(Duration::from_secs(6));
                let mut canvas = vec![0; width * height * 4];
                render_breathing(
                    &mut canvas,
                    (width, height),
                    &font,
                    scale,
                    BreathingView {
                        pattern: &entrance.pattern,
                        frame,
                        full: true,
                    },
                );
                let expected = canvas.clone();
                let damage = breathing_damage(width as f32, height as f32, scale);
                for (i, pixel) in canvas.chunks_exact(4).enumerate() {
                    let value = u32::from_ne_bytes(pixel.try_into().unwrap());
                    let alpha = value >> 24;
                    for shift in [0, 8, 16] {
                        assert!(((value >> shift) & 255) <= alpha);
                    }
                    if pixel != premultiply(BACKGROUND) {
                        assert!(damage.contains((i % width) as f32, (i / width) as f32));
                    }
                }
                render_breathing(
                    &mut canvas,
                    (width, height),
                    &font,
                    scale,
                    BreathingView {
                        pattern: &entrance.pattern,
                        frame,
                        full: false,
                    },
                );
                assert_eq!(canvas, expected);
                assert!(canvas.chunks_exact(4).any(|p| p != premultiply(BACKGROUND)));
                let next_frame = entrance.frame(Duration::from_secs(9));
                render_breathing(
                    &mut canvas,
                    (width, height),
                    &font,
                    scale,
                    BreathingView {
                        pattern: &entrance.pattern,
                        frame: next_frame,
                        full: false,
                    },
                );
                let mut next_full = vec![0; canvas.len()];
                render_breathing(
                    &mut next_full,
                    (width, height),
                    &font,
                    scale,
                    BreathingView {
                        pattern: &entrance.pattern,
                        frame: next_frame,
                        full: true,
                    },
                );
                assert_eq!(
                    canvas, next_full,
                    "no stale strokes, glows, or orb pixels for {family:?}"
                );
            }
        }
    }
    #[test]
    fn all_patterns_start_transparent_and_reduced_motion_pixels_stay_still() {
        let font = super::super::default_font().unwrap();
        for family in Family::ALL {
            let entrance = Entrance::new(
                Settings {
                    families: vec![family],
                    reduced_motion: true,
                    ..Settings::default()
                },
                42,
            );
            let mut canvas = vec![255; 800 * 600 * 4];
            let draw = |canvas: &mut [u8], seconds| {
                render_breathing(
                    canvas,
                    (800, 600),
                    &font,
                    1.0,
                    BreathingView {
                        pattern: &entrance.pattern,
                        frame: entrance.frame(Duration::from_secs(seconds)),
                        full: true,
                    },
                )
            };
            draw(&mut canvas, 0);
            assert!(canvas.iter().all(|&v| v == 0), "{family:?}");
            draw(&mut canvas, 3);
            let first = canvas.clone();
            draw(&mut canvas, 4);
            assert_eq!(canvas, first, "{family:?} reduced motion");
        }
    }
    #[test]
    fn arrival_starts_transparent_and_reveal_has_exact_endpoints() {
        let font = super::super::default_font().unwrap();
        let entrance = Entrance::new(Settings::default(), 42);
        let mut canvas = vec![255; 800 * 600 * 4];
        render_breathing(
            &mut canvas,
            (800, 600),
            &font,
            1.0,
            BreathingView {
                pattern: &entrance.pattern,
                frame: entrance.frame(Duration::ZERO),
                full: true,
            },
        );
        assert!(canvas.iter().all(|v| *v == 0));
        // Consecutive arrival frames may quantize to the same background alpha.
        // A partial repaint must not introduce an opaque central rectangle.
        for seconds in [0.0, 0.02, 0.3, 1.0] {
            let frame = entrance.frame(Duration::from_secs_f32(seconds));
            render_breathing(
                &mut canvas,
                (800, 600),
                &font,
                1.0,
                BreathingView {
                    pattern: &entrance.pattern,
                    frame,
                    full: true,
                },
            );
            let expected = canvas.clone();
            render_breathing(
                &mut canvas,
                (800, 600),
                &font,
                1.0,
                BreathingView {
                    pattern: &entrance.pattern,
                    frame,
                    full: false,
                },
            );
            assert_eq!(canvas, expected);
        }
        let source = vec![255; canvas.len()];
        render_reveal(&mut canvas, &source, 0.0);
        assert!(canvas.chunks_exact(4).all(|p| p == premultiply(BACKGROUND)));
        render_reveal(&mut canvas, &source, 1.0);
        assert_eq!(canvas, source);
        render_reveal(&mut canvas, &source, 0.5);
        let half = canvas.clone();
        render_reveal(&mut canvas, &source, 0.5);
        assert_eq!(canvas, half);
    }
}
