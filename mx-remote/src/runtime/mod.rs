// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The running client: the socket, the two threads that drive it, and the
//! read surface over what they discover.

mod control;
mod info;
mod schedule;

#[cfg(test)]
mod control_tests;
#[cfg(test)]
mod tests;

// The reuseport check reads a Linux socket option, and pinning an addressless
// interface is Linux-only, so the whole file is.
#[cfg(all(test, target_os = "linux"))]
mod socket;

use std::io;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::event::{Event, EventHandler};
use crate::rx::process_frame;
use crate::state::{Device, State};
use crate::types::*;
use crate::wire::{
    build_hello, op, Addressee, BayUid, Conn, DeviceFeature, DeviceUid, FirmwareType, Opcode,
    SendError, Tx, MULTICAST_IP, MULTICAST_PORT, PROTOCOL_VERSION, VERSION,
};

pub use control::ControlError;
pub use info::{BayInfo, DeviceInfo};
use schedule::Schedule;

/// The name this client advertises when the caller sets none.
const DEFAULT_NAME: &str = "MXR Rust";

/// The serial this client advertises.
///
/// A client is not a unit with a serial number, but the field is fixed-width
/// and every device fills it, so it carries a constant rather than a blank.
const CLIENT_SERIAL: &str = "P9SN00000000";

/// The file the client's identifier is kept in, under the user's home
/// directory. The identifier has to survive a restart, or every peer would see
/// each run as a new client.
const UID_FILE: &str = ".mxr-uid";

/// Largest datagram the receive buffer accepts.
const RECV_BUFFER: usize = 65535;

/// How often the background thread re-examines the network.
const PROBE_TICK: Duration = Duration::from_secs(1);

/// How often that thread looks up from waiting to see whether it should stop.
const SHUTDOWN_POLL: Duration = Duration::from_millis(50);

/// Shortest gap between two discovery requests.
const DISCOVER_INTERVAL: Duration = Duration::from_secs(5);

/// How long a device that has announced itself is given to finish sending its
/// configuration before discovery is asked for again.
const CONFIG_GRACE: Duration = Duration::from_secs(15);

/// How a [`Remote`] finds the network.
///
/// [`Config::default`] discovers over multicast on the interface the host
/// picks, which is the right answer on a single-homed machine and arbitrary on
/// any other.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct Config {
    /// Where to send. Unset means the multicast group, or the interface's
    /// broadcast address when [`Config::broadcast`] is set.
    pub target_ip: Option<Ipv4Addr>,
    /// UDP port. Unset means the default for the selected mode.
    pub port: Option<u16>,
    /// Selects the interface by address.
    ///
    /// It becomes both the multicast egress interface and the membership
    /// interface, so it decides which NIC frames leave by and which one they
    /// are accepted on. Getting it wrong on a multi-homed host is one-sided:
    /// periodic broadcasts still arrive, so discovery looks healthy while
    /// every request this client sends leaves by the wrong NIC and is never
    /// answered.
    pub local_ip: Option<Ipv4Addr>,
    /// Selects the interface by name, taking precedence over
    /// [`Config::local_ip`].
    ///
    /// An interface with no address of its own - a tagged VLAN - can only be
    /// named this way, and only on Linux.
    pub interface: Option<String>,
    /// Use broadcast rather than multicast.
    pub broadcast: bool,
    /// The name this client advertises. Unset means `MXR Rust`.
    pub name: Option<String>,
    /// This client's identifier. Unset loads it from
    /// [`Config::uid_path`], generating and storing one on first run.
    pub uid: Option<DeviceUid>,
    /// Where the identifier is kept. Unset means `.mxr-uid` in the user's home
    /// directory.
    pub uid_path: Option<PathBuf>,
}

/// Everything the threads share.
struct Shared {
    uid: DeviceUid,
    name: String,
    handler: Arc<dyn EventHandler>,
    state: Mutex<State>,
    tx: Mutex<Tx>,
    schedule: Mutex<Schedule>,
    network: Mutex<Network>,
    closing: AtomicBool,
}

/// The network parameters the socket was opened with, so it can be reopened.
#[derive(Clone, Debug)]
struct Network {
    target_ip: Option<Ipv4Addr>,
    port: Option<u16>,
    local_ip: Option<Ipv4Addr>,
    interface: Option<String>,
    broadcast: bool,
}

