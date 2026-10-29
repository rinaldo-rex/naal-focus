//! Settings drafts stay local until an explicit, successful shared save.
use super::SettingsMessage;
use crate::config::{self, ChartSettings, FocusSettings, Settings, limits};
use cosmic::widget;
use std::ops::RangeInclusive;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    Reminders,
    Breathing,
    Reflections,
    Charts,
    Focus,
    Appearance,
    Quotes,
}

impl Page {
    pub const ALL: [Self; 7] = [
        Self::Reminders,
        Self::Breathing,
        Self::Reflections,
        Self::Charts,
        Self::Focus,
        Self::Appearance,
        Self::Quotes,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Reminders => "Reminders",
            Self::Breathing => "Breathing",
            Self::Reflections => "Reflections",
            Self::Charts => "Reflection charts",
            Self::Focus => "Window history",
            Self::Appearance => "Appearance",
            Self::Quotes => "Quotes",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Reminders => "accessories-clock-symbolic",
            Self::Breathing => "weather-windy-symbolic",
            Self::Reflections => "document-edit-symbolic",
            Self::Charts => "view-list-symbolic",
            Self::Focus => "video-display-symbolic",
            Self::Appearance => "font-symbolic",
            Self::Quotes => "insert-text-symbolic",
        }
    }
}

pub(super) struct Navigation {
    pub model: widget::nav_bar::Model,
}

impl Default for Navigation {
    fn default() -> Self {
        let mut model = widget::nav_bar::Model::default();
        for page in Page::ALL {
            let id = model
                .insert()
                .text(page.title())
                .icon(widget::icon::from_name(page.icon()).symbolic(true).size(16))
                .data(page)
                .id();
            if matches!(page, Page::Focus | Page::Appearance) {
                model.divider_above_set(id, true);
            }
        }
        model.activate_position(0);
        Self { model }
    }
}

impl Navigation {
    pub fn page(&self) -> Page {
        self.model
            .data(self.model.active())
            .copied()
            .unwrap_or(Page::Reminders)
    }

