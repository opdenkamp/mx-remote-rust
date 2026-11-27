// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! A OneIP controller announcing its driver proxy on the customer network.

use crate::event::Event;
use crate::state::State;

use super::Rx;

/// The sub-op of a `PROXY` announcement: the only one sent to every device
/// unprotected and unasked.
pub(crate) const PROXY_ANNOUNCE: u8 = 0;

/// Marks a known device as a proxy controller when it announces its proxy:
/// sub-op 0, then a version byte. Any version counts: a controller this
/// client cannot join answers the join with its own refusal.
///
/// The announcement is no keep-alive, as a unit takes no frame past its
/// opcode table as one, and the mark proves nothing: anyone on the network
/// can announce under any uid.
pub(super) fn announce(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(device) = state.device_mut(rx.sender()) else {
        return;
    };
    if let [PROXY_ANNOUNCE, _version, ..] = rx.frame.payload() {
        device.proxy_announced(rx.timestamp, ev);
    }
}
