// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! A unit's health and IP configuration, and the commands that change them.

use std::net::Ipv4Addr;

use crate::wire::{
    DeviceUid, UnitCommandFlag, UnitCommandKind, UnitFlag, UnitPowerSave, V2ipPowerSaveLevel,
};

/// The most temperature sensors a unit reports.
pub const UNIT_MAX_TEMPERATURES: usize = 6;

/// The power save levels a unit reports what it offers at, from
/// [`REDUCED`](V2ipPowerSaveLevel::REDUCED) up.
pub const UNIT_POWER_SAVE_LEVELS: usize = 4;

/// A unit's health and IP configuration, as it last reported them.
///
/// A unit reports this about itself with its periodic broadcast, and within a
/// few seconds of a change to its flags, its status or its addresses.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnitStatus {
    /// The report's flags.
    pub flags: UnitFlag,
    /// Seconds since the unit booted.
    pub uptime: u32,
    /// How often the unit has booted, when `flags` has
    /// [`BOOT_COUNT`](UnitFlag::BOOT_COUNT).
    pub boot_count: u32,
    /// How often the unit has rebooted.
    pub reboot_count: u32,
    /// The encoder's health, a mask of the unit's encoder status bits.
    pub encoder_health: u32,
    /// The decoder's health, a mask of the unit's decoder status and state
    /// bits.
    pub decoder_health: u32,
    /// The IP configuration in use.
    pub network: UnitNetConfig,
    /// The fan speed in RPM, when `flags` has
    /// [`FAN_SPEED`](UnitFlag::FAN_SPEED).
    pub fan_rpm: u16,
    /// The fan duty cycle, in percent.
    pub fan_duty: u8,
    /// The unit's system status.
    pub status: u8,
    /// Seconds before a pending IP configuration is reverted.
    pub net_revert_s: u8,
    /// Temperatures, in degrees Celsius, at most
    /// [`UNIT_MAX_TEMPERATURES`] of them.
    pub temperatures: Vec<i8>,
    /// The message of the current system status.
    pub status_message: String,
    /// What the unit offers at each power save level, from
    /// [`REDUCED`](V2ipPowerSaveLevel::REDUCED) up; empty for a unit that
    /// predates the field. A route change or a command wakes the unit at every
    /// level it supports.
    pub power_save: [UnitPowerSave; UNIT_POWER_SAVE_LEVELS],
}

impl UnitStatus {
    /// How often the unit has booted, `None` while it does not report it.
    pub const fn boot_count(&self) -> Option<u32> {
        if !self.flags.has(UnitFlag::BOOT_COUNT) {
            return None;
        }
        Some(self.boot_count)
    }

    /// The fan speed in RPM, `None` while the unit does not report it.
    pub const fn fan_rpm(&self) -> Option<u16> {
        if !self.flags.has(UnitFlag::FAN_SPEED) {
            return None;
        }
        Some(self.fan_rpm)
    }

    /// What the unit offers at a power save level, empty for
    /// [`NONE`](V2ipPowerSaveLevel::NONE) or a level it does not report.
    pub fn power_save_level(&self, level: V2ipPowerSaveLevel) -> UnitPowerSave {
        usize::from(level.to_wire())
            .checked_sub(1)
            .and_then(|i| self.power_save.get(i).copied())
            .unwrap_or_default()
    }

    /// Whether an IP configuration changed through the mesh waits for the
    /// mesh controller to confirm it.
    pub const fn network_pending(&self) -> bool {
        self.flags.has(UnitFlag::NET_PENDING)
    }
}

/// An IP configuration: the addresses a unit uses, or is told to use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitNetConfig {
    /// The address.
    pub ip: Ipv4Addr,
    /// The netmask.
    pub netmask: Ipv4Addr,
    /// The gateway.
    pub gateway: Ipv4Addr,
    /// The primary DNS server.
    pub dns1: Ipv4Addr,
    /// The secondary DNS server.
    pub dns2: Ipv4Addr,
}

impl Default for UnitNetConfig {
    fn default() -> Self {
        Self {
            ip: Ipv4Addr::UNSPECIFIED,
            netmask: Ipv4Addr::UNSPECIFIED,
            gateway: Ipv4Addr::UNSPECIFIED,
            dns1: Ipv4Addr::UNSPECIFIED,
            dns2: Ipv4Addr::UNSPECIFIED,
        }
    }
}

impl UnitNetConfig {
    /// Whether the address is one a unit can hold and the netmask is a run of
    /// ones from the top.
    pub fn is_valid(&self) -> bool {
        let ip = self.ip;
        let mask = u32::from(self.netmask);
        !ip.is_unspecified()
            && !ip.is_broadcast()
            && !ip.is_multicast()
            && mask != 0
            && mask.leading_ones() + mask.trailing_zeros() == 32
    }
}

/// A command for one unit, as a peer sent it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitCommand {
    /// The unit the command is for.
    pub target: DeviceUid,
    /// What it asks.
    pub command: UnitCommandKind,
    /// The command's flags.
    pub flags: UnitCommandFlag,
    /// The IP configuration a [`NET_CONFIG`](UnitCommandKind::NET_CONFIG)
    /// carries, when the frame holds one.
    pub network: Option<UnitNetConfig>,
}
