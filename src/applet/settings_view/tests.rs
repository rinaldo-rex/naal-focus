use super::*;
use crate::config::Settings;
use cosmic::iced::{
    self, Rectangle, Size, Vector,
    advanced::{
        Layout, layout,
        widget::{Id, Operation, Tree, operation::Scrollable},
    },
};

fn demo_status() -> Status {
    Status {
        settings: Settings::default(),
        message: "Reminders paused.".into(),
        error: None,
        focus_error: None,
        running: false,
        safe: true,
        next_secs: 300,
        snoozed_secs: None,
    }
}

#[derive(Default)]
struct Measurements {
    scrolls: Vec<(Rectangle, Rectangle)>,
    footer: Option<Rectangle>,
    save: Option<Rectangle>,
    close_buttons: Vec<(&'static str, Rectangle)>,
    texts: Vec<String>,
}

impl Operation for Measurements {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if id == Some(&Id::new("settings-footer")) {
            self.footer = Some(bounds);
        }
    }

    fn text(&mut self, _: Option<&Id>, bounds: Rectangle, text: &str) {
        self.texts.push(text.to_owned());
        if text == "Save settings" {
            self.save = Some(bounds);
        }
        for label in ["Save & close", "Discard", "Keep editing"] {
            if text == label {
                self.close_buttons.push((label, bounds));
            }
        }
    }

    fn scrollable(
        &mut self,
        _: Option<&Id>,
        bounds: Rectangle,
        content: Rectangle,
        _: Vector,
        _: &mut dyn Scrollable,
    ) {
        self.scrolls.push((bounds, content));
    }
}

#[test]
fn close_dialog_buttons_respect_validation_and_in_flight_saves() {
    let renderer = cosmic::Renderer::new(cosmic::font::default(), iced::Pixels(14.0));
    let size = Size::new(760.0, 432.0);
    for mode in 0..3 {
        for label in ["Save & close", "Discard", "Keep editing"] {
            let status = demo_status();
            let mut form = SettingsForm::new(&status.settings);
            assert!(close_dialog(&form, &status).is_none());
            form.update(
                SettingsMessage::IntervalMinutes("6".into()),
                &status.settings,
            );
            if mode == 1 {
                form.update(
                    SettingsMessage::TextSize("invalid".into()),
                    &status.settings,
                );
            }
            form.update(SettingsMessage::RequestClose, &status.settings);
            if mode == 2 {
                form.update(SettingsMessage::SaveAndClose, &status.settings)
                    .unwrap();
            }
            let mut element = close_dialog(&form, &status).unwrap();
            let mut tree = Tree::new(&element);
            let node = element.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, size),
            );
            assert!(node.size().width <= size.width && node.size().height <= size.height);
            let mut measurements = Measurements::default();
            element.as_widget_mut().operate(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &mut measurements,
            );
            if mode == 1 {
                assert!(
                    measurements
                        .texts
                        .iter()
                        .any(|text| text == "Text size needs an integer.")
                );
            }
            let position = measurements
                .close_buttons
                .iter()
                .find(|(name, _)| *name == label)
                .unwrap()
                .1
                .center();
            let mut messages = Vec::new();
            for event in [
                iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
                iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left),
            ] {
                element.as_widget_mut().update(
                    &mut tree,
                    &iced::Event::Mouse(event),
                    Layout::new(&node),
                    iced::mouse::Cursor::Available(position),
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut iced::advanced::Shell::new(&mut messages),
                    &Rectangle::with_size(size),
                );
            }
            match label {
                "Save & close" => assert_eq!(
                    messages
                        .iter()
                        .any(|m| matches!(m, SettingsMessage::SaveAndClose)),
                    mode == 0
                ),
                "Discard" => assert_eq!(
                    messages
                        .iter()
                        .any(|m| matches!(m, SettingsMessage::Discard)),
                    mode != 2
                ),
                "Keep editing" => assert!(
                    messages
                        .iter()
                        .any(|m| matches!(m, SettingsMessage::KeepEditing))
                ),
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn dropdown_labels_keep_their_values_and_order() {
    assert_eq!(
        CHART_STYLES.map(|c| (c.0, c.1)),
        [
            (ChartStyle::Heatmap, "Heatmap — hourly mentions"),
            (ChartStyle::Treemap, "Treemap — daily mentions"),
        ]
    );
    assert_eq!(
        FOCUS_STYLES.map(|c| (c.0, c.1)),
        [
            (FocusStyle::Totals, "Time spent"),
            (FocusStyle::Timeline, "Day timeline"),
            (FocusStyle::Both, "Timeline + totals"),
        ]
    );
    assert_eq!(
        FOCUS_SIDES.map(|c| (c.0, c.1)),
        [(FocusSide::Left, "Left"), (FocusSide::Right, "Right")]
    );
    assert_eq!(
        BREATH_PACES.map(|c| (c.0, c.1)),
        [
            (Pace::Short, "3s in / 4s out"),
            (Pace::Balanced, "4s in / 6s out"),
            (Pace::Even, "5s in / 5s out"),
        ]
    );
    assert_eq!(
        BREATH_COUNTS.map(|c| (c.0, c.1)),
        [(1, "One breath"), (2, "Two breaths")]
    );
}

#[test]
fn save_button_retains_corrupt_file_repair_and_validation() {
    let renderer = cosmic::Renderer::new(cosmic::font::default(), iced::Pixels(14.0));
    let size = Size::new(900.0, 552.0);
    for (dirty, invalid, load_error, can_save) in [
        (false, false, false, false),
        (true, false, false, true),
        (false, false, true, true),
        (true, true, false, false),
    ] {
        let mut status = demo_status();
        status.error = load_error.then(|| "Settings file is invalid.".into());
        let mut form = SettingsForm::new(&status.settings);
        form.dirty = dirty;
        if invalid {
            form.interval.minutes = "invalid".into();
        }
        let navigation = Navigation::default();
        let mut element = view(&form, &navigation, &status, false);
        let mut tree = Tree::new(&element);
        let node =
            element
                .as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut measurements = Measurements::default();
        element.as_widget_mut().operate(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &mut measurements,
        );
        let position = measurements.save.expect("Save button label").center();
        let mut messages = Vec::new();
        for event in [
            iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
            iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left),
        ] {
            element.as_widget_mut().update(
                &mut tree,
                &iced::Event::Mouse(event),
                Layout::new(&node),
                iced::mouse::Cursor::Available(position),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut iced::advanced::Shell::new(&mut messages),
                &Rectangle::with_size(size),
            );
        }
        assert_eq!(
            messages
                .iter()
                .any(|m| matches!(m, Message::Settings(SettingsMessage::Save))),
            can_save
        );
    }
}

