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
    let mut size = text_size;
    let mut lines = wrap(font, quote, size, max_width);
    // Fit bounded long quotes vertically; wrapping also splits unbroken words.
    while lines.len() as f32 * size * 1.4 > height as f32 * 0.55 && size > 16.0 * scale {
        size -= scale;
        lines = wrap(font, quote, size, max_width);
    }
    let line_height = size * 1.4;
    let total_height = lines.len() as f32 * line_height;
    let top = (height as f32 - total_height) / 2.0;
    let mut image = Image {
        canvas,
        width,
        height,
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
    if top + total_height + 80.0 * scale < height as f32 {
        image.text(
            font,
            "Notice. Take a breath. Return.",
            17.0 * scale,
            top + total_height + 35.0 * scale,
            [173, 191, 182],
        );
    }
    image.text(
        font,
        "Esc or click to dismiss all screens · closes automatically",
        12.0 * scale,
        height as f32 - 45.0 * scale,
        [150, 160, 157],
    );
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
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}
struct Image<'a> {
    canvas: &'a mut [u8],
    width: usize,
    height: usize,
}
impl Image<'_> {
    fn text(&mut self, font: &Font, text: &str, size: f32, top: f32, rgb: [u8; 3]) {
        let mut pen_x = (self.width as f32 - text_width(font, text, size)) / 2.0;
        let baseline = top + size;
        for c in text.chars() {
            let (metrics, bitmap) = font.rasterize(c, size);
            let left = pen_x.round() as i32 + metrics.xmin;
            let top = baseline.round() as i32 - metrics.ymin - metrics.height as i32;
            for y in 0..metrics.height {
                for x in 0..metrics.width {
                    let px = left + x as i32;
                    let py = top + y as i32;
                    if px < 0 || py < 0 || px >= self.width as i32 || py >= self.height as i32 {
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
    fn premultiplied_background_is_valid() {
        let p = u32::from_ne_bytes(premultiply(BACKGROUND));
        let a = p >> 24;
        assert_eq!(a, 225);
        for shift in [16, 8, 0] {
            assert!(((p >> shift) & 255) <= a);
        }
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
