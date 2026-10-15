// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Overlay sync: a management unit having other units copy its overlay
//! settings and logos.
//!
//! The source sends a sync naming what it copies and how many settings
//! follow. Each unit asks the source for the settings it lacks, takes them,
//! and reports how that went. Every frame of a sync carries the same `seq`,
//! which the source numbers on its own.

use std::net::Ipv4Addr;

use crate::wire::{
    DeviceUid, OsdCaptionAlign, OsdCaptionFormat, OsdSyncSize, OsdSyncState, OsdSyncWhat,
};

/// The logo slots a sync can copy.
pub const OSD_SYNC_SLOTS: usize = 4;

/// One logo slot, as a sync describes it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OsdSyncSlot {
    /// The PNG's size in bytes; zero empties the slot.
    pub bytes: u32,
    /// The PNG's CRC-32.
    pub crc32: u32,
    /// The size it is shown at, in percent.
    pub percent: u8,
}

/// A sync, as its source sent it.
///
/// A unit takes one only from a management unit: the mesh controller, or
/// one that reports [`DeviceFeature::MANAGER`](crate::DeviceFeature::MANAGER).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OsdSync {
    /// The sync's number at its source.
    pub seq: u16,
    /// The unit it is for, or [`DeviceUid::ZERO`] for every unit.
    pub target: DeviceUid,
    /// What it copies.
    pub what: OsdSyncWhat,
    /// The source's HTTP port, zero meaning 80.
    pub http_port: u16,
    /// The source's address, unspecified meaning the sender's.
    pub source_ip: Ipv4Addr,
    /// How many settings follow.
    pub records: u16,
    /// The logo slots, when it copies them. A unit fetches a slot whose size
    /// or CRC differs from its own from
    /// `http://source_ip:http_port/osd-sync/slotN.png`, N being 1 to 4.
    pub slots: Option<[OsdSyncSlot; OSD_SYNC_SLOTS]>,
}

/// A unit asking for a sync, or for settings of one it is taking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OsdSyncRequest {
    /// The sync whose settings it asks for, or zero asking for a new sync.
    pub seq: u16,
    /// What it asks a new sync to copy.
    pub what: OsdSyncWhat,
    /// The settings it lacks, bit `i` standing for setting `i`.
    pub missing: [u8; 16],
    /// Who sent the sync it asks about. [`DeviceUid::ZERO`] when it asks for
    /// a new sync, and from a unit whose module does not name the source.
    pub source: DeviceUid,
}

impl OsdSyncRequest {
    /// Whether the unit lacks setting `index`.
    pub fn is_missing(&self, index: u16) -> bool {
        self.missing
            .get(usize::from(index / 8))
            .is_some_and(|b| b & (1 << (index % 8)) != 0)
    }
}

/// A caption, as a setting gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OsdCaption {
    /// The font's name, empty for the layout's.
    pub font: String,
    /// The size, zero for the layout's.
    pub size: u16,
    /// The horizontal position, `i16::MIN` for the layout's.
    pub x: i16,
    /// The vertical position, `i16::MIN` for the layout's.
    pub y: i16,
    /// The wrap width in pixels: -1 automatic, 0 none, -2 the layout's.
    pub wrap: i16,
    /// The alignment.
    pub align: OsdCaptionAlign,
    /// How the text is read.
    pub format: OsdCaptionFormat,
    /// The colour as 0xRRGGBB, `u32::MAX` for the layout's.
    pub colour: u32,
    /// The text.
    pub text: String,
}

/// A logo choice and place, as a setting gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OsdLogo {
    /// 0 the built-in logo, -1 none, 1 to 4 a slot.
    pub logo: i8,
    /// The horizontal position, `i16::MIN` for the layout's.
    pub x: i16,
    /// The vertical position, `i16::MIN` for the layout's.
    pub y: i16,
}

/// What one setting of a sync sets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OsdSyncValue {
    /// A caption.
    Caption(OsdCaption),
    /// A logo choice and place.
    Logo(OsdLogo),
    /// A background colour as 0xRRGGBB, `u32::MAX` for the layout's.
    Background(u32),
}

/// One setting of a sync, which its source sends to one unit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OsdSyncSetting {
    /// The sync it belongs to.
    pub seq: u16,
    /// The unit it is for.
    pub target: DeviceUid,
    /// Its place in the sync, below `records`.
    pub index: u16,
    /// How many settings the sync has.
    pub records: u16,
    /// The output size it applies at.
    pub size: OsdSyncSize,
    /// The layers it applies to, a bit per layer.
    pub layers: u16,
    /// The fields it gives: [`OsdCaptionField`](crate::OsdCaptionField)
    /// bits for a caption, [`OsdLogoField`](crate::OsdLogoField) bits for a
    /// logo, and bit 0 for a background's colour.
    pub set: u16,
    /// What it sets.
    pub value: OsdSyncValue,
}

/// How a unit is getting on with a sync.
///
/// A unit reports taking, then done or failed. One in power save reports
/// deferred, and reports again once it takes the sync on waking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OsdSyncReport {
    /// The sync it reports on, numbered by the source that sent it.
    pub seq: u16,
    /// How far it has got.
    pub state: OsdSyncState,
    /// The settings it wrote.
    pub written: u16,
    /// The settings it refused.
    pub refused: u16,
    /// Why it failed or refused any.
    pub reason: String,
}
