//! Offscreen composed reminders using fixed demo data; no Wayland or personal history.
mod support;

use naal_focus::{
    activity::Editor,
    charts::DailyCharts,
    config::{ChartSettings, ChartStyle, FocusSettings, FocusSide, FocusStyle},
    focus::{Day, Session, Snapshot, Window},
    recall::{Group, Recall},
    render::{self, ChartView, HistoryView, InputView, OverlayView},
    time::to_local,
};
use std::{
    error::Error,
    fs::File,
    io::{BufWriter, Write},
    path::Path,
    time::Duration,
};

fn main() -> Result<(), Box<dyn Error>> {
    let output = support::output_directory()?;
    let font = render::default_font()?;
    let now_ms = support::NOW_MS;
    let entries = support::entries();
    let chart_data = DailyCharts::from_entries(&entries, now_ms, to_local);
    let mut day = Day::empty(now_ms);
    day.sessions = vec![
        Session {
            window: Window::new("demo.editor", "Sample project"),
            start_ms: now_ms - 3_600_000,
            end_ms: now_ms - 1_800_000,
            duration_ms: 1_800_000,
        },
        Session {
            window: Window::new("demo.browser", "Sample documentation"),
            start_ms: now_ms - 1_200_000,
            end_ms: now_ms - 600_000,
            duration_ms: 600_000,
        },
    ];
    let snapshot = Snapshot {
        day,
        unavailable: false,
    };
    let long_quote = format!("Notice.\n\n{}\nReturn to café 🪷.", "m".repeat(120));
    let names = [
        "basic",
        "multiline-selection",
        "history-left-heatmap",
        "suggestions-right-treemap",
        "saved-right-totals",
        "reflections-off-left",
    ];
    for (width, height, scale) in [(1000, 660, 1.0), (1920, 1080, 1.0), (3414, 1920, 2.0)] {
        for (case, name) in names.into_iter().enumerate() {
            let reflections_enabled = case != 5;
            let charts = ChartSettings {
                style: if case == 3 {
                    ChartStyle::Treemap
                } else {
                    ChartStyle::Heatmap
                },
                projects: matches!(case, 2 | 3 | 5),
                feelings: matches!(case, 2 | 3 | 5),
                tags: matches!(case, 2 | 3 | 5),
            };
            let focus = FocusSettings {
                enabled: case >= 2,
                side: if matches!(case, 2 | 5) {
                    FocusSide::Left
                } else {
                    FocusSide::Right
                },
                style: if case == 4 {
                    FocusStyle::Totals
                } else {
                    FocusStyle::Both
                },
                use_24_hour_time: case == 3,
            };
            let mut editor = Editor::default();
            if case == 3 {
                editor.insert("Review @demo ??focused #r").unwrap();
            } else if case != 0 {
                editor.insert("Read café 🪷 #rust @demo ??calm").unwrap();
            }
            if case == 1 {
                editor.select_all();
            }
            let mut recall = Recall::default();
            recall.groups = entries
                .iter()
                .map(|entry| Group {
                    text: entry.text.clone(),
                    timestamp_unix_ms: entry.timestamp_unix_ms,
                    count: 2,
                })
                .collect();
            recall.older(Duration::ZERO);
            let suggestions = vec!["#reading".into(), "#rust".into()];
            let mut pixels = vec![0; width * height * 4];
            render::render_overlay(
                &mut pixels,
                (width, height),
                &font,
                scale,
                OverlayView {
                    quote: if case == 1 {
                        &long_quote
                    } else {
                        "Take a breath.\n\nNotice, then return."
                    },
                    text_size: 36.0 * scale,
                    reflections_enabled,
                    charts: &charts,
                    focus: &focus,
                },
            );
            let content = render::focus_content(width as f32, height as f32, scale, &focus);
            if reflections_enabled {
                render::render_charts_in(
                    &mut pixels,
                    (width, height),
                    &font,
                    scale,
                    content,
                    ChartView {
                        settings: &charts,
                        data: Some(&chart_data),
                        unavailable: false,
                    },
                );
            }
            render::render_focus(
                &mut pixels,
                (width, height),
                &font,
                scale,
                &focus,
                Some(&snapshot),
            );
            if reflections_enabled {
                render::render_input_in(
                    &mut pixels,
                    (width, height),
                    &font,
                    scale,
                    content,
                    InputView {
                        editor: &editor,
                        saved_text: (case == 4).then_some("Read #rust @demo ??calm"),
                        progress: matches!(case, 1 | 3).then_some(0.65),
                        message: None,
                        editing: case != 0 && case != 4,
                        caret_visible: case != 4,
                        history: (case == 2).then_some(HistoryView {
                            recall: &recall,
                            now: Duration::from_millis(80),
                            now_ms,
                        }),
                        suggestions: if case == 3 { &suggestions } else { &[] },
                        suggestion_index: 1,
                    },
                );
            }
            let path = output.join(format!("{width}x{height}-{name}.ppm"));
            write_ppm(&path, (width, height), &pixels)?;
            println!("{}", path.display());
        }
    }
    Ok(())
}

fn write_ppm(
    path: &Path,
    (width, height): (usize, usize),
    pixels: &[u8],
) -> Result<(), Box<dyn Error>> {
    let mut output = BufWriter::new(File::create(path)?);
    write!(output, "P6\n{width} {height}\n255\n")?;
    for pixel in pixels.chunks_exact(4) {
        let color = u32::from_ne_bytes(pixel.try_into()?);
        output.write_all(&[(color >> 16) as u8, (color >> 8) as u8, color as u8])?;
    }
    output.flush()?;
    Ok(())
}
