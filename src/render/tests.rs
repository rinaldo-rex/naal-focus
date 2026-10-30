use super::{test_helpers::render, text::text_width, *};
#[test]
fn disabled_reflections_reclaim_the_full_height_and_ignore_chart_reservations() {
    let font = default_font().unwrap();
    let no_charts = crate::config::ChartSettings::default();
    let charts = crate::config::ChartSettings {
        projects: true,
        feelings: true,
        tags: true,
        ..Default::default()
    };
    for (width, height, scale) in [(1000, 650, 1.0), (1920, 1080, 1.0), (3414, 1920, 2.0)] {
        for side in [
            crate::config::FocusSide::Left,
            crate::config::FocusSide::Right,
        ] {
            let focus = crate::config::FocusSettings {
                enabled: true,
                side,
                ..Default::default()
            };
            let draw = |reflections_enabled, charts| {
                let mut pixels = vec![0; width * height * 4];
                render_overlay(
                    &mut pixels,
                    (width, height),
                    &font,
                    scale,
                    OverlayView {
                        quote: "Take one breath.",
                        text_size: 36.0 * scale,
                        reflections_enabled,
                        charts,
                        focus: &focus,
                    },
                );
                pixels
            };
            let disabled = draw(false, &charts);
            assert_eq!(disabled, draw(false, &no_charts));
            assert_ne!(disabled, draw(true, &no_charts));
            let content = focus_content(width as f32, height as f32, scale, &focus);
            let center_y = content.height * 0.5;
            let near_center = Rect {
                y: center_y - 36.0 * scale,
                height: 72.0 * scale,
                ..content
            };
            let background = premultiply(BACKGROUND);
            assert!(disabled.chunks_exact(4).enumerate().any(|(i, pixel)| {
                near_center.contains((i % width) as f32, (i / width) as f32) && pixel != background
            }));
        }
    }
}

#[test]
fn metadata_and_suggestions_stay_inside_the_shared_desktop_damage_strip() {
    let font = default_font().unwrap();
    let mut activity = crate::activity::Activity::new(
        std::time::Duration::from_secs(5),
        std::time::Duration::from_secs(120),
    );
    activity.mapped(std::time::Duration::ZERO);
    activity
        .recall
        .vocabulary
        .include(crate::metadata::Metadata::parse(
            "#rust #reading @naal ??focused",
        ));
    for (width, height, scale) in [(1000, 650, 1.0), (1920, 1080, 1.0), (3414, 1920, 2.0)] {
        activity.editor = crate::activity::Editor::default();
        activity.editor.insert("Review @naal ??focused #r").unwrap();
        let mut pixels = vec![0; width * height * 4];
        render(
            &mut pixels,
            width,
            height,
            &font,
            "Take a breath.",
            36.0 * scale,
            scale,
        );
        let original = pixels.clone();
        let suggestions = activity.suggestions();
        render_input(
            &mut pixels,
            width,
            height,
            &font,
            scale,
            InputView {
                editor: &activity.editor,
                saved_text: None,
                progress: Some(0.5),
                message: None,
                editing: true,
                caret_visible: true,
                history: None,
                suggestions: &suggestions,
                suggestion_index: 0,
            },
        );
        let damage = input_damage(width as f32, height as f32, scale);
        assert_eq!(
            &pixels[..damage.y as usize * width * 4],
            &original[..damage.y as usize * width * 4]
        );
        let field = input_rect(width as f32, height as f32, scale);
        for row in [-20.0, -60.0] {
            // Pills and suggestion rows both paint.
            let y = (field.y + row * scale) as usize;
            let start = (y * width + field.x.ceil() as usize) * 4;
            let end = start + field.width as usize * 4;
            assert_ne!(&pixels[start..end], &original[start..end]);
        }
        for pixel in pixels.chunks_exact(4) {
            let pixel = u32::from_ne_bytes(pixel.try_into().unwrap());
            assert!(
                [0, 8, 16]
                    .into_iter()
                    .all(|shift| (pixel >> shift) & 255 <= pixel >> 24)
            );
        }
        // Many long metadata values collapse to a bounded overflow pill.
        activity.editor = crate::activity::Editor::default();
        activity
            .editor
            .insert(
                &(0..25)
                    .map(|i| format!("#long_tag_{i} @long_project_{i} ??long_feeling_{i} "))
                    .collect::<String>(),
            )
            .unwrap();
        render_input(
            &mut pixels,
            width,
            height,
            &font,
            scale,
            InputView {
                editor: &activity.editor,
                saved_text: None,
                progress: None,
                message: None,
                editing: true,
                caret_visible: true,
                history: None,
                suggestions: &[],
                suggestion_index: 0,
            },
        );
    }
}
#[test]
fn input_hit_testing_scroll_and_scale_share_geometry() {
    let font = default_font().unwrap();
    let mut editor = crate::activity::Editor::default();
    editor.insert("Reading 🪷").unwrap();
    for scale in [1.0, 2.0, 4.0] {
        let size = (320, 240);
        let field = input_rect(320.0 * scale, 240.0 * scale, scale);
        let inside = (
            (field.x + 12.0 * scale) as f64 / scale as f64,
            (field.y + 10.0 * scale) as f64 / scale as f64,
        );
        assert_eq!(input_caret(&font, &editor, size, scale, inside), Some(0));
        assert!(input_caret(&font, &editor, size, scale, (0.0, 0.0)).is_none());
    }
    editor.select_all();
    editor.insert(&"🪷".repeat(2000)).unwrap();
    let field = input_rect(320.0, 240.0, 1.0);
    let position = input_caret(
        &font,
        &editor,
        (320, 240),
        1.0,
        ((field.x + 12.0) as f64, (field.y + 10.0) as f64),
    )
    .unwrap();
    assert!(position > 0 && position <= editor.cursor());
    assert!(editor.text().is_char_boundary(position));
}

