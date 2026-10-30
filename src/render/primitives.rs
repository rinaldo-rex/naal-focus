use super::text::text_width;
use crate::breathing::{Orb, Point};
use fontdue::Font;

pub(super) fn premultiply([r, g, b, a]: [u8; 4]) -> [u8; 4] {
    ((u32::from(a) << 24)
        | ((u32::from(r) * u32::from(a) / 255) << 16)
        | ((u32::from(g) * u32::from(a) / 255) << 8)
        | (u32::from(b) * u32::from(a) / 255))
        .to_ne_bytes()
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
pub(super) struct Image<'a> {
    pub(super) canvas: &'a mut [u8],
    pub(super) width: usize,
    pub(super) height: usize,
    pub(super) clip: Rect,
}
impl Image<'_> {
    pub(super) fn text(&mut self, font: &Font, text: &str, size: f32, top: f32, rgb: [u8; 3]) {
        let left = self.clip.x + (self.clip.width - text_width(font, text, size)) / 2.0;
        self.text_at(font, text, size, left, top, rgb);
    }
    pub(super) fn text_at(
        &mut self,
        font: &Font,
        text: &str,
        size: f32,
        pen_x: f32,
        top: f32,
        rgb: [u8; 3],
    ) {
        self.text_at_opacity(font, text, size, (pen_x, top), rgb, 1.0);
    }
    pub(super) fn text_at_opacity(
        &mut self,
        font: &Font,
        text: &str,
        size: f32,
        position: (f32, f32),
        rgb: [u8; 3],
        opacity: f32,
    ) {
        let (mut pen_x, top) = position;
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
                    let alpha = (f32::from(bitmap[y * metrics.width + x]) * opacity.clamp(0.0, 1.0))
                        .round() as u32;
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
    pub(super) fn rect(&mut self, rect: Rect, color: [u8; 4]) {
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

fn source_over(destination: u32, source: u32) -> u32 {
    let inverse = 255 - (source >> 24);
    // Two separated byte channels fit in each product. This is exact floor / 255.
    let attenuate = |pair: u32| {
        let product = pair * inverse;
        ((product + 0x0001_0001 + ((product >> 8) & 0x00ff_00ff)) >> 8) & 0x00ff_00ff
    };
    source
        + attenuate(destination & 0x00ff_00ff)
        + (attenuate((destination >> 8) & 0x00ff_00ff) << 8)
}
impl Image<'_> {
    fn blend(&mut self, x: i32, y: i32, rgb: [u8; 3], opacity: f32) {
        if x < 0
            || y < 0
            || x >= self.width as i32
            || y >= self.height as i32
            || !self.clip.contains(x as f32, y as f32)
        {
            return;
        }
        let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u32;
        if alpha == 0 {
            return;
        }
        let offset = (y as usize * self.width + x as usize) * 4;
        let dest = &mut self.canvas[offset..offset + 4];
        let old = u32::from_ne_bytes(dest.try_into().unwrap());
        let mut color = (alpha + (old >> 24) * (255 - alpha) / 255) << 24;
        for (channel, shift) in rgb.into_iter().zip([16, 8, 0]) {
            color |= (u32::from(channel) * alpha / 255
                + ((old >> shift) & 255) * (255 - alpha) / 255)
                << shift;
        }
        dest.copy_from_slice(&color.to_ne_bytes());
    }
    pub(super) fn glow(&mut self, center: Point, radius: f32, rgb: [u8; 3], opacity: f32) {
        if radius <= 0.0 || opacity <= 0.0 {
            return;
        }
        // Cache radial falloff and premultiplication; large 2× glows otherwise
        // repeat a square root and four channel conversions for every pixel.
        let samples: [u32; 1025] = std::array::from_fn(|i| {
            let distance = (i as f32 / 1024.0).sqrt();
            let falloff = if distance < 0.38 {
                1.0 - distance / 0.38 * 0.68
            } else {
                (1.0 - distance) / 0.62 * 0.32
            };
            let alpha = (opacity * falloff * 255.0).clamp(0.0, 255.0).round() as u32;
            (alpha << 24)
                | ((u32::from(rgb[0]) * alpha / 255) << 16)
                | ((u32::from(rgb[1]) * alpha / 255) << 8)
                | (u32::from(rgb[2]) * alpha / 255)
        });
        let left = (center.0 - radius).max(self.clip.x).max(0.0).ceil() as usize;
        let right = (center.0 + radius)
            .min(self.clip.x + self.clip.width)
            .min(self.width as f32)
            .ceil()
            .max(0.0) as usize;
        let top = (center.1 - radius).max(self.clip.y).max(0.0).ceil() as usize;
        let bottom = (center.1 + radius)
            .min(self.clip.y + self.clip.height)
            .min(self.height as f32)
            .ceil()
            .max(0.0) as usize;
        if left >= right || top >= bottom {
            return;
        }
        let factor = 1024.0 / (radius * radius);
        for y in top..bottom {
            let dy = y as f32 - center.1;
            let offset = (y * self.width + left) * 4;
            let row = &mut self.canvas[offset..offset + (right - left) * 4];
            for (column, pixel) in row.chunks_exact_mut(4).enumerate() {
                let dx = (left + column) as f32 - center.0;
                let index = ((dx * dx + dy * dy) * factor).round() as usize;
                let source = samples[index.min(1024)];
                if source != 0 {
                    let old = u32::from_ne_bytes(pixel.try_into().unwrap());
                    pixel.copy_from_slice(&source_over(old, source).to_ne_bytes());
                }
            }
        }
    }
    pub(super) fn orb(&mut self, center: Point, radius: f32, orb: &Orb, opacity: f32) {
        if radius <= 0.0 || opacity <= 0.0 {
            return;
        }
        let stops = [
            (0.0, orb.palette[1], 0.94),
            (0.42, orb.palette[0], 0.65),
            (0.82, orb.palette[2], 0.22),
            (1.0, orb.palette[2], 0.03),
        ];
        for y in (center.1 - radius - 1.0).floor() as i32..=(center.1 + radius + 1.0).ceil() as i32
        {
            for x in
                (center.0 - radius - 1.0).floor() as i32..=(center.0 + radius + 1.0).ceil() as i32
            {
                let dx = x as f32 - center.0;
                let dy = y as f32 - center.1;
                let coverage = (radius + 0.5 - dx.hypot(dy)).clamp(0.0, 1.0);
                if coverage == 0.0 {
                    continue;
                }
                let distance = ((dx / radius + 0.33).hypot(dy / radius + 0.4) / 1.4).min(1.0);
                let pair = stops.windows(2).find(|pair| distance <= pair[1].0).unwrap();
                let amount = (distance - pair[0].0) / (pair[1].0 - pair[0].0);
                let color = std::array::from_fn(|i| {
                    (f32::from(pair[0].1[i]) * (1.0 - amount) + f32::from(pair[1].1[i]) * amount)
                        .round() as u8
                });
                let alpha = pair[0].2 * (1.0 - amount) + pair[1].2 * amount;
                self.blend(x, y, color, alpha * opacity * coverage);
            }
        }
    }
    pub(super) fn disc(&mut self, center: Point, radius: f32, rgb: [u8; 3], opacity: f32) {
        let extent = radius + 1.0;
        for y in (center.1 - extent).floor() as i32..=(center.1 + extent).ceil() as i32 {
            for x in (center.0 - extent).floor() as i32..=(center.0 + extent).ceil() as i32 {
                let coverage = (radius + 0.5 - (x as f32 - center.0).hypot(y as f32 - center.1))
                    .clamp(0.0, 1.0);
                self.blend(x, y, rgb, opacity * coverage);
            }
        }
    }
    // Major-axis scan conversion keeps long diagonals O(length), not O(area).
    pub(super) fn line(
        &mut self,
        mut a: Point,
        mut b: Point,
        width: f32,
        rgb: [u8; 3],
        opacity: f32,
    ) {
        if opacity <= 0.0 {
            return;
        }
        let steep = (b.1 - a.1).abs() > (b.0 - a.0).abs();
        if steep {
            a = Point(a.1, a.0);
            b = Point(b.1, b.0);
        }
        if a.0 > b.0 {
            std::mem::swap(&mut a, &mut b);
        }
        let dx = b.0 - a.0;
        if dx < 0.001 {
            return;
        }
        let gradient = (b.1 - a.1) / dx;
        let half = width * 0.5 * (1.0 + gradient * gradient).sqrt();
        for x in a.0.floor() as i32..=b.0.ceil() as i32 {
            let cap = ((x as f32 + 0.5).min(b.0) - (x as f32 - 0.5).max(a.0)).clamp(0.0, 1.0);
            let center = a.1 + (x as f32 - a.0) * gradient;
            for y in (center - half - 0.5).floor() as i32..=(center + half + 0.5).ceil() as i32 {
                let coverage = (half + 0.5 - (y as f32 - center).abs()).clamp(0.0, 1.0) * cap;
                let (px, py) = if steep { (y, x) } else { (x, y) };
                self.blend(px, py, rgb, opacity * coverage);
            }
        }
    }
}

#[cfg(test)]
mod tests;
