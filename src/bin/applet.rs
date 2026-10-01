fn main() -> cosmic::iced::Result {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "Naal Focus — COSMIC mindfulness reminder applet\n\nUsage: naal-focus [--settings]\n\nWithout arguments, run inside the COSMIC panel/dock.\n--settings opens a standalone window sharing the dock/panel controls.\nRecurring reminders are paused until explicitly enabled.\nUse naal-focus-overlay --check for a read-only desktop probe."
        );
        return Ok(());
    }
    if args == ["--settings"] {
        cosmic::app::run::<naal_focus::applet::App>(
            cosmic::app::Settings::default()
                .size(cosmic::iced::Size::new(460.0, 760.0))
                .exit_on_close(true),
            naal_focus::applet::Flags { standalone: true },
        )
    } else if args.is_empty() {
        cosmic::applet::run::<naal_focus::applet::App>(naal_focus::applet::Flags::default())
    } else {
        eprintln!("Unknown option; use --help.");
        std::process::exit(1);
    }
}
