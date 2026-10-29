use super::{
    Options, Payload,
    interaction::pointer_input,
    payload::{read_payload, read_stdin_quote},
};
use crate::{
    activity::{Activity, Expiry},
    breathing::Entrance,
    config::{ChartSettings, ChartStyle, FocusSide, FocusStyle},
    focus, render,
    time::system_ms,
};
use fontdue::Font;
use smithay_client_toolkit::seat::pointer::PointerEventKind;
use std::time::Duration;
fn parse(args: &[&str]) -> Result<Option<Options>, String> {
    Options::parse(args.iter().map(|s| (*s).into()))
}
fn press() -> PointerEventKind {
    PointerEventKind::Press {
        time: 0,
        button: 0x110,
        serial: 0,
    }
}
fn release() -> PointerEventKind {
    PointerEventKind::Release {
        time: 0,
        button: 0x110,
        serial: 0,
    }
}

#[test]
fn background_clicks_and_field_releases_preserve_reflection_and_timing() {
    let now = Duration::from_secs(2);
    for state in 0..5 {
        let mut activity = Activity::new(Duration::from_secs(10), Duration::from_secs(30));
        activity.mapped(Duration::ZERO);
        if state > 0 {
            activity.insert("Reflection", Duration::from_secs(1));
        }
        if state > 1 {
            let (generation, text) = activity.submission().unwrap();
            activity.saved(generation, &text, now);
        }
        if state > 2 {
            activity.open_saved(now);
        }
        if state > 3 {
            activity.insert(" correction", now);
        }
        let text = activity.editor.text().to_owned();
        let displayed = activity.displayed_text().map(str::to_owned);
        let editing = activity.editing();
        let visible = activity.field_visible();
        let generation = activity.generation();
        let remaining = activity.remaining(Duration::from_secs(3));
        let typing = activity.typing_fraction(Duration::from_secs(3));
        let field = render::input_rect(1920.0, 1080.0, 1.0);
        for (kind, position) in [
            (press(), (0.0, 0.0)),
            (release(), (0.0, 0.0)),
            // Releasing inside the field must also be a no-op.
            (release(), (field.x as f64 + 1.0, field.y as f64 + 1.0)),
        ] {
            assert!(!pointer_input(
                &mut activity,
                None,
                &kind,
                (1920, 1080),
                1.0,
                position,
                now
            ));
            assert_eq!(activity.editor.text(), text);
            assert_eq!(activity.displayed_text(), displayed.as_deref());
            assert_eq!(activity.editing(), editing);
            assert_eq!(activity.field_visible(), visible);
            assert_eq!(activity.generation(), generation);
            assert_eq!(activity.remaining(Duration::from_secs(3)), remaining);
            assert_eq!(activity.typing_fraction(Duration::from_secs(3)), typing);
        }
    }
}

#[test]
fn history_rows_are_keyboard_only_and_field_click_restores_the_draft() {
    let mut activity = Activity::new(Duration::from_secs(10), Duration::from_secs(30));
    activity.mapped(Duration::ZERO);
    activity.recall.groups.push(crate::recall::Group {
        text: "Previous".into(),
        timestamp_unix_ms: 0,
        count: 1,
    });
    activity.insert("My draft", Duration::from_secs(1));
    activity.browse_older(Duration::from_secs(2));
    let field = render::input_rect(1920.0, 1080.0, 1.0);
    let now = Duration::from_secs(3);
    assert!(!pointer_input(
        &mut activity,
        None,
        &press(),
        (1920, 1080),
        1.0,
        (field.x as f64 + 20.0, field.y as f64 - 30.0),
        now
    ));
    assert_eq!(activity.recall.selected(), Some(0));
    assert_eq!(activity.editor.text(), "My draft");
    assert_eq!(activity.remaining(now), Duration::from_secs(9));
    assert!(pointer_input(
        &mut activity,
        None,
        &press(),
        (1920, 1080),
        1.0,
        (field.x as f64 + 20.0, field.y as f64 + 10.0),
        now
    ));
    assert_eq!(activity.recall.selected(), None);
    assert_eq!(activity.editor.text(), "My draft");
    assert!(activity.editing());
    assert_eq!(activity.remaining(now), Duration::from_secs(9));
}
#[test]
fn field_press_still_reopens_saved_text_and_positions_caret_without_pausing() {
    let font = render::default_font().unwrap();
    let mut activity = Activity::new(Duration::from_secs(10), Duration::from_secs(30));
    activity.mapped(Duration::ZERO);
    activity.insert("Reflection", Duration::from_secs(1));
    let (generation, text) = activity.submission().unwrap();
    activity.saved(generation, &text, Duration::from_secs(2));
    let now = Duration::from_secs(3);
    let remaining = activity.remaining(now);
    let field = render::input_rect(1920.0, 1080.0, 1.0);
    assert!(pointer_input(
        &mut activity,
        Some(&font),
        &press(),
        (1920, 1080),
        1.0,
        (field.x as f64 + 1.0, field.y as f64 + 1.0),
        now,
    ));
    assert!(activity.field_visible());
    assert!(!activity.editing());
    assert_eq!(activity.editor.text(), "Reflection");
    assert_eq!(activity.editor.cursor(), 0);
    assert_eq!(activity.remaining(now), remaining);
    // A drag/release outside the field cannot cancel the reopened editor.
    assert!(!pointer_input(
        &mut activity,
        None,
        &release(),
        (1920, 1080),
        1.0,
        (0.0, 0.0),
        now
    ));
    assert!(activity.field_visible());
}

