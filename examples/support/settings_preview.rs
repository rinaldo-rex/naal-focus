use config::Settings;
use cosmic::{
    Element,
    iced::{self, Size},
};
use engine::Status;
use naal_focus::{
    applet::{Message, SettingsMessage},
    config, engine,
};

// Render the production form without starting a supervisor or reading user data.
#[allow(dead_code)]
#[path = "../../src/applet/settings.rs"]
mod settings;
#[path = "../../src/applet/settings_view.rs"]
mod settings_view;
use settings::{Navigation, SettingsForm};

#[derive(Debug, Clone)]
enum CaptureMessage {
    Opened(iced::window::Id),
    Screenshot(iced::window::Screenshot),
    Ignore(Message),
}

struct Capture {
    core: cosmic::Core,
    form: SettingsForm,
    navigation: Navigation,
    status: Status,
    step: usize,
    output: std::path::PathBuf,
}

// Capture after paint without polling and re-laying out the editor between frames.
fn screenshot(id: iced::window::Id) -> cosmic::app::Task<CaptureMessage> {
    cosmic::Task::perform(
        async {
            std::thread::sleep(std::time::Duration::from_secs(1));
        },
        |()| (),
    )
    .then(move |()| {
        iced::window::screenshot(id)
            .map(|shot| cosmic::Action::App(CaptureMessage::Screenshot(shot)))
    })
}

impl cosmic::Application for Capture {
    type Executor = cosmic::executor::Default;
    type Flags = std::path::PathBuf;
    type Message = CaptureMessage;
    const APP_ID: &'static str = "io.github.rinaldo_rex.NaalFocus.SettingsCapture";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }
    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(core: cosmic::Core, output: Self::Flags) -> (Self, cosmic::app::Task<Self::Message>) {
        let status = Status {
            settings: Settings::default(),
            message: "Reminders paused.".into(),
            error: None,
            focus_error: None,
            running: false,
            safe: true,
            next_secs: 300,
            snoozed_secs: None,
        };
        let mut app = Self {
            core,
            form: SettingsForm::new(&status.settings),
            navigation: Navigation::default(),
            status,
            step: 0,
            output,
        };
        app.core.set_header_title("Naal Focus Settings".into());
        (app, cosmic::app::Task::none())
    }

    fn view(&self) -> Element<'_, Self::Message> {
        settings_view::view(&self.form, &self.navigation, &self.status, false)
            .map(CaptureMessage::Ignore)
    }

    fn dialog(&self) -> Option<Element<'_, Self::Message>> {
        settings_view::close_dialog(&self.form, &self.status)
            .map(|dialog| dialog.map(|message| CaptureMessage::Ignore(Message::Settings(message))))
    }

    fn on_app_exit(&mut self) -> Option<Self::Message> {
        Some(CaptureMessage::Ignore(Message::Settings(
            SettingsMessage::RequestClose,
        )))
    }

    fn on_escape(&mut self) -> cosmic::app::Task<Self::Message> {
        self.form
            .update(SettingsMessage::KeepEditing, &self.status.settings);
        cosmic::app::Task::none()
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        iced::Subscription::batch([
            iced::window::open_events().map(CaptureMessage::Opened),
            iced::window::events().map(|(id, event)| {
                CaptureMessage::Ignore(match event {
                    iced::window::Event::CloseRequested => Message::SettingsCloseRequested(id),
                    iced::window::Event::Closed => Message::SettingsClosed(id),
                    _ => Message::WindowFocus { id, focused: true },
                })
            }),
        ])
    }

    fn update(&mut self, message: Self::Message) -> cosmic::app::Task<Self::Message> {
        match message {
            CaptureMessage::Opened(id) => return screenshot(id),
            CaptureMessage::Screenshot(shot) => {
                use std::io::Write;
                let names = [
                    "reminders",
                    "breathing",
                    "reflections",
                    "charts",
                    "focus",
                    "appearance",
                    "quotes",
                    "unsaved",
                    "unsaved-invalid",
                    "unsaved-saving",
                ];
                let path = self
                    .output
                    .join(format!("settings-{}.ppm", names[self.step]));
                let mut file = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
                write!(file, "P6\n{} {}\n255\n", shot.size.width, shot.size.height).unwrap();
                for pixel in shot.rgba.chunks_exact(4) {
                    file.write_all(&pixel[..3]).unwrap();
                }
                file.flush().unwrap();
                println!("Captured {} at scale {}", path.display(), shot.scale_factor);
                self.step += 1;
                if self.step == names.len() {
                    return iced::exit();
                }
                if self.step < 7 {
                    self.navigation.model.activate_position(self.step as u16);
                } else if self.step == 7 {
                    self.navigation.model.activate_position(0);
                    self.form.update(
                        SettingsMessage::IntervalMinutes("6".into()),
                        &self.status.settings,
                    );
                    self.form
                        .update(SettingsMessage::RequestClose, &self.status.settings);
                } else if self.step == 8 {
                    self.form.update(
                        SettingsMessage::TextSize("invalid".into()),
                        &self.status.settings,
                    );
                } else {
                    self.form.update(
                        SettingsMessage::TextSize("36".into()),
                        &self.status.settings,
                    );
                    self.form
                        .update(SettingsMessage::SaveAndClose, &self.status.settings);
                }
                if let Some(id) = self.core.main_window_id() {
                    return screenshot(id);
                }
            }
            CaptureMessage::Ignore(Message::Settings(message)) => {
                if let Some(settings) = self.form.update(message, &self.status.settings) {
                    // Demo acknowledgment only: never write user preferences.
                    self.status.settings = settings;
                    self.form.sync(&self.status.settings, None);
                }
                if self.form.close == settings::CloseState::Ready {
                    return iced::exit();
                }
            }
            CaptureMessage::Ignore(Message::SettingsPage(id)) => self.navigation.select(id),
            CaptureMessage::Ignore(Message::SettingsCloseRequested(_)) => {
                return self.update(CaptureMessage::Ignore(Message::Settings(
                    SettingsMessage::RequestClose,
                )));
            }
            CaptureMessage::Ignore(Message::SettingsClosed(_)) => return iced::exit(),
            CaptureMessage::Ignore(_) => {}
        }
        cosmic::app::Task::none()
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let output = std::path::PathBuf::from(args.next().ok_or("Output directory is required")?);
    std::fs::create_dir_all(&output)?;
    let width = args
        .next()
        .unwrap_or_else(|| "1000".into())
        .parse::<f32>()?;
    let height = args.next().unwrap_or_else(|| "660".into()).parse::<f32>()?;
    cosmic::app::run::<Capture>(
        cosmic::app::Settings::default()
            .size(Size::new(width, height))
            .theme(cosmic::Theme::dark())
            .exit_on_close(false),
        output,
    )?;
    Ok(())
}