    pub fn select(&mut self, id: widget::nav_bar::Id) {
        if self.model.data::<Page>(id).is_some() {
            self.model.activate(id);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TimeInput {
    pub minutes: String,
    pub seconds: String,
}

impl TimeInput {
    pub fn new(seconds: u64) -> Self {
        Self {
            minutes: (seconds / 60).to_string(),
            seconds: (seconds % 60).to_string(),
        }
    }

    pub fn parse(&self, name: &str, range: RangeInclusive<u64>) -> Result<u64, String> {
        let minutes = self.minutes.parse::<u64>();
        let seconds = self.seconds.parse::<u64>();
        if let (Ok(minutes), Ok(seconds)) = (minutes, seconds)
            && seconds < 60
            && let Some(total) = minutes.checked_mul(60).and_then(|m| m.checked_add(seconds))
            && range.contains(&total)
        {
            return Ok(total);
        }
        let (min, max) = range.into_inner();
        Err(format!(
            "{name}: use whole minutes and 0–59 seconds ({min}–{max} seconds total)."
        ))
    }
}

/// Live per-field message for one numeric settings input, matching the
/// ranges enforced by [`Settings::validate`]. `None` means the draft value
/// can be saved.
pub fn numeric_field_error(value: &str, range: RangeInclusive<u64>) -> Option<String> {
    match value.trim().parse::<u64>() {
        Ok(number) if range.contains(&number) => None,
        _ => {
            let (min, max) = range.into_inner();
            Some(format!("Must be a whole number from {min} to {max}."))
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum CloseState {
    #[default]
    Open,
    Confirm,
    Saving,
    Ready,
}

pub(super) struct SettingsForm {
    pub interval: TimeInput,
    pub duration: String,
    pub reflections_enabled: bool,
    pub typing_limit: TimeInput,
    pub text_size: String,
    pub idle_secs: TimeInput,
    pub charts: ChartSettings,
    pub focus: FocusSettings,
    pub breathing: crate::breathing::Settings,
    pub quotes: widget::text_editor::Content<cosmic::Renderer>,
    pub error: Option<String>,
    pub dirty: bool,
    pub close: CloseState,
    pending_save: Option<Settings>,
}

impl SettingsForm {
    pub fn new(settings: &Settings) -> Self {
        Self {
            interval: TimeInput::new(settings.interval_secs),
            duration: settings.duration_secs.to_string(),
            reflections_enabled: settings.reflections_enabled,
            typing_limit: TimeInput::new(settings.typing_limit_secs),
            text_size: settings.text_size.to_string(),
            idle_secs: TimeInput::new(u64::from(settings.idle_secs)),
            charts: settings.charts.clone(),
            focus: settings.focus.clone(),
            breathing: settings.breathing.clone(),
            quotes: widget::text_editor::Content::with_text(&config::format_quotes(
                &settings.quotes,
            )),
            error: None,
            dirty: false,
            close: CloseState::Open,
            pending_save: None,
        }
    }

    pub fn update(&mut self, message: SettingsMessage, settings: &Settings) -> Option<Settings> {
        match message {
            SettingsMessage::RequestClose => {
                if self.close == CloseState::Open {
                    self.close =
                        if self.dirty && self.candidate(settings).as_ref().ok() != Some(settings) {
                            CloseState::Confirm
                        } else {
                            CloseState::Ready
                        };
                }
                return None;
            }
            SettingsMessage::KeepEditing => {
                self.close = CloseState::Open;
                return None;
            }
            SettingsMessage::Discard => {
                if self.close == CloseState::Confirm {
                    self.close = CloseState::Ready;
                }
                return None;
            }
            SettingsMessage::SaveAndClose => {
                if self.close != CloseState::Confirm {
                    return None;
                }
                let saved = self.save(settings).ok()?;
                self.close = CloseState::Saving;
                return Some(saved);
            }
            SettingsMessage::IntervalMinutes(text) => self.interval.minutes = text,
            SettingsMessage::IntervalSeconds(text) => self.interval.seconds = text,
            SettingsMessage::Duration(text) => self.duration = text,
            SettingsMessage::TypingMinutes(text) => self.typing_limit.minutes = text,
            SettingsMessage::TypingSeconds(text) => self.typing_limit.seconds = text,
            SettingsMessage::ReflectionsEnabled(enabled) => self.reflections_enabled = enabled,
            SettingsMessage::TextSize(text) => self.text_size = text,
            SettingsMessage::IdleMinutes(text) => self.idle_secs.minutes = text,
            SettingsMessage::IdleSeconds(text) => self.idle_secs.seconds = text,
            SettingsMessage::ChartStyle(style) => self.charts.style = style,
            SettingsMessage::ChartProjects(enabled) => self.charts.projects = enabled,
            SettingsMessage::ChartFeelings(enabled) => self.charts.feelings = enabled,
            SettingsMessage::ChartTags(enabled) => self.charts.tags = enabled,
            SettingsMessage::FocusEnabled(enabled) => self.focus.enabled = enabled,
            SettingsMessage::FocusStyle(style) => self.focus.style = style,
            SettingsMessage::FocusSide(side) => self.focus.side = side,
            SettingsMessage::Focus24Hour(enabled) => self.focus.use_24_hour_time = enabled,
            SettingsMessage::BreathingEnabled(enabled) => self.breathing.enabled = enabled,
            SettingsMessage::BreathingReducedMotion(enabled) => {
                self.breathing.reduced_motion = enabled
            }
            SettingsMessage::BreathingPace(pace) => self.breathing.pace = pace,
            SettingsMessage::BreathingCycles(cycles) => {
                if !crate::breathing::Settings::CYCLES.contains(&cycles) {
                    return None;
                }
                self.breathing.cycles = cycles;
            }
            SettingsMessage::BreathingFamily(family, enabled) => {
                self.breathing.families.retain(|f| *f != family);
                if enabled {
                    self.breathing.families.push(family);
                }
            }
            SettingsMessage::Quotes(action) => {
                self.dirty |= action.is_edit();
                self.quotes.perform(action);
                return None;
            }
            SettingsMessage::Save => return self.save(settings).ok(),
        }
        self.dirty = true;
        None
    }

    pub fn sync(&mut self, settings: &Settings, error: Option<&str>) -> bool {
        if self.pending_save.as_ref() == Some(settings) && error.is_none() {
            self.pending_save = None;
            // Typing after Save must not be erased by its later acknowledgment.
            self.dirty = self.candidate(settings).as_ref().ok() != Some(settings);
            if matches!(self.close, CloseState::Confirm | CloseState::Saving) {
                self.close = if self.dirty {
                    CloseState::Confirm
                } else {
                    CloseState::Ready
                };
            }
            return true;
        }
        if error.is_some() && self.close == CloseState::Saving {
            self.close = CloseState::Confirm;
        }
        if !self.dirty {
            // Avoid rebuilding the editor and losing its cursor on every tick.
            if self.candidate(settings).as_ref().ok() != Some(settings) {
                *self = Self::new(settings);
            }
        }
        false
    }

    pub fn can_auto_save(&self) -> bool {
        // Losing focus at the close prompt must not bypass the Discard choice.
        self.dirty && self.close == CloseState::Open
    }

    pub fn save(&mut self, settings: &Settings) -> Result<Settings, String> {
        match self.candidate(settings) {
            Ok(candidate) => {
                self.pending_save = Some(candidate.clone());
                // Stay dirty until the supervisor reports the successful save.
                self.dirty = true;
                self.error = None;
                Ok(candidate)
            }
            Err(error) => {
                self.error = Some(error.clone());
                Err(error)
            }
        }
    }

    pub fn validation_error(&self, settings: &Settings) -> Option<String> {
        self.candidate(settings).err()
    }

    fn candidate(&self, settings: &Settings) -> Result<Settings, String> {
        let candidate = Settings {
            interval_secs: self.interval.parse("Reminders", limits::INTERVAL_SECS)?,
            duration_secs: self
                .duration
                .parse()
                .map_err(|_| "Duration needs an integer.")?,
            typing_limit_secs: self
                .typing_limit
                .parse("Reflections", limits::TYPING_LIMIT_SECS)?,
            text_size: self
                .text_size
                .parse()
                .map_err(|_| "Text size needs an integer.")?,
            idle_secs: self.idle_secs.parse("Idle threshold", limits::IDLE_SECS)? as u32,
            reflections_enabled: self.reflections_enabled,
            charts: self.charts.clone(),
            focus: self.focus.clone(),
            breathing: self.breathing.clone(),
            quotes: config::parse_quotes(&self.quotes.text())?,
            // Enabling reminders is a quick action, not part of this draft.
            ..settings.clone()
        };
        candidate.validate()?;
        Ok(candidate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_is_immediate_only_for_unchanged_settings() {
        let original = Settings::default();
        let mut form = SettingsForm::new(&original);
        assert!(
            form.update(SettingsMessage::RequestClose, &original)
                .is_none()
        );
        assert_eq!(form.close, CloseState::Ready);
        let mut form = SettingsForm::new(&original);
        form.update(SettingsMessage::IntervalMinutes("6".into()), &original);
        form.update(SettingsMessage::IntervalMinutes("5".into()), &original);
        form.update(SettingsMessage::RequestClose, &original);
        assert_eq!(form.close, CloseState::Ready);
        assert!(form.pending_save.is_none());
    }

    #[test]
    fn close_warning_blocks_auto_save_and_keep_editing_preserves_the_draft() {
        let original = Settings::default();
        let mut form = SettingsForm::new(&original);
        form.update(SettingsMessage::IntervalMinutes("6".into()), &original);
        assert!(form.can_auto_save());
        assert!(
            form.update(SettingsMessage::RequestClose, &original)
                .is_none()
        );
        assert_eq!(form.close, CloseState::Confirm);
        assert!(!form.can_auto_save());
        assert!(form.pending_save.is_none());
        form.update(SettingsMessage::KeepEditing, &original);
        assert_eq!(form.close, CloseState::Open);
        assert!(form.can_auto_save());
        assert_eq!(form.interval, TimeInput::new(360));
    }

    #[test]
    fn discard_closes_without_submitting_even_an_invalid_draft() {
        let original = Settings::default();
        let mut form = SettingsForm::new(&original);
        form.update(SettingsMessage::TextSize("invalid".into()), &original);
        form.update(SettingsMessage::RequestClose, &original);
        assert_eq!(form.close, CloseState::Confirm);
        assert!(form.update(SettingsMessage::Discard, &original).is_none());
        assert_eq!(form.close, CloseState::Ready);
        assert!(form.pending_save.is_none());
    }

    #[test]
    fn save_and_close_waits_for_acknowledgment_and_recovers_from_failure() {
        let original = Settings::default();
        let mut form = SettingsForm::new(&original);
        form.update(SettingsMessage::IntervalMinutes("6".into()), &original);
        form.update(SettingsMessage::RequestClose, &original);
        let saved = form
            .update(SettingsMessage::SaveAndClose, &original)
            .unwrap();
        assert_eq!(form.close, CloseState::Saving);
        assert!(!form.can_auto_save());
        assert!(
            form.update(SettingsMessage::SaveAndClose, &original)
                .is_none()
        );
        assert!(!form.sync(&original, None));
        assert_eq!(form.close, CloseState::Saving);
        assert!(!form.sync(&original, Some("Cannot save settings")));
        assert_eq!(form.close, CloseState::Confirm);
        assert!(form.dirty);
        assert_eq!(form.interval, TimeInput::new(360));
        assert_eq!(
            form.update(SettingsMessage::SaveAndClose, &original),
            Some(saved.clone())
        );
        assert!(form.sync(&saved, None));
        assert_eq!(form.close, CloseState::Ready);
        assert!(!form.dirty);
    }

    #[test]
    fn invalid_save_and_close_keeps_the_warning_and_draft() {
        let original = Settings::default();
        let mut form = SettingsForm::new(&original);
        form.update(SettingsMessage::TextSize("invalid".into()), &original);
        form.update(SettingsMessage::RequestClose, &original);
        assert!(
            form.update(SettingsMessage::SaveAndClose, &original)
                .is_none()
        );
        assert_eq!(form.close, CloseState::Confirm);
        assert!(form.pending_save.is_none());
        assert_eq!(form.text_size, "invalid");
        assert_eq!(form.error.as_deref(), Some("Text size needs an integer."));
    }

    #[test]
    fn keep_editing_cancels_a_pending_close_but_not_an_explicit_save() {
        let original = Settings::default();
        let mut form = SettingsForm::new(&original);
        form.update(SettingsMessage::IntervalMinutes("6".into()), &original);
        form.update(SettingsMessage::RequestClose, &original);
        let saved = form
            .update(SettingsMessage::SaveAndClose, &original)
            .unwrap();
        form.update(SettingsMessage::KeepEditing, &original);
        assert_eq!(form.close, CloseState::Open);
        assert!(form.sync(&saved, None));
        assert_eq!(form.close, CloseState::Open);
        assert!(!form.dirty);
    }

    #[test]
    fn edits_after_save_and_close_require_another_confirmation() {
        let original = Settings::default();
        let mut form = SettingsForm::new(&original);
        form.update(SettingsMessage::IntervalMinutes("6".into()), &original);
        form.update(SettingsMessage::RequestClose, &original);
        let saved = form
            .update(SettingsMessage::SaveAndClose, &original)
            .unwrap();
        form.update(SettingsMessage::IntervalSeconds("1".into()), &original);
        assert!(form.sync(&saved, None));
        assert_eq!(form.close, CloseState::Confirm);
        assert!(form.dirty);
        assert_eq!(form.interval, TimeInput::new(361));
    }

    #[test]
    fn field_messages_edit_only_the_draft_and_save_through_one_handler() {
        use crate::{
            breathing::Pace,
            config::{ChartStyle, FocusSide, FocusStyle},
        };
        type ReadValue = fn(&Settings) -> String;
        let cases: Vec<(SettingsMessage, ReadValue, &str)> = vec![
            (
                SettingsMessage::IntervalMinutes("6".into()),
                |s| s.interval_secs.to_string(),
                "360",
            ),
            (
                SettingsMessage::IntervalSeconds("1".into()),
                |s| s.interval_secs.to_string(),
                "301",
            ),
            (
                SettingsMessage::Duration("6".into()),
                |s| s.duration_secs.to_string(),
                "6",
            ),
            (
                SettingsMessage::TypingMinutes("3".into()),
                |s| s.typing_limit_secs.to_string(),
                "180",
            ),
            (
                SettingsMessage::TypingSeconds("1".into()),
                |s| s.typing_limit_secs.to_string(),
                "121",
            ),
            (
                SettingsMessage::TextSize("44".into()),
                |s| s.text_size.to_string(),
                "44",
            ),
            (
                SettingsMessage::IdleMinutes("3".into()),
                |s| s.idle_secs.to_string(),
                "180",
            ),
            (
                SettingsMessage::IdleSeconds("1".into()),
                |s| s.idle_secs.to_string(),
                "121",
            ),
            (
                SettingsMessage::ReflectionsEnabled(false),
                |s| s.reflections_enabled.to_string(),
                "false",
            ),
            (
                SettingsMessage::ChartStyle(ChartStyle::Treemap),
                |s| s.charts.style.as_str().into(),
                "treemap",
            ),
            (
                SettingsMessage::ChartProjects(true),
                |s| s.charts.projects.to_string(),
                "true",
            ),
            (
                SettingsMessage::ChartFeelings(true),
                |s| s.charts.feelings.to_string(),
                "true",
            ),
            (
                SettingsMessage::ChartTags(true),
                |s| s.charts.tags.to_string(),
                "true",
            ),
            (
                SettingsMessage::FocusEnabled(true),
                |s| s.focus.enabled.to_string(),
                "true",
            ),
            (
                SettingsMessage::FocusStyle(FocusStyle::Timeline),
                |s| s.focus.style.as_str().into(),
                "timeline",
            ),
            (
                SettingsMessage::FocusSide(FocusSide::Left),
                |s| s.focus.side.as_str().into(),
                "left",
            ),
            (
                SettingsMessage::Focus24Hour(true),
                |s| s.focus.use_24_hour_time.to_string(),
                "true",
            ),
            (
                SettingsMessage::BreathingEnabled(false),
                |s| s.breathing.enabled.to_string(),
                "false",
            ),
            (
                SettingsMessage::BreathingReducedMotion(true),
                |s| s.breathing.reduced_motion.to_string(),
                "true",
            ),
            (
                SettingsMessage::BreathingPace(Pace::Short),
                |s| s.breathing.pace.as_str().into(),
                "short",
            ),
            (
                SettingsMessage::BreathingCycles(2),
                |s| s.breathing.cycles.to_string(),
                "2",
            ),
        ];
        let original = Settings::default();
        for (message, read, expected) in cases {
            let mut form = SettingsForm::new(&original);
            assert!(form.update(message, &original).is_none());
            assert!(form.dirty);
            assert!(form.pending_save.is_none());
            let saved = form.update(SettingsMessage::Save, &original).unwrap();
            assert_eq!(read(&saved), expected);
            assert_eq!(form.pending_save.as_ref(), Some(&saved));
        }
    }

    #[test]
    fn grouped_updates_preserve_pending_edits_and_failed_save_drafts() {
        let original = Settings::default();
        let mut form = SettingsForm::new(&original);
        form.update(SettingsMessage::IntervalMinutes("6".into()), &original);
        let saved = form.update(SettingsMessage::Save, &original).unwrap();
        form.update(SettingsMessage::IntervalSeconds("1".into()), &saved);
        assert!(form.sync(&saved, None));
        assert!(form.dirty);
        assert_eq!(form.interval, TimeInput::new(361));
        form.update(SettingsMessage::TextSize("invalid".into()), &saved);
        assert!(form.update(SettingsMessage::Save, &saved).is_none());
        assert_eq!(form.text_size, "invalid");
        assert!(form.dirty);
        assert!(form.pending_save.is_none());
        assert_eq!(form.error.as_deref(), Some("Text size needs an integer."));
    }

    #[test]
    fn breathing_edits_keep_family_order_and_ignore_invalid_counts() {
        use crate::breathing::Family;
        let original = Settings::default();
        let mut form = SettingsForm::new(&original);
        for cycles in [0, 3, u8::MAX] {
            assert!(
                form.update(SettingsMessage::BreathingCycles(cycles), &original)
                    .is_none()
            );
            assert_eq!(form.breathing.cycles, 1);
            assert!(!form.dirty);
        }
        form.update(
            SettingsMessage::BreathingFamily(Family::Lattice, true),
            &original,
        );
        assert_eq!(form.breathing.families.last(), Some(&Family::Lattice));
        assert_eq!(form.breathing.families.len(), Family::ALL.len());
        for family in Family::ALL {
            form.update(SettingsMessage::BreathingFamily(family, false), &original);
        }
        assert!(form.update(SettingsMessage::Save, &original).is_none());
        assert_eq!(
            form.error.as_deref(),
            Some("Choose at least one breathing pattern.")
        );
    }

    #[test]
    fn navigation_has_seven_direct_pages_and_does_not_touch_the_draft() {
        let original = Settings::default();
        let mut form = SettingsForm::new(&original);
        let mut navigation = Navigation::default();
        assert_eq!(navigation.page(), Page::Reminders);
        form.interval = TimeInput::new(601);
        form.quotes = widget::text_editor::Content::with_text("- Unsaved quote.");
        form.dirty = true;
        let pending = form.save(&original).unwrap();
        form.interval.seconds = "2".into();
        let ids: Vec<_> = navigation.model.iter().collect();
        assert_eq!(ids.len(), Page::ALL.len());
        for (id, page) in ids.into_iter().zip(Page::ALL) {
            navigation.select(id);
            assert_eq!(navigation.page(), page);
            assert_eq!(form.interval, TimeInput::new(602));
            assert!(form.dirty);
            assert!(form.error.is_none());
            assert_eq!(form.pending_save.as_ref(), Some(&pending));
            assert_eq!(
                config::parse_quotes(&form.quotes.text()).unwrap(),
                ["Unsaved quote."]
            );
        }
        assert!(form.sync(&pending, None));
        assert!(form.dirty);
        assert_eq!(form.interval, TimeInput::new(602));
    }

    #[test]
    fn minutes_and_seconds_preserve_every_supported_second() {
        for seconds in 15..=86_400 {
            let input = TimeInput::new(seconds);
            assert_eq!(input.parse("Interval", limits::INTERVAL_SECS), Ok(seconds));
        }
        for (min, max) in [(30, 600), (30, 3_600)] {
            for seconds in [min, min + 1, 59, 60, 61, max - 1, max] {
                assert_eq!(
                    TimeInput::new(seconds).parse("Time", min..=max),
                    Ok(seconds)
                );
            }
            assert!(TimeInput::new(min - 1).parse("Time", min..=max).is_err());
            assert!(TimeInput::new(max + 1).parse("Time", min..=max).is_err());
        }
    }

    #[test]
    fn invalid_time_parts_cannot_save_or_overflow() {
        for (minutes, seconds) in [
            ("", "0"),
            ("1.5", "0"),
            ("-1", "0"),
            ("1", "60"),
            ("1", ""),
            ("1", "0.5"),
            ("18446744073709551615", "59"),
        ] {
            let input = TimeInput {
                minutes: minutes.into(),
                seconds: seconds.into(),
            };
            assert!(input.parse("Time", limits::INTERVAL_SECS).is_err());
        }
    }

    #[test]
    fn time_fields_keep_non_minute_preferences_and_validate_hidden_pages() {
        let original = Settings {
            interval_secs: 301,
            idle_secs: 121,
            typing_limit_secs: 119,
            ..Settings::default()
        };
        let mut form = SettingsForm::new(&original);
        assert_eq!(form.save(&original).unwrap(), original);
        form.typing_limit.seconds = "60".into();
        assert!(
            form.validation_error(&original)
                .unwrap()
                .contains("Reflections")
        );
        assert!(form.save(&original).is_err());
        form.typing_limit = TimeInput::new(119);
        assert!(form.validation_error(&original).is_none());
    }

    #[test]
    fn numeric_field_error_reports_unsaveable_values_only() {
        assert!(numeric_field_error("600", limits::INTERVAL_SECS).is_none());
        assert_eq!(
            numeric_field_error("5", limits::INTERVAL_SECS).as_deref(),
            Some("Must be a whole number from 15 to 86400.")
        );
        assert!(numeric_field_error("86401", limits::INTERVAL_SECS).is_some());
        assert!(numeric_field_error("", limits::INTERVAL_SECS).is_some());
        assert!(numeric_field_error("abc", limits::INTERVAL_SECS).is_some());
        assert!(numeric_field_error("1.5", limits::INTERVAL_SECS).is_some());
        assert!(numeric_field_error("-1", limits::INTERVAL_SECS).is_some());
    }

    #[test]
    fn sync_reports_when_a_save_is_acknowledged() {
        let mut form = SettingsForm::new(&Settings::default());
        form.interval = TimeInput::new(600);
        let saved = form.save(&Settings::default()).unwrap();
        assert!(!form.sync(&Settings::default(), None));
        assert!(form.sync(&saved, None));
        assert!(form.pending_save.is_none());
    }

    #[test]
    fn clean_form_tracks_shared_settings_but_dirty_draft_is_preserved() {
        let mut form = SettingsForm::new(&Settings::default());
        let settings = Settings {
            interval_secs: 600,
            ..Settings::default()
        };
        form.sync(&settings, None);
        assert_eq!(form.interval, TimeInput::new(600));
        form.interval = TimeInput::new(900);
        form.dirty = true;
        form.sync(&Settings::default(), None);
        assert_eq!(form.interval, TimeInput::new(900));
        assert!(form.dirty);
    }

    #[test]
    fn validation_errors_retain_draft_and_do_not_queue_a_save() {
        let mut form = SettingsForm::new(&Settings::default());
        form.interval.minutes = "not a number".into();
        form.dirty = true;
        assert!(form.save(&Settings::default()).is_err());
        assert!(form.error.as_deref().unwrap().contains("whole minutes"));
        assert!(form.pending_save.is_none());
        assert_eq!(form.interval.minutes, "not a number");
        form.interval = TimeInput::new(600);
        form.duration = "31".into();
        assert!(form.save(&Settings::default()).is_err());
        assert!(form.pending_save.is_none());
    }

    #[test]
    fn save_uses_latest_enabled_state_and_requires_successful_acknowledgment() {
        let mut form = SettingsForm::new(&Settings::default());
        form.interval = TimeInput::new(600);
        form.dirty = true;
        let latest = Settings {
            enabled: true,
            ..Settings::default()
        };
        let candidate = form.save(&latest).unwrap();
        assert!(candidate.enabled);
        form.sync(&latest, None);
        assert!(form.dirty);
        form.sync(&candidate, Some("Could not save settings"));
        assert!(form.dirty);
        assert_eq!(form.interval, TimeInput::new(600));
        form.sync(&candidate, None);
        assert!(!form.dirty);
        assert!(form.pending_save.is_none());
    }

    #[test]
    fn editing_after_save_survives_acknowledgment() {
        let original = Settings::default();
        let mut form = SettingsForm::new(&original);
        form.interval = TimeInput::new(600);
        let saved = form.save(&original).unwrap();
        form.interval = TimeInput::new(900);
        form.sync(&saved, None);
        assert_eq!(form.interval, TimeInput::new(900));
        assert!(form.dirty);
        assert!(form.pending_save.is_none());
    }

    #[test]
    fn chart_drafts_save_and_preserve_post_save_edits() {
        let original = Settings::default();
        let mut form = SettingsForm::new(&original);
        form.charts.style = config::ChartStyle::Treemap;
        form.charts.projects = true;
        form.charts.tags = true;
        form.dirty = true;
        form.sync(&original, None);
        assert!(form.charts.projects);
        let saved = form.save(&original).unwrap();
        assert_eq!(saved.charts, form.charts);
        form.charts.feelings = true;
        assert!(form.sync(&saved, None));
        assert!(form.dirty);
        assert!(form.charts.feelings);
        let mut clean = SettingsForm::new(&original);
        clean.sync(&saved, None);
        assert_eq!(clean.charts, saved.charts);
    }

    #[test]
    fn focus_preferences_keep_dirty_drafts_and_post_save_edits() {
        let original = Settings::default();
        let mut form = SettingsForm::new(&original);
        form.focus.enabled = true;
        form.focus.style = config::FocusStyle::Timeline;
        form.focus.side = config::FocusSide::Left;
        form.focus.use_24_hour_time = true;
        form.dirty = true;
        form.sync(&original, None);
        let saved = form.save(&original).unwrap();
        assert_eq!(saved.focus, form.focus);
        assert!(saved.focus.use_24_hour_time);
        form.focus.style = config::FocusStyle::Totals;
        form.focus.use_24_hour_time = false;
        assert!(form.sync(&saved, None));
        assert!(form.dirty);
        assert_eq!(form.focus.style, config::FocusStyle::Totals);
        assert!(!form.focus.use_24_hour_time);
        let mut clean = SettingsForm::new(&original);
        clean.sync(&saved, None);
        assert_eq!(clean.focus, saved.focus);
    }

    #[test]
    fn reflection_toggle_preserves_dirty_drafts_and_post_save_edits() {
        let original = Settings::default();
        let mut form = SettingsForm::new(&original);
        assert!(form.reflections_enabled);
        form.reflections_enabled = false;
        form.dirty = true;
        form.sync(&original, None);
        assert!(!form.reflections_enabled);
        let saved = form.save(&original).unwrap();
        assert!(!saved.reflections_enabled);
        assert_eq!(saved.quotes, original.quotes);
        assert_eq!(saved.charts, original.charts);
        form.reflections_enabled = true;
        assert!(form.sync(&saved, None));
        assert!(form.reflections_enabled);
        assert!(form.dirty);
        let mut clean = SettingsForm::new(&original);
        clean.sync(&saved, None);
        assert!(!clean.reflections_enabled);
    }

    #[test]
    fn breathing_preferences_keep_dirty_drafts_and_validate_families() {
        use crate::breathing::{Family, Pace};
        let original = Settings::default();
        let mut form = SettingsForm::new(&original);
        form.breathing.pace = Pace::Short;
        form.breathing.cycles = 2;
        form.breathing.reduced_motion = true;
        form.breathing.families = vec![
            Family::Weave,
            Family::EmberBloom,
            Family::MoonPool,
            Family::GlassJelly,
        ];
        form.dirty = true;
        form.sync(&original, None);
        let saved = form.save(&original).unwrap();
        assert_eq!(saved.breathing, form.breathing);
        form.breathing.enabled = false;
        assert!(form.sync(&saved, None));
        assert!(form.dirty);
        assert!(!form.breathing.enabled);
        form.breathing.families.clear();
        assert!(form.save(&original).is_err());
        let mut clean = SettingsForm::new(&original);
        clean.sync(&saved, None);
        assert_eq!(clean.breathing, saved.breathing);
    }

    #[test]
    fn multiline_quotes_round_trip_through_native_editor_and_save() {
        let settings = Settings {
            quotes: vec![
                "First line.\n\n- Literal bullet.".into(),
                "Another quote.\nAnother line.".into(),
            ],
            ..Settings::default()
        };
        let mut form = SettingsForm::new(&settings);
        assert_eq!(form.save(&settings).unwrap(), settings);
    }
}