#[test]
fn helper_reads_multiline_text_as_one_raw_quote_without_surfaces() {
    let quote = "Take a breath.\n\n- Notice.\n- Return.";
    assert_eq!(read_stdin_quote(quote.as_bytes()).unwrap(), quote);
    for bad in [
        String::new(),
        "a".repeat(501),
        "a".repeat(4097),
        "bad\u{1b}text".into(),
    ] {
        assert!(read_stdin_quote(bad.as_bytes()).is_err());
    }
    assert!(read_stdin_quote(&[0xff][..]).is_err());
}

#[test]
fn sidebars_preserve_scaled_field_hit_testing_and_background_clicks() {
    let font = Font::from_bytes(
        std::fs::read("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf").unwrap(),
        fontdue::FontSettings::default(),
    )
    .unwrap();
    for side in [FocusSide::Left, FocusSide::Right] {
        for scale in [1.0, 2.0] {
            let settings = crate::config::FocusSettings {
                enabled: true,
                side,
                ..Default::default()
            };
            let content = render::focus_content(1920.0 * scale, 1080.0 * scale, scale, &settings);
            let field = render::input_rect(content.width, content.height, scale);
            let virtual_size = ((content.width / scale) as u32, 1080);
            let mut activity = Activity::new(Duration::from_secs(10), Duration::from_secs(30));
            activity.mapped(Duration::ZERO);
            activity.insert("Saved reflection", Duration::from_secs(1));
            let (generation, text) = activity.submission().unwrap();
            activity.saved(generation, &text, Duration::from_secs(2));
            // Same coordinate transform used by the live pointer handler.
            let global = (
                (content.x + field.x + 15.0 * scale) / scale,
                (field.y + 10.0 * scale) / scale,
            );
            let local = (f64::from(global.0 - content.x / scale), f64::from(global.1));
            assert!(pointer_input(
                &mut activity,
                Some(&font),
                &press(),
                virtual_size,
                scale,
                local,
                Duration::from_secs(3)
            ));
            assert_eq!(activity.editor.cursor(), 0);
            let sidebar =
                render::focus_sidebar(1920.0 * scale, 1080.0 * scale, scale, &settings).unwrap();
            let background = (
                f64::from((sidebar.x - content.x + 10.0 * scale) / scale),
                f64::from((sidebar.y + 10.0 * scale) / scale),
            );
            assert!(!pointer_input(
                &mut activity,
                Some(&font),
                &press(),
                virtual_size,
                scale,
                background,
                Duration::from_secs(3)
            ));
        }
    }
}

#[test]
fn focus_cli_and_private_payload_are_validated_before_capturing_focus() {
    for args in [
        vec!["--focus-style"],
        vec!["--focus-style", "pie"],
        vec!["--focus-side", "top"],
        vec!["--stdin", "--payload"],
    ] {
        assert!(parse(&args).is_err());
    }
    let selected = parse(&[
        "--focus-history",
        "--focus-style",
        "timeline",
        "--focus-side",
        "left",
        "--focus-24-hour",
    ])
    .unwrap()
    .unwrap();
    assert!(selected.settings.focus.enabled);
    assert_eq!(selected.settings.focus.style, FocusStyle::Timeline);
    assert_eq!(selected.settings.focus.side, FocusSide::Left);
    assert!(selected.settings.focus.use_24_hour_time);
    let clock_only = parse(&["--focus-24-hour"]).unwrap().unwrap();
    assert!(clock_only.settings.focus.use_24_hour_time);
    assert!(!clock_only.settings.focus.enabled);
    assert!(!parse(&[]).unwrap().unwrap().settings.focus.use_24_hour_time);
    let payload = Payload {
        quote: "Take a breath.\nReturn.".into(),
        focus: Some(focus::Snapshot {
            day: focus::Day::empty(system_ms()),
            unavailable: false,
        }),
    };
    let bytes = serde_json::to_vec(&payload).unwrap();
    assert_eq!(read_payload(bytes.as_slice()).unwrap().quote, payload.quote);
    for invalid in [
        b"{}".as_slice(),
        b"not json",
        br#"{"quote":"", "focus":null}"#,
        br#"{"quote":"ok", "focus":null,"extra":1}"#,
    ] {
        assert!(read_payload(invalid).is_err());
    }
}

