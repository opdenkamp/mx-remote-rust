// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The handler for `THIRD_PARTY` (0x55): messages applications exchange over
//! the mesh.

use crate::event::Event;
use crate::types::{ThirdPartyMessage, ThirdPartyVendor};

use super::Rx;

/// Size of the vendor id at the front of the payload.
const VENDOR_LEN: usize = 4;

/// Reports a message, whatever its vendor: telling them apart is the
/// application's.
///
/// A payload too short for the vendor id is dropped. Everything after the id
/// is the application's, so the length is a minimum and nothing past it is
/// read.
pub(super) fn third_party(rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let payload = rx.frame.payload();
    let (Some(vendor), Some(rest)) = (
        payload
            .get(..VENDOR_LEN)
            .and_then(|b| <[u8; VENDOR_LEN]>::try_from(b).ok()),
        payload.get(VENDOR_LEN..),
    ) else {
        return;
    };
    ev.push(Event::ThirdPartyMessage {
        device: rx.sender(),
        message: ThirdPartyMessage {
            vendor: ThirdPartyVendor::from_wire(vendor),
            payload: rest.to_vec(),
            address: rx.address,
        },
    });
}