#[test]
fn cached_quote_pixels_survive_saved_and_editing_transitions() {
    let font = default_font().unwrap();
    let mut editor = crate::activity::Editor::default();
    editor.insert("A very long previous reflection").unwrap();
    let mut pixels = vec![0; 640 * 480 * 4];
    render(&mut pixels, 640, 480, &font, "Notice.", 36.0, 1.0);
    let baseline = pixels.clone();
    render_input(
        &mut pixels,
        640,
        480,
        &font,
        1.0,
        InputView {
            editor: &editor,
            suggestions: &[],
            suggestion_index: 0,
            saved_text: None,
            progress: Some(0.5),
            message: Some("Previous save error"),
            editing: true,
            caret_visible: true,
            history: None,
        },
    );
    render_input(
        &mut pixels,
        640,
        480,
        &font,
        1.0,
        InputView {
            editor: &editor,
            suggestions: &[],
            suggestion_index: 0,
            saved_text: Some("Reading docs"),
            progress: None,
            message: None,
            editing: false,
            caret_visible: false,
            history: None,
        },
    );
    let damage = input_damage(640.0, 480.0, 1.0);
    for y in 0..480 {
        if (y as f32) < damage.y || (y as f32) >= damage.y + damage.height {
            assert_eq!(
                &pixels[y * 640 * 4..(y + 1) * 640 * 4],
                &baseline[y * 640 * 4..(y + 1) * 640 * 4]
            );
        }
    }
    let field = input_rect(640.0, 480.0, 1.0);
    let offset = ((field.y as usize + 1) * 640 + field.x.ceil() as usize + 1) * 4;
    assert_eq!(&pixels[offset..offset + 4], &premultiply(BACKGROUND)); // textbox disappeared
    render_input(
        &mut pixels,
        640,
        480,
        &font,
        1.0,
        InputView {
            editor: &editor,
            suggestions: &[],
            suggestion_index: 0,
            saved_text: None,
            progress: Some(0.2),
            message: None,
            editing: true,
            caret_visible: true,
            history: None,
        },
    );
    assert_eq!(&pixels[offset..offset + 4], &premultiply([31, 36, 40, 235]));
}

