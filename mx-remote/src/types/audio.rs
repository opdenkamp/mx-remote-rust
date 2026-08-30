// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The audio endpoint tree a V2IP device or amplifier reports.

use std::collections::BTreeMap;

use crate::wire::DeviceUid;

use super::V2ipStreamSource;

/// What one audio endpoint can do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AudioFeatures(u32);

impl AudioFeatures {
    /// Accepts audio.
    pub const INPUT: Self = Self(1 << 0);
    /// Produces audio.
    pub const OUTPUT: Self = Self(1 << 1);
    /// Sends a V2IP audio stream.
    pub const V2IP_TX: Self = Self(1 << 2);
    /// Receives a V2IP audio stream.
    pub const V2IP_RX: Self = Self(1 << 3);
    /// Carries HDMI audio.
    pub const HDMI: Self = Self(1 << 4);
    /// Is an analogue RCA connector.
    pub const RCA: Self = Self(1 << 5);
    /// Is an S/PDIF connector.
    pub const SPDIF: Self = Self(1 << 6);
    /// Drives a trigger output.
    pub const TRIGGER: Self = Self(1 << 7);
    /// Can be muted.
    pub const MUTE: Self = Self(1 << 8);
    /// Can be routed to as an input.
    pub const ROUTE_INPUT: Self = Self(1 << 9);
    /// Can be routed from as an output.
    pub const ROUTE_OUTPUT: Self = Self(1 << 10);
    /// Accepts "no input" as a route.
    pub const ROUTE_IN_NONE: Self = Self(1 << 11);
    /// Is an amplifier output.
    pub const AMP_OUTPUT: Self = Self(1 << 12);
    /// Has a volume control.
    pub const VOLUME_CONTROL: Self = Self(1 << 13);
    /// Has a gain control.
    pub const GAIN_CONTROL: Self = Self(1 << 14);

    /// Wraps the raw wire bits, including ones this library has no name for.
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// Returns the raw wire bits.
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Reports whether every bit of `other` is set.
    pub const fn has(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Reports whether this endpoint is either end of a V2IP audio stream.
    pub const fn is_v2ip(self) -> bool {
        self.has(Self::V2IP_TX) || self.has(Self::V2IP_RX)
    }
}

/// One audio endpoint: an input, an output, or a processing node between them.
///
/// The tree is held by id rather than by reference: `parent` and `children`
/// name endpoints in the same [`AudioEndpoints`] collection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AudioEndpoint {
    /// The endpoint's id within its device.
    pub id: u8,
    /// What this endpoint can do.
    pub features: AudioFeatures,
    /// The stream this endpoint sends or receives, when it has one.
    pub address: Option<V2ipStreamSource>,
    /// The endpoint this one feeds into.
    pub parent: Option<u8>,
    /// The endpoints feeding into this one.
    pub children: Vec<u8>,
    /// Bitmask of the endpoints that may be routed to this one.
    pub inputs_available: Option<u32>,
    /// Bitmask of the endpoints currently routed to this one.
    pub inputs_routed: Option<u32>,
    /// The device holding the endpoint this one is linked to.
    pub linked_device: DeviceUid,
    /// The endpoint on `linked_device` this one is linked to.
    pub linked_endpoint: Option<u8>,
}

impl AudioEndpoint {
    /// The endpoint currently routed to this one, or `None` when none is.
    pub fn input(&self) -> Option<u8> {
        let routed = self.inputs_routed?;
        (0..32).find(|id| routed & (1 << id) != 0)
    }

    /// The endpoints that may be routed to this one.
    pub fn available_inputs(&self) -> Vec<u8> {
        let Some(mask) = self.inputs_available else {
            return Vec::new();
        };
        (0..32).filter(|id| mask & (1 << id) != 0).collect()
    }
}

/// The audio endpoints a device reports, in the order it reported them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AudioEndpoints {
    order: Vec<u8>,
    endpoints: BTreeMap<u8, AudioEndpoint>,
}

impl AudioEndpoints {
    /// Adds an endpoint, replacing one with the same id and keeping its place
    /// in the reported order.
    pub(crate) fn add(&mut self, endpoint: AudioEndpoint) {
        if !self.endpoints.contains_key(&endpoint.id) {
            self.order.push(endpoint.id);
        }
        self.endpoints.insert(endpoint.id, endpoint);
    }

    /// The endpoint with the given id.
    pub fn get(&self, id: u8) -> Option<&AudioEndpoint> {
        self.endpoints.get(&id)
    }

    pub(crate) fn get_mut(&mut self, id: u8) -> Option<&mut AudioEndpoint> {
        self.endpoints.get_mut(&id)
    }

    /// Every endpoint, in the order the device reported them.
    pub fn list(&self) -> impl Iterator<Item = &AudioEndpoint> {
        self.order.iter().filter_map(|id| self.endpoints.get(id))
    }

    /// The endpoints with no parent: the roots of the device's audio tree.
    pub fn roots(&self) -> impl Iterator<Item = &AudioEndpoint> {
        self.list().filter(|ep| ep.parent.is_none())
    }

    /// The first root that accepts audio.
    pub fn first_root_input(&self) -> Option<&AudioEndpoint> {
        self.roots()
            .find(|ep| ep.features.has(AudioFeatures::INPUT))
    }

    /// The first root that produces audio.
    pub fn first_root_output(&self) -> Option<&AudioEndpoint> {
        self.roots()
            .find(|ep| ep.features.has(AudioFeatures::OUTPUT))
    }

    /// Reports whether two collections describe the same tree.
    ///
    /// Only the id, the features and the parent are compared: the routing and
    /// link fields change on every report, and a device that re-sends an
    /// unchanged tree must not read as a new one.
    pub(crate) fn same_tree(&self, other: &Self) -> bool {
        self.endpoints.len() == other.endpoints.len()
            && self.endpoints.iter().all(|(id, ep)| {
                other
                    .endpoints
                    .get(id)
                    .is_some_and(|o| o.features == ep.features && o.parent == ep.parent)
            })
    }

    /// Records the endpoint and device one of these endpoints is linked to.
    pub(crate) fn apply_link(&mut self, link: &AudioLink) {
        if let Some(ep) = self.endpoints.get_mut(&link.endpoint) {
            ep.linked_device = link.linked_device;
            ep.linked_endpoint = Some(link.linked_endpoint);
        }
    }
}

/// A link from an audio endpoint on this device to one on another.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioLink {
    /// The local endpoint.
    pub endpoint: u8,
    /// The endpoint it is linked to.
    pub linked_endpoint: u8,
    /// The device holding the linked endpoint.
    pub linked_device: DeviceUid,
}
