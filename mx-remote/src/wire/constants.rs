// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Library, protocol and network constants.

/// Version of this library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Highest MX Remote protocol version understood.
pub const PROTOCOL_VERSION: u16 = 0x28;

/// UDP port used in broadcast mode.
pub const BROADCAST_PORT: u16 = 8811;

/// Multicast group address used for discovery.
pub const MULTICAST_IP: std::net::Ipv4Addr = std::net::Ipv4Addr::new(224, 8, 8, 8);

/// UDP port used in multicast mode.
pub const MULTICAST_PORT: u16 = 8812;

/// Default destination UDP port of a V2IP video stream.
pub const V2IP_PORT_VIDEO: u16 = 50020;

/// Default destination UDP port of a V2IP ancillary-data stream.
pub const V2IP_PORT_ANC: u16 = 50021;

/// Default destination UDP port of a V2IP audio stream.
pub const V2IP_PORT_AUDIO: u16 = 50022;

/// Sample rate a V2IP audio stream uses when none is given.
pub const V2IP_AUDIO_DEFAULT_SAMPLE_RATE: u32 = 48000;

/// Channel count a V2IP audio stream uses when none is given.
pub const V2IP_AUDIO_DEFAULT_CHANNELS: u8 = 2;

/// Lowest channel count a V2IP audio stream accepts.
pub const V2IP_AUDIO_MIN_CHANNELS: u8 = 1;

/// Highest channel count a V2IP audio stream accepts.
pub const V2IP_AUDIO_MAX_CHANNELS: u8 = 8;

/// Lowest valid encoder TX rate, in units of 10Mb/s.
///
/// A V2IP device-config sender with no rate to offer puts a value outside
/// [`V2IP_SOURCE_RATE_MIN`]..=[`V2IP_SOURCE_RATE_MAX`] in `tx_rate`; the
/// firmware drops that as invalid and keeps the rate it already had, which is
/// what stops an address-only or scaling-only write from resetting a peer.
pub const V2IP_SOURCE_RATE_MIN: u8 = 5;

/// Highest valid encoder TX rate, in units of 10Mb/s. See [`V2IP_SOURCE_RATE_MIN`].
pub const V2IP_SOURCE_RATE_MAX: u8 = 100;

/// Marks a DSCP byte as carrying a value.
///
/// DSCP 0 (CS0) is a legal marking, so the byte needs a bit of its own to
/// separate "no marking" from CS0; it is OR'd in alongside the value.
pub const V2IP_DSCP_SET: u8 = 0x80;

/// Highest DSCP value; the marking occupies the upper 6 bits of the IPv4 TOS byte.
pub const V2IP_DSCP_MAX: u8 = 63;

/// CS2, the marking the video processor applies at boot and the value firmware
/// falls back to when a peer sends none.
pub const V2IP_DSCP_DEFAULT: u8 = 16;