#[test]
fn chart_cli_requires_known_styles_and_categories() {
    for args in [
        vec!["--chart-style"],
        vec!["--chart-style", "pie"],
        vec!["--charts"],
        vec!["--charts", ""],
        vec!["--charts", "tags,unknown"],
        vec!["--charts", "none,tags"],
    ] {
        assert!(parse(&args).is_err(), "{args:?}");
    }
    let selected = parse(&["--charts", "projects,tags", "--chart-style", "treemap"])
        .unwrap()
        .unwrap();
    assert_eq!(
        selected.settings.charts,
        ChartSettings {
            style: ChartStyle::Treemap,
            projects: true,
            feelings: false,
            tags: true
        }
    );
    assert_eq!(
        parse(&["--charts", "tags", "--charts", "none"])
            .unwrap()
            .unwrap()
            .settings
            .charts
            .kinds()
            .count(),
        0
    );
}

#[test]
fn reflections_cli_defaults_on_and_off_suppresses_charts_not_focus() {
    assert!(parse(&[]).unwrap().unwrap().settings.reflections_enabled);
    for (mode, enabled) in [("on", true), ("off", false)] {
        let settings = parse(&[
            "--reflections",
            mode,
            "--charts",
            "projects,feelings,tags",
            "--focus-history",
        ])
        .unwrap()
        .unwrap()
        .settings;
        assert_eq!(settings.reflections_enabled, enabled);
        assert_eq!(settings.charts.kinds().count(), 3);
        assert_eq!(
            settings.overlay_charts().kinds().count(),
            if enabled { 3 } else { 0 }
        );
        assert!(settings.focus.enabled);
        assert!(settings.breathing.enabled);
    }
    for args in [
        vec!["--reflections"],
        vec!["--reflections", "yes"],
        vec!["--reflections", "false"],
    ] {
        assert!(parse(&args).is_err());
    }
}

#[test]
fn breathing_cli_rejects_unbounded_or_unknown_preferences() {
    for args in [
        vec!["--breathing", "yes"],
        vec!["--breath-pace", "fast"],
        vec!["--breath-cycles", "0"],
        vec!["--breath-cycles", "3"],
        vec!["--breath-cycles", "256"],
        vec!["--breath-patterns", ""],
        vec!["--breath-patterns", "lattice,lattice"],
        vec!["--breath-patterns", "random"],
        vec!["--breath-patterns"],
    ] {
        assert!(Options::parse(args.into_iter().map(str::to_owned)).is_err());
    }
    let options = Options::parse(["--breathing", "off", "--reduced-motion"].map(str::to_owned))
        .unwrap()
        .unwrap();
    assert!(!options.settings.breathing.enabled);
    assert!(options.settings.breathing.reduced_motion);
}

#[test]
fn breathing_preserves_the_entire_reflection_and_typing_budgets() {
    use crate::breathing::{Pace, Settings as BreathingSettings};
    for enabled in [false, true] {
        for pace in Pace::ALL {
            for cycles in [1, 2] {
                let settings = BreathingSettings {
                    enabled,
                    pace,
                    cycles,
                    ..BreathingSettings::default()
                };
                let entrance = Entrance::new(settings.clone(), 42);
                let mapped = Duration::from_secs(10);
                let ready = mapped + settings.duration();
                let mut activity = Activity::new(Duration::from_secs(5), Duration::from_secs(30));
                activity.mapped(ready);
                assert!(!entrance.ready(None, ready));
                assert!(!entrance.ready(Some(mapped), ready - Duration::from_millis(1)));
                assert!(entrance.ready(Some(mapped), ready));
                assert_eq!(activity.expired(mapped), None);
                assert_eq!(activity.remaining(mapped), Duration::from_secs(5));
                assert_eq!(activity.remaining(ready), Duration::from_secs(5));
                assert_eq!(activity.expired(ready + Duration::from_secs(4)), None);
                activity.insert("Reflect", ready + Duration::from_secs(1));
                assert_eq!(
                    activity.remaining(ready + Duration::from_secs(30)),
                    Duration::from_secs(4)
                );
                assert_eq!(activity.expired(ready + Duration::from_secs(30)), None);
                assert_eq!(
                    activity.expired(ready + Duration::from_secs(31)),
                    Some(Expiry::Typing)
                );
            }
        }
    }
}

#[test]
fn helper_has_no_recurring_mode_and_bounds_focus_capture() {
    assert!(parse(&[]).unwrap().unwrap().output.is_none());
    for args in [
        vec!["--loop"],
        vec!["--duration-secs", "0"],
        vec!["--duration-secs", "31"],
        vec!["--typing-limit-secs", "29"],
        vec!["--typing-limit-secs", "601"],
        vec!["--delay-secs", "61"],
        vec!["--all", "--output", "DP-1"],
        vec!["--output"],
        vec!["--text-size", "21"],
    ] {
        assert!(parse(&args).is_err(), "{args:?}");
    }
    assert!(parse(&["--check", "--output", "DP-1"]).is_ok());
}
