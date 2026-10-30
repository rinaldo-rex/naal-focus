use super::{OverlayView, render_overlay};
use fontdue::Font;

/// Software ARGB8888 renderer; dimensions and text size are physical pixels.
pub(super) fn render(
    canvas: &mut [u8],
    width: usize,
    height: usize,
    font: &Font,
    quote: &str,
    text_size: f32,
    scale: f32,
) {
    render_with_charts(
        canvas,
        (width, height),
        font,
        quote,
        text_size,
        scale,
        &crate::config::ChartSettings::default(),
    );
}

/// Initialize the immutable quote, reserving a separate chart band if enabled.
pub(super) fn render_with_charts(
    canvas: &mut [u8],
    size: (usize, usize),
    font: &Font,
    quote: &str,
    text_size: f32,
    scale: f32,
    charts: &crate::config::ChartSettings,
) {
    render_overlay(
        canvas,
        size,
        font,
        scale,
        OverlayView {
            quote,
            text_size,
            reflections_enabled: true,
            charts,
            focus: &crate::config::FocusSettings::default(),
        },
    );
}
