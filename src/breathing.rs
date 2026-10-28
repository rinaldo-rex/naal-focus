//! Seeded geometry and a bounded, monotonic entrance timeline. No per-frame randomness.
use serde::{Deserialize, Serialize};
use std::{f32::consts::TAU, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Lattice,
    Polygons,
    Constellation,
    Weave,
}
impl Family {
    pub const ALL: [Self; 4] = [
        Self::Lattice,
        Self::Polygons,
        Self::Constellation,
        Self::Weave,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lattice => "lattice",
            Self::Polygons => "polygons",
            Self::Constellation => "constellation",
            Self::Weave => "weave",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Lattice => "Radial lattice",
            Self::Polygons => "Nested polygons",
            Self::Constellation => "Constellation",
            Self::Weave => "Woven field",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pace {
    Short,
    #[default]
    Balanced,
    Even,
}
impl Pace {
    pub const ALL: [Self; 3] = [Self::Short, Self::Balanced, Self::Even];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Short => "short",
            Self::Balanced => "balanced",
            Self::Even => "even",
        }
    }
    pub fn seconds(self) -> (u64, u64) {
        match self {
            Self::Short => (3, 4),
            Self::Balanced => (4, 6),
            Self::Even => (5, 5),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub reduced_motion: bool,
    pub pace: Pace,
    pub cycles: u8,
    pub families: Vec<Family>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            reduced_motion: false,
            pace: Pace::Balanced,
            cycles: 1,
            families: Family::ALL.to_vec(),
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=2).contains(&self.cycles) {
            return Err("Choose one or two breaths.".into());
        }
        if self.families.is_empty() || self.families.len() > 4 {
            return Err("Choose at least one breathing pattern.".into());
        }
        if self
            .families
            .iter()
            .enumerate()
            .any(|(i, f)| self.families[..i].contains(f))
        {
            return Err("Breathing patterns must not be duplicated.".into());
        }
        Ok(())
    }
    pub fn duration(&self) -> Duration {
        let (inhale, exhale) = self.pace.seconds();
        Duration::from_secs(if self.enabled {
            4 + (inhale + exhale) * u64::from(self.cycles)
        } else {
            0
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point(pub f32, pub f32);
#[derive(Clone, Debug, PartialEq)]
enum Shape {
    Lattice {
        spokes: usize,
        rings: usize,
        twist: f32,
        skip: usize,
    },
    Polygons {
        sides: usize,
        layers: usize,
        twist: f32,
        step: f32,
    },
    Constellation {
        points: Vec<(Point, f32, f32)>,
        edges: Vec<(usize, usize)>,
    },
    Weave {
        count: usize,
        frequency: f32,
        bend: f32,
        phase: f32,
        tilt: f32,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    pub family: Family,
    pub color: [u8; 3],
    rotation: f32,
    direction: f32,
    shape: Shape,
}
struct Random(u64);
impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u32 << 24) as f32
    }
    fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.unit()
    }
    fn int(&mut self, low: usize, high: usize) -> usize {
        low + (self.unit() * (high - low + 1) as f32) as usize
    }
}
impl Pattern {
    pub fn generate(seed: u64, family: Family) -> Self {
        let mut rng = Random(seed ^ ((family as u64 + 1) * 0x123456789abcdef));
        let rotation = rng.unit() * TAU;
        let direction = if rng.unit() > 0.5 { 1.0 } else { -1.0 };
        let shape = match family {
            Family::Lattice => Shape::Lattice {
                spokes: rng.int(7, 14),
                rings: rng.int(3, 6),
                twist: rng.range(0.1, 0.6),
                skip: rng.int(1, 3),
            },
            Family::Polygons => Shape::Polygons {
                sides: rng.int(3, 8),
                layers: rng.int(5, 9),
                twist: rng.range(0.12, 0.5),
                step: rng.range(0.04, 0.14),
            },
            Family::Constellation => {
                let count = rng.int(28, 43);
                let points: Vec<_> = (0..count)
                    .map(|i| {
                        let angle = i as f32 * 2.399_963_1 + rng.unit() * 0.4;
                        let radius =
                            ((i as f32 + 0.5) / count as f32).sqrt() * rng.range(0.84, 1.13);
                        (
                            Point(angle.cos() * radius, angle.sin() * radius),
                            rng.unit() * TAU,
                            rng.range(0.7, 1.5),
                        )
                    })
                    .collect();
                let mut edges = Vec::new();
                for (i, (p, _, _)) in points.iter().enumerate() {
                    let mut neighbours: Vec<_> = points
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| *j != i)
                        .map(|(j, (q, _, _))| (j, (p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)))
                        .collect();
                    neighbours.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
                    for (j, _) in neighbours.into_iter().take(3) {
                        let edge = (i.min(j), i.max(j));
                        if !edges.contains(&edge) {
                            edges.push(edge);
                        }
                    }
                }
                Shape::Constellation { points, edges }
            }
            Family::Weave => Shape::Weave {
                count: rng.int(9, 15),
                frequency: rng.range(1.0, 2.1),
                bend: rng.range(0.12, 0.25),
                phase: rng.unit() * TAU,
                tilt: rng.range(-0.35, 0.35),
            },
        };
        let colors: ([u8; 3], [u8; 3]) = match family {
            Family::Lattice => ([175, 205, 186], [191, 210, 204]),
            Family::Polygons => ([211, 194, 156], [211, 206, 173]),
            Family::Constellation => ([160, 198, 206], [182, 207, 217]),
            Family::Weave => ([195, 177, 208], [211, 183, 203]),
        };
        let tint = rng.unit();
        let color = std::array::from_fn(|i| {
            (f32::from(colors.0[i]) * (1.0 - tint) + f32::from(colors.1[i]) * tint).round() as u8
        });
        Self {
            family,
            color,
            rotation,
            direction,
            shape,
        }
    }
    /// Normalized vertices; bounded size regardless of monitor size or scale.
    pub fn geometry(&self, amount: f32) -> Geometry {
        let a = amount.clamp(0.0, 1.0);
        let mut g = Geometry::default();
        match &self.shape {
            Shape::Lattice {
                spokes,
                rings,
                twist,
                skip,
            } => {
                let mut previous: Vec<Point> = Vec::new();
                for r in 0..*rings {
                    let radius = (0.62 + a * 0.38) * (r + 1) as f32 / *rings as f32;
                    let turn = if r % 2 == 0 { -1.0 } else { 1.0 } * twist * a;
                    let vertices: Vec<_> = (0..*spokes)
                        .map(|i| polar(radius, i as f32 * TAU / *spokes as f32 + turn))
                        .collect();
                    g.closed(&vertices, 0.18 + r as f32 / *rings as f32 * 0.29);
                    if r > 0 {
                        for (i, p) in vertices.iter().enumerate() {
                            g.line(previous[i], *p, 0.2);
                            g.line(previous[(i + skip) % spokes], *p, 0.09 + a * 0.13);
                        }
                    }
                    previous = vertices;
                }
                for p in previous {
                    g.dots.push((p, 1.6, 0.8));
                }
                g.dots.push((Point(0.0, 0.0), 2.0, 0.7));
            }
            Shape::Polygons {
                sides,
                layers,
                twist,
                step,
            } => {
                for layer in 0..*layers {
                    let f = layer as f32 / (layers - 1) as f32;
                    let radius = (0.19 + 0.78 * f) * (0.72 + 0.28 * a);
                    let turn = layer as f32 * step
                        + if layer % 2 == 0 { -1.0 } else { 1.0 } * twist * a * self.direction;
                    let vertices: Vec<_> = (0..*sides)
                        .map(|i| polar(radius, i as f32 * TAU / *sides as f32 + turn))
                        .collect();
                    g.closed(&vertices, 0.22 + 0.42 * f);
                    if layer == layers - 1 {
                        for p in vertices {
                            g.dots.push((p, 1.7, 0.8));
                        }
                    }
                }
            }
            Shape::Constellation { points, edges } => {
                let pts: Vec<_> = points
                    .iter()
                    .map(|(p, phase, _)| {
                        Point(
                            p.0 * (0.66 + 0.46 * a) + phase.cos() * a * 0.08,
                            p.1 * (0.66 + 0.46 * a) + phase.sin() * a * 0.08,
                        )
                    })
                    .collect();
                for (i, j) in edges {
                    g.line(pts[*i], pts[*j], 0.16 + a * 0.13);
                }
                for (i, p) in pts.into_iter().enumerate() {
                    g.dots.push((p, points[i].2 * 1.8, 0.48 + a * 0.35));
                }
            }
            Shape::Weave {
                count,
                frequency,
                bend,
                phase,
                tilt,
            } => {
                let warp = |x: f32, y: f32| {
                    let dist = x.hypot(y);
                    let falloff = (1.0 - dist / 1.5).max(0.0);
                    let multiplier = 1.0
                        + a * 0.2
                        + falloff
                            * (dist * frequency * std::f32::consts::PI + phase).sin()
                            * bend
                            * a;
                    rotate(Point(x * multiplier, y * multiplier), *tilt - self.rotation)
                };
                for i in 0..*count {
                    let v = -0.75 + 1.5 * i as f32 / (count - 1) as f32;
                    let opacity = 0.18 + 0.32 * (1.0 - v.abs());
                    for j in 0..48 {
                        let t = -0.85 + 1.7 * j as f32 / 48.0;
                        let next = t + 1.7 / 48.0;
                        g.line(warp(t, v), warp(next, v), opacity);
                        g.line(warp(v, t), warp(v, next), opacity);
                    }
                }
            }
        }
        for (p, q, _) in &mut g.lines {
            *p = rotate(*p, self.rotation);
            *q = rotate(*q, self.rotation);
        }
        for (p, _, _) in &mut g.dots {
            *p = rotate(*p, self.rotation);
        }
        g
    }
}
fn polar(radius: f32, angle: f32) -> Point {
    Point(radius * angle.cos(), radius * angle.sin())
}
fn rotate(p: Point, angle: f32) -> Point {
    Point(
        p.0 * angle.cos() - p.1 * angle.sin(),
        p.0 * angle.sin() + p.1 * angle.cos(),
    )
}
#[derive(Default, Debug, PartialEq)]
pub struct Geometry {
    pub lines: Vec<(Point, Point, f32)>,
    pub dots: Vec<(Point, f32, f32)>,
}
impl Geometry {
    fn line(&mut self, a: Point, b: Point, opacity: f32) {
        self.lines.push((a, b, opacity));
    }
    fn closed(&mut self, points: &[Point], opacity: f32) {
        for i in 0..points.len() {
            self.line(points[i], points[(i + 1) % points.len()], opacity);
        }
    }
}

