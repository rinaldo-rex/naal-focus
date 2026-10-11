use fontdue::Font;

const BACKGROUND: [u8; 4] = [20, 23, 29, 225];

/// Software ARGB8888 renderer; dimensions and text size are physical pixels.
pub fn render(
    canvas: &mut [u8],
    width: usize,
    height: usize,
    font: &Font,
    quote: &str,
    text_size: f32,
    scale: f32,
) {
    let bg = premultiply(BACKGROUND);
    for pixel in canvas.chunks_exact_mut(4) {
        pixel.copy_from_slice(&bg);
    }
    let max_width = (width as f32 * 0.8).min(1100.0 * scale);
    // Leave a quiet input area below the quote; never paint quote glyphs into it.
    let quote_height = (height as f32 - 150.0 * scale).max(0.0);
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
            x: 0.0,
            y: 0.0,
            width: width as f32,
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

fn premultiply([r, g, b, a]: [u8; 4]) -> [u8; 4] {
    ((u32::from(a) << 24)
        | ((u32::from(r) * u32::from(a) / 255) << 16)
        | ((u32::from(g) * u32::from(a) / 255) << 8)
        | (u32::from(b) * u32::from(a) / 255))
        .to_ne_bytes()
}
fn text_width(font: &Font, text: &str, size: f32) -> f32 {
    text.chars()
        .map(|c| font.metrics(c, size).advance_width)
        .sum()
}
fn wrap(font: &Font, text: &str, size: f32, max_width: f32) -> Vec<String> {
    text.split('\n')
        .flat_map(|line| wrap_line(font, line, size, max_width))
        .collect()
}

fn wrap_line(font: &Font, text: &str, size: f32, max_width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if !line.is_empty() && text_width(font, &candidate, size) > max_width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        for c in word.chars() {
            if !line.is_empty()
                && text_width(font, &line, size) + font.metrics(c, size).advance_width > max_width
            {
                lines.push(std::mem::take(&mut line));
            }
            line.push(c);
        }
    }
    if !line.is_empty() || lines.is_empty() {
        // A blank hard line still occupies one line of paragraph spacing.
        lines.push(line);
    }
    lines
}
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
impl Rect {
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}
struct Image<'a> {
    canvas: &'a mut [u8],
    width: usize,
    height: usize,
    clip: Rect,
}
impl Image<'_> {
    fn text(&mut self, font: &Font, text: &str, size: f32, top: f32, rgb: [u8; 3]) {
        let left = (self.width as f32 - text_width(font, text, size)) / 2.0;
        self.text_at(font, text, size, left, top, rgb);
    }
    fn text_at(
        &mut self,
        font: &Font,
        text: &str,
        size: f32,
        mut pen_x: f32,
        top: f32,
        rgb: [u8; 3],
    ) {
        let baseline = top + size;
        for c in text.chars() {
            let (metrics, bitmap) = font.rasterize(c, size);
            let left = pen_x.round() as i32 + metrics.xmin;
            let top = baseline.round() as i32 - metrics.ymin - metrics.height as i32;
            for y in 0..metrics.height {
                for x in 0..metrics.width {
                    let px = left + x as i32;
                    let py = top + y as i32;
                    if px < 0
                        || py < 0
                        || px >= self.width as i32
                        || py >= self.height as i32
                        || !self.clip.contains(px as f32, py as f32)
                    {
                        continue;
                    }
                    let alpha = u32::from(bitmap[y * metrics.width + x]);
                    let offset = (py as usize * self.width + px as usize) * 4;
                    let dest = &mut self.canvas[offset..offset + 4];
                    let previous = u32::from_ne_bytes(dest.try_into().unwrap());
                    let mut color = (alpha + ((previous >> 24) * (255 - alpha) / 255)) << 24;
                    for (channel, shift) in rgb.into_iter().zip([16, 8, 0]) {
                        let old = (previous >> shift) & 255;
                        color |=
                            (u32::from(channel) * alpha / 255 + old * (255 - alpha) / 255) << shift;
                    }
                    dest.copy_from_slice(&color.to_ne_bytes());
                }
            }
            pen_x += metrics.advance_width;
        }
    }
    fn rect(&mut self, rect: Rect, color: [u8; 4]) {
        let pixel = premultiply(color);
        let left = rect.x.floor().max(self.clip.x.ceil()).max(0.0) as usize;
        let top = rect.y.floor().max(self.clip.y.ceil()).max(0.0) as usize;
        let right = (rect.x + rect.width)
            .ceil()
            .min((self.clip.x + self.clip.width).ceil())
            .max(0.0) as usize;
        let bottom = (rect.y + rect.height)
            .ceil()
            .min((self.clip.y + self.clip.height).ceil())
            .max(0.0) as usize;
        let (right, bottom) = (right.min(self.width), bottom.min(self.height));
        if right <= left || bottom <= top {
            return;
        }
        for y in top..bottom {
            for dest in self.canvas[(y * self.width + left) * 4..(y * self.width + right) * 4]
                .chunks_exact_mut(4)
            {
                dest.copy_from_slice(&pixel);
            }
        }
    }
}

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
    Rect {
        x: 0.0,
        y: field.y,
        width,
        height: (field.height + 44.0 * scale).min((height - field.y).max(0.0)),
    }
}

pub struct InputView<'a> {
    pub editor: &'a crate::activity::Editor,
    pub saved_text: Option<&'a str>,
    pub progress: Option<f32>,
    pub message: Option<&'a str>,
    pub editing: bool,
    pub caret_visible: bool,
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
    let field = input_rect(width as f32, height as f32, scale);
    let mut image = Image {
        canvas,
        width,
        height,
        clip: Rect {
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: height as f32,
        },
    };
    // Clear old hints/caret/text without touching the cached quote pixels.
    image.rect(input_damage(width as f32, height as f32, scale), BACKGROUND);
    let full_clip = image.clip;
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
            "Saved · click your reflection to edit · Esc dismisses",
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
    let hint = view.message.unwrap_or(if view.editing {
        "Enter saves · Esc discards · countdown resumes"
    } else {
        "Type to reflect, or just breathe · Esc dismisses"
    });
    let hint_size = 12.0 * scale;
    // Bound error/hint text to the output too; no submitted activity is echoed here.
    let mut hint_end = hint.len();
    while hint_end > 0 && text_width(font, &hint[..hint_end], hint_size) > width as f32 * 0.9 {
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

pub fn default_font() -> Result<Font, String> {
    for path in [
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    ] {
        if let Ok(bytes) = std::fs::read(path)
            && let Ok(font) = Font::from_bytes(bytes, fontdue::FontSettings::default())
        {
            return Ok(font);
        }
    }
    Err("Install fonts-liberation, fonts-dejavu-core, or fonts-noto-core; no supported sans-serif font was found.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
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
                saved_text: None,
                progress: Some(0.5),
                message: Some("Previous save error"),
                editing: true,
                caret_visible: true,
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
                saved_text: Some("Reading docs"),
                progress: None,
                message: None,
                editing: false,
                caret_visible: false,
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
                saved_text: None,
                progress: Some(0.2),
                message: None,
                editing: true,
                caret_visible: true,
            },
        );
        assert_eq!(&pixels[offset..offset + 4], &premultiply([31, 36, 40, 235]));
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
                saved_text: None,
                progress: Some(0.5),
                message: None,
                editing: true,
                caret_visible: true,
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
}