#[test]
fn history_fades_and_shows_metadata_only_for_the_highlighted_row() {
    let font = default_font().unwrap();
    let editor = crate::activity::Editor::default();
    let mut recall = crate::recall::Recall::default();
    recall.groups = (0..5)
        .map(|_| crate::recall::Group {
            text: "Task".into(),
            timestamp_unix_ms: 0,
            count: 3,
        })
        .collect();
    let mut background = vec![0; 640 * 480 * 4];
    render(&mut background, 640, 480, &font, "Notice.", 36.0, 1.0);
    let paint = |recall: &crate::recall::Recall, now| {
        let mut pixels = background.clone();
        render_input(
            &mut pixels,
            640,
            480,
            &font,
            1.0,
            InputView {
                editor: &editor,
                suggestions: &[],
                suggestion_index: 0,
                saved_text: None,
                progress: None,
                message: None,
                editing: false,
                caret_visible: false,
                history: Some(HistoryView {
                    recall,
                    now,
                    now_ms: 300_000,
                }),
            },
        );
        let damage = input_damage(640.0, 480.0, 1.0);
        for y in 0..damage.y as usize {
            assert_eq!(
                &pixels[y * 640 * 4..(y + 1) * 640 * 4],
                &background[y * 640 * 4..(y + 1) * 640 * 4]
            );
        }
        pixels
    };
    let field = input_rect(640.0, 480.0, 1.0);
    let brightness = |pixels: &[u8], slot: usize, right: bool| -> u64 {
        let top = (field.y - 182.0) as usize + slot * 44;
        let left = if right {
            (field.x + field.width - 140.0) as usize
        } else {
            (field.x + 12.0) as usize
        };
        let width = if right { 125 } else { 60 };
        let bg = u32::from_ne_bytes(premultiply(BACKGROUND));
        let mut total = 0;
        for y in top..top + 44 {
            for x in left..left + width {
                let offset = (y * 640 + x) * 4;
                let pixel = u32::from_ne_bytes(pixels[offset..offset + 4].try_into().unwrap());
                total += [0, 8, 16]
                    .into_iter()
                    .map(|shift| {
                        ((pixel >> shift) & 255).saturating_sub((bg >> shift) & 255) as u64
                    })
                    .sum::<u64>();
            }
        }
        total
    };
    let unselected = paint(&recall, std::time::Duration::ZERO);
    assert!(brightness(&unselected, 0, false) < brightness(&unselected, 1, false));
    assert!(brightness(&unselected, 1, false) < brightness(&unselected, 2, false));
    assert_eq!(
        (0..3)
            .map(|slot| brightness(&unselected, slot, true))
            .sum::<u64>(),
        0
    );
    recall.older(std::time::Duration::ZERO);
    let selected = paint(&recall, std::time::Duration::ZERO);
    assert_eq!(brightness(&selected, 0, true), 0);
    assert_eq!(brightness(&selected, 1, true), 0);
    assert!(brightness(&selected, 2, true) > 0);
    recall.older(std::time::Duration::ZERO);
    for millis in [80, 160, 200] {
        paint(&recall, std::time::Duration::from_millis(millis));
    }
}
#[test]
fn history_damage_and_long_rows_fit_desktop_sizes_and_scaling() {
    let font = default_font().unwrap();
    let mut recall = crate::recall::Recall::default();
    recall.groups.push(crate::recall::Group {
        text: "é🪷".repeat(1000),
        timestamp_unix_ms: 0,
        count: 9999,
    });
    recall.older(std::time::Duration::ZERO);
    let editor = crate::activity::Editor::default();
    for (width, height, scale) in [(1000, 650, 1.0), (1920, 1080, 1.0), (3414, 1920, 2.0)] {
        let field = input_rect(width as f32, height as f32, scale);
        let damage = input_damage(width as f32, height as f32, scale);
        assert!(damage.y >= 0.0 && damage.y + damage.height <= height as f32);
        assert!(damage.y + 182.0 * scale <= field.y);
        let mut pixels = vec![0; width * height * 4];
        render(
            &mut pixels,
            width,
            height,
            &font,
            "Notice.",
            36.0 * scale,
            scale,
        );
        render_input(
            &mut pixels,
            width,
            height,
            &font,
            scale,
            InputView {
                editor: &editor,
                suggestions: &[],
                suggestion_index: 0,
                saved_text: None,
                progress: None,
                message: None,
                editing: true,
                caret_visible: false,
                history: Some(HistoryView {
                    recall: &recall,
                    now: std::time::Duration::ZERO,
                    now_ms: 300_000,
                }),
            },
        );
        for pixel in pixels.chunks_exact(4) {
            let pixel = u32::from_ne_bytes(pixel.try_into().unwrap());
            assert!(
                [0, 8, 16]
                    .into_iter()
                    .all(|shift| (pixel >> shift) & 255 <= pixel >> 24)
            );
        }
    }
}
#[test]
fn input_progress_and_long_text_render_in_bounds() {
    let font = default_font().unwrap();
    let mut editor = crate::activity::Editor::default();
    editor.insert(&"é🪷".repeat(1000)).unwrap();
    editor.home(false);
    editor.right(true);
    let mut pixels = vec![0; 320 * 240 * 4];
    render(&mut pixels, 320, 240, &font, "Notice.", 36.0, 1.0);
    render_input(
        &mut pixels,
        320,
        240,
        &font,
        1.0,
        InputView {
            editor: &editor,
            suggestions: &[],
            suggestion_index: 0,
            saved_text: None,
            progress: Some(0.5),
            message: None,
            editing: true,
            caret_visible: true,
            history: None,
        },
    );
    let field = input_rect(320.0, 240.0, 1.0);
    let y = (field.y + field.height) as usize;
    let x = (field.x + field.width * 0.25) as usize;
    assert_eq!(
        &pixels[(y * 320 + x) * 4..(y * 320 + x) * 4 + 4],
        &premultiply([145, 181, 160, 255])
    );
    let x = (field.x + field.width * 0.75) as usize;
    assert_eq!(
        &pixels[(y * 320 + x) * 4..(y * 320 + x) * 4 + 4],
        &premultiply([74, 84, 82, 235])
    );
    for pixel in pixels.chunks_exact(4) {
        let pixel = u32::from_ne_bytes(pixel.try_into().unwrap());
        assert!(
            [0, 8, 16]
                .into_iter()
                .all(|shift| (pixel >> shift) & 255 <= pixel >> 24)
        );
    }
}

