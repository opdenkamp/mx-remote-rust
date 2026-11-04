// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Replacing a unit of a mesh, and the settings a mesh keeps of its units.

use crate::wire::{DeviceFeature, DeviceUid};

/// The most units a controller lists as replaceable, and the most a unit
/// keeps of every controller together.
pub const MESH_REPLACE_UNITS_MAX: usize = 32;

/// The most bytes of a bay name a replaceable unit's record carries.
pub const MESH_REPLACE_NAME_MAX: usize = 16;

/// The most bytes a unit's kept settings take, as they are carried: each
/// setting a byte of name length, the name, a byte of value length and the
/// value.
pub const MESH_CONFIG_MAX: usize = 1984;

/// A unit a controller can have replaced: it has bays on the controller, it
/// is not the controller, and it is offline.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MeshReplaceUnit {
    /// The unit.
    pub uid: DeviceUid,
    /// Its [`DeviceFeature::V2IP_SOURCE`], [`DeviceFeature::V2IP_SINK`] and
    /// [`DeviceFeature::MULTIVIEWER`], which a unit replacing it must have
    /// too.
    pub features: DeviceFeature,
    /// Its first input bay on the controller, 0 when it has none.
    pub input: u16,
    /// Its first output bay on the controller, 0 when it has none.
    pub output: u16,
    /// The name of that input, at most [`MESH_REPLACE_NAME_MAX`] bytes.
    pub input_name: String,
    /// The name of that output, at most [`MESH_REPLACE_NAME_MAX`] bytes.
    pub output_name: String,
}

/// The units one device listed as replaceable.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MeshReplaceUnits {
    /// The units are the sender's own controller, which is offline. A member
    /// lists its controller this way; a controller lists its own offline
    /// units.
    pub offline_controller: bool,
    /// The units, as many of the list as has arrived.
    pub units: Vec<MeshReplaceUnit>,
    /// How many units the whole list holds.
    pub total: u16,
}

/// A unit this client can ask to replace, and who to ask.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MeshReplaceable {
    /// The device that listed it, which a request to replace it goes to.
    pub lister: DeviceUid,
    /// The unit is the lister's own controller, which is offline.
    pub offline_controller: bool,
    /// The unit.
    pub unit: MeshReplaceUnit,
}

/// One setting a unit keeps for its mesh: an application configuration name
/// and its value, each at most 255 bytes.
///
/// A unit carries only the settings that differ from their default.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct MeshSetting {
    /// The configuration name, such as `led.status`.
    pub name: String,
    /// The value, as text.
    pub value: String,
}

impl MeshSetting {
    /// A setting of `name` to `value`.
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// The length and crc32 of the settings a unit keeps for its mesh, as the
/// unit announces them. A unit announces them every 60 seconds while it is
/// in a mesh, and shortly after one of them changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MeshConfigDigest {
    /// The settings' length in bytes, as they are carried.
    pub length: u16,
    /// The crc32 of those bytes.
    pub crc32: u32,
}

/// A unit's kept settings, as a device sent them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MeshConfig {
    /// The unit the settings belong to.
    pub subject: DeviceUid,
    /// The device that sent them.
    pub holder: DeviceUid,
    /// They are a copy `holder` keeps of `subject`, not its own: the
    /// controller keeps one of every member, and a member one of its
    /// controller.
    pub copy: bool,
    /// The settings, in the order they were sent.
    pub settings: Vec<MeshSetting>,
}
