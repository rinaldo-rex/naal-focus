use super::{Family, Geometry, Glow, Orb, Point, Random};
use std::f32::consts::{PI, TAU};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Study {
    family: Family,
    phase: f32,
    variation: f32,
    palette: [[u8; 3]; 3],
}

impl Study {
    pub(super) fn generate(family: Family, rng: &mut Random) -> Self {
        let palette = match family {
            Family::EmberBloom => [[232, 157, 106], [245, 207, 154], [158, 103, 128]],
            Family::MoonPool => [[173, 212, 204], [218, 229, 209], [103, 161, 174]],
            Family::GlassJelly => [[187, 164, 223], [149, 213, 219], [237, 192, 204]],
            _ => unreachable!("only organic families construct a study"),
        };
        Self {
            family,
            phase: rng.unit() * TAU,
            variation: rng.range(0.9, 1.1),
            palette,
        }
    }

    pub(super) fn geometry(&self, g: &mut Geometry, amount: f32, motion: f32) {
        match self.family {
            Family::EmberBloom => self.ember(g, amount, motion),
            Family::MoonPool => self.moon(g, amount, motion),
            Family::GlassJelly => self.jelly(g, amount, motion),
            _ => unreachable!("only organic families construct a study"),
        }
    }

    fn ember(&self, g: &mut Geometry, amount: f32, motion: f32) {
        let spin = motion * 0.025 + self.phase;
        g.glows.push(Glow {
            center: Point(0.0, 0.0),
            radius: 1.35,
            color: self.palette[0],
            opacity: 0.08 + amount * 0.035,
        });
        for layer in (0..64).rev() {
            let f = layer as f32 / 63.0;
            let radius = (0.12 + f * 0.93) * (0.76 + amount * 0.25);
            let points: Vec<_> = (0..144)
                .map(|k| {
                    let a = k as f32 / 144.0 * TAU;
                    let edge = 1.0
                        + 0.09 * (a * 5.0 + spin + f * 4.0).sin()
                        + 0.055 * (a * 3.0 - spin * 1.3 + f * 6.0).cos()
                        + 0.025 * (a * 9.0 + f * 8.0 * self.variation).sin();
                    let theta = a + spin * 0.25;
                    Point(
                        theta.cos() * radius * edge + (f * 4.0 + spin).sin() * 0.11,
                        theta.sin() * radius * edge * 0.92 + (f * 3.0 + spin).cos() * 0.04,
                    )
                })
                .collect();
            g.curve(
                &points,
                mix(self.palette[0], self.palette[1], 1.0 - f),
                0.15 + (1.0 - f) * 0.32,
                true,
            );
        }
        g.glows.push(Glow {
            center: Point(0.03, -0.02),
            radius: 0.19,
            color: self.palette[1],
            opacity: 0.12,
        });
    }

    fn moon(&self, g: &mut Geometry, amount: f32, motion: f32) {
        let t = motion + self.phase * 3.0;
        for ring in (0..=34).rev() {
            let f = ring as f32 / 34.0;
            let radius = (0.22 + f * 1.35) * (0.86 + amount * 0.12);
            let points: Vec<_> = (0..180)
                .map(|k| {
                    let a = k as f32 / 180.0 * TAU;
                    let ripple = 1.0
                        + 0.018 * (a * 7.0 + f * 11.0 - t * 0.4).sin()
                        + 0.009 * (a * 13.0 - t * 0.3).cos();
                    Point(
                        a.cos() * radius * ripple,
                        0.19 + a.sin() * radius * 0.33 * ripple
                            + (f * 18.0 - t * 0.7).sin() * 0.018,
                    )
                })
                .collect();
            let crest = 0.5 + 0.5 * (f * 24.0 - t * 0.65).cos();
            g.curve(
                &points,
                mix(self.palette[2], self.palette[0], crest),
                (0.08 + crest * 0.28) * (1.0 - f * 0.62),
                true,
            );
        }
        let orb = Point(0.0, 0.19 - (0.32 + amount * 0.18));
        for (center, radius, opacity) in [(Point(0.0, 0.19), 0.47, 0.14), (orb, 0.44, 0.11)] {
            g.glows.push(Glow {
                center,
                radius,
                opacity,
                color: self.palette[0],
            });
        }
        let radius = 0.19 + amount * 0.045;
        g.orbs.push(Orb {
            center: orb,
            radius,
            palette: self.palette,
        });
        for j in 0..14 {
            let f = j as f32 / 14.0;
            let y = 0.22 + f * 0.42;
            let width = radius * (1.0 - f * 0.76) * (1.0 + 0.24 * (j as f32 * 2.5 + t * 0.4).sin());
            g.curve(
                &[Point(-width, y), Point(width, y)],
                self.palette[0],
                0.16 * (1.0 - f),
                false,
            );
        }
    }

