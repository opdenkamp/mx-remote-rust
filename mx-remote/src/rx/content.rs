// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! What the source on an input is showing.

use crate::event::Event;
use crate::state::State;
use crate::types::{BayContentField, BAY_CONTENT_KEY_MAX, BAY_CONTENT_VALUE_MAX};
use crate::wire::BayUid;

use super::handlers::u16_at;
use super::Rx;

/// The ports a unit reports content for are below this.
const CONTENT_PORTS: u16 = 12;

/// Applies the content a unit reports for one of its inputs.
///
/// The input's port is a `u16` at 0, and every field behind it is a key and a
/// value, each NUL terminated, packed back to back up to the end of the
/// payload. A report with no fields says the input shows nothing now.
///
/// Each field is cleaned as a receiving unit cleans it, so this client shows
/// what a unit's own API would. A field that is empty once cleaned, or whose
/// key an earlier field already gave, is dropped.
pub(super) fn bay_content(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let p = rx.frame.payload();
    if p.len() < 2 {
        return;
    }
    let port = u16_at(p, 0);
    if port >= CONTENT_PORTS {
        return;
    }
    let mut content: Vec<BayContentField> = Vec::new();
    let mut strings = p[2..].split(|&b| b == 0);
    // `split` yields what follows the last NUL as one more piece, so a value
    // is terminated only when another piece comes after it.
    while let (Some(key), Some(value)) = (strings.next(), strings.next()) {
        if strings.clone().next().is_none() {
            break;
        }
        let key: String = key
            .iter()
            .copied()
            .filter(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            .take(BAY_CONTENT_KEY_MAX)
            .map(char::from)
            .collect();
        let value = clean_value(value);
        if key.is_empty() || value.is_empty() || content.iter().any(|f| f.key == key) {
            continue;
        }
        content.push(BayContentField { key, value });
    }
    if let Some(bay) = state.bay_mut(BayUid::new(rx.sender(), port)) {
        bay.set_content(content, ev);
    }
}

/// Cleans a value as a unit does: control characters, `"` and `\` dropped,
/// then every byte that is not part of a valid UTF-8 character, then the rest
/// cut to [`BAY_CONTENT_VALUE_MAX`] bytes before a character that would not
/// fit whole.
fn clean_value(raw: &[u8]) -> String {
    let kept: Vec<u8> = raw
        .iter()
        .copied()
        .filter(|&b| b >= 0x20 && b != 0x7F && b != b'"' && b != b'\\')
        .collect();
    let mut out = String::new();
    let mut rest = &kept[..];
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(valid) => {
                out.push_str(valid);
                break;
            }
            Err(e) => {
                let (valid, invalid) = rest.split_at(e.valid_up_to());
                out.push_str(std::str::from_utf8(valid).unwrap_or_default());
                rest = &invalid[e.error_len().unwrap_or(invalid.len())..];
            }
        }
    }
    let mut end = out.len().min(BAY_CONTENT_VALUE_MAX);
    while !out.is_char_boundary(end) {
        end -= 1;
    }
    out.truncate(end);
    out
}
