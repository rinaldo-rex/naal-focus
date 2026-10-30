//! Quiet, noninteractive charts in a band above the existing reflection strip.
use super::{
    BACKGROUND,
    input::input_damage,
    primitives::{Image, Rect},
    text::{shortened, text_width},
};
use crate::{
    charts::{DailyCharts, Series},
    config::{ChartSettings, ChartStyle},
    metadata::Kind,
};
use fontdue::Font;

pub struct ChartView<'a> {
    pub settings: &'a ChartSettings,
    pub data: Option<&'a DailyCharts>,
    pub unavailable: bool,
}

pub fn chart_damage(width: f32, height: f32, scale: f32, settings: &ChartSettings) -> Option<Rect> {
    let count = settings.kinds().count();
    let gap = 24.0 * scale;
    let band_width = (width * 0.9).min(1180.0 * scale);
    let card_width = (band_width - gap * count.saturating_sub(1) as f32) / count.max(1) as f32;
    let bottom = input_damage(width, height, scale).y - 16.0 * scale;
    let band_height = (bottom - 120.0 * scale).min(174.0 * scale);
    // On unusually small desktop outputs, keep quote/input usable rather than
    // squeezing unreadable charts over them. Preferences remain unchanged.
    if count == 0 || card_width < 190.0 * scale || band_height < 130.0 * scale {
        return None;
    }
    Some(Rect {
        x: (width - band_width) / 2.0,
        y: bottom - band_height,
        width: band_width,
        height: band_height,
    })
}

pub fn chart_damage_in(content: Rect, scale: f32, settings: &ChartSettings) -> Option<Rect> {
    let mut band = chart_damage(content.width, content.height, scale, settings)?;
    band.x += content.x;
    Some(band)
}

pub fn render_charts(
    canvas: &mut [u8],
    size: (usize, usize),
    font: &Font,
    scale: f32,
    view: ChartView<'_>,
) {
    render_charts_in(
        canvas,
        size,
        font,
        scale,
        Rect {
            x: 0.0,
            y: 0.0,
            width: size.0 as f32,
            height: size.1 as f32,
        },
        view,
    );
}

pub fn render_charts_in(
    canvas: &mut [u8],
    size: (usize, usize),
    font: &Font,
    scale: f32,
    content: Rect,
    view: ChartView<'_>,
) {
    let (width, height) = size;
    let Some(band) = chart_damage_in(content, scale, view.settings) else {
        return;
    };
    let mut image = Image {
        canvas,
        width,
        height,
        clip: band,
    };
    image.rect(band, BACKGROUND);
    let count = view.settings.kinds().count();
    let gap = 24.0 * scale;
    let card_width = (band.width - gap * count.saturating_sub(1) as f32) / count as f32;
    for (index, kind) in view.settings.kinds().enumerate() {
        let card = Rect {
            x: band.x + index as f32 * (card_width + gap),
            y: band.y,
            width: card_width,
            height: band.height,
        };
        image.clip = card;
        let title = match kind {
            Kind::Project => "Projects",
            Kind::Feeling => "Feelings",
            Kind::Tag => "Tags",
        };
        image.text_at(font, title, 12.0 * scale, card.x, card.y, [180, 195, 207]);
        let today = "Today";
        image.text_at(
            font,
            today,
            10.0 * scale,
            card.x + card.width - text_width(font, today, 10.0 * scale),
            card.y + 2.0 * scale,
            [115, 132, 147],
        );
        let rows = view.data.map(|data| data.rows(kind, 4)).unwrap_or_default();
        let message = if view.unavailable {
            Some("History unavailable")
        } else if view.data.is_none() {
            Some("Loading today’s mentions…")
        } else if rows.is_empty() {
            Some("No mentions today")
        } else {
            None
        };
        if let Some(message) = message {
            image.text_at(
                font,
                message,
                12.0 * scale,
                card.x,
                card.y + 52.0 * scale,
                [129, 146, 158],
            );
            continue;
        }
        let body = Rect {
            y: card.y + 25.0 * scale,
            height: card.height - 45.0 * scale,
            ..card
        };
        match view.settings.style {
            ChartStyle::Heatmap => heatmap(
                &mut image,
                font,
                body,
                &rows,
                view.data.unwrap().hour,
                scale,
            ),
            ChartStyle::Treemap => treemap(&mut image, font, body, &rows, scale),
        }
        let caption = match view.settings.style {
            ChartStyle::Heatmap => {
                let max = rows.iter().flat_map(|row| row.hours).max().unwrap_or(0);
                format!("0–{max} mentions/hour · future hours blank")
            }
            ChartStyle::Treemap => "Area = mentions · not time spent".to_owned(),
        };
        let caption = shortened(font, &caption, 9.0 * scale, card.width);
        image.text_at(
            font,
            &caption,
            9.0 * scale,
            card.x,
            card.y + card.height - 12.0 * scale,
            [111, 129, 145],
        );
    }
}

