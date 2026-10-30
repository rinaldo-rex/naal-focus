use fontdue::Font;

mod breathing;
pub use breathing::{
    BreathingView, breathing_background, breathing_damage, render_breathing, render_reveal,
};
mod charts;
pub use charts::{ChartView, chart_damage, chart_damage_in, render_charts, render_charts_in};
mod focus;
pub use focus::{focus_content, focus_sidebar, render_focus};
mod input;
pub use input::{
    HistoryView, InputView, input_caret, input_damage, input_rect, render_input, render_input_in,
};
mod primitives;
pub use primitives::Rect;
mod text;
pub use text::default_font;

use primitives::{Image, premultiply};
use text::wrap;

const BACKGROUND: [u8; 4] = [20, 23, 29, 225];

pub struct OverlayView<'a> {
    pub quote: &'a str,
    pub text_size: f32,
    pub reflections_enabled: bool,
    pub charts: &'a crate::config::ChartSettings,
    pub focus: &'a crate::config::FocusSettings,
}

pub fn render_overlay(
    canvas: &mut [u8],
    size: (usize, usize),
    font: &Font,
    scale: f32,
    view: OverlayView<'_>,
) {
    let (width, height) = size;
    let OverlayView {
        quote,
        text_size,
        reflections_enabled,
        charts,
        focus,
    } = view;
    let content = focus_content(width as f32, height as f32, scale, focus);
    let bg = premultiply(BACKGROUND);
    for pixel in canvas.chunks_exact_mut(4) {
        pixel.copy_from_slice(&bg);
    }
    let max_width = (content.width * 0.8).min(1100.0 * scale);
    // Quote, charts, and the existing history/input strip never overlap.
    let quote_bottom = if reflections_enabled {
        chart_damage_in(content, scale, charts).map_or_else(
            || input_damage(content.width, content.height, scale).y,
            |rect| rect.y,
        )
    } else {
        content.height
    };
    let quote_height = (quote_bottom - 20.0 * scale).max(0.0);
    let mut size = text_size;
    let mut lines = wrap(font, quote, size, max_width);
    // Fit bounded long quotes vertically; wrapping also splits unbroken words.
    while lines.len() as f32 * size * 1.4 > quote_height * 0.65 && size > 16.0 * scale {
        size -= scale;
        lines = wrap(font, quote, size, max_width);
    }
    let line_height = size * 1.4;
    let total_height = lines.len() as f32 * line_height;
    let top = (quote_height - total_height) / 2.0;
    let mut image = Image {
        canvas,
        width,
        height,
        clip: Rect {
            x: content.x,
            y: 0.0,
            width: content.width,
            height: quote_height,
        },
    };
    if top > 60.0 * scale {
        image.text(
            font,
            "A MOMENT OF AWARENESS",
            13.0 * scale,
            top - 48.0 * scale,
            [173, 207, 187],
        );
    }
    for (i, line) in lines.iter().enumerate() {
        image.text(
            font,
            line,
            size,
            top + i as f32 * line_height,
            [239, 240, 242],
        );
    }
    if top + total_height + 80.0 * scale < quote_height {
        image.text(
            font,
            "Notice. Take a breath. Return.",
            17.0 * scale,
            top + total_height + 35.0 * scale,
            [173, 191, 182],
        );
    }
}

#[cfg(test)]
mod test_helpers;
#[cfg(test)]
mod tests;
