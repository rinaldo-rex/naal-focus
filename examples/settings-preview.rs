use config::Settings;
use cosmic::{
    Element,
    iced::{self, Size},
};
use engine::Status;
use naal_focus::{
    applet::{Message, SettingsMessage},
    breathing, config, engine,
};

// Render the production form without starting a supervisor or reading user data.
#[allow(dead_code)]
#[path = "../src/applet/settings.rs"]
mod settings;
#[path = "../src/applet/settings_view.rs"]
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

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        iced::window::open_events().map(CaptureMessage::Opened)
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
                self.navigation.model.activate_position(self.step as u16);
                if let Some(id) = self.core.main_window_id() {
                    return screenshot(id);
                }
            }
            CaptureMessage::Ignore(message) => {
                let _ = message;
            }
        }
        cosmic::app::Task::none()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
            .exit_on_close(true),
        output,
    )?;
    Ok(())
}
