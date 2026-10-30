use crate::config::{ChartSettings, ChartStyle, FocusSide, FocusStyle, Settings};

pub const HELP: &str = "Naal Focus — one-shot overlay helper for COSMIC/Wayland

Usage: naal-focus-overlay [OPTIONS]
  --check              Read-only protocol, monitor, and session safety probe
  --once               Show one reminder (default); never runs its own timer
  --all                Cover all monitors (default)
  --output NAME        Cover one named monitor for manual testing
  --duration-secs N    Normal visible countdown, 1–30 (default 5)
  --reflections MODE   on (default) or off; off hides writing, history, and charts
  --typing-limit-secs N  Typing deadline, 30–600 (default 120)
  --delay-secs N       Delay before showing, 0–60 (default 0)
  --text-size N        Logical text size, 22–72 (default 36)
  --idle-secs N        Skip idle sessions, 30–3600 (default 120)
  --chart-style STYLE  heatmap (hourly) or treemap (daily totals)
  --charts KINDS       Comma-separated projects,feelings,tags or none (default)
  --focus-history      Show collected focus history; does not start tracking
  --focus-style STYLE  totals, timeline, or both (default)
  --focus-side SIDE    left or right (default)
  --focus-24-hour      Use 24-hour sidebar times (default: 12-hour AM/PM)
  --breathing MODE     on (default) or off; entrance precedes reflection
  --breath-pace PACE   short (3/4), balanced (4/6, default), even (5/5)
  --breath-cycles N    One or two breaths (default 1)
  --breath-patterns LIST  lattice,polygons,constellation,weave,ember_bloom,moon_pool,glass_jelly
  --reduced-motion    Static geometry with timed breath cues
  --payload            Read the supervisor's bounded JSON quote/history payload
  --stdin              Read one raw UTF-8 quote (including line breaks) from stdin
  -h, --help           Print help

WARNING: captures keyboard focus on all covered monitors. Type to reflect;
During the breathing entrance or with reflections off, typing is ignored and Escape dismisses all.
With reflections on after the entrance, Up/Down browse history; Enter recalls, then saves.
Use #tags, @projects, and ??feelings; arrows select suggestions, Tab inserts.
After saving, Enter reopens the reflection for correction (or click it).
With suggestions open, Enter saves as written; Escape closes suggestions first.
Typing or browsing holds the normal countdown up to the typing limit. Escape
while browsing returns to the draft; while typing discards and resumes;
otherwise it dismisses ALL
surfaces. Background clicks do nothing. No raw keys are logged. This is not a screen lock.
";

#[derive(Debug, Default)]
pub struct Options {
    pub settings: Settings,
    pub check: bool,
    pub delay: u64,
    pub output: Option<String>,
    pub stdin: bool,
    pub payload: bool,
}

