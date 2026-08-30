// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Device and bay identifiers.

use core::fmt;

/// Width of a device UID on the wire.
pub(crate) const UID_LEN: usize = 16;

/// The 16-byte unique identifier of an MX Remote device on the network.
///
/// The default value is the empty (all-zero) UID.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceUid([u8; UID_LEN]);

/// A UID could not be read from the given text or bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UidParseError {
    input: String,
}

impl fmt::Display for UidParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid uid {:?}", self.input)
    }
}

impl std::error::Error for UidParseError {}

impl DeviceUid {
    /// The all-zero UID.
    pub const ZERO: Self = Self([0; UID_LEN]);

    /// Builds a UID from its raw 16 bytes.
    pub const fn from_array(raw: [u8; UID_LEN]) -> Self {
        Self(raw)
    }

    /// Builds a UID from raw bytes.
    ///
    /// An empty slice yields [`DeviceUid::ZERO`]; any other length shorter than
    /// 16 is an error. Trailing bytes past the first 16 are ignored.
    pub fn from_bytes(b: &[u8]) -> Result<Self, UidParseError> {
        if b.is_empty() {
            return Ok(Self::ZERO);
        }
        match b
            .get(..UID_LEN)
            .and_then(|s| <[u8; UID_LEN]>::try_from(s).ok())
        {
            Some(raw) => Ok(Self(raw)),
            None => Err(UidParseError {
                input: format!("{b:02x?}"),
            }),
        }
    }

    /// Reports whether the UID is all zero.
    pub fn is_zero(&self) -> bool {
        self.0 == [0; UID_LEN]
    }

    /// Returns the raw 16-byte value.
    pub const fn as_bytes(&self) -> &[u8; UID_LEN] {
        &self.0
    }
}

impl fmt::Display for DeviceUid {
    /// Writes the dotted-hex form: four little-endian 32-bit words, each
    /// printed big-endian, separated by dots.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, word) in self.0.chunks_exact(4).enumerate() {
            if i > 0 {
                f.write_str(".")?;
            }
            for b in word.iter().rev() {
                write!(f, "{b:02x}")?;
            }
        }
        Ok(())
    }
}

impl std::str::FromStr for DeviceUid {
    type Err = UidParseError;

    /// Parses the dotted-hex form written by [`fmt::Display`]. Fields past the
    /// fourth are ignored.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || UidParseError {
            input: s.to_owned(),
        };
        let mut raw = [0u8; UID_LEN];
        let mut parts = s.split('.');
        for word in raw.chunks_exact_mut(4) {
            let part = parts.next().ok_or_else(err)?;
            let v = u32::from_str_radix(part, 16).map_err(|_| err())?;
            word.copy_from_slice(&v.to_le_bytes());
        }
        Ok(Self(raw))
    }
}

/// Identifies a single bay (port) by its owning device and port number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BayUid {
    /// The device the bay belongs to.
    pub device: DeviceUid,
    /// The bay's port number on that device.
    pub port: u16,
}

impl BayUid {
    /// Builds a bay identifier from a device and a port number.
    pub const fn new(device: DeviceUid, port: u16) -> Self {
        Self { device, port }
    }
}

impl fmt::Display for BayUid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.device, self.port)
    }
}
