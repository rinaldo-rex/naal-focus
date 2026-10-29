//! Cached, noninteractive focus sidebar; all styles share one daily snapshot.
use super::{BACKGROUND, Image, Rect, shortened, text_width};
use crate::{
    config::{FocusSettings, FocusSide, FocusStyle},
    dashboard::to_local,
    focus::{Day, Snapshot, Total, Window, duration_label},
};
use fontdue::Font;

pub fn focus_sidebar(
    width: f32,
    height: f32,
    scale: f32,
    settings: &FocusSettings,
) -> Option<Rect> {
    if !settings.enabled || width < 960.0 * scale || height < 600.0 * scale {
        return None;
    }
    let sidebar_width = if width < 1100.0 * scale {
        220.0 * scale
    } else {
        264.0 * scale
    };
    Some(Rect {
        x: if settings.side == FocusSide::Left {
            32.0 * scale
        } else {
            width - sidebar_width - 32.0 * scale
        },
        y: 44.0 * scale,
        width: sidebar_width,
        height: height - 88.0 * scale,
    })
}
pub fn focus_content(width: f32, height: f32, scale: f32, settings: &FocusSettings) -> Rect {
    let mut content = Rect {
        x: 0.0,
        y: 0.0,
        width,
        height,
    };
    if let Some(sidebar) = focus_sidebar(width, height, scale, settings) {
        let reserved = sidebar.width + 64.0 * scale;
        content.width -= reserved;
        if settings.side == FocusSide::Left {
            content.x = reserved;
        }
    }
    content
}

struct Palette(std::collections::BTreeMap<String, [u8; 3]>);
impl Palette {
    fn new(day: &Day) -> Self {
        const COLORS: [[u8; 3]; 8] = [
            [166, 201, 177],
            [185, 173, 206],
            [209, 181, 134],
            [139, 175, 198],
            [167, 184, 143],
            [200, 156, 158],
            [143, 190, 185],
            [179, 171, 153],
        ];
        let mut apps = std::collections::BTreeMap::new();
        // First-seen order is preserved by today's chronological history, so
        // new applications cannot reshuffle existing colors during the day.
        for session in &day.sessions {
            if apps.contains_key(&session.window.app_id) {
                continue;
            }
            let color = COLORS
                .get(apps.len())
                .copied()
                .unwrap_or_else(|| generated_color(&session.window));
            apps.insert(session.window.app_id.clone(), color);
        }
        Self(apps)
    }
    fn color(&self, window: &Window) -> [u8; 3] {
        self.0
            .get(&window.app_id)
            .copied()
            .unwrap_or([137, 153, 144])
    }
}

fn generated_color(window: &Window) -> [u8; 3] {
    let hash = window.app_id.bytes().fold(2166136261u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16777619)
    });
    // A broad, quiet hue range avoids the frequent exact collisions of an
    // eight-color palette while remaining stable across styles and monitors.
    let hue = (hash % 360) as f32 / 60.0;
    let chroma = 0.30;
    let x = chroma * (1.0 - (hue % 2.0 - 1.0).abs());
    let rgb = match hue as u32 {
        0 => [chroma, x, 0.0],
        1 => [x, chroma, 0.0],
        2 => [0.0, chroma, x],
        3 => [0.0, x, chroma],
        4 => [x, 0.0, chroma],
        _ => [chroma, 0.0, x],
    };
    rgb.map(|channel| ((channel + 0.52) * 255.0) as u8)
}
fn app_label(window: &Window) -> &str {
    window
        .app_id
        .trim_end_matches(".desktop")
        .rsplit('.')
        .next()
        .unwrap_or(&window.app_id)
}
fn rgba(rgb: [u8; 3]) -> [u8; 4] {
    [rgb[0], rgb[1], rgb[2], 225]
}

