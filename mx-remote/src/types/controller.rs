// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! What a dedicated mesh controller, such as a router, announces about itself
//! and its state in a `CONTROLLER_STATE` (0x58) hello.

use crate::wire::{ControllerRole, DeviceUid};

/// The most parts a `CONTROLLER_STATE` hello lists: the state, and the four
/// overlay logo slots.
pub const CONTROLLER_PARTS_MAX: usize = 5;

/// One part of a dedicated controller's state, as its hello lists it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ControllerPart {
    /// Which part: 0 the state, 1-4 the PNG of overlay logo slot N.
    pub part: u8,
    /// The part's length in bytes.
    pub length: u32,
    /// The crc32 of the whole part.
    pub crc: u32,
}

/// A dedicated controller's `CONTROLLER_STATE` hello: its role, its state's
/// generation, the primary in its state and the parts its state is made of.
///
/// Sending one marks the sender as a dedicated controller to every unit from
/// protocol 0x34 on, which then lets no OneIP take the controller role from it
/// while it is online.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ControllerHello {
    /// Primary, failsafe or neither.
    pub role: ControllerRole,
    /// Raised on every change of the sender's state.
    pub generation: u32,
    /// The primary in the sender's state, zero for none.
    pub primary: DeviceUid,
    /// The parts; only the first [`CONTROLLER_PARTS_MAX`] are sent.
    pub parts: Vec<ControllerPart>,
}