pub struct Entrance {
    pub settings: Settings,
    pub pattern: Pattern,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub active: bool,
    pub backdrop: f32,
    pub pattern: f32,
    pub reflection: f32,
    pub amount: f32,
    pub cue: &'static str,
}
impl Entrance {
    /// `settings` must have passed validation at the configuration boundary.
    pub fn new(settings: Settings, seed: u64) -> Self {
        let index = Random(seed).int(0, settings.families.len().saturating_sub(1));
        let family = settings
            .families
            .get(index)
            .copied()
            .unwrap_or(Family::Lattice);
        Self {
            pattern: Pattern::generate(seed, family),
            settings,
        }
    }
    pub fn frame(&self, elapsed: Duration) -> Frame {
        let duration = self.settings.duration();
        if !self.settings.enabled || elapsed >= duration {
            return Frame {
                active: false,
                backdrop: 1.0,
                pattern: 0.0,
                reflection: 1.0,
                amount: 0.0,
                cue: "",
            };
        }
        let e = elapsed.as_secs_f32();
        let (inhale, exhale) = self.settings.pace.seconds();
        let cycle = (inhale + exhale) as f32;
        let end = 2.0 + cycle * f32::from(self.settings.cycles);
        let t = (e - 2.0).max(0.0);
        let within = t % cycle;
        let amount = if e < 2.0 || e >= end {
            0.0
        } else if within < inhale as f32 {
            ease(within / inhale as f32)
        } else {
            1.0 - ease((within - inhale as f32) / exhale as f32)
        };
        Frame {
            active: true,
            backdrop: ease(e / 1.8),
            pattern: ease((e - 0.5) / 1.3) * (1.0 - ease((e - end) / 1.1)),
            reflection: ease((e - end - 0.8) / 1.2),
            amount: if self.settings.reduced_motion {
                0.42
            } else {
                amount
            },
            cue: if e >= end {
                ""
            } else if within < inhale as f32 {
                "Breathe in"
            } else {
                "Breathe out"
            },
        }
    }
}
fn ease(value: f32) -> f32 {
    (1.0 - (value.clamp(0.0, 1.0) * std::f32::consts::PI).cos()) / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_validation_and_strict_legacy_loading() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings, Settings::default());
        assert_eq!(settings.duration(), Duration::from_secs(14));
        assert!(serde_json::from_str::<Settings>(r#"{"pace":"fast"}"#).is_err());
        assert!(serde_json::from_str::<Settings>(r#"{"typo":true}"#).is_err());
        for cycles in [0, 3, u8::MAX] {
            assert!(
                Settings {
                    cycles,
                    ..Settings::default()
                }
                .validate()
                .is_err()
            );
        }
        for families in [vec![], vec![Family::Weave, Family::Weave]] {
            assert!(
                Settings {
                    families,
                    ..Settings::default()
                }
                .validate()
                .is_err()
            );
        }
        for pace in Pace::ALL {
            for cycles in [1, 2] {
                let s = Settings {
                    pace,
                    cycles,
                    ..Settings::default()
                };
                s.validate().unwrap();
                assert!(s.duration() <= Duration::from_secs(24));
                assert_eq!(
                    serde_json::from_slice::<Settings>(&serde_json::to_vec(&s).unwrap()).unwrap(),
                    s
                );
            }
        }
        assert_eq!(
            Settings {
                enabled: false,
                ..settings
            }
            .duration(),
            Duration::ZERO
        );
    }
    #[test]
    fn seeded_models_are_stable_varied_and_bounded() {
        for family in Family::ALL {
            for seed in 0..64 {
                let p = Pattern::generate(seed, family);
                assert_eq!(p, Pattern::generate(seed, family));
                assert_ne!(p, Pattern::generate(seed + 1, family));
                let initial = p.geometry(0.0);
                let expanded = p.geometry(1.0);
                assert_ne!(initial, expanded);
                assert_eq!(initial, p.geometry(0.0));
                for amount in [0.0, 0.3, 0.7, 1.0] {
                    let g = p.geometry(amount);
                    assert!(g.lines.len() <= 1500);
                    assert!(g.dots.len() <= 43);
                    for (a, b, opacity) in g.lines {
                        for Point(x, y) in [a, b] {
                            assert!(x.is_finite() && y.is_finite() && x.hypot(y) < 1.8);
                        }
                        assert!((0.0..=1.0).contains(&opacity));
                    }
                }
            }
        }
    }
    #[test]
    fn only_enabled_families_are_chosen() {
        for family in Family::ALL {
            for seed in 0..100 {
                assert_eq!(
                    Entrance::new(
                        Settings {
                            families: vec![family],
                            ..Settings::default()
                        },
                        seed
                    )
                    .pattern
                    .family,
                    family
                );
            }
        }
        let chosen: Vec<_> = (0..100)
            .map(|seed| Entrance::new(Settings::default(), seed).pattern.family)
            .collect();
        for f in Family::ALL {
            assert!(chosen.contains(&f));
        }
    }
    #[test]
    fn timeline_is_bounded_and_returns_to_the_same_shape() {
        let entrance = Entrance::new(Settings::default(), 42);
        let at = |s| entrance.frame(Duration::from_secs_f32(s));
        assert_eq!(at(0.0).backdrop, 0.0);
        assert_eq!(at(2.0).amount, 0.0);
        assert_eq!(at(6.0).amount, 1.0);
        assert_eq!(at(12.0).amount, 0.0);
        assert_eq!(at(3.0).cue, "Breathe in");
        assert_eq!(at(8.0).cue, "Breathe out");
        assert!(at(12.5).reflection == 0.0);
        assert!(at(13.5).reflection > 0.0);
        assert!(at(13.99).active);
        assert!(!at(14.0).active);
        assert!(!at(1000.0).active);
        let two = Entrance::new(
            Settings {
                cycles: 2,
                ..Settings::default()
            },
            42,
        );
        assert_eq!(two.frame(Duration::from_secs(12)).amount, 0.0);
        assert_eq!(two.frame(Duration::from_secs(16)).amount, 1.0);
        assert!(!two.frame(Duration::from_secs(24)).active);
        let reduced = Entrance::new(
            Settings {
                reduced_motion: true,
                ..Settings::default()
            },
            42,
        );
        assert_eq!(
            reduced.frame(Duration::from_secs(3)).amount,
            reduced.frame(Duration::from_secs(8)).amount
        );
        let off = Entrance::new(
            Settings {
                enabled: false,
                ..Settings::default()
            },
            42,
        );
        assert!(!off.frame(Duration::ZERO).active);
    }
}
