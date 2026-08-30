// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! What the host's network interfaces offer: addresses, broadcast addresses
//! and indices.

use std::io;
use std::net::Ipv4Addr;

/// One IPv4 address on one interface.
struct Interface {
    name: String,
    index: Option<u32>,
    address: Ipv4Addr,
    netmask: Ipv4Addr,
}

/// Every non-loopback IPv4 address the host has.
fn interfaces() -> Vec<Interface> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.addr {
            if_addrs::IfAddr::V4(ref v4) => Some(Interface {
                name: i.name.clone(),
                index: i.index,
                address: v4.ip,
                netmask: v4.netmask,
            }),
            if_addrs::IfAddr::V6(_) => None,
        })
        .collect()
}

/// The non-loopback IPv4 addresses that can be used as a local address.
///
/// The order is the host's own, which no interface property justifies reading
/// as a preference; a caller with more than one address to choose from should
/// choose.
pub fn valid_addresses() -> Vec<Ipv4Addr> {
    interfaces().into_iter().map(|i| i.address).collect()
}

/// The address to use when the caller named none.
///
/// The first address the host enumerates, which on a multi-homed machine is
/// arbitrary. It is picked anyway rather than refused, because the single-homed
/// case is both the common one and unambiguous.
pub(crate) fn default_local_ip() -> io::Result<Ipv4Addr> {
    interfaces().first().map(|i| i.address).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            "the host has no non-loopback IPv4 address",
        )
    })
}

/// The directed broadcast address of the interface holding `local`, or of the
/// first interface when `local` is `None`.
///
/// Directed rather than 255.255.255.255 so that the frame leaves by the chosen
/// interface: a limited broadcast is routed by the host, which puts it back
/// under exactly the decision naming an interface was meant to settle.
pub(crate) fn broadcast_address(local: Option<Ipv4Addr>) -> io::Result<Ipv4Addr> {
    interfaces()
        .into_iter()
        .find(|i| local.map_or(true, |want| i.address == want))
        .map(|i| {
            let (a, m) = (i.address.octets(), i.netmask.octets());
            Ipv4Addr::new(a[0] | !m[0], a[1] | !m[1], a[2] | !m[2], a[3] | !m[3])
        })
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::AddrNotAvailable,
                "no interface to derive a broadcast address from",
            )
        })
}

/// The kernel index of the named interface.
pub(crate) fn index_of(name: &str) -> io::Result<u32> {
    interfaces()
        .into_iter()
        .find(|i| i.name == name)
        .and_then(|i| i.index)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("no interface named {name:?}"),
            )
        })
}

/// The first non-loopback IPv4 address of the named interface.
///
/// An interface can carry discovery without having an address of its own, so
/// this returns `None` rather than an error for one that has none.
pub(crate) fn address_of(name: &str) -> Option<Ipv4Addr> {
    interfaces()
        .into_iter()
        .find(|i| i.name == name)
        .map(|i| i.address)
}
