// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Network port link state and cable diagnostics.

use std::net::Ipv4Addr;

use crate::wire::UtpLinkSpeed;

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
