#[cfg(target_os = "linux")]
use naal_focus::{breathing, config, engine};

#[cfg(target_os = "linux")]
#[path = "support/settings_preview.rs"]
mod linux;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    {
        linux::run()
    }
    #[cfg(target_os = "macos")]
    {
        let output = std::env::args()
            .nth(1)
            .ok_or("Output directory is required")?;
        naal_focus::macos::run(vec!["--preview-settings".into(), output])
    }
}