#[test]
fn every_native_page_fits_desktop_sizes_with_a_visible_footer() {
    let renderer = cosmic::Renderer::new(cosmic::font::default(), iced::Pixels(14.0));
    for (width, height) in [
        (760.0, 480.0),
        (900.0, 600.0),
        (1000.0, 620.0),
        (1060.0, 660.0),
        (1440.0, 900.0),
    ] {
        // Reserve the native header bar as well as the settings view itself.
        let size = Size::new(width, height - 48.0);
        for mode in 0..5 {
            let mut status = demo_status();
            let mut form = SettingsForm::new(&status.settings);
            if mode == 1 {
                form.focus.enabled = true;
                form.reflections_enabled = false;
                form.breathing.enabled = false;
            } else if mode == 2 {
                form.interval.minutes = "invalid".into();
                form.breathing.families.clear();
                status.focus_error = Some("Window history is unavailable.".into());
            } else if mode == 3 {
                form.text_size = "72".into();
                form.focus.enabled = true;
            } else if mode == 4 {
                form.text_size = "NaN".into();
            }
            let mut navigation = Navigation::default();
            let ids: Vec<_> = navigation.model.iter().collect();
            for id in ids {
                navigation.select(id);
                let mut element = view(&form, &navigation, &status, false);
                let mut tree = Tree::new(&element);
                let node = element.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(size, size),
                );
                let mut measurements = Measurements::default();
                element.as_widget_mut().operate(
                    &mut tree,
                    Layout::new(&node),
                    &renderer,
                    &mut measurements,
                );
                let context = format!("{:?} {width}×{height}, mode {mode}", navigation.page());
                assert!(
                    node.size().width <= size.width,
                    "{context}: horizontal overflow"
                );
                for (bounds, content) in measurements.scrolls {
                    if width >= 900.0 {
                        assert!(
                            content.height <= bounds.height + 1.0,
                            "{context}: content {} > viewport {}",
                            content.height,
                            bounds.height
                        );
                    }
                    assert!(
                        content.width <= bounds.width + 1.0,
                        "{context}: horizontal scroll"
                    );
                }
                let footer = measurements.footer.expect("fixed footer");
                assert!(
                    footer.y >= 0.0 && footer.y + footer.height <= size.height + 1.0,
                    "{context}: footer is clipped {footer:?}"
                );
            }
        }
    }
}
