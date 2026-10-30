use super::{
    BACKGROUND,
    primitives::{Image, Rect},
    text::{shortened, text_width},
};
use fontdue::Font;

pub fn input_rect(width: f32, height: f32, scale: f32) -> Rect {
    let field_width = (width * 0.7).min(640.0 * scale);
    Rect {
        x: (width - field_width) / 2.0,
        y: (height - 130.0 * scale).max(0.0),
        width: field_width,
        height: 36.0 * scale,
    }
}

/// Only this strip changes after each buffer's immutable quote was initialized.
pub fn input_damage(width: f32, height: f32, scale: f32) -> Rect {
    let field = input_rect(width, height, scale);
    let top = (field.y - 182.0 * scale).max(0.0);
    Rect {
        x: 0.0,
        y: top,
        width,
        height: (field.y + field.height + 44.0 * scale - top).min((height - top).max(0.0)),
    }
}

pub struct InputView<'a> {
    pub editor: &'a crate::activity::Editor,
    pub saved_text: Option<&'a str>,
    pub progress: Option<f32>,
    pub message: Option<&'a str>,
    pub editing: bool,
    pub caret_visible: bool,
    pub history: Option<HistoryView<'a>>,
    pub suggestions: &'a [String],
    pub suggestion_index: usize,
}

pub struct HistoryView<'a> {
    pub recall: &'a crate::recall::Recall,
    pub now: std::time::Duration,
    pub now_ms: u64,
}

fn render_history(
    image: &mut Image<'_>,
    font: &Font,
    field: Rect,
    scale: f32,
    view: &HistoryView<'_>,
) {
    let row_height = 44.0 * scale;
    let bottom = field.y - 50.0 * scale;
    let full_clip = image.clip;
    image.clip = Rect {
        x: field.x,
        y: (bottom - row_height * 3.0).max(0.0),
        width: field.width,
        height: row_height * 3.0,
    };
    let window = view.recall.window(view.now);
    let start = window.floor() as usize;
    let end = (window.ceil() as usize + 3).min(view.recall.groups.len());
    for index in start..end {
        let group = &view.recall.groups[index];
        let slot = 2.0 + window - index as f32;
        let top = bottom - row_height + (window - index as f32) * row_height;
        let opacities = crate::recall::ROW_OPACITY.map(|value| f32::from(value) / 100.0);
        let opacity = if slot <= 1.0 {
            opacities[0] + (opacities[1] - opacities[0]) * slot.clamp(0.0, 1.0)
        } else {
            opacities[1] + (opacities[2] - opacities[1]) * (slot - 1.0).clamp(0.0, 1.0)
        };
        let selected = view.recall.selected() == Some(index);
        let metadata = if selected {
            group.label(view.now_ms)
        } else {
            String::new()
        };
        let meta_width = text_width(font, &metadata, 12.0 * scale);
        let reserved = if selected {
            meta_width + 16.0 * scale
        } else {
            0.0
        };
        let text = shortened(
            font,
            &group.text,
            16.0 * scale,
            (field.width - 24.0 * scale - reserved).max(0.0),
        );
        image.text_at_opacity(
            font,
            &text,
            16.0 * scale,
            (field.x + 12.0 * scale, top + 10.0 * scale),
            if selected {
                [215, 234, 221]
            } else {
                [192, 201, 197]
            },
            opacity,
        );
        if selected {
            image.text_at_opacity(
                font,
                &metadata,
                12.0 * scale,
                (
                    field.x + field.width - 12.0 * scale - meta_width,
                    top + 13.0 * scale,
                ),
                [160, 170, 165],
                opacity,
            );
            image.rect(
                Rect {
                    x: field.x,
                    y: top + 14.0 * scale,
                    width: scale,
                    height: 16.0 * scale,
                },
                [127, 152, 140, 225],
            );
        }
    }
    image.clip = full_clip;
}

