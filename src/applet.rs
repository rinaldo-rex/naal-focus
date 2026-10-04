use crate::{
    config::{self, Settings},
    engine::{Engine, Request, Status},
};
use cosmic::iced::platform_specific::shell::wayland::commands::popup::{destroy_popup, get_popup};
use cosmic::{
    iced::{self, Limits, Subscription, window::Id},
    prelude::*,
    widget,
};
use std::time::Duration;

#[derive(Clone, Copy, Default)]
pub struct Flags {
    pub standalone: bool,
}

pub struct App {
    core: cosmic::Core,
    popup: Option<Id>,
    standalone: bool,
    engine: Engine,
    status: Status,
    interval: String,
    duration: String,
    typing_limit: String,
    text_size: String,
    idle_secs: String,
    quotes: widget::text_editor::Content<cosmic::Renderer>,
    form_error: Option<String>,
    dirty: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
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
    Quotes(widget::text_editor::Action),
    Save,
    Surface(cosmic::surface::Action<Message>),
}

impl App {
    fn controls(&self) -> Element<'_, Message> {
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
                    .push(widget::button::standard("Snooze 15 min").on_press(Message::Snooze))
                    .push(
                        widget::button::text("Resume")
                            .on_press_maybe(resume_request(&self.status).map(|_| Message::Resume)),
                    ),
            )
            .push(
                widget::text(
                    "All monitors · type to reflect, or just breathe. Enter saves; Esc discards while writing.",
                )
                .size(12),
            )
            .push(widget::divider::horizontal::default())
            .push(widget::settings::item(
                "Interval (seconds)",
                widget::text_input("300", &self.interval)
                    .on_input(Message::Interval)
                    .width(100),
            ))
            .push(widget::settings::item(
                "Duration (1–30 seconds)",
                widget::text_input("5", &self.duration)
                    .on_input(Message::Duration)
                    .width(100),
            ))
            .push(widget::settings::item(
                "Typing time limit (30–600 seconds)",
                widget::text_input("120", &self.typing_limit)
                    .on_input(Message::TypingLimit)
                    .width(100),
            ))
            .push(widget::settings::item(
                "Text size (22–72 px)",
                widget::text_input("36", &self.text_size)
                    .on_input(Message::TextSize)
                    .width(100),
            ))
            .push(widget::settings::item(
                "Skip after idle (seconds)",
                widget::text_input("120", &self.idle_secs)
                    .on_input(Message::IdleSecs)
                    .width(100),
            ))
            .push(widget::text::heading("Quotes — bullet-separated"))
            .push(widget::text("Start each quote with “- ” at the left edge.\nFollowing lines and paragraphs stay in that quote.").size(12))
            .push(
                widget::text_editor::text_editor(&self.quotes)
                    .height(150)
                    .on_action(Message::Quotes),
            )
            .push(
                widget::text("1–100 quotes, up to 500 characters each. Rotate in order.").size(12),
            )
            .push(widget::button::suggested("Save settings").on_press(Message::Save))
            .push(
                widget::text(
                    "Paused on lock, idle, or sleep. A full interval starts after returning.",
                )
                .size(12),
            );
        if self.dirty {
            content =
                content.push(widget::text("Unsaved edits — Save settings to apply.").size(12));
        }
        if let Some(error) = self.form_error.as_ref().or(self.status.error.as_ref()) {
            content = content.push(widget::text(format!("⚠ {error}")).size(13));
        }
        widget::scrollable(content.padding(16))
            .width(iced::Length::Fill)
            .into()
    }

    fn sync_status(&mut self, status: Status) {
        if !self.dirty && status.settings != self.status.settings {
            self.interval = status.settings.interval_secs.to_string();
            self.duration = status.settings.duration_secs.to_string();
            self.typing_limit = status.settings.typing_limit_secs.to_string();
            self.text_size = status.settings.text_size.to_string();
            self.idle_secs = status.settings.idle_secs.to_string();
            self.quotes = widget::text_editor::Content::with_text(&config::format_quotes(
                &status.settings.quotes,
            ));
        }
        self.status = status;
    }

    fn candidate(&self) -> Result<Settings, String> {
        let settings = Settings {
            interval_secs: self
                .interval
                .parse()
                .map_err(|_| "Interval needs an integer.")?,
            duration_secs: self
                .duration
                .parse()
                .map_err(|_| "Duration needs an integer.")?,
            typing_limit_secs: self
                .typing_limit
                .parse()
                .map_err(|_| "Typing time limit needs an integer.")?,
            text_size: self
                .text_size
                .parse()
                .map_err(|_| "Text size needs an integer.")?,
            idle_secs: self
                .idle_secs
                .parse()
                .map_err(|_| "Idle threshold needs an integer.")?,
            quotes: config::parse_quotes(&self.quotes.text())?,
            ..self.status.settings.clone()
        };
        settings.validate()?;
        Ok(settings)
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
            status: engine.status(),
            engine,
            interval: settings.interval_secs.to_string(),
            duration: settings.duration_secs.to_string(),
            typing_limit: settings.typing_limit_secs.to_string(),
            text_size: settings.text_size.to_string(),
            idle_secs: settings.idle_secs.to_string(),
            quotes: widget::text_editor::Content::with_text(&config::format_quotes(
                &settings.quotes,
            )),
            form_error: None,
            dirty: false,
        };
        let task = if app.standalone {
            app.set_header_title("Naal Focus".into());
            app.core
                .main_window_id()
                .map(|id| app.set_window_title("Naal Focus".into(), id))
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
            return self.controls();
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
        self.core.applet.popup_container(self.controls()).into()
    }
    fn subscription(&self) -> Subscription<Message> {
        iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick)
    }
    fn update(&mut self, message: Message) -> Task<cosmic::Action<Message>> {
        match message {
            Message::Tick => self.sync_status(self.engine.status()),
            Message::Enabled(enabled) => {
                self.engine.request(Request::Enabled(enabled));
                self.form_error = None;
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
                self.interval = text;
                self.dirty = true;
            }
            Message::Duration(text) => {
                self.duration = text;
                self.dirty = true;
            }
            Message::TypingLimit(text) => {
                self.typing_limit = text;
                self.dirty = true;
            }
            Message::TextSize(text) => {
                self.text_size = text;
                self.dirty = true;
            }
            Message::IdleSecs(text) => {
                self.idle_secs = text;
                self.dirty = true;
            }
            Message::Quotes(action) => {
                self.dirty |= action.is_edit();
                self.quotes.perform(action);
            }
            Message::Save => match self.candidate() {
                Ok(settings) => {
                    self.engine.request(Request::Settings(settings));
                    self.form_error = None;
                    self.dirty = false;
                }
                Err(error) => self.form_error = Some(error),
            },
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
                    .min_width(390.0)
                    .max_width(420.0)
                    .min_height(640.0)
                    .max_height(760.0);
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
        status.settings.enabled = true;
        status.snoozed_secs = Some(900);
        assert!(matches!(resume_request(&status), Some(Request::Resume)));
        status.snoozed_secs = None;
        assert!(resume_request(&status).is_none());
    }
}