fn heatmap(image: &mut Image<'_>, font: &Font, body: Rect, rows: &[Series], hour: u32, scale: f32) {
    let label_width = (body.width * 0.32).min(108.0 * scale);
    let grid_x = body.x + label_width;
    let cell_width = (body.width - label_width) / 24.0;
    let row_height = ((body.height - 19.0 * scale) / 4.0).min(25.0 * scale);
    let max = rows
        .iter()
        .flat_map(|row| row.hours)
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    for (index, row) in rows.iter().enumerate() {
        let y = body.y + index as f32 * row_height;
        let label = shortened(font, &row.label, 11.0 * scale, label_width - 7.0 * scale);
        image.text_at(
            font,
            &label,
            11.0 * scale,
            body.x,
            y + 1.0 * scale,
            [177, 191, 203],
        );
        for (hour_index, &count) in row.hours.iter().enumerate() {
            let color = if hour_index > hour as usize {
                [23, 27, 34, 225]
            } else if count == 0 {
                [35, 42, 49, 225]
            } else {
                let strength = 0.3 + 0.7 * count as f32 / max;
                [
                    (43.0 + 112.0 * strength) as u8,
                    (59.0 + 133.0 * strength) as u8,
                    (62.0 + 112.0 * strength) as u8,
                    225,
                ]
            };
            image.rect(
                Rect {
                    x: grid_x + hour_index as f32 * cell_width,
                    y,
                    width: (cell_width - scale).max(scale),
                    height: (row_height - 7.0 * scale).max(scale),
                },
                color,
            );
        }
    }
    let axis_y = body.y + row_height * 4.0;
    for hour in [0, 6, 12, 18] {
        image.text_at(
            font,
            &format!("{hour:02}"),
            9.0 * scale,
            grid_x + hour as f32 * cell_width,
            axis_y,
            [113, 132, 148],
        );
    }
}

/// Balanced binary partition; all count-weighted area is retained, with only
/// visual gutters applied later. Input contains at most four nonzero values.
fn tile_rects(weights: &[u64], area: Rect) -> Vec<Rect> {
    partition_tiles(weights, area, area.width >= area.height)
}

fn partition_tiles(weights: &[u64], area: Rect, horizontal: bool) -> Vec<Rect> {
    if weights.len() <= 1 {
        return weights.first().map(|_| vec![area]).unwrap_or_default();
    }
    let total: u64 = weights.iter().sum();
    let mut partial = 0;
    let split = (1..weights.len())
        .min_by_key(|&i| {
            // Four rows: tiny bounded work, no dependence on history length.
            weights[..i].iter().sum::<u64>().abs_diff(total / 2)
        })
        .unwrap();
    for value in &weights[..split] {
        partial += value;
    }
    let fraction = partial as f32 / total.max(1) as f32;
    // Alternate cuts to retain the nested-tile character of the approved
    // treemap, rather than degenerating into side-by-side bars on wide cards.
    let (first, second) = if horizontal {
        let width = area.width * fraction;
        (
            Rect { width, ..area },
            Rect {
                x: area.x + width,
                width: area.width - width,
                ..area
            },
        )
    } else {
        let height = area.height * fraction;
        (
            Rect { height, ..area },
            Rect {
                y: area.y + height,
                height: area.height - height,
                ..area
            },
        )
    };
    let mut result = partition_tiles(&weights[..split], first, !horizontal);
    result.extend(partition_tiles(&weights[split..], second, !horizontal));
    result
}

