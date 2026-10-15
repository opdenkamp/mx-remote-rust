// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! What the source on an input is showing.

use crate::event::Event;
use crate::state::State;
use crate::types::{BayContentField, BAY_CONTENT_KEY_MAX, BAY_CONTENT_VALUE_MAX};
use crate::wire::BayUid;

use super::handlers::u16_at;
use super::Rx;

/// Applies the content a unit reports for one of its inputs.
///
/// The input's port is a `u16` at 0, and every field behind it is a key and a
/// value, each NUL terminated, packed back to back up to the end of the
/// payload. A report with no fields says the input shows nothing now.
///
/// Each field is cleaned as a receiving unit cleans it, so this client shows
/// what a unit's own API would. A field that is empty once cleaned, or whose
/// key an earlier field already gave, is dropped. So is one that is not
/// UTF-8, which a unit passes on as bytes and a `String` cannot hold.
pub(super) fn bay_content(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let p = rx.frame.payload();
    if p.len() < 2 {
        return;
    }
    let port = u16_at(p, 0);
    let mut content: Vec<BayContentField> = Vec::new();
    let mut strings = p[2..].split(|&b| b == 0);
    // `split` yields what follows the last NUL as one more piece, so a value
    // is terminated only when another piece comes after it.
    while let (Some(key), Some(value)) = (strings.next(), strings.next()) {
        if strings.clone().next().is_none() {
            break;
        }
        let key = clean(key, BAY_CONTENT_KEY_MAX, |b| {
            b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
        });
        let mut value = clean(value, BAY_CONTENT_VALUE_MAX, |b| {
            b >= 0x20 && b != 0x7F && b != b'"' && b != b'\\'
        });
        drop_cut_sequence(&mut value);
        // Cleaning leaves a key ASCII, so only a value can fail to be UTF-8.
        let key: String = key.into_iter().map(char::from).collect();
        let Ok(value) = String::from_utf8(value) else {
            continue;
        };
        if key.is_empty() || value.is_empty() || content.iter().any(|f| f.key == key) {
            continue;
        }
        content.push(BayContentField { key, value });
    }
    if let Some(bay) = state.bay_mut(BayUid::new(rx.sender(), port)) {
        bay.set_content(content, ev);
    }
}

/// The bytes `keep` allows, up to `max` of them.
fn clean(raw: &[u8], max: usize, keep: impl Fn(u8) -> bool) -> Vec<u8> {
    raw.iter().copied().filter(|&b| keep(b)).take(max).collect()
}

/// Removes a UTF-8 sequence the length limit cut short at the end of `s`.
///
/// The lead byte says how long its sequence is: 0xC0 and up two bytes, 0xE0
/// and up three, 0xF0 and up four.
fn drop_cut_sequence(s: &mut Vec<u8>) {
    let tail = s.iter().rev().take_while(|&&b| b & 0xC0 == 0x80).count();
    let Some(lead_at) = s.len().checked_sub(tail + 1) else {
        return;
    };
    let need = match s[lead_at] {
        b if b >= 0xF0 => 4,
        b if b >= 0xE0 => 3,
        b if b >= 0xC0 => 2,
        _ => return,
    };
    if tail + 1 < need {
        s.truncate(lead_at);
    }
}
