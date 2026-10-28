//! Offscreen native-renderer snapshots and timings. Never connects to Wayland.
//! cargo run --locked --release --example breathing-preview -- /tmp/naal-breathing-native
use naal_focus::{
    breathing::{Entrance, Family, Settings},
    render::{self, BreathingView},
};
use std::{
    error::Error,
    fs::{self, File},
    io::{BufWriter, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn Error>> {
    let directory = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("Supply an output directory.")?,
    );
    fs::create_dir_all(&directory)?;
    let font = render::default_font()?;
    for family in Family::ALL {
        let entrance = Entrance::new(
            Settings {
                families: vec![family],
                ..Settings::default()
            },
            42,
        );
        let mut pixels = vec![0; 1920 * 1080 * 4];
        render::render_breathing(
            &mut pixels,
            (1920, 1080),
            &font,
            1.0,
            BreathingView {
                pattern: &entrance.pattern,
                frame: entrance.frame(Duration::from_secs(6)),
                full: true,
            },
        );
        let path = directory.join(format!("{}.ppm", family.as_str()));
        let mut output = BufWriter::new(File::create(&path)?);
        output.write_all(b"P6\n1920 1080\n255\n")?;
        for pixel in pixels.chunks_exact(4) {
            let color = u32::from_ne_bytes(pixel.try_into()?);
            output.write_all(&[(color >> 16) as u8, (color >> 8) as u8, color as u8])?;
        }
        output.flush()?;
        println!("{}", path.display());
        for (width, height, scale) in [(1920, 1080, 1.0), (3414, 1920, 2.0)] {
            let mut canvas = vec![0; width * height * 4];
            let begin = Instant::now();
            for i in 0..40 {
                render::render_breathing(
                    &mut canvas,
                    (width, height),
                    &font,
                    scale,
                    BreathingView {
                        pattern: &entrance.pattern,
                        frame: entrance.frame(Duration::from_secs_f32(2.0 + i as f32 * 0.2)),
                        full: i == 0,
                    },
                );
            }
            println!(
                "{} {width}×{height} scale {scale}: {:.2} ms/geometry frame",
                family.label(),
                begin.elapsed().as_secs_f64() * 1000.0 / 40.0
            );
            let cache = canvas.clone();
            let begin = Instant::now();
            for i in 0..20 {
                render::render_reveal(&mut canvas, &cache, i as f32 / 19.0);
            }
            println!(
                "  reveal: {:.2} ms/frame",
                begin.elapsed().as_secs_f64() * 1000.0 / 20.0
            );
        }
    }
    Ok(())
}