#[test]
fn premultiplied_background_is_valid() {
    let p = u32::from_ne_bytes(premultiply(BACKGROUND));
    let a = p >> 24;
    assert_eq!(a, 225);
    for shift in [16, 8, 0] {
        assert!(((p >> shift) & 255) <= a);
    }
}
#[test]
fn hard_line_breaks_and_blank_paragraph_lines_are_preserved() {
    let font = default_font().expect("a test font must be installed");
    assert_eq!(
        wrap(&font, "Notice.\nTake a breath.\n\nReturn.", 36.0, 1100.0),
        ["Notice.", "Take a breath.", "", "Return."]
    );
    assert_eq!(
        wrap(&font, "\nNotice.\n", 36.0, 1100.0),
        ["", "Notice.", ""]
    );
}

#[test]
fn multiline_text_still_wraps_each_hard_line_to_the_width() {
    let font = default_font().expect("a test font must be installed");
    let long_word = "m".repeat(100);
    let lines = wrap(&font, &format!("{long_word}\n\nReturn."), 36.0, 220.0);
    assert!(lines.len() > 3);
    assert!(
        lines
            .iter()
            .all(|line| text_width(&font, line, 36.0) <= 220.0)
    );
    assert_eq!(lines[..lines.len() - 2].concat(), long_word);
    assert_eq!(&lines[lines.len() - 2..], ["", "Return."]);
}

#[test]
fn unbroken_words_wrap_and_render_stays_in_bounds() {
    let font = default_font().expect("a test font must be installed");
    let quote = "m".repeat(500);
    let lines = wrap(&font, &quote, 36.0, 300.0);
    assert!(lines.len() > 1);
    assert!(
        lines
            .iter()
            .all(|line| text_width(&font, line, 36.0) <= 300.0)
    );
    assert_eq!(lines.concat(), quote);
    let mut pixels = vec![0; 320 * 240 * 4];
    render(&mut pixels, 320, 240, &font, &quote, 36.0, 1.0);
    for p in pixels.chunks_exact(4) {
        let p = u32::from_ne_bytes(p.try_into().unwrap());
        assert!(
            [0, 8, 16]
                .into_iter()
                .all(|shift| (p >> shift) & 255 <= p >> 24)
        );
    }
}
