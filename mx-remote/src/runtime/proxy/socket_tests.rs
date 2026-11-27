// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Peer mode over real sockets, against a stand-in controller on loopback:
//! the join, a frame each way, `INFO`, and `LEAVE` on close.
//!
//! The stand-in binds 127.0.0.2 on the proxy port and the client every
//! address on it, so each takes only what is sent to it. Linux-only: other
//! hosts give no 127.0.0.2.

use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::crypto::{controller_proof, driver_proof, open, seal, session_key};
use super::{ProxyConfig, ProxyJoinStatus};
use crate::raw::FrameHeader;
use crate::runtime::tests::hello_datagram;
use crate::testing::uid_n;
use crate::wire::DeviceUid;
use crate::{Config, EventHandler, Remote};

const CONTROLLER: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 2);
const PORT: u16 = 18830;
const KEY: [u8; 32] = [0x5C; 32];

#[derive(Default)]
struct Statuses(Mutex<Vec<ProxyJoinStatus>>);

impl EventHandler for Statuses {
    fn on_proxy_join_status(&self, status: ProxyJoinStatus) {
        self.0.lock().expect("only pushed to").push(status);
    }
}

fn proxy(sender: DeviceUid, payload: &[u8]) -> Vec<u8> {
    let mut f = vec![0; FrameHeader::LEN];
    FrameHeader {
        protocol: 0x36,
        sender,
        opcode: 0x59,
        payload_len: payload.len() as u16,
    }
    .write_into(&mut f);
    f.extend_from_slice(payload);
    f
}

/// What the stand-in reads next, skipping nothing: `None` once nothing
/// arrives for two seconds.
fn next(socket: &UdpSocket) -> Option<(Vec<u8>, SocketAddrV4)> {
    let mut buf = vec![0u8; 2048];
    match socket.recv_from(&mut buf) {
        Ok((n, std::net::SocketAddr::V4(from))) => Some((buf[..n].to_vec(), from)),
        _ => None,
    }
}

/// The stand-in's socket, sharing the port as the client's does.
fn stand_in() -> Option<UdpSocket> {
    use socket2::{Domain, Protocol, SockAddr, Socket, Type};
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).ok()?;
    socket.set_reuse_address(true).ok()?;
    socket
        .bind(&SockAddr::from(SocketAddrV4::new(CONTROLLER, PORT)))
        .ok()?;
    Some(socket.into())
}

#[test]
fn a_peer_joins_and_trades_frames_with_its_controller() {
    let Some(controller) = stand_in() else {
        eprintln!("skipped: 127.0.0.2 cannot be bound");
        return;
    };
    controller
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("a timeout");
    let (driver, uplink, mesh, unit) = (uid_n(250), uid_n(251), uid_n(252), uid_n(253));
    let statuses = Arc::new(Statuses::default());
    let mut config = ProxyConfig::new(CONTROLLER, uplink, KEY);
    config.port = PORT;
    let remote = Remote::new(
        Config {
            uid: Some(driver),
            proxy: Some(config),
            ..Config::default()
        },
        Arc::clone(&statuses) as Arc<dyn EventHandler>,
    )
    .expect("a peer");
    if let Err(e) = remote.start() {
        eprintln!("skipped: the mesh ports cannot be opened to listen on: {e}");
        return;
    }

    // JOIN, from the proxy port.
    let (join, peer) = next(&controller).expect("no JOIN");
    assert_eq!(peer.port(), PORT, "the JOIN came from another port");
    assert_eq!(&join[24..26], &[1, 1], "not a JOIN");
    let nd: [u8; 16] = join[26..42].try_into().expect("a nonce");
    let nc = [0x3C; 16];
    let mut challenge = vec![2];
    challenge.extend_from_slice(&nd);
    challenge.extend_from_slice(&nc);
    controller
        .send_to(&proxy(uplink, &challenge), peer)
        .expect("sent");

    // PROOF.
    let (proof, _) = next(&controller).expect("no PROOF");
    assert_eq!(
        &proof[24 + 33..24 + 65],
        &driver_proof(&KEY, &nd, &nc, driver, uplink)[..]
    );
    let mut result = vec![4, 0, 0, 60];
    result.extend_from_slice(&nd);
    result.extend_from_slice(&nc);
    result.extend_from_slice(&controller_proof(&KEY, &nd, &nc, driver, uplink, 60));
    controller
        .send_to(&proxy(uplink, &result), peer)
        .expect("sent");

    // The client's hello and a discover, sealed, follow the join at once:
    // the discover at start went nowhere, and the next is a minute off.
    let d2c = session_key(&KEY, &nd, &nc, driver, uplink, b"d2c");
    let joined_at = std::time::Instant::now();
    let mut hello = None;
    let mut discover = None;
    while let Some((data, _)) = next(&controller) {
        let inner = open(
            &d2c,
            u64::from_be_bytes(data[25..33].try_into().expect("8")),
            &data[..33],
            &data[33..],
        )
        .expect("a frame from the client that does not open");
        let header = FrameHeader::parse_accepted(&inner).expect("a frame");
        assert_eq!(header.sender, driver);
        if header.opcode == 0 && hello.is_none() {
            hello = Some(joined_at.elapsed());
        }
        if header.opcode == 1 && discover.is_none() {
            discover = Some(joined_at.elapsed());
        }
        if hello.is_some() && discover.is_some() {
            break;
        }
    }
    let soon = Duration::from_millis(500);
    assert!(
        hello.is_some_and(|t| t < soon),
        "no hello at once after the join: {hello:?}"
    );
    assert!(
        discover.is_some_and(|t| t < soon),
        "no discover at once after the join: {discover:?}"
    );

    // The controller relays INFO and a unit's hello.
    let c2d = session_key(&KEY, &nd, &nc, driver, uplink, b"c2d");
    let sealed = |counter: u64, inner: &[u8]| {
        let mut f = proxy(uplink, &[0; 9]);
        f[22..24].copy_from_slice(&((9 + inner.len() + 16) as u16).to_le_bytes());
        f[24] = 6;
        f[25..33].copy_from_slice(&counter.to_be_bytes());
        let s = seal(&c2d, counter, &f, inner);
        f.extend_from_slice(&s);
        f
    };
    let mut info = proxy(uplink, &[7, 1]);
    info.extend_from_slice(mesh.as_bytes());
    info[22..24].copy_from_slice(&18u16.to_le_bytes());
    controller.send_to(&sealed(1, &info), peer).expect("sent");
    controller
        .send_to(&sealed(2, &hello_datagram(unit, "unit", "EP0253")), peer)
        .expect("sent");
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline && remote.device(unit).is_none() {
        std::thread::sleep(Duration::from_millis(10));
    }
    let seen = remote.device(unit).and_then(|d| d.address);
    let mesh_uid = remote.proxy_mesh_uid();
    let joined = remote.proxy_joined();

    remote.close();
    let mut left = false;
    while let Some((data, _)) = next(&controller) {
        if let Some(inner) = open(
            &d2c,
            u64::from_be_bytes(data[25..33].try_into().expect("8")),
            &data[..33],
            &data[33..],
        ) {
            left |= inner == proxy(driver, &[5]);
        }
        if left {
            break;
        }
    }

    assert!(joined, "not joined");
    assert_eq!(
        statuses.0.lock().expect("read").first(),
        Some(&ProxyJoinStatus::Joined {
            lease: Duration::from_secs(60)
        })
    );
    assert_eq!(
        seen,
        Some(CONTROLLER),
        "the relayed unit is not at the controller's address"
    );
    assert_eq!(mesh_uid, Some(mesh));
    assert!(left, "no LEAVE on close");
}
