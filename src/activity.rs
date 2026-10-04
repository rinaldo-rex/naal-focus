//! Pure activity editor/countdown policy; no surfaces, disk I/O, or wall clock.
use crate::activity_log::{MAX_ACTIVITY_CHARS, validate_text};
use std::time::Duration;

#[derive(Debug, Default)]
pub struct Editor {
    text: String,
    cursor: usize,
    anchor: Option<usize>,
}
impl Editor {
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    pub fn selection(&self) -> std::ops::Range<usize> {
        let anchor = self.anchor.unwrap_or(self.cursor);
        anchor.min(self.cursor)..anchor.max(self.cursor)
    }
    pub fn set_cursor(&mut self, position: usize, extend: bool) {
        if !self.text.is_char_boundary(position) {
            return;
        }
        if extend {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = position;
    }
    pub fn left(&mut self, extend: bool) {
        let selection = self.selection();
        let pos = if !extend && !selection.is_empty() {
            selection.start
        } else {
            self.text[..self.cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i)
        };
        self.set_cursor(pos, extend);
    }
    pub fn right(&mut self, extend: bool) {
        let selection = self.selection();
        let pos = if !extend && !selection.is_empty() {
            selection.end
        } else {
            self.cursor
                + self.text[self.cursor..]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8)
        };
        self.set_cursor(pos, extend);
    }
    pub fn home(&mut self, extend: bool) {
        self.set_cursor(0, extend);
    }
    pub fn end(&mut self, extend: bool) {
        self.set_cursor(self.text.len(), extend);
    }
    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.text.len();
    }
    pub fn insert(&mut self, text: &str) -> Result<bool, String> {
        if text.is_empty() {
            return Ok(false);
        }
        if text.chars().any(char::is_control) {
            return Ok(false);
        }
        let range = self.selection();
        if self.text.chars().count() - self.text[range.clone()].chars().count()
            + text.chars().count()
            > MAX_ACTIVITY_CHARS
        {
            return Err(format!("Up to {MAX_ACTIVITY_CHARS} characters."));
        }
        self.text.replace_range(range.clone(), text);
        self.cursor = range.start + text.len();
        self.anchor = None;
        Ok(true)
    }
    pub fn backspace(&mut self) {
        if self.selection().is_empty() {
            self.left(true);
        }
        self.erase_selection();
    }
    pub fn delete(&mut self) {
        if self.selection().is_empty() {
            self.right(true);
        }
        self.erase_selection();
    }
    fn erase_selection(&mut self) {
        let range = self.selection();
        self.text.replace_range(range.clone(), "");
        self.cursor = range.start;
        self.anchor = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Expiry {
    Normal,
    Typing,
}

#[derive(Debug)]
pub struct Activity {
    pub editor: Editor,
    pub error: Option<String>,
    editing: bool,
    mapped: bool,
    field_visible: bool,
    saved_text: Option<String>,
    remaining: Duration,
    resumed_at: Option<Duration>,
    typing_started: Option<Duration>,
    typing_limit: Duration,
    generation: u64,
}
impl Activity {
    pub fn new(duration: Duration, typing_limit: Duration) -> Self {
        Self {
            editor: Editor::default(),
            error: None,
            editing: false,
            mapped: false,
            field_visible: true,
            saved_text: None,
            remaining: duration,
            resumed_at: None,
            typing_started: None,
            typing_limit,
            generation: 0,
        }
    }
    pub fn mapped(&mut self, now: Duration) {
        if !self.mapped {
            self.resumed_at = Some(now);
            self.mapped = true;
        }
    }
    pub fn editing(&self) -> bool {
        self.editing
    }
    pub fn field_visible(&self) -> bool {
        self.field_visible
    }
    pub fn displayed_text(&self) -> Option<&str> {
        if self.field_visible {
            None
        } else {
            self.saved_text.as_deref()
        }
    }
    pub fn open_saved(&mut self, now: Duration) -> bool {
        if self.field_visible || self.expired(now).is_some() {
            return false;
        }
        let Some(text) = &self.saved_text else {
            return false;
        };
        self.editor = Editor::default();
        if self.editor.insert(text).is_err() {
            return false;
        }
        self.field_visible = true;
        self.error = None;
        true
    }
    /// Enter without changes simply leaves the editor, without resetting time.
    pub fn close_unchanged(&mut self) -> bool {
        if self.field_visible && !self.editing && self.saved_text.is_some() {
            self.editor = Editor::default();
            self.field_visible = false;
            self.error = None;
            self.generation += 1;
            true
        } else {
            false
        }
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn remaining(&self, now: Duration) -> Duration {
        self.resumed_at.map_or(self.remaining, |at| {
            self.remaining.saturating_sub(now.saturating_sub(at))
        })
    }
    pub fn typing_fraction(&self, now: Duration) -> Option<f32> {
        if !self.editing {
            return None;
        }
        self.typing_started.map(|at| {
            self.typing_limit
                .saturating_sub(now.saturating_sub(at))
                .as_secs_f32()
                / self.typing_limit.as_secs_f32()
        })
    }
    pub fn expired(&self, now: Duration) -> Option<Expiry> {
        if self.editing && self.typing_expired(now) {
            Some(Expiry::Typing)
        } else if self.resumed_at.is_some() && self.remaining(now).is_zero() {
            Some(Expiry::Normal)
        } else {
            None
        }
    }
    fn typing_expired(&self, now: Duration) -> bool {
        self.typing_started
            .is_some_and(|at| now.saturating_sub(at) >= self.typing_limit)
    }
    fn can_modify(&mut self, now: Duration) -> bool {
        if !self.field_visible || !self.mapped || self.expired(now).is_some() {
            return false;
        }
        if self.typing_expired(now) {
            self.error = Some("Typing time used — take a breath and return.".into());
            return false;
        }
        true
    }
    fn pause(&mut self, now: Duration) {
        if !self.editing {
            self.remaining = self.remaining(now);
            self.resumed_at = None;
            self.typing_started.get_or_insert(now);
            self.editing = true;
        }
        self.error = None;
    }
    pub fn insert(&mut self, text: &str, now: Duration) -> bool {
        if !self.can_modify(now) {
            return false;
        }
        match self.editor.insert(text) {
            Ok(true) => {
                self.pause(now);
                true
            }
            Ok(false) => false,
            Err(error) => {
                self.error = Some(error);
                true
            }
        }
    }
    pub fn backspace(&mut self, now: Duration) {
        self.erase(now, Editor::backspace);
    }
    pub fn delete(&mut self, now: Duration) {
        self.erase(now, Editor::delete);
    }
    fn erase(&mut self, now: Duration, change: fn(&mut Editor)) {
        if !self.can_modify(now) {
            return;
        }
        let before = self.editor.text().to_owned();
        change(&mut self.editor);
        if self.editor.text() != before {
            self.pause(now);
        }
    }
    /// true means normal immediate dismissal; false means cancel and resume.
    pub fn cancel(&mut self, now: Duration) -> bool {
        if self.close_unchanged() {
            return false;
        }
        if !self.editing {
            return true;
        }
        self.resume(now);
        false
    }
    pub fn submission(&mut self) -> Option<(u64, String)> {
        if !self.editing {
            return None;
        }
        match validate_text(self.editor.text()) {
            Ok(()) => {
                self.error = None;
                Some((self.generation, self.editor.text().to_owned()))
            }
            Err(error) => {
                self.error = Some(error);
                None
            }
        }
    }
    /// A late acknowledgment updates accepted text but never erases a newer draft.
    pub fn saved(&mut self, generation: u64, text: &str, now: Duration) {
        self.saved_text = Some(text.to_owned());
        if self.editing && self.generation == generation {
            self.resume(now);
        } else if !self.editing {
            self.editor = Editor::default();
            self.field_visible = false;
        }
    }
    pub fn failed(&mut self, generation: u64, error: String) {
        if self.editing && self.generation == generation {
            self.error = Some(error);
        }
    }
    fn resume(&mut self, now: Duration) {
        self.editor = Editor::default();
        self.error = None;
        self.editing = false;
        self.field_visible = self.saved_text.is_none();
        self.resumed_at = Some(now);
        self.generation += 1;
        // Never clear typing_started: cancellation cannot renew the deadline.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn at(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }
    fn activity() -> Activity {
        let mut a = Activity::new(at(5), at(120));
        a.mapped(at(10));
        a
    }
    #[test]
    fn no_input_keeps_normal_countdown() {
        let mut a = activity();
        assert!(!a.insert("", at(11)));
        assert!(!a.insert("\t", at(11)));
        assert!(a.submission().is_none());
        assert_eq!(a.remaining(at(12)), at(3));
        assert_eq!(a.expired(at(15)), Some(Expiry::Normal));
        assert!(!a.insert("too late", at(15)));
        assert!(a.cancel(at(12)));
    }
    #[test]
    fn submit_or_cancel_resumes_exact_remaining_duration() {
        for save in [true, false] {
            let mut a = activity();
            a.insert("Reading", at(12));
            assert_eq!(a.remaining(at(100)), at(3));
            assert_eq!(a.expired(at(100)), None);
            assert!(a.typing_fraction(at(100)).unwrap() < 1.0);
            if save {
                let (id, text) = a.submission().unwrap();
                assert_eq!(text, "Reading");
                a.saved(id, &text, at(100));
            } else {
                assert!(!a.cancel(at(100)));
            }
            assert_eq!(a.editor.text(), "");
            assert_eq!(a.remaining(at(102)), at(1));
            assert_eq!(a.expired(at(103)), Some(Expiry::Normal));
            assert!(a.submission().is_none());
        }
    }
    #[test]
    fn empty_editing_and_failed_save_stay_paused() {
        let mut a = activity();
        a.insert(" ", at(12));
        assert!(a.submission().is_none());
        a.editor.backspace();
        assert!(a.editing());
        a.insert("Reading", at(13));
        let (id, _) = a.submission().unwrap();
        a.failed(id, "Disk full".into());
        assert_eq!(a.editor.text(), "Reading");
        assert_eq!(a.error.as_deref(), Some("Disk full"));
        assert_eq!(a.remaining(at(100)), at(3));
    }
    #[test]
    fn cancel_restart_and_stale_ack_cannot_extend_or_erase_new_draft() {
        let mut a = activity();
        a.insert("First", at(12));
        let (old, _) = a.submission().unwrap();
        a.cancel(at(20));
        a.insert("New", at(21));
        a.saved(old, "First", at(22));
        a.failed(old, "Old failure".into());
        assert_eq!(a.editor.text(), "New");
        assert!(a.editing());
        assert!(a.error.is_none());
        assert_eq!(a.expired(at(132)), Some(Expiry::Typing));
        assert!(!a.insert("after deadline", at(132)));
    }
    #[test]
    fn repeated_mapping_cannot_resume_typing_or_round_off_remaining_time() {
        let mut a = activity();
        let started = Duration::from_millis(12_250);
        a.insert("Reading", started);
        a.mapped(at(20));
        assert_eq!(a.remaining(at(30)), Duration::from_millis(2750));
        assert_eq!(a.expired(at(30)), None);
        let (id, _) = a.submission().unwrap();
        a.saved(id, "Reading", Duration::from_millis(42_500));
        assert_eq!(a.expired(Duration::from_millis(45_249)), None);
        assert_eq!(
            a.expired(Duration::from_millis(45_250)),
            Some(Expiry::Normal)
        );
    }

    #[test]
    fn saved_text_reopens_without_pausing_until_an_actual_change() {
        let mut a = activity();
        a.insert("Reading", at(12));
        let (id, text) = a.submission().unwrap();
        a.saved(id, &text, at(20));
        assert_eq!(a.displayed_text(), Some("Reading"));
        assert!(!a.field_visible());
        assert!(!a.insert("Ignored without a click", at(20)));
        assert!(a.open_saved(at(21)));
        assert!(!a.editing());
        assert_eq!(a.remaining(at(22)), at(1));
        a.backspace(at(22));
        assert!(a.editing());
        assert_eq!(a.editor.text(), "Readin");
        assert_eq!(a.remaining(at(40)), at(1));
        assert!(!a.cancel(at(40)));
        assert_eq!(a.displayed_text(), Some("Reading"));
        assert_eq!(a.expired(at(41)), Some(Expiry::Normal));
    }

    #[test]
    fn saved_countdown_is_not_cut_short_by_spent_typing_allowance() {
        let mut a = activity();
        a.insert("Reading", at(12));
        let (id, text) = a.submission().unwrap();
        a.saved(id, &text, at(131)); // typing expires at 132, but 3 seconds remain
        assert_eq!(a.expired(at(132)), None);
        a.open_saved(at(132));
        a.backspace(at(132)); // cannot renew spent typing time
        assert_eq!(a.editor.text(), "Reading");
        assert!(!a.editing());
        assert_eq!(a.remaining(at(133)), at(1));
        assert!(a.close_unchanged());
        assert_eq!(a.expired(at(134)), Some(Expiry::Normal));
    }

    #[test]
    fn clicking_and_closing_unchanged_text_never_resets_normal_time() {
        let mut a = activity();
        a.insert("Reading", at(12));
        let (id, text) = a.submission().unwrap();
        a.saved(id, &text, at(20));
        a.open_saved(at(21));
        assert!(a.close_unchanged());
        assert_eq!(a.remaining(at(22)), at(1));
        a.open_saved(at(22));
        assert!(!a.cancel(at(22)));
        assert_eq!(a.displayed_text(), Some("Reading"));
        assert_eq!(a.expired(at(23)), Some(Expiry::Normal));
    }

    #[test]
    fn utf8_editing_selection_and_bounds() {
        let mut e = Editor::default();
        e.insert("a🪷é").unwrap();
        e.left(false);
        e.backspace();
        assert_eq!(e.text(), "aé");
        e.home(false);
        e.delete();
        assert_eq!(e.text(), "é");
        e.select_all();
        e.insert("Rust").unwrap();
        e.home(false);
        e.right(true);
        e.right(true);
        e.insert("Tr").unwrap();
        assert_eq!(e.text(), "Trst");
        e.select_all();
        e.insert(&"🪷".repeat(MAX_ACTIVITY_CHARS)).unwrap();
        assert!(e.insert("x").is_err());
        assert_eq!(e.text().chars().count(), MAX_ACTIVITY_CHARS);
        e.set_cursor(1, false); // not a UTF-8 boundary
        assert_eq!(e.cursor(), e.text().len());
    }
}
