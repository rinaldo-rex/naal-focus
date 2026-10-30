use crate::{config::validate_quotes, focus, time::system_ms};
use std::io::Read;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Payload {
    pub quote: String,
    pub focus: Option<focus::Snapshot>,
}

pub(super) fn read_stdin_quote(input: impl Read) -> Result<String, String> {
    let mut quote = String::new();
    input
        .take(4097)
        .read_to_string(&mut quote)
        .map_err(|e| e.to_string())?;
    if quote.len() > 4096 {
        return Err("Quote input is too large.".into());
    }
    // This is already one selected quote, not the bulk-editor format. Parsing
    // bullet delimiters here would split or corrupt literal lists in its text.
    validate_quotes(std::slice::from_ref(&quote))?;
    Ok(quote)
}

pub(super) fn read_payload(input: impl Read) -> Result<Payload, String> {
    let limit = focus::MAX_BYTES + 8192;
    let mut bytes = Vec::new();
    input
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("Overlay payload is too large.".into());
    }
    let mut payload: Payload =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid overlay payload.")?;
    validate_quotes(std::slice::from_ref(&payload.quote))?;
    if let Some(snapshot) = &mut payload.focus {
        snapshot.day.validate(system_ms())?;
        snapshot.day.advance_day(system_ms());
    }
    Ok(payload)
}
