use crate::{
    config::{self, ChartStyle, Settings},
    dashboard::PendingOpen,
    engine::{Engine, Request, Status},
};
use cosmic::iced::platform_specific::shell::wayland::commands::{
    activation,
    popup::{destroy_popup, get_popup},
};
use cosmic::{
    applet::token::subscription::{TokenRequest, TokenUpdate, activation_token_subscription},
    cctk::sctk::reexports::calloop,
    iced::{self, Limits, Subscription, window::Id},
    prelude::*,
    widget,
};
use std::time::{Duration, Instant};

mod launch;
mod settings;
pub use launch::run_settings;
use launch::settings_command;
use settings::SettingsForm;

#[derive(Clone, Copy, Default)]
pub struct Flags {
    pub standalone: bool,
}

impl cosmic::app::CosmicFlags for Flags {
    type SubCommand = String;
    type Args = Vec<String>;
}

pub struct App {
    core: cosmic::Core,
    popup: Option<Id>,
    standalone: bool,
    startup_token: Option<String>,
    token_tx: Option<calloop::channel::Sender<TokenRequest>>,
    opening_settings: bool,
    opening_dashboard: bool,
    pending_dashboard: Option<PendingOpen>,
    launch_error: Option<String>,
    engine: Engine,
    status: Status,
    form: SettingsForm,
    saved_at: Option<Instant>,
}

#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    OpenSettings,
    OpenDashboard,
    DashboardOpened(Result<(), String>),
    Token(TokenUpdate),
    SettingsToken(Option<String>),
    SettingsLaunched(Result<(), String>),
    SettingsOpened(Id),
    PopupClosed(Id),
    Tick,
    Enabled(bool),
    ShowNow,
    Snooze,
    Resume,
    Interval(String),
    Duration(String),
    TypingLimit(String),
    TextSize(String),
    IdleSecs(String),
    ChartStyle(usize),
    ChartProjects(bool),
    ChartFeelings(bool),
    ChartTags(bool),
    Quotes(widget::text_editor::Action),
    Save,
    WindowFocus { id: Id, focused: bool },
    Surface(cosmic::surface::Action<Message>),
}

const CHART_STYLES: [&str; 2] = ["Heatmap — hourly mentions", "Treemap — daily mentions"];