impl Network {
    /// The address to send to: what the caller asked for, else the multicast
    /// group, else - in broadcast mode - the chosen interface's own broadcast
    /// address.
    fn target(&self) -> io::Result<Ipv4Addr> {
        if let Some(ip) = self.target_ip {
            return Ok(ip);
        }
        if !self.broadcast {
            return Ok(MULTICAST_IP);
        }
        Ok(crate::wire::broadcast_address(self.local_ip).unwrap_or(MULTICAST_IP))
    }

    fn port(&self) -> u16 {
        self.port.unwrap_or(if self.broadcast {
            crate::wire::BROADCAST_PORT
        } else {
            MULTICAST_PORT
        })
    }

    fn open(&self) -> io::Result<Conn> {
        Conn::open(
            self.target()?,
            self.port(),
            self.local_ip,
            self.interface.as_deref(),
        )
    }
}

/// A client on the MX Remote network.
///
/// Create one with [`Remote::new`], then [`Remote::start`] it. Discovery runs
/// on threads this client owns until [`Remote::close`], or until the `Remote`
/// is dropped.
pub struct Remote {
    shared: Arc<Shared>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

impl Remote {
    /// Builds a client, loading or generating its identifier.
    ///
    /// Nothing is sent and no socket is opened until [`Remote::start`].
    pub fn new(config: Config, handler: Arc<dyn EventHandler>) -> io::Result<Self> {
        let uid = match config.uid {
            Some(uid) => uid,
            None => load_uid(config.uid_path.clone())?,
        };
        let name = config.name.unwrap_or_else(|| DEFAULT_NAME.to_owned());
        Ok(Self {
            shared: Arc::new(Shared {
                uid,
                name,
                handler,
                state: Mutex::new(State::new(uid)),
                tx: Mutex::new(Tx::default()),
                schedule: Mutex::new(Schedule::new()),
                network: Mutex::new(Network {
                    target_ip: config.target_ip,
                    port: config.port,
                    local_ip: config.local_ip,
                    interface: config.interface,
                    broadcast: config.broadcast,
                }),
                closing: AtomicBool::new(false),
            }),
            workers: Mutex::new(Vec::new()),
        })
    }

    /// Opens the socket, announces this client and begins discovery.
    ///
    /// Returns once the receive thread is running; discovery continues in the
    /// background.
    pub fn start(&self) -> io::Result<()> {
        let conn = lock(&self.shared.network).open()?;
        lock(&self.shared.tx).set_conn(Some(conn));
        self.shared.closing.store(false, Ordering::SeqCst);
        self.spawn_workers()?;
        self.shared.announce();
        let _ = self.shared.discover();
        Ok(())
    }

    /// Stops discovery, closes the socket and waits for the threads to finish.
    ///
    /// Calling it more than once, or before [`Remote::start`], does nothing.
    pub fn close(&self) {
        self.shared.closing.store(true, Ordering::SeqCst);
        for worker in std::mem::take(&mut *lock(&self.workers)) {
            let _ = worker.join();
        }
        // Only once no thread can still be reading from it: a descriptor
        // released while another thread is parked on it can be reissued to
        // something else before that thread returns.
        lock(&self.shared.tx).set_conn(None);
    }

    fn spawn_workers(&self) -> io::Result<()> {
        let mut workers = lock(&self.workers);
        if !workers.is_empty() {
            return Ok(());
        }
        for (name, body) in [
            ("mxr-rx", Shared::receive_loop as fn(&Shared)),
            ("mxr-probe", Shared::probe_loop as fn(&Shared)),
        ] {
            let shared = Arc::clone(&self.shared);
            workers.push(
                std::thread::Builder::new()
                    .name(name.to_owned())
                    .spawn(move || body(&shared))?,
            );
        }
        Ok(())
    }

    // ---- identity ----

    /// This client's identifier, as peers see it.
    pub fn uid(&self) -> DeviceUid {
        self.shared.uid
    }

    /// The name this client advertises.
    pub fn name(&self) -> &str {
        &self.shared.name
    }

    /// The address frames are being sent to, once started.
    pub fn target(&self) -> Option<std::net::SocketAddrV4> {
        lock(&self.shared.tx).conn().map(|conn| conn.target())
    }

    // ---- reading the registry ----

    /// Every device heard from, in no particular order.
    pub fn devices(&self) -> Vec<DeviceUid> {
        self.shared
            .read(|state| state.devices.keys().copied().collect())
    }