impl Options {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Option<Self>, String> {
        let mut options = Self::default();
        let mut all = false;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => return Ok(None),
                "--check" => options.check = true,
                "--once" => {}
                "--all" => all = true,
                "--stdin" => options.stdin = true,
                "--payload" => options.payload = true,
                "--focus-history" => options.settings.focus.enabled = true,
                "--focus-24-hour" => options.settings.focus.use_24_hour_time = true,
                "--reflections" => {
                    options.settings.reflections_enabled = match args.next().as_deref() {
                        Some("on") => true,
                        Some("off") => false,
                        _ => return Err("--reflections requires on or off.".into()),
                    };
                }
                "--breathing" => {
                    options.settings.breathing.enabled = match args.next().as_deref() {
                        Some("on") => true,
                        Some("off") => false,
                        _ => return Err("--breathing requires on or off.".into()),
                    };
                }
                "--breath-pace" => {
                    options.settings.breathing.pace = match args.next().as_deref() {
                        Some("short") => crate::breathing::Pace::Short,
                        Some("balanced") => crate::breathing::Pace::Balanced,
                        Some("even") => crate::breathing::Pace::Even,
                        _ => return Err("--breath-pace requires short, balanced, or even.".into()),
                    };
                }
                "--breath-cycles" => {
                    options.settings.breathing.cycles = number(&arg, args.next())?
                        .try_into()
                        .map_err(|_| "Choose one or two breaths.")?;
                }
                "--breath-patterns" => {
                    let list = args.next().ok_or("--breath-patterns requires a list.")?;
                    options.settings.breathing.families = list
                        .split(',')
                        .map(|name| {
                            crate::breathing::Family::ALL
                                .into_iter()
                                .find(|f| f.as_str() == name)
                                .ok_or_else(|| format!("Unknown breathing pattern: {name}"))
                        })
                        .collect::<Result<_, _>>()?;
                }
                "--reduced-motion" => options.settings.breathing.reduced_motion = true,
                "--focus-style" => {
                    options.settings.focus.style = match args.next().as_deref() {
                        Some("totals") => FocusStyle::Totals,
                        Some("timeline") => FocusStyle::Timeline,
                        Some("both") => FocusStyle::Both,
                        _ => return Err("--focus-style requires totals, timeline, or both.".into()),
                    };
                }
                "--focus-side" => {
                    options.settings.focus.side = match args.next().as_deref() {
                        Some("left") => FocusSide::Left,
                        Some("right") => FocusSide::Right,
                        _ => return Err("--focus-side requires left or right.".into()),
                    };
                }
                "--output" => {
                    options.output = Some(
                        args.next()
                            .filter(|v| !v.is_empty() && !v.starts_with('-'))
                            .ok_or("--output requires a monitor name.")?,
                    )
                }
                "--chart-style" => {
                    options.settings.charts.style = match args.next().as_deref() {
                        Some("heatmap") => ChartStyle::Heatmap,
                        Some("treemap") => ChartStyle::Treemap,
                        _ => return Err("--chart-style requires heatmap or treemap.".into()),
                    };
                }
                "--charts" => {
                    let value = args.next().ok_or("--charts requires categories or none.")?;
                    let mut charts = ChartSettings {
                        style: options.settings.charts.style,
                        ..ChartSettings::default()
                    };
                    if value != "none" {
                        for category in value.split(',') {
                            match category {
                                "projects" => charts.projects = true,
                                "feelings" => charts.feelings = true,
                                "tags" => charts.tags = true,
                                _ => {
                                    return Err(
                                        "--charts requires projects,feelings,tags or none.".into(),
                                    );
                                }
                            }
                        }
                    }
                    options.settings.charts = charts;
                }
                "--duration-secs" => options.settings.duration_secs = number(&arg, args.next())?,
                "--typing-limit-secs" => {
                    options.settings.typing_limit_secs = number(&arg, args.next())?
                }
                "--delay-secs" => options.delay = number(&arg, args.next())?,
                "--text-size" => {
                    options.settings.text_size = number(&arg, args.next())?
                        .try_into()
                        .map_err(|_| "Text size is too large.")?
                }
                "--idle-secs" => {
                    options.settings.idle_secs = number(&arg, args.next())?
                        .try_into()
                        .map_err(|_| "Idle threshold is too large.")?
                }
                _ => return Err(format!("Unknown option {arg}; use --help.")),
            }
        }
        options.settings.validate()?;
        if options.stdin && options.payload {
            return Err("Choose --stdin or --payload, not both.".into());
        }
        if options.delay > 60 {
            return Err("Delay must be 0–60 seconds.".into());
        }
        if all && options.output.is_some() {
            return Err("Choose --all or --output, not both.".into());
        }
        Ok(Some(options))
    }
}
fn number(option: &str, text: Option<String>) -> Result<u64, String> {
    text.ok_or_else(|| format!("{option} requires an integer."))?
        .parse()
        .map_err(|_| format!("{option} requires an integer."))
}
