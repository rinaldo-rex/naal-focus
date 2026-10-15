//! Explicit inline reflection metadata, shared by storage, editor, and dashboard.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, ops::Range};
use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Tag,
    Project,
    Feeling,
}
impl Kind {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Tag => "#",
            Self::Project => "@",
            Self::Feeling => "??",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: Kind,
    pub range: Range<usize>,
    pub value: String,
}

fn word(c: char) -> bool {
    c.is_alphanumeric()
        || c.general_category_group() == GeneralCategoryGroup::Mark
        || matches!(c, '_' | '-')
}

/// Byte ranges include trailing separators so completion replaces the whole
/// token. Values omit trailing `_`/`-`; empty markers remain completion targets.
pub fn tokens(text: &str) -> Vec<Token> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let kind = if rest.starts_with('#') {
            Some(Kind::Tag)
        } else if rest.starts_with('@')
            && text[..i]
                .chars()
                .next_back()
                .is_none_or(|c| c.is_whitespace() || "([{,:;".contains(c))
        {
            Some(Kind::Project)
        } else if rest.starts_with("??")
            && text[..i]
                .chars()
                .next_back()
                .is_none_or(|c| !word(c) && c != '?')
        {
            Some(Kind::Feeling)
        } else {
            None
        };
        if let Some(kind) = kind {
            let start = i + kind.prefix().len();
            let end = start
                + text[start..]
                    .chars()
                    .take_while(|c| word(*c))
                    .map(char::len_utf8)
                    .sum::<usize>();
            let value = text[start..end].trim_end_matches(['_', '-']).to_lowercase();
            result.push(Token {
                kind,
                range: i..end,
                value,
            });
            i = end;
        } else {
            i += rest.chars().next().unwrap().len_utf8();
        }
    }
    result
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub tags: Vec<String>,
    pub projects: Vec<String>,
    pub feelings: Vec<String>,
}
impl Metadata {
    pub fn parse(text: &str) -> Self {
        let mut result = Self::default();
        for token in tokens(text) {
            let values = match token.kind {
                Kind::Tag => &mut result.tags,
                Kind::Project => &mut result.projects,
                Kind::Feeling => &mut result.feelings,
            };
            if !token.value.is_empty() && !values.contains(&token.value) {
                values.push(token.value);
            }
        }
        result
    }
    pub fn labels(&self) -> Vec<String> {
        [
            (Kind::Tag, &self.tags),
            (Kind::Project, &self.projects),
            (Kind::Feeling, &self.feelings),
        ]
        .into_iter()
        .flat_map(|(kind, values)| {
            values
                .iter()
                .map(move |value| format!("{}{value}", kind.prefix()))
        })
        .collect()
    }
}

#[derive(Debug, Default)]
pub struct Vocabulary {
    tags: BTreeSet<String>,
    projects: BTreeSet<String>,
    feelings: BTreeSet<String>,
}
impl Vocabulary {
    pub fn include(&mut self, metadata: Metadata) {
        self.tags.extend(metadata.tags);
        self.projects.extend(metadata.projects);
        self.feelings.extend(metadata.feelings);
    }
    pub fn suggestions(&self, token: &Token, text: &str, cursor: usize) -> Vec<String> {
        let prefix = text[token.range.start + token.kind.prefix().len()..cursor].to_lowercase();
        let values = match token.kind {
            Kind::Tag => &self.tags,
            Kind::Project => &self.projects,
            Kind::Feeling => &self.feelings,
        };
        values
            .range(prefix.clone()..)
            .take_while(|value| value.starts_with(&prefix))
            .filter(|value| **value != token.value)
            .take(5)
            .map(|value| format!("{}{value}", token.kind.prefix()))
            .collect()
    }
}

pub fn active_token(text: &str, cursor: usize) -> Option<Token> {
    tokens(text).into_iter().find(|token| {
        cursor >= token.range.start + token.kind.prefix().len() && cursor <= token.range.end
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_metadata_is_unicode_case_insensitive_and_deduplicated() {
        let data =
            Metadata::parse("Review @Naal #Rust #RUST. ??Focused ??calm #café #deep_work- @naal");
        assert_eq!(data.tags, ["rust", "café", "deep_work"]);
        assert_eq!(data.projects, ["naal"]);
        assert_eq!(data.feelings, ["focused", "calm"]);
        assert!(
            Metadata::parse("me@example.com https://example.com/@user why?? # @_ ??-")
                .labels()
                .is_empty()
        );
    }
    #[test]
    fn completion_uses_caret_and_kind_and_preserves_byte_ranges() {
        let mut vocabulary = Vocabulary::default();
        vocabulary.include(Metadata::parse("#café #calm @calendar ??calm"));
        let text = "🪷 #caFé later";
        let cursor = "🪷 #ca".len();
        let token = active_token(text, cursor).unwrap();
        assert_eq!(&text[token.range.clone()], "#caFé");
        // Exact normalized matches need no replacement, even at an interior caret.
        assert!(
            vocabulary
                .suggestions(&token, text, cursor)
                .iter()
                .all(|s| s != "#café")
        );
        for (text, expected) in [("#ca", "#café"), ("@", "@calendar"), ("??c", "??calm")] {
            let token = active_token(text, text.len()).unwrap();
            assert!(
                vocabulary
                    .suggestions(&token, text, text.len())
                    .contains(&expected.to_owned())
            );
        }
        assert!(active_token("#café ", "#café ".len()).is_none());
    }
    #[test]
    fn combining_marks_survive_normalization_and_completion() {
        let data = Metadata::parse("#İstanbul @நாள் ??शांत #cafe\u{301}");
        assert_eq!(data.tags, ["i\u{307}stanbul", "cafe\u{301}"]);
        assert_eq!(data.projects, ["நாள்"]);
        assert_eq!(data.feelings, ["शांत"]);
        assert_eq!(Metadata::parse(&data.labels().join(" ")), data);
    }
}
