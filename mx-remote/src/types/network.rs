// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Network port link state and cable diagnostics.

use std::net::Ipv4Addr;

use crate::wire::{DeviceUid, MeshOperationKind, UtpLinkSpeed, UID_LEN};

/// The decoded link-error bitmask for a network port.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UtpLinkErrors {
    /// A receive error was counted.
    pub in_error: bool,
    /// A receive frame failed its checksum.
    pub in_fcs_error: bool,
    /// A receive collision was counted.
    pub in_collision: bool,
    /// A transmit was deferred.
    pub out_deferred: bool,
    /// A transmit was deferred excessively.
    pub out_excessive: bool,
    /// A pair is wired with reversed polarity.
    pub polarity_error: bool,
    /// Pair skew is out of tolerance.
    pub skew_warning: bool,
    /// Cable length is out of tolerance.
    pub length_warning: bool,
}

impl UtpLinkErrors {
    /// Decodes the error byte.
    pub(crate) const fn from_wire(v: u8) -> Self {
        Self {
            in_error: v & (1 << 0) != 0,
            in_fcs_error: v & (1 << 1) != 0,
            in_collision: v & (1 << 2) != 0,
            out_deferred: v & (1 << 3) != 0,
            out_excessive: v & (1 << 4) != 0,
            polarity_error: v & (1 << 5) != 0,
            skew_warning: v & (1 << 6) != 0,
            length_warning: v & (1 << 7) != 0,
        }
    }
}

/// The diagnostic status of a single UTP cable pair.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UtpCableStatus {
    /// Whether the pair is wired with normal polarity.
    pub polarity: bool,
    /// Which pair this describes.
    pub pair: u8,
    /// Measured skew.
    pub skew: u32,
    /// Measured length.
    pub length: u32,
}

/// The result of a virtual cable test on one pair.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VctStatus {
    /// The pair tested clean.
    #[default]
    Healthy,
    /// The pair raised a warning.
    Warning,
}

/// A hardware address.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MacAddress(pub [u8; 6]);

impl core::fmt::Display for MacAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let [a, b, c, d, e, g] = self.0;
        write!(f, "{a:02X}:{b:02X}:{c:02X}:{d:02X}:{e:02X}:{g:02X}")
    }
}

/// The link state and diagnostics of a network port.
///
/// A `None` field is one the port or its firmware does not report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NetworkPortStatus {
    /// Port number.
    pub port: u16,
    /// Port name.
    pub name: String,
    /// Negotiated link speed.
    pub link_speed: UtpLinkSpeed,
    /// Whether the link negotiated full duplex.
    pub link_full_duplex: bool,
    /// The port's own address.
    pub ip: Option<Ipv4Addr>,
    /// The IGMP querier the port sees.
    pub querier: Option<Ipv4Addr>,
    /// The port's hardware address.
    pub mac_address: Option<MacAddress>,
    /// Decoded link errors.
    pub errors: Option<UtpLinkErrors>,
    /// Virtual cable test result per pair.
    pub vct_status: Option<[VctStatus; 4]>,
    /// Cable diagnostics per pair.
    pub cable_status: Vec<UtpCableStatus>,
}

/// One operation on a mesh: what it asks, the device it concerns, and its
/// parameter.
///
/// The parameter is a uid for every operation but
/// [`MeshOperationKind::REPORT_CONTROLLER`], where it carries two 32-bit words
/// instead; [`MeshOperation::priority`] and [`MeshOperation::installer`] read
/// them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshOperation {
    /// What the operation asks.
    pub kind: MeshOperationKind,
    /// The device it concerns.
    pub target: DeviceUid,
    /// Its parameter.
    pub param: DeviceUid,
}

impl MeshOperation {
    const fn new(kind: MeshOperationKind, target: DeviceUid, param: DeviceUid) -> Self {
        Self {
            kind,
            target,
            param,
        }
    }

    /// `unit` joins the mesh of `controller`.
    pub const fn register(unit: DeviceUid, controller: DeviceUid) -> Self {
        Self::new(MeshOperationKind::REGISTER, unit, controller)
    }

    /// `unit` leaves the mesh of `controller`.
    pub const fn unregister(unit: DeviceUid, controller: DeviceUid) -> Self {
        Self::new(MeshOperationKind::UNREGISTER, unit, controller)
    }

    /// `replacement` takes the place of `unit` in its mesh.
    pub const fn replace(unit: DeviceUid, replacement: DeviceUid) -> Self {
        Self::new(MeshOperationKind::REPLACE, unit, replacement)
    }

    /// `unit` picks new stream addresses.
    pub const fn regenerate_addresses(unit: DeviceUid) -> Self {
        Self::new(
            MeshOperationKind::REGENERATE_ADDRESSES,
            unit,
            DeviceUid::ZERO,
        )
    }

    /// `unit` hands its source addresses back to automatic assignment.
    pub const fn auto_addresses(unit: DeviceUid) -> Self {
        Self::new(MeshOperationKind::AUTO_ADDRESSES, unit, DeviceUid::ZERO)
    }

    /// `controller` announces itself as a mesh controller. A lower `priority`
    /// wins, and `installer` is the installer id its members take, zero for
    /// none.
    pub fn report_controller(controller: DeviceUid, priority: u16, installer: u16) -> Self {
        let mut param = [0; UID_LEN];
        param[..4].copy_from_slice(&u32::from(priority).to_le_bytes());
        param[4..8].copy_from_slice(&u32::from(installer).to_le_bytes());
        Self::new(
            MeshOperationKind::REPORT_CONTROLLER,
            controller,
            DeviceUid::from_array(param),
        )
    }

    /// `controller` takes over as mesh controller from `previous`, and every
    /// unit following `previous` follows `controller` from then on.
    pub const fn promote_controller(controller: DeviceUid, previous: DeviceUid) -> Self {
        Self::new(MeshOperationKind::PROMOTE_CONTROLLER, controller, previous)
    }

    /// The sender reports `controller` as its mesh controller, the zero uid
    /// for none.
    pub const fn report_membership(controller: DeviceUid) -> Self {
        Self::new(
            MeshOperationKind::REPORT_MEMBERSHIP,
            controller,
            DeviceUid::ZERO,
        )
    }

    /// The priority a controller report carries. Units read the word as 16
    /// bits.
    pub fn priority(&self) -> u16 {
        let b = self.param.as_bytes();
        u16::from_le_bytes([b[0], b[1]])
    }

    /// The installer id a controller report carries. Units read the word as
    /// 16 bits.
    pub fn installer(&self) -> u16 {
        let b = self.param.as_bytes();
        u16::from_le_bytes([b[4], b[5]])
    }
}

/// What a device last announced about itself as a mesh controller.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MeshControllerReport {
    /// Its priority as controller. Lower wins.
    pub priority: u16,
    /// The installer id its members take, zero for none.
    pub installer: u16,
}
