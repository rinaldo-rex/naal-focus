use super::{
    Message,
    settings::{Navigation, Page, SettingsForm, TimeInput},
};
use crate::{
    breathing::{Family, Pace},
    config::{ChartStyle, FocusSide, FocusStyle},
    engine::Status,
};
use cosmic::{Element, iced::Length, widget};

#[cfg(test)]
#[path = "settings_view/tests.rs"]
mod tests;

const CHART_STYLES: [&str; 2] = ["Heatmap — hourly mentions", "Treemap — daily mentions"];
const FOCUS_STYLES: [&str; 3] = ["Time spent", "Day timeline", "Timeline + totals"];
const FOCUS_SIDES: [&str; 2] = ["Left", "Right"];
const BREATH_PACES: [&str; 3] = ["3s in / 4s out", "4s in / 6s out", "5s in / 5s out"];
const BREATH_COUNTS: [&str; 2] = ["One breath", "Two breaths"];

fn error_text_style(theme: &cosmic::Theme) -> cosmic::iced::widget::text::Style {
    cosmic::iced::widget::text::Style {
        color: Some(theme.cosmic().destructive_text_color().into()),
        selected_fill: theme.cosmic().accent.base.into(),
        selected_text_color: Some(theme.cosmic().on_accent_color().into()),
    }
}

pub(super) fn view<'a>(
    form: &'a SettingsForm,
    navigation: &'a Navigation,
    status: &'a Status,
    saved: bool,
) -> Element<'a, Message> {
    let page = navigation.page();
    let sidebar = widget::container(
        widget::column![
            widget::text::title3("Settings"),
            widget::nav_bar(&navigation.model, Message::SettingsPage),
            widget::text(&status.message).size(12),
        ]
        .spacing(16)
        .padding([20, 12]),
    )
    .class(cosmic::theme::Container::custom(
        widget::nav_bar::nav_bar_style,
    ))
    .width(208)
    .height(Length::Fill);

    let content = widget::column![widget::text::title2(page.title()), page_content(form, page),]
        .spacing(20)
        .padding([24, 28])
        .width(Length::Fill);
    let content: Element<'_, Message> = if page == Page::Quotes {
        content.height(Length::Fill).into()
    } else {
        widget::scrollable(content)
            .id(page_scroll_id())
            .height(Length::Fill)
            .into()
    };
    widget::row![
        sidebar,
        widget::column![
            content,
            widget::divider::horizontal::default(),
            footer(form, status, saved),
        ]
        .width(Length::Fill)
        .height(Length::Fill),
    ]
    .height(Length::Fill)
    .into()
}

pub(super) fn page_scroll_id() -> cosmic::iced::advanced::widget::Id {
    cosmic::iced::advanced::widget::Id::new("settings-page")
}