    /// A snapshot of one device.
    pub fn device(&self, uid: DeviceUid) -> Option<DeviceInfo> {
        let now = Instant::now();
        self.shared
            .read(|state| state.device(uid).map(|d| DeviceInfo::of(d, now)))
    }

    /// The device with the given serial number.
    pub fn device_by_serial(&self, serial: &str) -> Option<DeviceUid> {
        self.shared
            .read(|state| state.device_by_serial(serial).map(|d| d.uid))
    }

    /// Resolves a device from its dotted-hex identifier, falling back to a
    /// serial-number match.
    pub fn resolve_device(&self, name: &str) -> Option<DeviceUid> {
        if let Ok(uid) = name.parse::<DeviceUid>() {
            if self.shared.read(|state| state.device(uid).is_some()) {
                return Some(uid);
            }
        }
        self.device_by_serial(name)
    }

    /// A snapshot of one bay.
    pub fn bay(&self, uid: BayUid) -> Option<BayInfo> {
        self.shared
            .read(|state| state.bay(uid).map(|bay| BayInfo::of(state, bay)))
    }

    /// The bay on `device` with the given port name, such as `Output 1`.
    pub fn bay_by_name(&self, device: DeviceUid, port_name: &str) -> Option<BayUid> {
        self.shared.read(|state| {
            state
                .device(device)?
                .bay_by_name(port_name)
                .map(crate::state::Bay::uid)
        })
    }

    /// The source bay advertising the given multicast group, for the video or
    /// the audio stream.
    pub fn bay_by_stream_ip(&self, ip: Ipv4Addr, audio: bool) -> Option<BayUid> {
        self.shared.read(|state| state.bay_by_stream_ip(ip, audio))
    }

    /// The V2IP streams a device advertises.
    pub fn v2ip_sources(&self, uid: DeviceUid) -> Option<Vec<V2ipStreamSources>> {
        self.shared
            .read(|state| state.device(uid)?.v2ip_sources.clone())
    }

    /// A V2IP device's own encoder configuration.
    pub fn v2ip_details(&self, uid: DeviceUid) -> Option<DeviceV2ipDetails> {
        self.shared.read(|state| state.device(uid)?.v2ip_details)
    }

    /// The streams a V2IP sink is subscribed to.
    pub fn v2ip_sink(&self, uid: DeviceUid) -> Option<DeviceV2ipSink> {
        self.shared.read(|state| state.device(uid)?.v2ip_sink)
    }

    /// Transport statistics a V2IP device reports.
    pub fn v2ip_stats(&self, uid: DeviceUid) -> Option<V2ipDeviceStats> {
        self.shared.read(|state| state.device(uid)?.v2ip_stats)
    }

    /// The video-wall tiling a V2IP device is configured for.
    pub fn v2ip_tiling(&self, uid: DeviceUid) -> Option<V2ipTilingConfig> {
        self.shared.read(|state| state.device(uid)?.tiling)
    }

    /// The audio endpoint tree a device exposes.
    pub fn audio_endpoints(&self, uid: DeviceUid) -> Option<AudioEndpoints> {
        self.shared.read(|state| state.device(uid)?.audio.clone())
    }

    /// The multiviewer layout a device is showing.
    pub fn multiviewer_status(&self, uid: DeviceUid) -> Option<MultiviewerStatus> {
        self.shared
            .read(|state| state.device(uid)?.multiviewer.clone())
    }

    /// An amplifier's Dolby decoder settings.
    pub fn dolby_settings(&self, uid: DeviceUid) -> Option<AmpDolbySettings> {
        self.shared.read(|state| state.device(uid)?.dolby_settings)
    }

    /// A device's remote-control settings.
    pub fn rc_settings(&self, uid: DeviceUid) -> Option<RcSettings> {
        self.shared
            .read(|state| state.device(uid)?.rc_settings.clone())
    }

    /// Every network port a device reports, in port order.
    pub fn network_status(&self, uid: DeviceUid) -> Vec<NetworkPortStatus> {
        self.shared.read(|state| {
            state
                .device(uid)
                .map(|d| d.network.values().cloned().collect())
                .unwrap_or_default()
        })
    }

    /// The mesh topology a device reports.
    pub fn topology(&self, uid: DeviceUid) -> Vec<TopologyEntry> {
        self.shared.read(|state| {
            state
                .device(uid)
                .map(|d| d.topology.clone())
                .unwrap_or_default()
        })
    }

