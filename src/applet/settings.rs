//! Settings drafts stay local until an explicit, successful shared save.
use crate::config::{self, ChartSettings, FocusSettings, Settings};
use cosmic::widget;

/// Live per-field message for one numeric settings input, matching the
/// ranges enforced by [`Settings::validate`]. `None` means the draft value
/// can be saved.
pub fn numeric_field_error(value: &str, min: u64, max: u64) -> Option<String> {
    match value.trim().parse::<u64>() {
        Ok(number) if (min..=max).contains(&number) => None,
        _ => Some(format!("Must be a whole number from {min} to {max}.")),
    }
}

pub(super) struct SettingsForm {
    pub interval: String,
    pub duration: String,
    pub typing_limit: String,
    pub text_size: String,
    pub idle_secs: String,
    pub charts: ChartSettings,
    pub focus: FocusSettings,
    pub quotes: widget::text_editor::Content<cosmic::Renderer>,
    pub error: Option<String>,
    pub dirty: bool,
    pending_save: Option<Settings>,
}

impl SettingsForm {
    pub fn new(settings: &Settings) -> Self {
        Self {
            interval: settings.interval_secs.to_string(),
            duration: settings.duration_secs.to_string(),
            typing_limit: settings.typing_limit_secs.to_string(),
            text_size: settings.text_size.to_string(),
            idle_secs: settings.idle_secs.to_string(),
            charts: settings.charts.clone(),
            focus: settings.focus.clone(),
            quotes: widget::text_editor::Content::with_text(&config::format_quotes(
                &settings.quotes,
            )),
            error: None,
            dirty: false,
            pending_save: None,
        }
    }

    pub fn sync(&mut self, settings: &Settings, error: Option<&str>) -> bool {
        if self.pending_save.as_ref() == Some(settings) && error.is_none() {
            self.pending_save = None;
            // Typing after Save must not be erased by its later acknowledgment.
            self.dirty = self.candidate(settings).as_ref().ok() != Some(settings);
            return true;
        }
        if !self.dirty {
            // Avoid rebuilding the editor and losing its cursor on every tick.
            if self.candidate(settings).as_ref().ok() != Some(settings) {
                *self = Self::new(settings);
            }
        }
        false
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

    fn candidate(&self, settings: &Settings) -> Result<Settings, String> {
        let candidate = Settings {
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
            charts: self.charts.clone(),
            focus: self.focus.clone(),
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
    fn numeric_field_error_reports_unsaveable_values_only() {
        assert!(numeric_field_error("600", 15, 86_400).is_none());
        assert_eq!(
            numeric_field_error("5", 15, 86_400).as_deref(),
            Some("Must be a whole number from 15 to 86400.")
        );
        assert!(numeric_field_error("86401", 15, 86_400).is_some());
        assert!(numeric_field_error("", 15, 86_400).is_some());
        assert!(numeric_field_error("abc", 15, 86_400).is_some());
        assert!(numeric_field_error("1.5", 15, 86_400).is_some());
        assert!(numeric_field_error("-1", 15, 86_400).is_some());
    }

    #[test]
    fn sync_reports_when_a_save_is_acknowledged() {
        let mut form = SettingsForm::new(&Settings::default());
        form.interval = "600".into();
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
        assert_eq!(form.interval, "600");
        form.interval = "900".into();
        form.dirty = true;
        form.sync(&Settings::default(), None);
        assert_eq!(form.interval, "900");
        assert!(form.dirty);
    }

    #[test]
    fn validation_errors_retain_draft_and_do_not_queue_a_save() {
        let mut form = SettingsForm::new(&Settings::default());
        form.interval = "not a number".into();
        form.dirty = true;
        assert!(form.save(&Settings::default()).is_err());
        assert!(form.error.as_deref().unwrap().contains("integer"));
        assert!(form.pending_save.is_none());
        assert_eq!(form.interval, "not a number");
        form.interval = "600".into();
        form.duration = "31".into();
        assert!(form.save(&Settings::default()).is_err());
        assert!(form.pending_save.is_none());
    }

    #[test]
    fn save_uses_latest_enabled_state_and_requires_successful_acknowledgment() {
        let mut form = SettingsForm::new(&Settings::default());
        form.interval = "600".into();
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
        assert_eq!(form.interval, "600");
        form.sync(&candidate, None);
        assert!(!form.dirty);
        assert!(form.pending_save.is_none());
    }

    #[test]
    fn editing_after_save_survives_acknowledgment() {
        let original = Settings::default();
        let mut form = SettingsForm::new(&original);
        form.interval = "600".into();
        let saved = form.save(&original).unwrap();
        form.interval = "900".into();
        form.sync(&saved, None);
        assert_eq!(form.interval, "900");
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