impl App {
    fn quick_controls(&self) -> Element<'_, Message> {
        let (secondary_label, secondary_action) = quick_secondary_action(&self.status);
        let mut content = widget::column([])
            .spacing(12)
            .push(widget::text::title3("Naal Focus"))
            .push(widget::text(&self.status.message))
            .push(widget::settings::item(
                "Recurring reminders",
                widget::toggler(self.status.settings.enabled).on_toggle(Message::Enabled),
            ))
            .push(
                widget::row([])
                    .spacing(8)
                    .push(widget::button::suggested("Show now").on_press_maybe(
                        (self.status.safe && !self.status.running).then_some(Message::ShowNow),
                    ))
                    .push(widget::button::standard(secondary_label).on_press(secondary_action)),
            );
        content = content.push(widget::divider::horizontal::default()).push(
            widget::column![]
                .spacing(4)
                .push(
                    widget::button::text("Dashboard…").on_press_maybe(
                        (!self.opening_dashboard).then_some(Message::OpenDashboard),
                    ),
                )
                .push(
                    widget::button::text("Settings…")
                        .on_press_maybe((!self.opening_settings).then_some(Message::OpenSettings)),
                ),
        );
        if let Some(error) = self.launch_error.as_ref().or(self.status.error.as_ref()) {
            content = content.push(widget::text(format!("⚠ {error}")).size(13));
        }
        widget::scrollable(content.padding(16))
            .width(iced::Length::Fill)
            .into()
    }

    fn settings_view(&self) -> Element<'_, Message> {
        let timing = widget::settings::section()
            .title("Timing")
            .add(numeric_field(
                "Interval (seconds)",
                "300",
                &self.form.interval,
                15,
                86_400,
                Message::Interval,
            ))
            .add(numeric_field(
                "Duration (1–30 seconds)",
                "5",
                &self.form.duration,
                1,
                30,
                Message::Duration,
            ))
            .add(numeric_field(
                "Skip after idle (seconds)",
                "120",
                &self.form.idle_secs,
                30,
                3_600,
                Message::IdleSecs,
            ));
        let appearance = widget::settings::section()
            .title("Appearance")
            .add(numeric_field(
                "Text size (22–72 px)",
                "36",
                &self.form.text_size,
                22,
                72,
                Message::TextSize,
            ));
        let charts = widget::settings::section()
            .title("Overlay charts")
            .add(widget::settings::item(
                "Chart style",
                widget::dropdown(
                    &CHART_STYLES[..],
                    Some(match self.form.charts.style {
                        ChartStyle::Heatmap => 0,
                        ChartStyle::Treemap => 1,
                    }),
                    Message::ChartStyle,
                ),
            ))
            .add(widget::settings::item(
                "Projects",
                widget::toggler(self.form.charts.projects).on_toggle(Message::ChartProjects),
            ))
            .add(widget::settings::item(
                "Feelings",
                widget::toggler(self.form.charts.feelings).on_toggle(Message::ChartFeelings),
            ))
            .add(widget::settings::item(
                "Tags",
                widget::toggler(self.form.charts.tags).on_toggle(Message::ChartTags),
            ))
            .add(widget::text("Today’s saved mentions, not time spent. All switches off hides charts. Heatmap shows hourly counts; Treemap shows daily totals.").size(12));
        let reflection = widget::settings::section()
            .title("Reflection")
            .add(numeric_field(
                "Typing time limit (30–600 seconds)",
                "120",
                &self.form.typing_limit,
                30,
                600,
                Message::TypingLimit,
            ))
            .add(
                widget::column([])
                    .spacing(8)
                    .push(widget::text::heading("Quotes — bullet-separated"))
                    .push(widget::text("Start each quote with “- ” at the left edge.\nFollowing lines and paragraphs stay in that quote.").size(12))
                    .push(
                        widget::text_editor::text_editor(&self.form.quotes)
                            .height(150)
                            .on_action(Message::Quotes),
                    )
                    .push(widget::text("1–100 quotes, up to 500 characters each. Rotate in order.").size(12)),
            );
        let mut content = widget::column([]).spacing(20);
        content = content.push_maybe(self.saved_at.is_some().then(save_banner));
        content = content
            .push(widget::text(&self.status.message))
            .push(timing)
            .push(appearance)
            .push(charts)
            .push(reflection)
            .push(widget::button::suggested("Save settings").on_press(Message::Save))
            .push(
                widget::text(
                    "Paused on lock, idle, or sleep. A full interval starts after returning.",
                )
                .size(12),
            );
        if self.form.dirty {
            content =
                content.push(widget::text("Unsaved edits — Save settings to apply.").size(12));
        }
        if let Some(error) = self.form.error.as_ref().or(self.status.error.as_ref()) {
            content = content.push(widget::text(format!("⚠ {error}")).size(13));
        }
        widget::scrollable(content.padding(16))
            .width(iced::Length::Fill)
            .into()
    }

    fn sync_status(&mut self, status: Status) {
        if self.form.sync(&status.settings, status.error.as_deref()) {
            self.saved_at = Some(Instant::now());
        }
        self.status = status;
    }
}

/// A validated numeric field rendered web-form style: the error lives
/// directly under the input, not only at the bottom of the window.
fn numeric_field<'a>(
    title: &'a str,
    placeholder: &'a str,
    value: &'a str,
    min: u64,
    max: u64,
    message: fn(String) -> Message,
) -> Element<'a, Message> {
    let mut control = widget::column![].push(
        widget::text_input(placeholder, value)
            .on_input(message)
            .on_submit(|_| Message::Save)
            .width(100),
    );
    if let Some(problem) = settings::numeric_field_error(value, min, max) {
        control = control.push(
            widget::text(problem)
                .size(11)
                .class(cosmic::theme::Text::Custom(error_text_style)),
        );
    }
    widget::settings::item(title, control).into()
}

fn error_text_style(theme: &cosmic::Theme) -> cosmic::iced::widget::text::Style {
    cosmic::iced::widget::text::Style {
        color: Some(theme.cosmic().destructive_text_color().into()),
        selected_fill: theme.cosmic().accent.base.into(),
        selected_text_color: Some(theme.cosmic().on_accent_color().into()),
    }
}