    /// The EDID a device last reported: the display on its output, or the one
    /// it presents to the source on its input.
    ///
    /// Filled in by a device's answer to [`Remote::request_edid`], and by any
    /// answer to a peer's request that this client happened to hear.
    pub fn edid(&self, uid: DeviceUid, output: bool) -> Option<Vec<u8>> {
        self.shared
            .read(|state| state.device(uid)?.edid(output).map(<[u8]>::to_vec))
    }

    /// How many frames from other senders have parsed since this client
    /// started.
    ///
    /// It separates a mesh with nothing on it from an interface nothing is on:
    /// a client that has discovered no device but is counting frames is
    /// hearing traffic it cannot get answers from, which on a multi-homed host
    /// is what a wrong [`Config::local_ip`] looks like. Frames this client
    /// sent are not counted, because the host loops its own multicast back
    /// whichever interface was selected.
    pub fn frames_received(&self) -> u64 {
        self.shared.read(|state| state.frames_received)
    }

    /// Every firmware image a device reports a version for.
    pub fn firmware(&self, uid: DeviceUid) -> Vec<(FirmwareType, FirmwareVersion)> {
        self.shared.read(|state| {
            state
                .device(uid)
                .map(|d| d.firmware.iter().map(|(k, v)| (*k, v.clone())).collect())
                .unwrap_or_default()
        })
    }

    // ---- reconfiguring ----

    /// Changes the interface and the multicast/broadcast mode while running,
    /// reopening the socket when either differs from what is in use.
    pub fn update_config(&self, local_ip: Option<Ipv4Addr>, broadcast: bool) -> io::Result<()> {
        let network = {
            let mut network = lock(&self.shared.network);
            if network.local_ip == local_ip && network.broadcast == broadcast {
                return Ok(());
            }
            network.local_ip = local_ip;
            network.broadcast = broadcast;
            network.clone()
        };
        // Opened before the old one is dropped, so a bind that fails leaves the
        // client on the socket it had rather than on none.
        let conn = network.open()?;
        lock(&self.shared.tx).set_conn(Some(conn));
        self.shared.announce();
        let _ = self.shared.discover();
        Ok(())
    }

    /// Asks every device on the network to announce itself.
    pub fn discover(&self) -> Result<(), SendError> {
        self.shared.discover()
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        self.close();
    }
}

impl Shared {
    /// Reads the registry.
    fn read<R>(&self, f: impl FnOnce(&State) -> R) -> R {
        f(&lock(&self.state))
    }

    /// Mutates the registry, then delivers what changed.
    ///
    /// The queue is drained after the lock is released, so an event handler
    /// may call back into the library.
    fn mutate<R>(&self, f: impl FnOnce(&mut State, &mut Vec<Event>) -> R) -> R {
        let mut events = Vec::new();
        let result = f(&mut lock(&self.state), &mut events);
        self.dispatch(events);
        result
    }

    fn dispatch(&self, events: Vec<Event>) {
        for event in events {
            event.dispatch(&*self.handler);
        }
    }

    /// Decodes one datagram and delivers what it changed.
    ///
    /// This is the receive entry point. Keeping it distinct from the decode
    /// below matters even though the wrapper is thin: announcing hello from
    /// here, driven by arriving traffic rather than by a clock, is a mistake
    /// this shape makes visible.
    fn process_datagram(&self, data: &[u8], from: Ipv4Addr) {
        let events = process_frame(&mut lock(&self.state), data, Some(from), Instant::now());
        self.dispatch(events);
    }

    /// Sends a frame, refusing one the addressee cannot decode.
    fn send(&self, to: &Addressee, opcode: Opcode, payload: &[u8]) -> Result<usize, SendError> {
        lock(&self.tx).send(to, self.uid, opcode, payload)
    }

    fn discover(&self) -> Result<(), SendError> {
        lock(&self.schedule).discovered(Instant::now());
        self.send(&Addressee::Broadcast, op::SYS_DISCOVER, &[])?;
        Ok(())
    }

