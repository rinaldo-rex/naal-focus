use super::{
    Message, SettingsMessage,
    settings::{Navigation, Page, SettingsForm, TimeInput},
};
use crate::{
    breathing::{Family, Pace},
    config::{self, ChartStyle, FocusSide, FocusStyle, limits},
    engine::Status,
};
use cosmic::{Element, iced::Length, widget};
use std::ops::RangeInclusive;

#[cfg(test)]
#[path = "settings_view/tests.rs"]
mod tests;

#[derive(Clone, Copy)]
struct Choice<T>(T, &'static str);

impl<T> AsRef<str> for Choice<T> {
    fn as_ref(&self) -> &str {
        self.1
    }
}

const CHART_STYLES: [Choice<ChartStyle>; 2] = [
    Choice(ChartStyle::Heatmap, "Heatmap — hourly mentions"),
    Choice(ChartStyle::Treemap, "Treemap — daily mentions"),
];
const FOCUS_STYLES: [Choice<FocusStyle>; 3] = [
    Choice(FocusStyle::Totals, "Time spent"),
    Choice(FocusStyle::Timeline, "Day timeline"),
    Choice(FocusStyle::Both, "Timeline + totals"),
];
const FOCUS_SIDES: [Choice<FocusSide>; 2] = [
    Choice(FocusSide::Left, "Left"),
    Choice(FocusSide::Right, "Right"),
];
const BREATH_PACES: [Choice<Pace>; 3] = [
    Choice(Pace::Short, "3s in / 4s out"),
    Choice(Pace::Balanced, "4s in / 6s out"),
    Choice(Pace::Even, "5s in / 5s out"),
];
const BREATH_COUNTS: [Choice<u8>; 2] = [Choice(1, "One breath"), Choice(2, "Two breaths")];

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
    let content: Element<'_, SettingsMessage> = if page == Page::Quotes {
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
            content.map(Message::Settings),
            widget::divider::horizontal::default(),
            footer(form, status, saved).map(Message::Settings),
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

fn page_content(form: &SettingsForm, page: Page) -> Element<'_, SettingsMessage> {
    match page {
        Page::Reminders => widget::column![
            panel(vec![
                time_row(
                    "Remind me every",
                    &form.interval,
                    SettingsMessage::IntervalMinutes,
                    SettingsMessage::IntervalSeconds,
                    true,
                ),
                number_row(
                    "Time to reflect",
                    &form.duration,
                    "sec",
                    limits::DURATION_SECS,
                    SettingsMessage::Duration,
                ),
                time_row(
                    "Skip when idle for",
                    &form.idle_secs,
                    SettingsMessage::IdleMinutes,
                    SettingsMessage::IdleSeconds,
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
                        widget::toggler(enabled).on_toggle(SettingsMessage::BreathingEnabled),
                    ),
                    setting_row(
                        "Reduced motion",
                        widget::toggler(form.breathing.reduced_motion)
                            .on_toggle_maybe(enabled.then_some(SettingsMessage::BreathingReducedMotion)),
                    ),
                ]),
                panel(vec![
                    setting_row(
                        "Pace",
                        choice(
                            &BREATH_PACES,
                            form.breathing.pace,
                            SettingsMessage::BreathingPace,
                            enabled,
                        ),
                    ),
                    setting_row(
                        "Breaths",
                        choice(
                            &BREATH_COUNTS,
                            form.breathing.cycles,
                            SettingsMessage::BreathingCycles,
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
                                .on_toggle_maybe(enabled.then_some(move |on| SettingsMessage::BreathingFamily(family, on)));
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
                    widget::toggler(form.reflections_enabled).on_toggle(SettingsMessage::ReflectionsEnabled),
                ),
                time_row(
                    "Typing time limit",
                    &form.typing_limit,
                    SettingsMessage::TypingMinutes,
                    SettingsMessage::TypingSeconds,
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
                            form.charts.style,
                            SettingsMessage::ChartStyle,
                            enabled,
                        ),
                    ),
                    setting_row(
                        "Projects",
                        widget::toggler(form.charts.projects)
                            .on_toggle_maybe(enabled.then_some(SettingsMessage::ChartProjects)),
                    ),
                    setting_row(
                        "Feelings",
                        widget::toggler(form.charts.feelings)
                            .on_toggle_maybe(enabled.then_some(SettingsMessage::ChartFeelings)),
                    ),
                    setting_row(
                        "Tags",
                        widget::toggler(form.charts.tags)
                            .on_toggle_maybe(enabled.then_some(SettingsMessage::ChartTags)),
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
                        widget::toggler(enabled).on_toggle(SettingsMessage::FocusEnabled),
                    ),
                    setting_row(
                        "Graph",
                        choice(
                            &FOCUS_STYLES,
                            form.focus.style,
                            SettingsMessage::FocusStyle,
                            enabled,
                        ),
                    ),
                    setting_row(
                        "Position",
                        choice(
                            &FOCUS_SIDES,
                            form.focus.side,
                            SettingsMessage::FocusSide,
                            enabled,
                        ),
                    ),
                    setting_row(
                        "24-hour time",
                        widget::toggler(form.focus.use_24_hour_time)
                            .on_toggle_maybe(enabled.then_some(SettingsMessage::Focus24Hour)),
                    ),
                ]),
                note("App names and window titles · Today only · Stored on this device"),
            ]
            .spacing(14)
            .into()
        }
        Page::Appearance => widget::column![
            panel(vec![number_row(
                "Quote text size", &form.text_size, "px", limits::TEXT_SIZE, SettingsMessage::TextSize,
            )]),
            widget::container(
                widget::text("You are here.").size(f32::from(
                    form.text_size.parse::<u16>().unwrap_or(config::DEFAULT_TEXT_SIZE as u16)
                        .clamp(*limits::TEXT_SIZE.start() as u16, *limits::TEXT_SIZE.end() as u16),
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
                .on_action(SettingsMessage::Quotes),
            note("Start each quote with “- ”. Following lines stay in that quote. Rotate in order."),
        ]
        .spacing(12)
        .height(Length::Fill)
        .into(),
    }
}

fn footer<'a>(
    form: &'a SettingsForm,
    status: &'a Status,
    saved: bool,
) -> Element<'a, SettingsMessage> {
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
                .then_some(SettingsMessage::Save),
        ),
    ]
    .align_y(cosmic::iced::Alignment::Center)
    .spacing(20)
    .padding([14, 28]);
    widget::container(row).id("settings-footer").into()
}