const SAVE_BANNER_SECS: u64 = 6;

fn save_banner<'a>() -> Element<'a, Message> {
    widget::container(
        widget::row![]
            .push(widget::text("✓").size(14))
            .push(widget::text("Settings saved"))
            .spacing(8),
    )
    .class(cosmic::theme::Container::custom(success_banner_style))
    .padding([8, 14])
    .width(iced::Length::Fill)
    .into()
}

fn success_banner_style(theme: &cosmic::Theme) -> cosmic::iced::widget::container::Style {
    cosmic::iced::widget::container::Style {
        background: Some(cosmic::iced::Background::Color(
            theme.cosmic().success.base.into(),
        )),
        text_color: Some(theme.cosmic().success.on.into()),
        icon_color: Some(theme.cosmic().success.on.into()),
        border: cosmic::iced::Border {
            radius: theme.cosmic().corner_radii.radius_s.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

impl cosmic::Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = Flags;
    type Message = Message;
    const APP_ID: &'static str = config::APP_ID;
    fn core(&self) -> &cosmic::Core {
        &self.core
    }
    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }
    fn init(core: cosmic::Core, flags: Flags) -> (Self, Task<cosmic::Action<Message>>) {
        let path = config::config_path();
        let (settings, error) = match path
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|path| config::load(path))
        {
            Ok(settings) => (settings, None),
            Err(error) => (Settings::default(), Some(error)),
        };
        let engine = Engine::start(settings.clone(), path.ok(), error);
        let mut app = Self {
            core,
            popup: None,
            standalone: flags.standalone,
            startup_token: flags
                .standalone
                .then(|| std::env::var("XDG_ACTIVATION_TOKEN").ok())
                .flatten(),
            token_tx: None,
            opening_settings: false,
            opening_dashboard: false,
            pending_dashboard: None,
            launch_error: None,
            status: engine.status(),
            engine,
            form: SettingsForm::new(&settings),
            saved_at: None,
        };
        let task = if app.standalone {
            app.set_header_title("Naal Focus Settings".into());
            app.core
                .main_window_id()
                .map(|id| app.set_window_title("Naal Focus Settings".into(), id))
                .unwrap_or_else(Task::none)
        } else {
            Task::none()
        };
        (app, task)
    }
    fn on_close_requested(&self, id: Id) -> Option<Message> {
        if self.popup == Some(id) {
            Some(Message::PopupClosed(id))
        } else {
            None
        }
    }
    fn view(&self) -> Element<'_, Message> {
        if self.standalone {
            return self.settings_view();
        }
        let handle = if self.status.settings.enabled && self.status.snoozed_secs.is_none() {
            widget::icon::from_svg_bytes(
                include_bytes!("../data/naal-focus-symbolic.svg").as_slice(),
            )
            .symbolic(true)
        } else {
            widget::icon::from_name("media-playback-pause-symbolic")
                .symbolic(true)
                .into()
        };
        let button = self
            .core
            .applet
            .icon_button_from_handle(handle)
            .on_press(Message::TogglePopup);
        self.core
            .applet
            .applet_tooltip(
                button,
                format!("Naal Focus — {}", self.status.message),
                self.popup.is_some(),
                Message::Surface,
                self.core.main_window_id(),
            )
            .into()
    }
    fn view_window(&self, _: Id) -> Element<'_, Message> {
        self.core
            .applet
            .popup_container(self.quick_controls())
            .into()
    }
    fn subscription(&self) -> Subscription<Message> {
        let tick = iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick);
        if self.standalone {
            Subscription::batch([
                tick,
                iced::window::open_events().map(Message::SettingsOpened),
                iced::window::events().map(|(id, event)| Message::WindowFocus {
                    id,
                    focused: matches!(event, iced::window::Event::Focused),
                }),
            ])
        } else {
            Subscription::batch([
                tick,
                activation_token_subscription(config::APP_ID).map(Message::Token),
            ])
        }
    }
    fn update(&mut self, message: Message) -> Task<cosmic::Action<Message>> {
        match message {
            Message::Tick => {
                if let Some(at) = self.saved_at
                    && at.elapsed().as_secs() >= SAVE_BANNER_SECS
                {
                    self.saved_at = None;
                }
                self.sync_status(self.engine.status());
                // Generation and opening run on the dashboard worker; this
                // loop only polls its result without ever blocking on I/O.
                if let Some(result) = self.pending_dashboard.as_mut().and_then(|p| p.poll()) {
                    return self.update(Message::DashboardOpened(result));
                }
            }
            Message::OpenSettings => {
                if self.opening_settings {
                    return Task::none();
                }
                self.opening_settings = true;
                self.launch_error = None;
                // COSMIC's privileged token channel reaches the host compositor;
                // the panel's nested Wayland connection cannot authorize focus.
                if let Some(tx) = &self.token_tx
                    && tx
                        .send(TokenRequest {
                            app_id: config::APP_ID.into(),
                            exec: "naal-focus --settings".into(),
                        })
                        .is_ok()
                {
                    return Task::none();
                }
                return self.update(Message::SettingsToken(None));
            }
            Message::Token(update) => match update {
                TokenUpdate::Init(tx) => self.token_tx = Some(tx),
                TokenUpdate::Finished => {
                    self.token_tx = None;
                    if self.opening_settings {
                        return self.update(Message::SettingsToken(None));
                    }
                }
                TokenUpdate::ActivationToken { token, .. } => {
                    return self.update(Message::SettingsToken(token));
                }
            },
            Message::SettingsToken(token) => {
                return Task::perform(
                    async move {
                        let command = settings_command(token.as_deref())?;
                        cosmic::process::spawn(command)
                            .await
                            .map(|_| ())
                            .ok_or_else(|| "Could not open the settings window. Try again.".into())
                    },
                    |result| cosmic::Action::App(Message::SettingsLaunched(result)),
                );
            }
            Message::SettingsLaunched(result) => {
                self.opening_settings = false;
                match result {
                    Ok(()) => {
                        if let Some(popup) = self.popup.take() {
                            return destroy_popup(popup);
                        }
                    }
                    Err(error) => self.launch_error = Some(error),
                }
            }
            Message::OpenDashboard => {
                if self.opening_dashboard {
                    return Task::none();
                }
                match PendingOpen::start() {
                    Ok(pending) => {
                        self.pending_dashboard = Some(pending);
                        self.opening_dashboard = true;
                        self.launch_error = None;
                    }
                    Err(error) => self.launch_error = Some(error),
                }
            }
            Message::DashboardOpened(result) => {
                self.opening_dashboard = false;
                self.pending_dashboard = None;
                match result {
                    // Rebuilt fresh on every open; a stale browser tab keeps
                    // its old snapshot until the next open or a reload.
                    Ok(()) => {
                        if let Some(popup) = self.popup.take() {
                            return destroy_popup(popup);
                        }
                    }
                    Err(error) => self.launch_error = Some(error),
                }
            }
            Message::SettingsOpened(id) => {
                if self.core.main_window_id() == Some(id)
                    && let Some(token) = self.startup_token.take()
                {
                    return activation::activate(id, token);
                }
            }
            Message::WindowFocus { id, focused } => {
                // Leaving the settings window is the commit point for drafting:
                // an entirely valid draft saves itself here; an invalid draft
                // stays queued for the next visit with its inline errors.
                if self.standalone
                    && !focused
                    && self.core.main_window_id() == Some(id)
                    && self.form.dirty
                    && let Ok(settings) = self.form.save(&self.status.settings)
                {
                    self.engine.request(Request::Settings(settings));
                }
            }
            Message::Enabled(enabled) => {
                self.engine.request(Request::Enabled(enabled));
                self.form.error = None;
            }
            Message::Snooze => self
                .engine
                .request(Request::Snooze(Duration::from_secs(15 * 60))),
            Message::Resume => {
                if let Some(request) = resume_request(&self.status) {
                    self.engine.request(request);
                }
            }
            Message::ShowNow => {
                self.engine.request(Request::ShowNow);
                if let Some(popup) = self.popup.take() {
                    return destroy_popup(popup);
                }
            }
            Message::Interval(text) => {
                self.form.interval = text;
                self.form.dirty = true;
            }
            Message::Duration(text) => {
                self.form.duration = text;
                self.form.dirty = true;
            }
            Message::TypingLimit(text) => {
                self.form.typing_limit = text;
                self.form.dirty = true;
            }
            Message::TextSize(text) => {
                self.form.text_size = text;
                self.form.dirty = true;
            }
            Message::IdleSecs(text) => {
                self.form.idle_secs = text;
                self.form.dirty = true;
            }
            Message::ChartStyle(index) => {
                if let Some(style) = [ChartStyle::Heatmap, ChartStyle::Treemap].get(index) {
                    self.form.charts.style = *style;
                    self.form.dirty = true;
                }
            }
            Message::ChartProjects(enabled) => {
                self.form.charts.projects = enabled;
                self.form.dirty = true;
            }
            Message::ChartFeelings(enabled) => {
                self.form.charts.feelings = enabled;
                self.form.dirty = true;
            }
            Message::ChartTags(enabled) => {
                self.form.charts.tags = enabled;
                self.form.dirty = true;
            }
            Message::Quotes(action) => {
                self.form.dirty |= action.is_edit();
                self.form.quotes.perform(action);
            }
            Message::Save => {
                if let Ok(settings) = self.form.save(&self.status.settings) {
                    self.engine.request(Request::Settings(settings));
                }
            }
            Message::TogglePopup => {
                if let Some(popup) = self.popup.take() {
                    return destroy_popup(popup);
                }
                let id = Id::unique();
                self.popup = Some(id);
                let mut settings = self.core.applet.get_popup_settings(
                    self.core.main_window_id().unwrap(),
                    id,
                    None,
                    None,
                    None,
                );
                settings.positioner.size_limits = Limits::NONE
                    .min_width(340.0)
                    .max_width(380.0)
                    .min_height(1.0)
                    .max_height(420.0);
                return get_popup(settings);
            }
            Message::PopupClosed(id) => {
                if self.popup == Some(id) {
                    self.popup = None;
                }
            }
            Message::Surface(action) => {
                return cosmic::task::message(cosmic::Action::Surface(action));
            }
        }
        Task::none()
    }
    fn style(&self) -> Option<iced::theme::Style> {
        if self.standalone {
            None
        } else {
            Some(cosmic::applet::style())
        }
    }
}