pub fn render_focus(
    canvas: &mut [u8],
    size: (usize, usize),
    font: &Font,
    scale: f32,
    settings: &FocusSettings,
    snapshot: Option<&Snapshot>,
) {
    let (width, height) = size;
    let Some(area) = focus_sidebar(width as f32, height as f32, scale, settings) else {
        return;
    };
    let mut image = Image {
        canvas,
        width,
        height,
        clip: area,
    };
    image.rect(area, BACKGROUND);
    image.text_at(font, "Today", 14.0 * scale, area.x, area.y, [210, 224, 215]);
    if let Some(snapshot) = snapshot {
        let local = to_local(snapshot.day.observed_ms);
        let time = clock_label(local.hour, Some(local.minute), settings.use_24_hour_time);
        image.text_at(
            font,
            &time,
            11.0 * scale,
            area.x + area.width - text_width(font, &time, 11.0 * scale),
            area.y + 2.0 * scale,
            [127, 147, 135],
        );
    }
    let message = match snapshot {
        None => Some("Focus history unavailable"),
        Some(s) if s.unavailable => Some("Focus tracking unavailable"),
        Some(s) if s.day.sessions.is_empty() => Some("No tracked time today"),
        _ => None,
    };
    if let Some(message) = message {
        image.text_at(
            font,
            message,
            12.0 * scale,
            area.x,
            area.y + 58.0 * scale,
            [141, 158, 146],
        );
        return;
    }
    let day = &snapshot.unwrap().day;
    let palette = Palette::new(day);
    let mut body = Rect {
        x: area.x,
        y: area.y + 38.0 * scale,
        width: area.width,
        height: area.height - 58.0 * scale,
    };
    if settings.style != FocusStyle::Timeline {
        image.text_at(
            font,
            &duration_label(day.total_ms()),
            27.0 * scale,
            area.x,
            area.y + 34.0 * scale,
            [218, 232, 222],
        );
        image.text_at(
            font,
            "Window time",
            11.0 * scale,
            area.x,
            area.y + 70.0 * scale,
            [132, 151, 138],
        );
        body.y = area.y + 111.0 * scale;
        body.height = area.height - 131.0 * scale;
    }
    match settings.style {
        FocusStyle::Totals => ranking(&mut image, font, body, &day.totals(6), scale, &palette),
        FocusStyle::Timeline => timeline(&mut image, font, body, day, scale, settings, &palette),
        FocusStyle::Both => {
            timeline(
                &mut image,
                font,
                Rect {
                    width: 53.0 * scale,
                    ..body
                },
                day,
                scale,
                settings,
                &palette,
            );
            let list = Rect {
                x: body.x + 68.0 * scale,
                width: body.width - 68.0 * scale,
                ..body
            };
            ranking(&mut image, font, list, &day.totals(5), scale, &palette);
        }
    }
}

fn ranking(
    image: &mut Image<'_>,
    font: &Font,
    area: Rect,
    rows: &[Total],
    scale: f32,
    palette: &Palette,
) {
    let max = rows.first().map_or(1, |r| r.duration_ms).max(1) as f32;
    let height = (area.height / rows.len().max(1) as f32).min(78.0 * scale);
    for (index, row) in rows.iter().enumerate() {
        let y = area.y + index as f32 * height;
        let (label, title, rgb) = match &row.window {
            Some(window) => (
                app_label(window),
                window.title.as_str(),
                palette.color(window),
            ),
            None => ("Other", "", [137, 153, 144]),
        };
        let size = if area.width < 180.0 * scale {
            10.0 * scale
        } else {
            12.0 * scale
        };
        let duration = duration_label(row.duration_ms);
        let duration_width = text_width(font, &duration, size);
        image.rect(
            Rect {
                x: area.x,
                y: y + 5.0 * scale,
                width: 5.0 * scale,
                height: 5.0 * scale,
            },
            rgba(rgb),
        );
        image.text_at(
            font,
            &shortened(
                font,
                label,
                size,
                (area.width - duration_width - 19.0 * scale).max(0.0),
            ),
            size,
            area.x + 10.0 * scale,
            y,
            [193, 209, 198],
        );
        image.text_at(
            font,
            &duration,
            size,
            area.x + area.width - duration_width,
            y,
            [183, 203, 190],
        );
        image.text_at(
            font,
            &shortened(font, title, 11.0 * scale, area.width),
            11.0 * scale,
            area.x,
            y + 21.0 * scale,
            [136, 156, 143],
        );
        let track = Rect {
            x: area.x,
            y: y + 44.0 * scale,
            width: area.width,
            height: 4.0 * scale,
        };
        image.rect(track, [37, 45, 42, 225]);
        image.rect(
            Rect {
                width: track.width * row.duration_ms as f32 / max,
                ..track
            },
            rgba(rgb),
        );
    }
}

