// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Amplifier zone and Dolby settings.

use crate::event::Event;
use crate::state::State;
use crate::types::{AmpDolbySettings, AmpZoneSettings, AMP_EQ_BANDS};
use crate::wire::{BayUid, DeviceUid};

use super::handlers::{byte, u32_at};
use super::Rx;

/// `sizeof(mxr_amp_zone_settings)` on the wire.
const AMP_ZONE_SETTINGS_SIZE: usize = 56;

/// `sizeof(mxr_amp_dolby_settings)` on the wire: a uid, the mode and the flag
/// byte, rounded out from 18 to the struct's 8-byte alignment.
const AMP_DOLBY_SETTINGS_SIZE: usize = 24;

/// The device an amp-settings frame concerns: the payload target, or the
/// sender when that target is zero.
///
/// The two cases are different kinds of frame, not two spellings of one. An
/// amp acts on this opcode only when the payload target is its own uid, and it
/// leaves that field zeroed when transmitting - so a zero-target frame is one
/// no amp will ever accept, which makes it a status notification by
/// construction and the header uid the only way to attribute it. A frame with
/// the target set is a controller addressing one unit, and is a command.
///
/// Both are cached, since nothing here acts on a received frame and the mesh
/// applies commands unvalidated, so an addressed command is what that unit
/// will end up holding.
fn amp_target(rx: &Rx<'_>) -> DeviceUid {
    let uid = rx.uid_or_zero(0);
    if uid.is_zero() {
        rx.sender()
    } else {
        uid
    }
}

pub(super) fn zone_settings(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let target = amp_target(rx);
    let p = rx.frame.payload();
    // The transmitter allocates `sizeof(mxr_amp_zone_settings)` and writes
    // through a struct pointer, so the wire image is the compiler's layout
    // including its padding and 2-byte tail: 56 bytes, not the 54 the fields
    // occupy. The amp's own receive path rejects anything shorter with the
    // same constant.
    if p.len() < AMP_ZONE_SETTINGS_SIZE {
        return;
    }
    let Some(zone) = rx.frame.u16(16) else {
        return;
    };
    let mut settings = AmpZoneSettings {
        gain_left: byte(p, 18),
        gain_right: byte(p, 19),
        volume_min: byte(p, 20),
        volume_max: byte(p, 21),
        // `mxr_amp_zone_settings` is aligned to 8 rather than packed, so the
        // u32 delays cannot start at 22: they pad to 24 and 28. The rest of
        // the record only decodes with those two padding bytes ahead of the
        // delays rather than behind them - bass at 32, the power timeout at 40
        // and the EQ bands at 44 and 49 all depend on it.
        //
        // It survives a wrong reading quietly, because the padding is zero:
        // reading the delay at 22 yields (delay & 0xFFFF) << 16, which is 0
        // for the 0 every zone has until someone sets a delay.
        delay_left: u32_at(p, 24),
        delay_right: u32_at(p, 28),
        bass: byte(p, 32),
        treble: byte(p, 33),
        bridged: byte(p, 34),
        power_mode: byte(p, 35),
        power_level: byte(p, 36),
        power_timeout: u32_at(p, 40),
        eq_left: [0; AMP_EQ_BANDS],
        eq_right: [0; AMP_EQ_BANDS],
    };
    for band in 0..AMP_EQ_BANDS {
        settings.eq_left[band] = byte(p, 44 + band);
        settings.eq_right[band] = byte(p, 49 + band);
    }
    if let Some(bay) = state.bay_mut(BayUid::new(target, zone)) {
        bay.set_amp_settings(settings, ev);
    }
}

pub(super) fn dolby_settings(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let target = amp_target(rx);
    // The amp measures the whole struct before it reads a field, so a shorter
    // frame changed nothing on the device. Without this the flag byte falls
    // back to zero and a truncated frame reports every flag clear, which is a
    // state a caller cannot tell from an amp that really has them clear.
    if rx.frame.payload().len() < AMP_DOLBY_SETTINGS_SIZE {
        return;
    }
    let Some(mode) = rx.frame.u8(16) else {
        return;
    };
    let flags = rx.frame.u8(17).unwrap_or(0);
    let settings = AmpDolbySettings {
        mode,
        pcm_upmix: flags & 0x1 != 0,
        dolby_detected: flags & 0x2 != 0,
        pcm_upmix_active: flags & 0x4 != 0,
    };
    if let Some(device) = state.device_mut(target) {
        device.set_dolby_settings(settings, ev);
    }
}