fn quick_secondary_action(status: &Status) -> (&'static str, Message) {
    if status.snoozed_secs.is_some() {
        ("Resume", Message::Resume)
    } else {
        ("Snooze 15 min", Message::Snooze)
    }
}

fn resume_request(status: &Status) -> Option<Request> {
    if !status.settings.enabled {
        Some(Request::Enabled(true))
    } else if status.snoozed_secs.is_some() {
        Some(Request::Resume)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_editor_preserves_multiline_quotes_and_literal_bullets() {
        let quotes = vec![
            "First line.\n\n- Literal bullet.".into(),
            "Another quote.\nAnother line.".into(),
        ];
        let content = widget::text_editor::Content::<cosmic::Renderer>::with_text(
            &config::format_quotes(&quotes),
        );
        assert_eq!(config::parse_quotes(&content.text()).unwrap(), quotes);
    }

    #[test]
    fn resume_unpauses_or_clears_snooze_and_only_disables_when_running() {
        let mut status = Status {
            settings: Settings::default(),
            message: String::new(),
            error: None,
            running: false,
            safe: true,
            next_secs: 300,
            snoozed_secs: None,
        };
        assert!(matches!(
            resume_request(&status),
            Some(Request::Enabled(true))
        ));
        assert!(matches!(
            quick_secondary_action(&status),
            ("Snooze 15 min", Message::Snooze)
        ));
        status.settings.enabled = true;
        status.snoozed_secs = Some(900);
        assert!(matches!(resume_request(&status), Some(Request::Resume)));
        assert!(matches!(
            quick_secondary_action(&status),
            ("Resume", Message::Resume)
        ));
        status.snoozed_secs = None;
        assert!(resume_request(&status).is_none());
        assert!(matches!(
            quick_secondary_action(&status),
            ("Snooze 15 min", Message::Snooze)
        ));
    }
}