fn clock_label(hour: u32, minute: Option<u32>, use_24_hour_time: bool) -> String {
    let display_hour = if use_24_hour_time {
        hour
    } else {
        (hour + 11) % 12 + 1
    };
    let mut label = if use_24_hour_time {
        format!("{display_hour:02}")
    } else {
        display_hour.to_string()
    };
    if let Some(minute) = minute {
        label.push_str(&format!(":{minute:02}"));
    }
    if !use_24_hour_time {
        label.push_str(if hour < 12 { " AM" } else { " PM" });
    }
    label
}

fn timeline_gutter(font: &Font, scale: f32, mini: bool, use_24_hour_time: bool) -> f32 {
    (0..24).fold(
        if mini { 28.0 * scale } else { 45.0 * scale },
        |width, hour| {
            let label = clock_label(hour, (!mini).then_some(0), use_24_hour_time);
            width.max(text_width(font, &label, 9.0 * scale) + 6.0 * scale)
        },
    )
}

fn timeline(
    image: &mut Image<'_>,
    font: &Font,
    area: Rect,
    day: &Day,
    scale: f32,
    settings: &FocusSettings,
    palette: &Palette,
) {
    let mini = settings.style == FocusStyle::Both;
    let Some(first) = day.sessions.first() else {
        return;
    };
    let local = to_local(first.start_ms);
    let start = first.start_ms.saturating_sub(
        (u64::from(local.minute) * 60 + u64::from(local.second)) * 1000 + first.start_ms % 1000,
    );
    let end = day.observed_ms.max(first.end_ms);
    let span = end.saturating_sub(start).max(1) as f64;
    let height = (area.height - 18.0 * scale).max(0.0);
    let at = |timestamp: u64| {
        area.y + (timestamp.saturating_sub(start) as f64 / span).clamp(0.0, 1.0) as f32 * height
    };
    let bar_x = area.x + timeline_gutter(font, scale, mini, settings.use_24_hour_time);
    let bar_width = if mini { 12.0 * scale } else { 9.0 * scale };
    let mut tick = start;
    for _ in 0..26 {
        if tick >= end {
            break;
        }
        let local = to_local(tick);
        let label = clock_label(local.hour, (!mini).then_some(0), settings.use_24_hour_time);
        image.text_at(font, &label, 9.0 * scale, area.x, at(tick), [122, 143, 130]);
        tick = tick.saturating_add(3_600_000);
    }
    for session in &day.sessions {
        let top = at(session.start_ms);
        let bottom = at(session.end_ms);
        if bottom <= top {
            continue;
        }
        let rgb = palette.color(&session.window);
        image.rect(
            Rect {
                x: bar_x,
                y: top,
                width: bar_width,
                height: (bottom - top - scale).max(scale),
            },
            rgba(rgb),
        );
        if mini {
            continue;
        }
        let x = bar_x + bar_width + 12.0 * scale;
        let width = (area.x + area.width - x).max(0.0);
        if bottom - top >= 18.0 * scale {
            let app = format!(
                "{} · {}",
                app_label(&session.window),
                duration_label(session.duration_ms)
            );
            image.text_at(
                font,
                &shortened(font, &app, 10.0 * scale, width),
                10.0 * scale,
                x,
                top,
                [182, 205, 189],
            );
        }
        if bottom - top >= 36.0 * scale {
            image.text_at(
                font,
                &shortened(font, &session.window.title, 10.0 * scale, width),
                10.0 * scale,
                x,
                top + 16.0 * scale,
                [130, 151, 137],
            );
        }
    }
    let local = to_local(end);
    let label = clock_label(local.hour, Some(local.minute), settings.use_24_hour_time);
    image.text_at(
        font,
        &label,
        9.0 * scale,
        area.x,
        area.y + height + 5.0 * scale,
        [163, 192, 174],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::ChartSettings,
        focus::Session,
        render::{self, OverlayView},
    };
    fn font() -> Font {
        Font::from_bytes(
            std::fs::read("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf").unwrap(),
            fontdue::FontSettings::default(),
        )
        .unwrap()
    }
    fn snapshot() -> Snapshot {
        let now = 1_792_000_000_000;
        let mut day = Day::empty(now);
        day.sessions = (0..12)
            .map(|i| Session {
                window: Window::new(
                    &format!("app{}", i % 3),
                    &format!("Private window {i} {}", "long title ".repeat(30)),
                ),
                start_ms: now - 7_200_000 + i * 500_000,
                end_ms: now - 7_200_000 + i * 500_000 + 300_000,
                duration_ms: 300_000,
            })
            .collect();
        Snapshot {
            day,
            unavailable: false,
        }
    }
    #[test]
    fn clock_labels_cover_midnight_noon_and_evening() {
        for (hour, minute, full, compact, military) in [
            (0, 0, "12:00 AM", "12 AM", "00:00"),
            (0, 5, "12:05 AM", "12 AM", "00:05"),
            (11, 59, "11:59 AM", "11 AM", "11:59"),
            (12, 0, "12:00 PM", "12 PM", "12:00"),
            (12, 7, "12:07 PM", "12 PM", "12:07"),
            (13, 5, "1:05 PM", "1 PM", "13:05"),
            (23, 59, "11:59 PM", "11 PM", "23:59"),
        ] {
            assert_eq!(clock_label(hour, Some(minute), false), full);
            assert_eq!(clock_label(hour, None, false), compact);
            assert_eq!(clock_label(hour, Some(minute), true), military);
            assert_eq!(clock_label(hour, None, true), format!("{hour:02}"));
        }
    }

    #[test]
    fn timeline_clock_labels_fit_beside_bars_in_both_formats() {
        let font = font();
        for scale in [1.0, 2.0] {
            for mini in [false, true] {
                for use_24_hour_time in [false, true] {
                    let gutter = timeline_gutter(&font, scale, mini, use_24_hour_time);
                    for hour in 0..24 {
                        let label = clock_label(hour, (!mini).then_some(0), use_24_hour_time);
                        assert!(text_width(&font, &label, 9.0 * scale) + 4.0 * scale <= gutter);
                        if mini {
                            let end = clock_label(hour, Some(59), use_24_hour_time);
                            assert!(text_width(&font, &end, 9.0 * scale) <= 53.0 * scale);
                            assert!(gutter + 12.0 * scale <= 53.0 * scale);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn disabled_and_small_desktop_outputs_reserve_no_space() {
        let snapshot = snapshot();
        let palette = Palette::new(&snapshot.day);
        let colors: std::collections::BTreeSet<_> = snapshot
            .day
            .sessions
            .iter()
            .map(|s| palette.color(&s.window))
            .collect();
        assert_eq!(
            colors.len(),
            3,
            "common applications need distinct day colors"
        );
        let off = FocusSettings::default();
        assert!(focus_sidebar(1920.0, 1080.0, 1.0, &off).is_none());
        let on = FocusSettings {
            enabled: true,
            ..off
        };
        assert!(focus_sidebar(800.0, 600.0, 1.0, &on).is_none());
        assert_eq!(focus_content(800.0, 600.0, 1.0, &on).width, 800.0);
    }
    #[test]
    fn all_styles_clip_to_sidebar_without_touching_quote_or_reflection() {
        let font = font();
        let snapshot = snapshot();
        for (width, height, scale) in [
            (1000, 650, 1.0),
            (1920, 1080, 1.0),
            (2560, 1080, 1.0),
            (1080, 1920, 1.0),
            (3414, 1920, 2.0),
        ] {
            for side in [FocusSide::Left, FocusSide::Right] {
                for style in [FocusStyle::Totals, FocusStyle::Timeline, FocusStyle::Both] {
                    for use_24_hour_time in [false, true] {
                        let settings = FocusSettings {
                            enabled: true,
                            style,
                            side,
                            use_24_hour_time,
                        };
                        let mut canvas = vec![0; width * height * 4];
                        render::render_overlay(
                            &mut canvas,
                            (width, height),
                            &font,
                            scale,
                            OverlayView {
                                quote: "Take one breath.",
                                text_size: 36.0 * scale,
                                reflections_enabled: true,
                                charts: &ChartSettings::default(),
                                focus: &settings,
                            },
                        );
                        let before = canvas.clone();
                        render_focus(
                            &mut canvas,
                            (width, height),
                            &font,
                            scale,
                            &settings,
                            Some(&snapshot),
                        );
                        let area =
                            focus_sidebar(width as f32, height as f32, scale, &settings).unwrap();
                        let mut changed = 0;
                        for (i, (a, b)) in before
                            .chunks_exact(4)
                            .zip(canvas.chunks_exact(4))
                            .enumerate()
                        {
                            if a != b {
                                changed += 1;
                                assert!(area.contains((i % width) as f32, (i / width) as f32));
                            }
                            let pixel = u32::from_ne_bytes(b.try_into().unwrap());
                            let alpha = pixel >> 24;
                            assert!(
                                [16, 8, 0]
                                    .into_iter()
                                    .all(|shift| (pixel >> shift) & 255 <= alpha)
                            );
                        }
                        assert!(changed > 100);
                    }
                }
            }
        }
    }
    #[test]
    fn reflection_charts_and_typing_leave_the_sidebar_cached() {
        let font = font();
        let snapshot = snapshot();
        let entry = crate::activity_log::Entry {
            version: 1,
            id: "example".into(),
            timestamp_unix_ms: snapshot.day.observed_ms,
            text: "#rust @naal ??focused".into(),
            metadata: None,
        };
        let data =
            crate::charts::DailyCharts::from_entries(&[entry], snapshot.day.observed_ms, to_local);
        for (width, height, scale) in [(1000, 650, 1.0), (3414, 1920, 2.0)] {
            for side in [FocusSide::Left, FocusSide::Right] {
                for style in [
                    crate::config::ChartStyle::Heatmap,
                    crate::config::ChartStyle::Treemap,
                ] {
                    let settings = FocusSettings {
                        enabled: true,
                        side,
                        ..Default::default()
                    };
                    let charts = ChartSettings {
                        style,
                        projects: true,
                        feelings: true,
                        tags: true,
                    };
                    let content = focus_content(width as f32, height as f32, scale, &settings);
                    let mut canvas = vec![0; width * height * 4];
                    render::render_overlay(
                        &mut canvas,
                        (width, height),
                        &font,
                        scale,
                        OverlayView {
                            quote: "Take a breath.",
                            text_size: 36.0 * scale,
                            reflections_enabled: true,
                            charts: &charts,
                            focus: &settings,
                        },
                    );
                    render_focus(
                        &mut canvas,
                        (width, height),
                        &font,
                        scale,
                        &settings,
                        Some(&snapshot),
                    );
                    let before = canvas.clone();
                    render::render_charts_in(
                        &mut canvas,
                        (width, height),
                        &font,
                        scale,
                        content,
                        render::ChartView {
                            settings: &charts,
                            data: Some(&data),
                            unavailable: false,
                        },
                    );
                    let mut activity = crate::activity::Activity::new(
                        std::time::Duration::from_secs(5),
                        std::time::Duration::from_secs(120),
                    );
                    activity.insert("A draft #rust @naal", std::time::Duration::ZERO);
                    render::render_input_in(
                        &mut canvas,
                        (width, height),
                        &font,
                        scale,
                        content,
                        render::InputView {
                            editor: &activity.editor,
                            saved_text: None,
                            progress: Some(0.5),
                            message: None,
                            editing: true,
                            caret_visible: true,
                            history: None,
                            suggestions: &[],
                            suggestion_index: 0,
                        },
                    );
                    let sidebar =
                        focus_sidebar(width as f32, height as f32, scale, &settings).unwrap();
                    let band = render::chart_damage_in(content, scale, &charts).unwrap();
                    for (i, (a, b)) in before
                        .chunks_exact(4)
                        .zip(canvas.chunks_exact(4))
                        .enumerate()
                    {
                        let (x, y) = ((i % width) as f32, (i / width) as f32);
                        if sidebar.contains(x, y) || y < band.y {
                            assert_eq!(a, b);
                        }
                    }
                    assert_ne!(canvas, before);
                }
            }
        }
    }

    #[test]
    fn empty_and_failed_history_are_visibly_different() {
        let font = font();
        let settings = FocusSettings {
            enabled: true,
            ..FocusSettings::default()
        };
        let mut snapshot = snapshot();
        snapshot.day.sessions.clear();
        let mut empty = vec![0; 1000 * 650 * 4];
        render_focus(
            &mut empty,
            (1000, 650),
            &font,
            1.0,
            &settings,
            Some(&snapshot),
        );
        snapshot.unavailable = true;
        let mut failed = vec![0; empty.len()];
        render_focus(
            &mut failed,
            (1000, 650),
            &font,
            1.0,
            &settings,
            Some(&snapshot),
        );
        assert_ne!(empty, failed);
    }
}