    /// Announces this client, and re-arms the announcement timer only once the
    /// frame is away.
    ///
    /// The firmware resets its own hello timeout inside the branch where the
    /// transmit succeeded. A send that fails is then retried on the next tick
    /// rather than costing a whole interval of silence, which matters most at
    /// startup and after a network blip: exactly when being heard is worth the
    /// most.
    fn announce(&self) {
        let payload = build_hello(
            PROTOCOL_VERSION,
            &self.name,
            CLIENT_SERIAL,
            VERSION,
            DeviceFeature::MANAGER.bits(),
        );
        match self.send(&Addressee::Broadcast, op::SYS_HELLO, &payload) {
            Ok(n) if n > 0 => lock(&self.schedule).announced(Instant::now()),
            _ => {}
        }
    }

    /// Waits out one tick, reporting whether the client is still running.
    ///
    /// Slept in short steps rather than one, so closing does not have to wait
    /// out a whole tick before the thread can be joined.
    fn sleep_until_next_tick(&self) -> bool {
        let deadline = Instant::now() + PROBE_TICK;
        while Instant::now() < deadline {
            if self.closing.load(Ordering::SeqCst) {
                return false;
            }
            std::thread::sleep(SHUTDOWN_POLL);
        }
        !self.closing.load(Ordering::SeqCst)
    }

    /// Whether it is time to announce again.
    ///
    /// This is a timer, not a response to traffic: a device announces itself on
    /// a schedule whether or not anything is talking to it, and a client that
    /// only re-announced when a datagram arrived would go silent on a quiet
    /// network and stay unknown to every peer that started after it.
    fn announce_due(&self, now: Instant) -> bool {
        !self.closing.load(Ordering::SeqCst) && lock(&self.schedule).announce_due(now)
    }

    /// Reads datagrams until the client is closing.
    fn receive_loop(&self) {
        let mut buf = vec![0u8; RECV_BUFFER];
        while !self.closing.load(Ordering::SeqCst) {
            let Some(conn) = lock(&self.tx).conn() else {
                break;
            };
            match conn.recv(&mut buf) {
                Ok(Some((data, from))) => self.process_datagram(data, from),
                Ok(None) => {}
                Err(_) => break,
            }
        }
    }

    /// Re-examines the network once a second: liveness, the announcement timer
    /// and whether anything still owes us its configuration.
    fn probe_loop(&self) {
        while self.sleep_until_next_tick() {
            let now = Instant::now();
            let want_discover = self.mutate(|state, ev| {
                let mut incomplete = false;
                let mut any_complete = false;
                for device in state.devices.values_mut() {
                    device.check_online(now, ev);
                    if device.configuration_complete() {
                        any_complete = true;
                    } else if now.saturating_duration_since(device.hello_received) > CONFIG_GRACE {
                        // Past the grace period a device has said nothing more,
                        // so ask the network again rather than wait forever.
                        incomplete = true;
                    }
                }
                // Nothing has finished describing itself, so nothing has been
                // discovered yet at all.
                incomplete || !any_complete
            });

            let discover_due = lock(&self.schedule).discover_due(now);
            if self.announce_due(now) {
                self.announce();
            }
            if want_discover && discover_due {
                let _ = self.discover();
            }
        }
    }
}

/// Takes a lock, continuing through a poisoning.
///
/// A poisoned lock here means a panic somewhere that held it. The state behind
/// it is a cache of what devices have reported and is rebuilt by the next
/// frame from each, so refusing to touch it again would retire a working
/// client over one bad datagram.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Loads the client's identifier, generating and storing one on first run.
///
/// A generated identifier that cannot be stored is still used: a client with a
/// new identity each run is worse than one that works today, and the failure is
/// the caller's home directory, not the network.
fn load_uid(path: Option<PathBuf>) -> io::Result<DeviceUid> {
    let path = path.or_else(|| {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(|home| PathBuf::from(home).join(UID_FILE))
    });
    if let Some(path) = &path {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(array) = <[u8; 16]>::try_from(bytes.get(..16).unwrap_or_default()) {
                return Ok(DeviceUid::from_array(array));
            }
        }
    }
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|e| io::Error::other(e.to_string()))?;
    if let Some(path) = &path {
        let _ = std::fs::write(path, bytes);
    }
    Ok(DeviceUid::from_array(bytes))
}

/// The protocol floor is checked against what the device says it can decode.
impl crate::wire::ProtocolTarget for Device {
    fn serial(&self) -> &str {
        Device::serial(self)
    }

    fn supported_protocol(&self) -> u16 {
        self.hello.supported_protocol
    }
}