fn panel<'a>(items: Vec<Element<'a, SettingsMessage>>) -> Element<'a, SettingsMessage> {
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
    control: impl Into<Element<'a, SettingsMessage>>,
) -> Element<'a, SettingsMessage> {
    widget::row![
        widget::text(label).size(14).width(Length::Fill),
        control.into()
    ]
    .align_y(cosmic::iced::Alignment::Center)
    .spacing(16)
    .padding([9, 14])
    .into()
}

fn note(text: &str) -> Element<'_, SettingsMessage> {
    widget::text(text).size(12).into()
}

fn choice<T: Copy + PartialEq + Send + Sync + 'static>(
    options: &'static [Choice<T>],
    value: T,
    message: fn(T) -> SettingsMessage,
    enabled: bool,
) -> Element<'static, SettingsMessage> {
    let selected = options
        .iter()
        .position(|option| option.0 == value)
        .unwrap_or(0);
    if enabled {
        widget::dropdown(options, Some(selected), move |index| {
            message(options[index].0)
        })
        .into()
    } else {
        widget::text(options[selected].1).size(14).into()
    }
}

fn input<'a>(
    value: &'a str,
    label: &str,
    message: fn(String) -> SettingsMessage,
    enabled: bool,
) -> Element<'a, SettingsMessage> {
    let field = widget::text_input("0", value)
        .id(cosmic::iced::advanced::widget::Id::new(label.to_owned()))
        .size(14)
        .width(64);
    if enabled {
        field
            .on_input(message)
            .on_submit(|_| SettingsMessage::Save)
            .into()
    } else {
        field.into()
    }
}

fn time_row<'a>(
    label: &'a str,
    value: &'a TimeInput,
    minutes: fn(String) -> SettingsMessage,
    seconds: fn(String) -> SettingsMessage,
    enabled: bool,
) -> Element<'a, SettingsMessage> {
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
    range: RangeInclusive<u64>,
    message: fn(String) -> SettingsMessage,
) -> Element<'a, SettingsMessage> {
    let mut controls = widget::column![
        widget::row![
            input(value, label, message, true),
            widget::text(unit).size(12)
        ]
        .spacing(8)
        .align_y(cosmic::iced::Alignment::Center),
    ];
    if let Some(error) = super::settings::numeric_field_error(value, range) {
        controls = controls.push(
            widget::text(error)
                .size(11)
                .class(cosmic::theme::Text::Custom(error_text_style)),
        );
    }
    setting_row(label, controls)
}
