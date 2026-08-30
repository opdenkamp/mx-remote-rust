// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Virtual links between bays on different devices.

use std::collections::HashMap;

use crate::wire::BayUid;

/// A link from one bay to a bay on another device: an amplifier output wired
/// to a OneIP sink, say.
///
/// The far end is named by serial and bay name rather than by [`BayUid`],
/// because that is what the device reports and the named device may not have
/// been discovered yet.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct BayLink {
    pub(crate) linked_serial: String,
    pub(crate) linked_bay: String,
    pub(crate) features: u32,
}

impl BayLink {
    /// Whether this entry names a far end at all.
    pub(crate) fn is_configured(&self) -> bool {
        !self.linked_serial.is_empty() && !self.linked_bay.is_empty()
    }
}

/// The link configuration of every device, keyed by the origin bay's
/// cross-device identity.
#[derive(Clone, Debug, Default)]
pub(crate) struct BayLinks {
    links: HashMap<BayUid, BayLink>,
}

impl BayLinks {
    pub(crate) fn get(&self, key: BayUid) -> Option<&BayLink> {
        self.links.get(&key)
    }

    pub(crate) fn insert(&mut self, key: BayUid, link: BayLink) {
        self.links.insert(key, link);
    }
}
