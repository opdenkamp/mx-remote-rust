// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Messages other applications exchange over the mesh.

use std::net::Ipv4Addr;

use crate::wire::ThirdPartyVendorFlag;

/// The largest payload a third-party message carries.
///
/// A frame this size fills one 1500-byte Ethernet frame exactly: 20 bytes of
/// IPv4 header, 8 of UDP, the 24-byte frame header and the 4-byte vendor id
/// leave 1444. A larger send is refused rather than fragmented, because a lost
/// fragment loses the whole datagram and nothing on these paths retries.
pub const THIRD_PARTY_PAYLOAD_MAX: usize = 1444;

/// Whose message a third-party message is: a flag byte, then three bytes
/// naming the vendor.
///
/// The three bytes are the vendor's IEEE OUI, in the order it is written, or
/// with [`ThirdPartyVendorFlag::NOT_OUI`] set an identifier of the vendor's
/// own choosing, such as three ASCII letters. The flag keeps the two spaces
/// apart, so a chosen identifier never collides with a registered OUI.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ThirdPartyVendor {
    /// The flag byte, at offset 0.
    pub flags: ThirdPartyVendorFlag,
    /// The vendor's three identifying bytes, at offsets 1 to 3.
    pub id: [u8; 3],
}

impl ThirdPartyVendor {
    /// A vendor named by its IEEE OUI.
    pub const fn oui(oui: [u8; 3]) -> Self {
        Self {
            flags: ThirdPartyVendorFlag::NONE,
            id: oui,
        }
    }

    /// A vendor named by an identifier it chose, rather than an OUI.
    pub const fn private(id: [u8; 3]) -> Self {
        Self {
            flags: ThirdPartyVendorFlag::NOT_OUI,
            id,
        }
    }

    /// Whether `other` names the same vendor.
    ///
    /// Only [`ThirdPartyVendorFlag::NOT_OUI`] and the three identifying bytes
    /// say who the vendor is, so the other flag bits are left out.
    pub const fn same_vendor(self, other: Self) -> bool {
        let a = self.id;
        let b = other.id;
        self.flags.has(ThirdPartyVendorFlag::NOT_OUI)
            == other.flags.has(ThirdPartyVendorFlag::NOT_OUI)
            && a[0] == b[0]
            && a[1] == b[1]
            && a[2] == b[2]
    }

    /// The four bytes on the wire.
    pub(crate) const fn to_wire(self) -> [u8; 4] {
        [self.flags.bits(), self.id[0], self.id[1], self.id[2]]
    }

    /// Reads the four bytes on the wire.
    pub(crate) const fn from_wire(b: [u8; 4]) -> Self {
        Self {
            flags: ThirdPartyVendorFlag::from_bits(b[0]),
            id: [b[1], b[2], b[3]],
        }
    }
}

/// A message an application sent to every device, under its own vendor id.
///
/// The library reads only the vendor id. What follows is the application's,
/// and arrives exactly as it was sent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThirdPartyMessage {
    /// Which application's message this is. The library checks none: each
    /// application ignores every message whose vendor is not its own.
    pub vendor: ThirdPartyVendor,
    /// The application's bytes, at most [`THIRD_PARTY_PAYLOAD_MAX`].
    pub payload: Vec<u8>,
    /// Where the datagram came from, when the socket said.
    pub address: Option<Ipv4Addr>,
}