fn render_metadata(image: &mut Image<'_>, font: &Font, field: Rect, scale: f32, text: &str) {
    let labels = crate::metadata::Metadata::parse(text).labels();
    let original_clip = image.clip;
    image.clip = Rect {
        x: field.x,
        y: (field.y - 32.0 * scale).max(0.0),
        width: field.width,
        height: 26.0 * scale,
    };
    let mut left = field.x;
    for (index, label) in labels.iter().enumerate() {
        // Reserve room to report overflow rather than silently dropping pills.
        let remaining = field.x + field.width - left;
        let available = (remaining
            - if index + 1 < labels.len() {
                52.0 * scale
            } else {
                0.0
            })
        .max(0.0);
        let label = if available < 48.0 * scale {
            format!("+{}", labels.len() - index)
        } else {
            shortened(font, label, 12.0 * scale, available - 18.0 * scale)
        };
        let width = (text_width(font, &label, 12.0 * scale) + 18.0 * scale).min(remaining);
        let top = field.y - 30.0 * scale;
        // Rounded pill, painted in scanlines with the same clipping as text.
        let radius = 10.0 * scale;
        let height = 23.0 * scale;
        for row in 0..height.ceil() as usize {
            let y = row as f32 + 0.5;
            let dy = if y < radius {
                radius - y
            } else {
                (y - (height - radius)).max(0.0)
            };
            let inset = radius - (radius * radius - dy * dy).max(0.0).sqrt();
            image.rect(
                Rect {
                    x: left + inset,
                    y: top + row as f32,
                    width: (width - inset * 2.0).max(0.0),
                    height: 1.0,
                },
                [43, 54, 51, 240],
            );
        }
        let color = if label.starts_with('@') {
            [183, 202, 228]
        } else if label.starts_with("??") {
            [223, 194, 161]
        } else {
            [173, 207, 187]
        };
        image.text_at(
            font,
            &label,
            12.0 * scale,
            left + 9.0 * scale,
            top + 4.0 * scale,
            color,
        );
        left += width + 6.0 * scale;
        if label.starts_with('+') {
            break;
        }
    }
    image.clip = original_clip;
}

fn render_suggestions(
    image: &mut Image<'_>,
    font: &Font,
    field: Rect,
    scale: f32,
    values: &[String],
    selected: usize,
) {
    let original_clip = image.clip;
    let top = field.y - 48.0 * scale - values.len() as f32 * 26.0 * scale;
    image.clip = Rect {
        x: field.x,
        y: top.max(0.0),
        width: field.width,
        height: (field.y - 48.0 * scale - top.max(0.0)).max(0.0),
    };
    for (index, value) in values.iter().enumerate() {
        let y = top + index as f32 * 26.0 * scale;
        image.rect(
            Rect {
                x: field.x,
                y,
                width: field.width,
                height: 26.0 * scale,
            },
            if index == selected {
                [49, 65, 58, 245]
            } else {
                [29, 35, 38, 240]
            },
        );
        let label = shortened(font, value, 14.0 * scale, field.width - 24.0 * scale);
        image.text_at(
            font,
            &label,
            14.0 * scale,
            field.x + 12.0 * scale,
            y + 5.0 * scale,
            [207, 222, 214],
        );
    }
    image.clip = original_clip;
}

fn input_window(
    font: &Font,
    editor: &crate::activity::Editor,
    size: f32,
    width: f32,
) -> std::ops::Range<usize> {
    let text = editor.text();
    let mut start = editor.cursor();
    let mut before_cursor = 0.0;
    for (index, c) in text[..editor.cursor()].char_indices().rev() {
        before_cursor += font.metrics(c, size).advance_width;
        if before_cursor > (width - size).max(0.0) {
            break;
        }
        start = index;
    }
    let mut end = start;
    let mut used = 0.0;
    for c in text[start..].chars() {
        used += font.metrics(c, size).advance_width;
        if used > width {
            break;
        }
        end += c.len_utf8();
    }
    start..end
}

pub fn input_caret(
    font: &Font,
    editor: &crate::activity::Editor,
    size: (u32, u32),
    scale: f32,
    position: (f64, f64),
) -> Option<usize> {
    let field = input_rect(size.0 as f32 * scale, size.1 as f32 * scale, scale);
    let (x, y) = (position.0 as f32 * scale, position.1 as f32 * scale);
    if !field.contains(x, y) {
        return None;
    }
    let text_size = 16.0 * scale;
    let range = input_window(font, editor, text_size, field.width - 24.0 * scale);
    let mut pen = field.x + 12.0 * scale;
    for (i, c) in editor.text()[range.clone()].char_indices() {
        let advance = font.metrics(c, text_size).advance_width;
        if x < pen + advance / 2.0 {
            return Some(range.start + i);
        }
        pen += advance;
    }
    Some(range.end)
}

pub fn render_input(
    canvas: &mut [u8],
    width: usize,
    height: usize,
    font: &Font,
    scale: f32,
    view: InputView<'_>,
) {
    render_input_in(
        canvas,
        (width, height),
        font,
        scale,
        Rect {
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: height as f32,
        },
        view,
    );
}

