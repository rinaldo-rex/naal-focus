use fontdue::Font;

pub(super) fn text_width(font: &Font, text: &str, size: f32) -> f32 {
    text.chars()
        .map(|c| font.metrics(c, size).advance_width)
        .sum()
}
pub(super) fn wrap(font: &Font, text: &str, size: f32, max_width: f32) -> Vec<String> {
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
pub(super) fn shortened(font: &Font, text: &str, size: f32, max_width: f32) -> String {
    if text_width(font, text, size) <= max_width {
        return text.to_owned();
    }
    let mut result = String::new();
    let mut used = text_width(font, "…", size);
    for c in text.chars() {
        used += font.metrics(c, size).advance_width;
        if used > max_width {
            break;
        }
        result.push(c);
    }
    result.push('…');
    result
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
