//! Deterministic offline pages; never reads saved history or launches a browser.
mod support;

use naal_focus::{dashboard, time::to_local};
use std::{error::Error, fs};
use support::{NOW_MS, entries, entry};

fn main() -> Result<(), Box<dyn Error>> {
    let output = support::output_directory()?;
    let mut demo = entries();
    demo.push(entry(3, NOW_MS - 86_400_000, "Walk #outside @life ??calm"));
    demo.push(entry(4, NOW_MS - 2 * 86_400_000, "Read a chapter."));
    let hostile = entry(
        5,
        NOW_MS,
        "</script><script>window.__naal_injected=true</script><img src=\"https://example.invalid/leak\" onerror=\"window.__naal_injected=true\"> & \u{2028}\u{2029}",
    );
    let leap_ms = 1_709_204_400_000;
    let year_end_ms = 1_798_714_800_000;
    let pages = [
        ("dashboard", dashboard::generate(&demo, 2, NOW_MS, to_local)),
        ("empty", dashboard::generate(&[], 0, NOW_MS, to_local)),
        (
            "error",
            dashboard::generate_error("Demo history unavailable.", NOW_MS, to_local),
        ),
        (
            "hostile",
            dashboard::generate(&[hostile], 0, NOW_MS, to_local),
        ),
        (
            "leap",
            dashboard::generate(
                &[entry(6, leap_ms, "Leap-day fixture #rust")],
                0,
                leap_ms,
                to_local,
            ),
        ),
        (
            "year-end",
            dashboard::generate(
                &[entry(7, year_end_ms, "Year-end fixture #rust")],
                0,
                year_end_ms,
                to_local,
            ),
        ),
    ];
    for (name, html) in pages {
        let path = output.join(format!("{name}.html"));
        fs::write(&path, html)?;
        println!("{}", path.display());
    }
    Ok(())
}