fn page_content(form: &SettingsForm, page: Page) -> Element<'_, Message> {
    match page {
        Page::Reminders => widget::column![
            panel(vec![
                time_row(
                    "Remind me every",
                    &form.interval,
                    Message::IntervalMinutes,
                    Message::IntervalSeconds,
                    true,
                ),
                number_row(
                    "Time to reflect",
                    &form.duration,
                    "sec",
                    1,
                    30,
                    Message::Duration,
                ),
                time_row(
                    "Skip when idle for",
                    &form.idle_secs,
                    Message::IdleMinutes,
                    Message::IdleSeconds,
                    true,
                ),
            ]),
            note("Pauses on lock, idle, and sleep. Reflection time starts after breathing."),
        ]
        .spacing(14)
        .into(),
        Page::Breathing => {
            let enabled = form.breathing.enabled;
            let controls = widget::row![
                panel(vec![
                    setting_row(
                        "Breathing entrance",
                        widget::toggler(enabled).on_toggle(Message::BreathingEnabled),
                    ),
                    setting_row(
                        "Reduced motion",
                        widget::toggler(form.breathing.reduced_motion)
                            .on_toggle_maybe(enabled.then_some(Message::BreathingReducedMotion)),
                    ),
                ]),
                panel(vec![
                    setting_row(
                        "Pace",
                        choice(
                            &BREATH_PACES,
                            Pace::ALL.iter().position(|p| *p == form.breathing.pace).unwrap_or(1),
                            Message::BreathingPace,
                            enabled,
                        ),
                    ),
                    setting_row(
                        "Breaths",
                        choice(
                            &BREATH_COUNTS,
                            usize::from(form.breathing.cycles.saturating_sub(1)),
                            Message::BreathingCycles,
                            enabled,
                        ),
                    ),
                ]),
            ].spacing(12);
            let patterns = |families: &[Family]| {
                panel(
                    families
                        .iter()
                        .map(|&family| {
                            let control = widget::checkbox(form.breathing.families.contains(&family))
                                .label(family.label())
                                .on_toggle_maybe(enabled.then_some(move |on| Message::BreathingFamily(family, on)));
                            widget::container(control).padding([8, 14]).into()
                        })
                        .collect(),
                )
            };
            widget::column![
                controls,
                widget::text::heading("Patterns"),
                widget::row![patterns(&Family::ALL[..Family::ALL.len().div_ceil(2)]),
                    patterns(&Family::ALL[Family::ALL.len().div_ceil(2)..])]
                    .spacing(12),
            ]
            .spacing(12)
            .into()
        }
        Page::Reflections => widget::column![
            panel(vec![
                setting_row(
                    "Enable reflections",
                    widget::toggler(form.reflections_enabled).on_toggle(Message::ReflectionsEnabled),
                ),
                time_row(
                    "Typing time limit",
                    &form.typing_limit,
                    Message::TypingMinutes,
                    Message::TypingSeconds,
                    form.reflections_enabled,
                ),
            ]),
            note(if form.reflections_enabled {
                "Writing is optional. Typing has its own time allowance."
            } else {
                "Writing, past reflections, and reflection charts are hidden. Saved history is kept."
            }),
        ]
        .spacing(14)
        .into(),
        Page::Charts => {
            let enabled = form.reflections_enabled;
            widget::column![
                panel(vec![
                    setting_row(
                        "Chart style",
                        choice(
                            &CHART_STYLES,
                            match form.charts.style {
                                ChartStyle::Heatmap => 0,
                                ChartStyle::Treemap => 1,
                            },
                            Message::ChartStyle,
                            enabled,
                        ),
                    ),
                    setting_row(
                        "Projects",
                        widget::toggler(form.charts.projects)
                            .on_toggle_maybe(enabled.then_some(Message::ChartProjects)),
                    ),
                    setting_row(
                        "Feelings",
                        widget::toggler(form.charts.feelings)
                            .on_toggle_maybe(enabled.then_some(Message::ChartFeelings)),
                    ),
                    setting_row(
                        "Tags",
                        widget::toggler(form.charts.tags)
                            .on_toggle_maybe(enabled.then_some(Message::ChartTags)),
                    ),
                ]),
                note(if enabled {
                    "Today’s saved mentions, not time spent. All categories off hides charts."
                } else {
                    "Turn on Reflections to show reflection charts."
                }),
            ]
            .spacing(14)
            .into()
        }
        Page::Focus => {
            let enabled = form.focus.enabled;
            widget::column![
                panel(vec![
                    setting_row(
                        "Track window time",
                        widget::toggler(enabled).on_toggle(Message::FocusEnabled),
                    ),
                    setting_row(
                        "Graph",
                        choice(
                            &FOCUS_STYLES,
                            match form.focus.style {
                                FocusStyle::Totals => 0,
                                FocusStyle::Timeline => 1,
                                FocusStyle::Both => 2,
                            },
                            Message::FocusStyle,
                            enabled,
                        ),
                    ),
                    setting_row(
                        "Position",
                        choice(
                            &FOCUS_SIDES,
                            match form.focus.side {
                                FocusSide::Left => 0,
                                FocusSide::Right => 1,
                            },
                            Message::FocusSide,
                            enabled,
                        ),
                    ),
                    setting_row(
                        "24-hour time",
                        widget::toggler(form.focus.use_24_hour_time)
                            .on_toggle_maybe(enabled.then_some(Message::Focus24Hour)),
                    ),
                ]),
                note("App names and window titles · Today only · Stored on this device"),
            ]
            .spacing(14)
            .into()
        }
        Page::Appearance => widget::column![
            panel(vec![number_row(
                "Quote text size", &form.text_size, "px", 22, 72, Message::TextSize,
            )]),
            widget::container(
                widget::text("You are here.").size(f32::from(
                    form.text_size.parse::<u16>().unwrap_or(36).clamp(22, 72),
                )),
            )
            .center_x(Length::Fill)
            .center_y(160)
            .class(cosmic::theme::Container::Card),
        ]
        .spacing(20)
        .into(),
        Page::Quotes => widget::column![
            widget::text_editor::text_editor(&form.quotes)
                .font(cosmic::font::default())
                .size(14)
                .padding(14)
                .height(Length::Fill)
                .on_action(Message::Quotes),
            note("Start each quote with “- ”. Following lines stay in that quote. Rotate in order."),
        ]
        .spacing(12)
        .height(Length::Fill)
        .into(),
    }
}