    fn jelly(&self, g: &mut Geometry, amount: f32, motion: f32) {
        let width = 0.72 + amount * 0.2;
        let height = 0.64 - amount * 0.12;
        let sway = motion * 0.2 + self.phase;
        let lift = -0.1 - (motion * 0.23 + self.phase).sin() * 0.055;
        g.glows.push(Glow {
            center: Point(0.0, lift - 0.06),
            radius: 1.1,
            color: self.palette[0],
            opacity: 0.065,
        });
        for rib in 0..=36 {
            let u = rib as f32 / 36.0;
            let theta = u * PI;
            let x = theta.cos() * width;
            let depth = theta.sin();
            let points: Vec<_> = (0..=64)
                .map(|k| {
                    let angle = k as f32 / 64.0 * PI / 2.0;
                    Point(
                        x * angle.sin(),
                        lift - height * angle.cos() + angle.sin() * depth * 0.15,
                    )
                })
                .collect();
            g.curve(
                &points,
                mix(self.palette[0], self.palette[1], u),
                0.12 + depth * 0.22,
                false,
            );
        }
        for ring in 0..12 {
            let f = ring as f32 / 11.0;
            let points: Vec<_> = (0..=96)
                .map(|k| {
                    let angle = k as f32 / 96.0 * PI;
                    Point(
                        angle.cos() * width * (f * PI / 2.0).sin(),
                        lift - height * (f * PI / 2.0).cos() + angle.sin() * 0.15 * f,
                    )
                })
                .collect();
            g.curve(&points, self.palette[2], 0.12 + f * 0.09, false);
        }
        for tendril in 0..26 {
            let u = tendril as f32 / 25.0;
            let x = (u - 0.5) * width * 1.55;
            let length = 0.8 + 0.45 * (tendril as f32 * 3.8 + self.phase).sin().powi(2);
            let points: Vec<_> = (0..=64)
                .map(|k| {
                    let v = k as f32 / 64.0;
                    Point(
                        x * (1.0 - v * 0.37)
                            + (v * 8.0 * self.variation - sway + tendril as f32 * 0.31).sin()
                                * 0.09
                                * v,
                        lift + (u * PI).sin() * 0.1 + v * length,
                    )
                })
                .collect();
            g.curve(
                &points,
                mix(self.palette[0], self.palette[1], u),
                0.14 + (1.0 - (u - 0.5).abs() * 2.0) * 0.13,
                false,
            );
            if tendril % 4 == 0 {
                g.dots.push((*points.last().unwrap(), 1.4, 0.5));
            }
        }
        g.glows.push(Glow {
            center: Point(0.0, lift - height * 0.62),
            radius: 0.25,
            color: self.palette[2],
            opacity: 0.08,
        });
    }
}

fn mix(a: [u8; 3], b: [u8; 3], amount: f32) -> [u8; 3] {
    std::array::from_fn(|i| {
        (f32::from(a[i]) * (1.0 - amount) + f32::from(b[i]) * amount).round() as u8
    })
}