fn treemap(image: &mut Image<'_>, font: &Font, body: Rect, rows: &[Series], scale: f32) {
    let weights: Vec<_> = rows.iter().map(Series::total).collect();
    let colors = [
        [153, 194, 171],
        [150, 177, 205],
        [198, 170, 141],
        [163, 157, 186],
    ];
    for ((row, tile), color) in rows.iter().zip(tile_rects(&weights, body)).zip(colors) {
        let tile = Rect {
            x: tile.x + scale,
            y: tile.y + scale,
            width: (tile.width - 2.0 * scale).max(0.0),
            height: (tile.height - 2.0 * scale).max(0.0),
        };
        image.rect(tile, [color[0] / 3, color[1] / 3, color[2] / 3, 225]);
        let original_clip = image.clip;
        image.clip = tile;
        let label = shortened(
            font,
            &row.label,
            12.0 * scale,
            (tile.width - 16.0 * scale).max(0.0),
        );
        let y = tile.y + ((tile.height - 33.0 * scale) / 2.0).max(2.0 * scale);
        if tile.width >= 38.0 * scale && tile.height >= 18.0 * scale {
            image.text_at(font, &label, 12.0 * scale, tile.x + 8.0 * scale, y, color);
        }
        if tile.width >= 38.0 * scale && tile.height >= 38.0 * scale {
            let count = shortened(
                font,
                &format!("{} mentions", row.total()),
                10.0 * scale,
                (tile.width - 16.0 * scale).max(0.0),
            );
            image.text_at(
                font,
                &count,
                10.0 * scale,
                tile.x + 8.0 * scale,
                y + 19.0 * scale,
                [165, 182, 195],
            );
        }
        image.clip = original_clip;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{activity_log::Entry, time::Local};
    fn settings(style: ChartStyle) -> ChartSettings {
        ChartSettings {
            style,
            projects: true,
            feelings: true,
            tags: true,
        }
    }
    fn local(_: u64) -> Local {
        Local {
            year: 2026,
            month: 10,
            day: 15,
            hour: 16,
            minute: 0,
            second: 0,
        }
    }
    #[test]
    fn treemap_partitions_preserve_area_and_proportions() {
        let area = Rect {
            x: 2.0,
            y: 3.0,
            width: 300.0,
            height: 120.0,
        };
        let nested = tile_rects(&[8, 5, 3], area);
        assert_eq!(nested[1].x, nested[2].x);
        assert!(nested[1].y < nested[2].y);
        for weights in [vec![100], vec![8, 5, 3], vec![1000, 1, 1, 1]] {
            let tiles = tile_rects(&weights, area);
            let total: u64 = weights.iter().sum();
            assert_eq!(tiles.len(), weights.len());
            for (tile, count) in tiles.iter().zip(&weights) {
                assert!(
                    (tile.width * tile.height / (area.width * area.height)
                        - *count as f32 / total as f32)
                        .abs()
                        < 0.0001
                );
                assert!(tile.x >= area.x && tile.y >= area.y);
                assert!(tile.x + tile.width <= area.x + area.width + 0.001);
                assert!(tile.y + tile.height <= area.y + area.height + 0.001);
            }
        }
    }
    #[test]
    fn native_charts_clip_cache_and_scale_across_desktop_outputs() {
        let font = super::super::default_font().unwrap();
        let entries: Vec<_> = (0..7)
            .map(|i| Entry {
                version: 1,
                id: i.to_string(),
                timestamp_unix_ms: i,
                text: format!(
                    "@project_{i} ??feeling_{i} #tag_{}",
                    "long_value_".repeat(30)
                ),
                metadata: None,
            })
            .collect();
        let data = DailyCharts::from_entries(&entries, 100, local);
        let empty = DailyCharts::from_entries(&[], 100, local);
        for (width, height, scale) in [
            (800, 600, 1.0),
            (1000, 650, 1.0),
            (1920, 1080, 1.0),
            (2560, 1080, 1.0),
            (1080, 1920, 1.0),
            (3414, 1920, 2.0),
        ] {
            for style in [ChartStyle::Heatmap, ChartStyle::Treemap] {
                let settings = settings(style);
                let band = chart_damage(width as f32, height as f32, scale, &settings).unwrap();
                assert!(band.y + band.height < input_damage(width as f32, height as f32, scale).y);
                let mut pixels = vec![0; width * height * 4];
                super::super::test_helpers::render_with_charts(
                    &mut pixels,
                    (width, height),
                    &font,
                    "Take a breath.",
                    36.0 * scale,
                    scale,
                    &settings,
                );
                let original = pixels.clone();
                for (data, unavailable) in [
                    (None, false),
                    (Some(&data), false),
                    (Some(&empty), false),
                    (Some(&data), true),
                ] {
                    render_charts(
                        &mut pixels,
                        (width, height),
                        &font,
                        scale,
                        ChartView {
                            settings: &settings,
                            data,
                            unavailable,
                        },
                    );
                    let start = band.y.floor() as usize * width * 4;
                    let end = (band.y + band.height).ceil() as usize * width * 4;
                    assert_eq!(&pixels[..start], &original[..start]);
                    assert_eq!(&pixels[end..], &original[end..]);
                    assert_ne!(&pixels[start..end], &original[start..end]);
                }
                for pixel in pixels.chunks_exact(4) {
                    let color = u32::from_ne_bytes(pixel.try_into().unwrap());
                    assert!(
                        [0, 8, 16]
                            .into_iter()
                            .all(|shift| (color >> shift) & 255 <= color >> 24)
                    );
                }
            }
        }
        assert!(chart_damage(1920.0, 1080.0, 1.0, &ChartSettings::default()).is_none());
        assert!(chart_damage(640.0, 480.0, 1.0, &settings(ChartStyle::Heatmap)).is_none());
    }
    #[test]
    fn category_combinations_and_future_heatmap_cells_are_distinct() {
        for mask in 1..8 {
            let settings = ChartSettings {
                projects: mask & 1 != 0,
                feelings: mask & 2 != 0,
                tags: mask & 4 != 0,
                ..ChartSettings::default()
            };
            assert!(chart_damage(800.0, 600.0, 1.0, &settings).is_some());
        }
        let font = super::super::default_font().unwrap();
        let mut pixels = vec![0; 300 * 160 * 4];
        let body = Rect {
            x: 0.0,
            y: 0.0,
            width: 300.0,
            height: 130.0,
        };
        let mut image = Image {
            canvas: &mut pixels,
            width: 300,
            height: 160,
            clip: body,
        };
        let mut hours = [0; 24];
        hours[8] = 1;
        hours[9] = 2;
        heatmap(
            &mut image,
            &font,
            body,
            &[Series {
                label: "@naal".into(),
                hours,
            }],
            10,
            1.0,
        );
        let cell = |hour: usize| {
            let x = (96.0 + (hour as f32 + 0.5) * 204.0 / 24.0) as usize;
            pixels[(3 * 300 + x) * 4..(3 * 300 + x) * 4 + 4].to_vec()
        };
        assert_ne!(cell(7), cell(8));
        assert_ne!(cell(8), cell(9));
        assert_ne!(cell(7), cell(11));
    }

    #[test]
    fn disabled_charts_preserve_the_existing_render_exactly() {
        let font = super::super::default_font().unwrap();
        let mut before = vec![0; 800 * 600 * 4];
        super::super::test_helpers::render(&mut before, 800, 600, &font, "Notice.", 36.0, 1.0);
        let mut after = before.clone();
        render_charts(
            &mut after,
            (800, 600),
            &font,
            1.0,
            ChartView {
                settings: &ChartSettings::default(),
                data: None,
                unavailable: false,
            },
        );
        assert_eq!(before, after);
    }
}