fn footer<'a>(form: &'a SettingsForm, status: &'a Status, saved: bool) -> Element<'a, Message> {
    let validation = form.validation_error(&status.settings);
    let error = validation.as_deref().or(status.error.as_deref());
    let mut messages = widget::column![].spacing(4).width(Length::Fill);
    if let Some(error) = error {
        messages = messages.push(
            widget::text(error.to_owned())
                .size(12)
                .class(cosmic::theme::Text::Custom(error_text_style)),
        );
    } else {
        messages = messages.push(
            widget::text(if form.dirty {
                "Unsaved changes"
            } else if saved {
                "✓ Settings saved"
            } else {
                "No unsaved changes"
            })
            .size(12),
        );
    }
    if let Some(error) = &status.focus_error {
        messages = messages.push(
            widget::text(error)
                .size(12)
                .class(cosmic::theme::Text::Custom(error_text_style)),
        );
    }
    let row = widget::row![
        messages,
        widget::button::suggested("Save settings").on_press_maybe(
            ((form.dirty || status.error.is_some()) && validation.is_none())
                .then_some(Message::Save),
        ),
    ]
    .align_y(cosmic::iced::Alignment::Center)
    .spacing(20)
    .padding([14, 28]);
    widget::container(row).id("settings-footer").into()
}

fn panel<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let mut rows = widget::column![].width(Length::Fill);
    for (index, item) in items.into_iter().enumerate() {
        if index > 0 {
            rows = rows.push(widget::divider::horizontal::default());
        }
        rows = rows.push(item);
    }
    widget::container(rows)
        .class(cosmic::theme::Container::List)
        .width(Length::Fill)
        .into()
}

fn setting_row<'a>(
    label: &'a str,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    widget::row![
        widget::text(label).size(14).width(Length::Fill),
        control.into()
    ]
    .align_y(cosmic::iced::Alignment::Center)
    .spacing(16)
    .padding([9, 14])
    .into()
}

fn note(text: &str) -> Element<'_, Message> {
    widget::text(text).size(12).into()
}

fn choice<'a>(
    options: &'a [&'static str],
    selected: usize,
    message: fn(usize) -> Message,
    enabled: bool,
) -> Element<'a, Message> {
    if enabled {
        widget::dropdown(options, Some(selected), message).into()
    } else {
        widget::text(options[selected]).size(14).into()
    }
}

fn input<'a>(
    value: &'a str,
    label: &str,
    message: fn(String) -> Message,
    enabled: bool,
) -> Element<'a, Message> {
    let field = widget::text_input("0", value)
        .id(cosmic::iced::advanced::widget::Id::new(label.to_owned()))
        .size(14)
        .width(64);
    if enabled {
        field.on_input(message).on_submit(|_| Message::Save).into()
    } else {
        field.into()
    }
}

fn time_row<'a>(
    label: &'a str,
    value: &'a TimeInput,
    minutes: fn(String) -> Message,
    seconds: fn(String) -> Message,
    enabled: bool,
) -> Element<'a, Message> {
    setting_row(
        label,
        widget::row![
            input(
                &value.minutes,
                &format!("{label} minutes"),
                minutes,
                enabled
            ),
            widget::text("min").size(12),
            input(
                &value.seconds,
                &format!("{label} seconds"),
                seconds,
                enabled
            ),
            widget::text("sec").size(12),
        ]
        .align_y(cosmic::iced::Alignment::Center)
        .spacing(8),
    )
}

fn number_row<'a>(
    label: &'a str,
    value: &'a str,
    unit: &'a str,
    min: u64,
    max: u64,
    message: fn(String) -> Message,
) -> Element<'a, Message> {
    let mut controls = widget::column![
        widget::row![
            input(value, label, message, true),
            widget::text(unit).size(12)
        ]
        .spacing(8)
        .align_y(cosmic::iced::Alignment::Center),
    ];
    if let Some(error) = super::settings::numeric_field_error(value, min, max) {
        controls = controls.push(
            widget::text(error)
                .size(11)
                .class(cosmic::theme::Text::Custom(error_text_style)),
        );
    }
    setting_row(label, controls)
}
