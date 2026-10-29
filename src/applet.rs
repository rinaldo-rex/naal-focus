use crate::{
    config::{self, ChartStyle, FocusSide, FocusStyle, Settings},
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
mod settings_view;
pub use launch::run_settings;
use launch::settings_command;
use settings::{Navigation, SettingsForm};

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
    settings_nav: Navigation,
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
    SettingsPage(widget::nav_bar::Id),
    Settings(SettingsMessage),
    WindowFocus { id: Id, focused: bool },
    Surface(cosmic::surface::Action<Message>),
}

#[derive(Debug, Clone)]
pub enum SettingsMessage {
    IntervalMinutes(String),
    IntervalSeconds(String),
    Duration(String),
    TypingMinutes(String),
    TypingSeconds(String),
    ReflectionsEnabled(bool),
    TextSize(String),
    IdleMinutes(String),
    IdleSeconds(String),
    ChartStyle(ChartStyle),
    ChartProjects(bool),
    ChartFeelings(bool),
    ChartTags(bool),
    FocusEnabled(bool),
    FocusStyle(FocusStyle),
    FocusSide(FocusSide),
    Focus24Hour(bool),
    BreathingEnabled(bool),
    BreathingReducedMotion(bool),
    BreathingPace(crate::breathing::Pace),
    BreathingCycles(u8),
    BreathingFamily(crate::breathing::Family, bool),
    Quotes(widget::text_editor::Action),
    Save,
}

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
        if let Some(error) = &self.status.focus_error {
            content = content.push(widget::text(format!("⚠ {error}")).size(12));
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

const SAVE_BANNER_SECS: u64 = 6;

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
            settings_nav: Navigation::default(),
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
            return settings_view::view(
                &self.form,
                &self.settings_nav,
                &self.status,
                self.saved_at.is_some(),
            );
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
                iced::window::events().map(settings_window_message),
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
                {
                    return self.update(Message::Settings(SettingsMessage::Save));
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
            Message::SettingsPage(id) => {
                self.settings_nav.select(id);
                return iced::widget::scrollable::scroll_to(
                    settings_view::page_scroll_id(),
                    iced::widget::scrollable::AbsoluteOffset {
                        x: None,
                        y: Some(0.0),
                    },
                );
            }
            Message::Settings(message) => {
                if let Some(settings) = self.form.update(message, &self.status.settings) {
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

fn settings_window_message((id, event): (Id, iced::window::Event)) -> Message {
    Message::WindowFocus {
        id,
        // Only a real focus loss commits a draft, not resizing or moving it.
        focused: !matches!(event, iced::window::Event::Unfocused),
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
    fn only_real_focus_loss_can_trigger_an_implicit_save() {
        let id = Id::unique();
        for event in [
            iced::window::Event::Focused,
            iced::window::Event::Resized(iced::Size::new(900.0, 600.0)),
        ] {
            assert!(matches!(
                settings_window_message((id, event)),
                Message::WindowFocus { focused: true, .. }
            ));
        }
        assert!(matches!(
            settings_window_message((id, iced::window::Event::Unfocused)),
            Message::WindowFocus { focused: false, .. }
        ));
    }

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
            focus_error: None,
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
