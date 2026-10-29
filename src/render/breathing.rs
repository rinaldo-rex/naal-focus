//! Small damaged-region geometry rendering; full surfaces change only during fades.
use super::{
    BACKGROUND,
    primitives::{Image, Rect, premultiply},
};
use crate::breathing::{Frame, Pattern, Point};
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
    let text_width = super::text::text_width(font, frame.cue, text_size);
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::breathing::{Entrance, Family, Settings};
    use std::time::Duration;
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