pub fn render_input_in(
    canvas: &mut [u8],
    size: (usize, usize),
    font: &Font,
    scale: f32,
    content: Rect,
    view: InputView<'_>,
) {
    let (width, height) = size;
    let mut field = input_rect(content.width, content.height, scale);
    field.x += content.x;
    let mut image = Image {
        canvas,
        width,
        height,
        clip: content,
    };
    // Clear only the content strip, never the adjacent focus sidebar.
    let mut damage = input_damage(content.width, content.height, scale);
    damage.x += content.x;
    image.rect(damage, BACKGROUND);
    let full_clip = image.clip;
    if !view.suggestions.is_empty() && view.saved_text.is_none() {
        render_suggestions(
            &mut image,
            font,
            field,
            scale,
            view.suggestions,
            view.suggestion_index,
        );
    } else if let Some(history) = &view.history {
        render_history(&mut image, font, field, scale, history);
    }
    render_metadata(
        &mut image,
        font,
        field,
        scale,
        view.saved_text.unwrap_or(view.editor.text()),
    );
    if let Some(text) = view.saved_text {
        let size = 16.0 * scale;
        let mut visible = String::new();
        let mut used = 0.0;
        for c in text.chars() {
            used += font.metrics(c, size).advance_width;
            if used > field.width - 24.0 * scale - size {
                visible.push('…');
                break;
            }
            visible.push(c);
        }
        image.text(font, &visible, size, field.y + 8.0 * scale, [173, 191, 182]);
        image.text(
            font,
            "Saved · Enter or click to edit · Esc dismisses",
            12.0 * scale,
            field.y + field.height + 14.0 * scale,
            [150, 160, 157],
        );
        return;
    }
    image.rect(field, [31, 36, 40, 235]);
    let size = 16.0 * scale;
    let left = field.x + 12.0 * scale;
    let top = field.y + 8.0 * scale;
    image.clip = Rect {
        x: left,
        y: field.y,
        width: (field.width - 24.0 * scale).max(0.0),
        height: field.height,
    };
    let range = input_window(font, view.editor, size, image.clip.width);
    let text = view.editor.text();
    let selection = view.editor.selection();
    let selected_start = selection.start.max(range.start);
    let selected_end = selection.end.min(range.end);
    if selected_start < selected_end {
        let selection_x = left + text_width(font, &text[range.start..selected_start], size);
        image.rect(
            Rect {
                x: selection_x,
                y: top,
                width: text_width(font, &text[selected_start..selected_end], size),
                height: 21.0 * scale,
            },
            [62, 81, 74, 245],
        );
    }
    if text.is_empty() {
        image.text_at(
            font,
            "What have you been doing?",
            size,
            left,
            top,
            [139, 153, 149],
        );
    } else {
        image.text_at(font, &text[range.clone()], size, left, top, [207, 215, 212]);
    }
    if view.caret_visible {
        let cursor = view.editor.cursor().clamp(range.start, range.end);
        image.rect(
            Rect {
                x: left + text_width(font, &text[range.start..cursor], size),
                y: top,
                width: scale,
                height: 20.0 * scale,
            },
            [173, 191, 182, 255],
        );
    }
    image.clip = full_clip;
    let underline = Rect {
        x: field.x,
        y: field.y + field.height,
        width: field.width,
        height: scale,
    };
    image.rect(underline, [74, 84, 82, 235]);
    if let Some(progress) = view.progress {
        image.rect(
            Rect {
                width: field.width * progress.clamp(0.0, 1.0),
                height: 2.0 * scale,
                ..underline
            },
            [145, 181, 160, 255],
        );
    }
    let browsing = view
        .history
        .as_ref()
        .is_some_and(|history| history.recall.selected().is_some());
    let has_history = view
        .history
        .as_ref()
        .is_some_and(|history| !history.recall.groups.is_empty());
    let hint = view.message.unwrap_or(if browsing {
        "↑ older · ↓ newer / return to draft · Enter recalls · Esc returns"
    } else if !view.suggestions.is_empty() {
        "↑ / ↓ suggestions · Tab inserts · Enter saves · Esc closes suggestions"
    } else if view.editing && has_history {
        "Enter saves · ↑ browse today's reflections · Esc discards"
    } else if view.editing {
        "Enter saves · #tag @project ??feeling · Esc discards"
    } else if has_history {
        "↑ browse today's reflections · Type to reflect · Esc dismisses"
    } else {
        "Type to reflect · #tag @project ??feeling · Esc dismisses"
    });
    let hint_size = 12.0 * scale;
    // Bound error/hint text to the output too; no submitted activity is echoed here.
    let mut hint_end = hint.len();
    while hint_end > 0 && text_width(font, &hint[..hint_end], hint_size) > content.width * 0.9 {
        hint_end = hint[..hint_end]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i);
    }
    image.text(
        font,
        &hint[..hint_end],
        hint_size,
        field.y + field.height + 14.0 * scale,
        [150, 160, 157],
    );
}
