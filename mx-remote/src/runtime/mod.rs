// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The running client: the socket, the two threads that drive it, and the
//! read surface over what they discover.

mod control;
mod info;
mod logo_server;
mod osd;
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
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::event::{Event, EventHandler};
use crate::rx::{hello_source, process_frame_on};
use crate::state::{Device, State, CONFIG_GRACE};
use crate::types::*;
use crate::wire::{
    build_hello, op, Addressee, BayUid, Capture, Conn, DeviceFeature, DeviceUid, FirmwareType,
    Opcode, Path, SendError, Tx, V2ipFpgaFeature, MULTICAST_PORT, PROTOCOL_VERSION, VERSION,
};

pub use control::ControlError;
pub use info::{BayInfo, DeviceInfo, DeviceTreeEntry};
use osd::OsdSender;
use schedule::Schedule;

/// The name this client advertises when the caller sets none.
const DEFAULT_NAME: &str = "MXR Rust";

/// The serial this client advertises when the caller sets none.
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

/// Longest a unit leaves its hello off the broadcast endpoint.
const HELLO_BROADCAST: Duration = Duration::from_secs(30);

/// Whether a hello at `now` goes to the broadcast endpoint whatever peers
/// need: the first since the sockets opened, or [`HELLO_BROADCAST`] or more
/// after the last one there.
fn hello_due_on_broadcast(last: Option<Instant>, now: Instant) -> bool {
    last.map_or(true, |last| {
        now.saturating_duration_since(last) >= HELLO_BROADCAST
    })
}

/// Shortest gap between two discovers to every device.
///
/// Every unit answers one with everything it holds, so each costs the whole
/// mesh's state, and a unit merges only the discovers that arrive while its
/// answer is pending.
const DISCOVER_INTERVAL: Duration = Duration::from_secs(60);

/// Shortest gap between two discovers asking one device for what it still
/// owes.
const OWED_ASK_INTERVAL: Duration = Duration::from_secs(60);

/// How many times a device is asked for what it still owes.
///
/// Every unit broadcasts everything it holds every five minutes unasked, so a
/// device still owing something after these asks is described by that.
const OWED_ASK_LIMIT: u8 = 3;

/// How long a discover to every device stands in for asking any one of them.
///
/// Each device answers it with everything it holds, after a jitter of up to a
/// few seconds, so asking one again inside this window repeats an answer
/// already on its way.
const DISCOVER_COVERS: Duration = Duration::from_secs(60);

/// How a [`Remote`] finds the network.
///
/// A client always runs both endpoints a unit has: the discovery group
/// 224.8.8.8 on 8812, and the subnet broadcast address on 8811, on the
/// interface the host picks unless one is selected, which is the right answer
/// on a single-homed machine and arbitrary on any other.
///
/// It picks each frame's endpoints as a unit does. A frame for every unit at
/// once goes to both, and the hello does at least every 30 seconds. A frame
/// meant for one device alone, such as a ping, goes by the endpoint that
/// device is heard by. Every other frame goes to the group, and to the
/// broadcast address as well while some peer has not been heard by multicast
/// for two minutes or reports that its multicast fails. A peer heard only by
/// broadcast in that time has this client announce that its own multicast
/// fails. A datagram arriving by both endpoints within 100ms is acted on
/// once.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Config {
    /// For tests and development only: replaces both endpoints with one on
    /// this address, so a test runs on a scratch group rather than a live
    /// network. A production client leaves it unset, and always runs both
    /// endpoints a unit has.
    pub target_ip: Option<Ipv4Addr>,
    /// For tests and development only: the port of the one endpoint
    /// [`Config::target_ip`] opens, 8812 when unset. Ignored without it.
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
    /// The name this client advertises. Unset means `MXR Rust`.
    pub name: Option<String>,
    /// The serial number this client advertises. Unset means `P9SN00000000`.
    pub serial: Option<String>,
    /// The firmware version this client advertises. Unset means this
    /// library's version.
    pub version: Option<String>,
    /// This client's identifier. Unset loads it from
    /// [`Config::uid_path`], generating and storing one on first run.
    pub uid: Option<DeviceUid>,
    /// Where the identifier is kept. Unset means `.mxr-uid` in the user's home
    /// directory.
    pub uid_path: Option<PathBuf>,
    /// The TCP port units fetch an overlay sync's logo images from. Unset
    /// picks a free one when the first sync with images is sent.
    pub logo_port: Option<u16>,
    /// Announce this client as a mesh device rather than a management
    /// application.
    ///
    /// A mesh device announces mesh support instead of management, which is
    /// what a mesh takes as a member and can hand its controller role to.
    /// Units then take writes from it only while they follow it as their
    /// controller.
    pub mesh_device: bool,
    /// Read the source hardware address of every hello, as a unit stores it,
    /// into [`DeviceInfo::mac`].
    ///
    /// A UDP socket never sees that address, so this opens a packet socket
    /// taking a copy of every IPv4 frame on every interface, which
    /// [`Remote::start`] fails without. Linux only, and it needs
    /// `CAP_NET_RAW`. Only a hello to one of this client's own ports counts.
    pub capture_hello_mac: bool,
}

impl Default for Config {
    /// Both endpoints a unit has, on the interface the host picks, with
    /// every other setting unset.
    fn default() -> Self {
        Self {
            target_ip: None,
            port: None,
            local_ip: None,
            interface: None,
            name: None,
            serial: None,
            version: None,
            uid: None,
            uid_path: None,
            logo_port: None,
            mesh_device: false,
            capture_hello_mac: false,
        }
    }
}

/// Everything the threads share.
struct Shared {
    uid: DeviceUid,
    name: String,
    serial: String,
    version: String,
    mesh_device: bool,
    capture_hello_mac: bool,
    /// The mesh controller this client follows, zero for none.
    mesh_controller: Mutex<DeviceUid>,
    /// Whether the caller has mesh bays mapped, without which a controller
    /// does not announce the role.
    mesh_bays_mapped: AtomicBool,
    handler: Arc<dyn EventHandler>,
    state: Mutex<State>,
    tx: Mutex<Tx>,
    schedule: Mutex<Schedule>,
    network: Mutex<Network>,
    osd: Mutex<OsdSender>,
    /// Woken when units ask for overlay settings.
    osd_queued: Condvar,
    logo_port: Option<u16>,
    closing: AtomicBool,
    /// Whether some peer has to be reached by broadcast as well.
    broadcast_needed: AtomicBool,
    /// When a hello last went to the broadcast endpoint, `None` since the
    /// sockets opened.
    hello_on_broadcast: Mutex<Option<Instant>>,
    /// Whether a live peer is heard by broadcast but not by multicast.
    multicast_fault: AtomicBool,
}

/// The network parameters the socket was opened with, so it can be reopened.
#[derive(Clone, Debug)]
struct Network {
    target_ip: Option<Ipv4Addr>,
    port: Option<u16>,
    local_ip: Option<Ipv4Addr>,
    interface: Option<String>,
}

/// Which endpoints a client opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Plan {
    /// Both a unit has: the discovery group and the subnet broadcast address.
    Both,
    /// One endpoint on this address and port, for tests and development.
    One(Ipv4Addr, u16),
}

impl Network {
    /// The endpoints to open: both a unit has, unless a test or development
    /// setup named one of its own.
    fn plan(&self) -> Plan {
        match self.target_ip {
            Some(ip) => Plan::One(ip, self.port.unwrap_or(MULTICAST_PORT)),
            None => Plan::Both,
        }
    }

    fn open(&self) -> io::Result<Conn> {
        match self.plan() {
            Plan::One(ip, port) => Conn::open(ip, port, self.local_ip, self.interface.as_deref()),
            Plan::Both => {
                let broadcast = crate::wire::broadcast_address(
                    self.local_ip
                        .or_else(|| self.interface.as_deref().and_then(crate::wire::address_of)),
                )?;
                Conn::open_both(
                    broadcast,
                    crate::wire::BROADCAST_PORT,
                    self.local_ip,
                    self.interface.as_deref(),
                )
            }
        }
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
        let serial = config.serial.unwrap_or_else(|| CLIENT_SERIAL.to_owned());
        let version = config.version.unwrap_or_else(|| VERSION.to_owned());
        Ok(Self {
            shared: Arc::new(Shared {
                uid,
                name,
                serial,
                version,
                mesh_device: config.mesh_device,
                capture_hello_mac: config.capture_hello_mac,
                mesh_controller: Mutex::new(DeviceUid::ZERO),
                mesh_bays_mapped: AtomicBool::new(false),
                handler,
                state: Mutex::new(State::new(uid)),
                tx: Mutex::new(Tx::default()),
                schedule: Mutex::new(Schedule::new()),
                network: Mutex::new(Network {
                    target_ip: config.target_ip,
                    port: config.port,
                    local_ip: config.local_ip,
                    interface: config.interface,
                }),
                osd: Mutex::new(OsdSender::new()),
                osd_queued: Condvar::new(),
                logo_port: config.logo_port,
                closing: AtomicBool::new(false),
                broadcast_needed: AtomicBool::new(false),
                hello_on_broadcast: Mutex::new(None),
                multicast_fault: AtomicBool::new(false),
            }),
            workers: Mutex::new(Vec::new()),
        })
    }

    /// Opens the socket, announces this client and begins discovery.
    ///
    /// Returns once the receive thread is running; discovery continues in the
    /// background.
    pub fn start(&self) -> io::Result<()> {
        // Opened first, so a client that cannot capture what it was asked to
        // has opened nothing else either.
        let capture = if self.shared.capture_hello_mac {
            Some(Capture::open()?)
        } else {
            None
        };
        let conn = lock(&self.shared.network).open()?;
        lock(&self.shared.tx).set_conn(Some(conn));
        // The first hello on new sockets goes to both endpoints.
        *lock(&self.shared.hello_on_broadcast) = None;
        self.shared.closing.store(false, Ordering::SeqCst);
        self.spawn_workers(capture)?;
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
        // A closed client answers no unit, so it holds no sync either.
        self.shared.drop_osd_sync();
        // Only once no thread can still be reading from it: a descriptor
        // released while another thread is parked on it can be reissued to
        // something else before that thread returns.
        lock(&self.shared.tx).set_conn(None);
    }

    fn spawn_workers(&self, capture: Option<Capture>) -> io::Result<()> {
        let mut workers = lock(&self.workers);
        if !workers.is_empty() {
            return Ok(());
        }
        if let Some(capture) = capture {
            let shared = Arc::clone(&self.shared);
            workers.push(
                std::thread::Builder::new()
                    .name("mxr-capture".to_owned())
                    .spawn(move || shared.capture_loop(&capture))?,
            );
        }
        for (name, body) in [
            ("mxr-rx", Shared::receive_loop as fn(&Shared)),
            (
                "mxr-rx-bcast",
                Shared::receive_secondary_loop as fn(&Shared),
            ),
            ("mxr-probe", Shared::probe_loop as fn(&Shared)),
            ("mxr-osd", Shared::osd_loop as fn(&Shared)),
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

    /// The serial number this client advertises.
    pub fn serial(&self) -> &str {
        &self.shared.serial
    }

    /// The firmware version this client advertises.
    pub fn version(&self) -> &str {
        &self.shared.version
    }

    /// The mesh controller this client announces it follows, zero for none.
    pub fn mesh_controller(&self) -> DeviceUid {
        *lock(&self.shared.mesh_controller)
    }

    /// Sets the mesh controller this client announces it follows: the zero
    /// uid for none, and its own uid while it is the controller.
    ///
    /// The hello then announces mesh membership while there is one, and the
    /// mesh controller role while it is this client and
    /// [`Remote::set_mesh_bays_mapped`] says it has bays. Only the announcement
    /// changes: membership reports, and the units that follow this client,
    /// are the caller's to keep in step. A change is announced at once rather
    /// than at the next scheduled hello.
    pub fn set_mesh_controller(&self, controller: DeviceUid) {
        let changed = {
            let mut stored = lock(&self.shared.mesh_controller);
            std::mem::replace(&mut *stored, controller) != controller
        };
        // What a write to this client is judged against.
        lock(&self.shared.state).mesh_controller = controller;
        if changed {
            self.shared.announce();
        }
    }

    /// Sets whether the caller has mesh bays mapped: the inputs and outputs of
    /// the units in its mesh, which a controller keeps.
    ///
    /// A unit announces the mesh controller role only while it is its own
    /// controller and has bays mapped, so a freshly promoted controller with
    /// none does not, and neither does this client until this is set. A
    /// change is announced at once.
    pub fn set_mesh_bays_mapped(&self, mapped: bool) {
        let before = self.shared.features();
        self.shared.mesh_bays_mapped.store(mapped, Ordering::SeqCst);
        if self.shared.features() != before {
            self.shared.announce();
        }
    }

    /// The features this client announces in its hello as of now, which is
    /// what a peer lists it with.
    pub fn features(&self) -> DeviceFeature {
        self.shared.features()
    }

    /// The address frames are being sent to, once started.
    pub fn target(&self) -> Option<std::net::SocketAddrV4> {
        lock(&self.shared.tx).conn().map(|conn| conn.target())
    }

    // ---- reading the registry ----

    /// Every device heard from, in the order a unit lists them: the
    /// [`Remote::device_tree`] walk without this client.
    ///
    /// A device silent for three times the window that takes it offline is
    /// forgotten, as a unit forgets one, and one heard after that is listed
    /// last, as a new device.
    pub fn devices(&self) -> Vec<DeviceUid> {
        self.shared.read(State::devices_in_order)
    }

    /// Every device heard from and this client, as a unit keeps its registry:
    /// a tree, walked depth first, a parent before its children.
    ///
    /// This client heads the root, and each device joins the end of it when
    /// first heard. A device another one's topology report lists moves to the
    /// end of that one's children, unless it is below it already, and back to
    /// the end of the root once a later report leaves it out. Only a report
    /// differing byte for byte from that device's last reshapes the tree, and
    /// only for devices registered by then; a device not registered is
    /// skipped. A device the sender sits below, the sender itself included,
    /// stays where it is. A forgotten device's children move to the end of
    /// its parent's.
    pub fn device_tree(&self) -> Vec<DeviceTreeEntry> {
        self.shared.read(|state| {
            state
                .device_tree()
                .into_iter()
                .map(|(uid, depth)| DeviceTreeEntry { uid, depth })
                .collect()
        })
    }

    /// A snapshot of one device.
    pub fn device(&self, uid: DeviceUid) -> Option<DeviceInfo> {
        let now = Instant::now();
        let paths_known = lock(&self.shared.tx).conn().is_some_and(|c| c.is_dual());
        self.shared.read(|state| {
            let mut info = DeviceInfo::of(state.device(uid)?, now, paths_known);
            info.mac = state.hello_source(uid);
            Some(info)
        })
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

    /// The OneIP streams a device advertises.
    pub fn v2ip_sources(&self, uid: DeviceUid) -> Option<Vec<V2ipStreamSources>> {
        self.shared
            .read(|state| state.device(uid)?.v2ip_sources.clone())
    }

    /// A OneIP device's own encoder configuration.
    pub fn v2ip_details(&self, uid: DeviceUid) -> Option<DeviceV2ipDetails> {
        self.shared.read(|state| state.device(uid)?.v2ip_details)
    }

    /// The streams a OneIP sink is subscribed to.
    pub fn v2ip_sink(&self, uid: DeviceUid) -> Option<DeviceV2ipSink> {
        self.shared.read(|state| state.device(uid)?.v2ip_sink)
    }

    /// Transport statistics a OneIP device reports.
    pub fn v2ip_stats(&self, uid: DeviceUid) -> Option<V2ipDeviceStats> {
        self.shared.read(|state| state.device(uid)?.v2ip_stats)
    }

    /// What a OneIP device's video processor supports.
    ///
    /// `None` until the device has reported a non-empty mask about itself.
    /// A device reports nothing at all while its processor has yet to answer,
    /// and an older processor answers with none of the optional commands, so
    /// "no features" and "not yet known" are the same bytes on the wire and
    /// both read as `None` here. A mask that has arrived only gains bits.
    pub fn v2ip_features(&self, uid: DeviceUid) -> Option<V2ipFpgaFeature> {
        self.shared.read(|state| state.device(uid)?.v2ip_features)
    }

    /// A OneIP device's settings, `None` until it has reported any.
    ///
    /// A device reports only the settings it has, so one missing from
    /// [`V2ipDeviceSettings::valid`] once the rest have arrived is one the
    /// device does not have.
    pub fn v2ip_device_settings(&self, uid: DeviceUid) -> Option<V2ipDeviceSettings> {
        self.shared.read(|state| state.device(uid)?.v2ip_settings)
    }

    /// How a OneIP device's output mixes down to stereo, as it last reported,
    /// `None` while it reports no downmix: until its video processor has
    /// answered for it, and always for one whose output cannot mix down.
    pub fn v2ip_downmix(&self, uid: DeviceUid) -> Option<V2ipDownmix> {
        self.shared.read(|state| state.device(uid)?.v2ip_downmix)
    }

    /// The locale a OneIP device runs, by its number, as it last reported or
    /// as it was last written; `None` while it reports none, and always for
    /// one whose firmware has no locales.
    /// [`V2IP_LOCALE_UNKNOWN`](crate::V2IP_LOCALE_UNKNOWN) is a stored locale
    /// its firmware doesn't know.
    pub fn v2ip_locale(&self, uid: DeviceUid) -> Option<u8> {
        self.shared.read(|state| state.device(uid)?.v2ip_locale)
    }

    /// The VLAN configuration a OneIP device last reported about itself,
    /// `None` until it has reported one.
    pub fn v2ip_vlan(&self, uid: DeviceUid) -> Option<V2ipVlan> {
        self.shared.read(|state| state.device(uid)?.v2ip_vlan)
    }

    /// What a OneIP output last reported playing from a session description,
    /// `None` until it has reported.
    pub fn v2ip_sink_sdp(&self, uid: DeviceUid) -> Option<V2ipSinkSdp> {
        self.shared
            .read(|state| state.device(uid)?.v2ip_sink_sdp.clone())
    }

    /// The session description a OneIP output last reported playing, `None`
    /// until it has and once it reports playing none.
    ///
    /// An output reports one when it starts a stream other than by its mesh
    /// controller's command, and when asked with
    /// [`Remote::request_v2ip_sink_description`].
    pub fn v2ip_sink_description(&self, uid: DeviceUid) -> Option<String> {
        self.shared
            .read(|state| state.device(uid)?.v2ip_sink_description.clone())
    }

    /// How a OneIP source unit's streams are encrypted, as it last reported
    /// it, `None` until it has reported since its hello last showed it may
    /// have restarted: it was offline, its reboot toggle flipped, or it is
    /// about to reboot.
    pub fn v2ip_privacy(&self, uid: DeviceUid) -> Option<V2ipPrivacy> {
        self.shared.read(|state| state.device(uid)?.v2ip_privacy)
    }

    /// The overlay sync a management unit last sent, `None` until it has
    /// sent one.
    pub fn osd_sync(&self, uid: DeviceUid) -> Option<OsdSync> {
        self.shared
            .read(|state| state.device(uid)?.osd_sync.clone())
    }

    /// How a unit last reported getting on with an overlay sync, `None` until
    /// it has reported. Match it to the sync by `seq` and the sync's source.
    pub fn osd_sync_report(&self, uid: DeviceUid) -> Option<OsdSyncReport> {
        self.shared
            .read(|state| state.device(uid)?.osd_sync_report.clone())
    }

    /// The message on a unit's output, as the unit last reported it to this
    /// client, `None` until it has. A unit reports it only when asked, or
    /// when this client writes it.
    pub fn osd_message(&self, uid: DeviceUid) -> Option<OsdMessage> {
        self.shared
            .read(|state| state.device(uid)?.osd_message.clone())
    }

    /// The health and IP configuration a unit last reported, `None` until it
    /// has reported them.
    pub fn unit_status(&self, uid: DeviceUid) -> Option<UnitStatus> {
        self.shared
            .read(|state| state.device(uid)?.unit_status.clone())
    }

    /// The PTP state a OneIP device last reported about itself, `None` until
    /// it has reported one, and while it has no PTP to report: none on its
    /// video processor, or in power save.
    pub fn v2ip_ptp(&self, uid: DeviceUid) -> Option<V2ipPtpState> {
        self.shared.read(|state| state.device(uid)?.v2ip_ptp)
    }

    /// The test card a OneIP sink last reported, `None` until it has answered
    /// [`Remote::request_v2ip_testcard`] or a change.
    pub fn v2ip_testcard(&self, uid: DeviceUid) -> Option<V2ipTestcard> {
        self.shared.read(|state| state.device(uid)?.v2ip_testcard)
    }

    /// The time zone a device announced for its mesh, `None` until it has.
    ///
    /// The mesh controller announces it with every periodic broadcast, empty
    /// when it has none.
    pub fn time_zone(&self, uid: DeviceUid) -> Option<TimeZone> {
        self.shared
            .read(|state| state.device(uid)?.time_zone.clone())
    }

    /// A device's clock as of now: the time it last announced, advanced by
    /// how long ago that arrived. `None` until it has announced one.
    ///
    /// The mesh controller announces its clock with every periodic broadcast,
    /// and only once it has been set.
    pub fn device_clock(&self, uid: DeviceUid) -> Option<SystemTime> {
        let (utc, at) = self.shared.read(|state| state.device(uid)?.clock)?;
        let announced = UNIX_EPOCH.checked_add(Duration::from_secs(u64::from(utc)))?;
        announced.checked_add(at.elapsed())
    }

    /// The video-wall tiling a OneIP device is configured for.
    pub fn v2ip_tiling(&self, uid: DeviceUid) -> Option<V2ipTilingConfig> {
        self.shared.read(|state| state.device(uid)?.tiling)
    }

    /// The audio endpoint tree a device exposes.
    pub fn audio_endpoints(&self, uid: DeviceUid) -> Option<AudioEndpoints> {
        self.shared.read(|state| state.device(uid)?.audio.clone())
    }

    /// When the device's audio features report was last taken, `None` before
    /// one; a report ignored in the 500 ms after an audio command to the
    /// device doesn't count.
    pub fn audio_reported(&self, uid: DeviceUid) -> Option<Instant> {
        self.shared.read(|state| state.device(uid)?.audio_reported)
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

    /// Changes the interface by address while running, reopening the sockets
    /// when it differs from the one in use. `None` lets the host choose again.
    pub fn update_config(&self, local_ip: Option<Ipv4Addr>) -> io::Result<()> {
        let network = {
            let mut network = lock(&self.shared.network);
            if network.local_ip == local_ip {
                return Ok(());
            }
            network.local_ip = local_ip;
            network.clone()
        };
        // Opened before the old one is dropped, so a bind that fails leaves the
        // client on the socket it had rather than on none.
        let conn = network.open()?;
        lock(&self.shared.tx).set_conn(Some(conn));
        // The first hello on new sockets goes to both endpoints.
        *lock(&self.shared.hello_on_broadcast) = None;
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
        let result = {
            let mut state = lock(&self.state);
            let result = f(&mut state, &mut events);
            state.mirror_mesh_events(&mut events);
            result
        };
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
    /// below matters even though the wrapper is thin: announcing is driven by a
    /// clock, and the one frame that may add a hello here is a ping addressed
    /// to this client, which asks for exactly that.
    #[cfg(test)]
    fn process_datagram(&self, data: &[u8], from: Ipv4Addr) {
        self.process_datagram_on(data, from, None);
    }

    /// [`Self::process_datagram`] for a datagram that arrived by a known one
    /// of two endpoints.
    fn process_datagram_on(&self, data: &[u8], from: Ipv4Addr, path: Option<Path>) {
        let (events, hello_requested, state_requests, osd_requests) = {
            let mut state = lock(&self.state);
            let now = Instant::now();
            let events = process_frame_on(&mut state, data, Some(from), path, now);
            let fault_changed = self.judge_paths(&state, now);
            (
                events,
                std::mem::take(&mut state.hello_requested) || fault_changed,
                std::mem::take(&mut state.state_requests),
                std::mem::take(&mut state.osd_requests),
            )
        };
        self.dispatch(events);
        if hello_requested {
            self.announce();
        }
        for device in state_requests {
            self.request_state(device);
        }
        if !osd_requests.is_empty() {
            self.answer_osd_requests(osd_requests);
        }
    }

    /// Takes one frame captured below the IP stack, recording the source
    /// address of a hello it carries to one of `ports`.
    fn note_link_frame_on(&self, frame: &[u8], ports: &[u16]) {
        if let Some((sender, mac)) = hello_source(frame, ports) {
            lock(&self.state).note_hello_source(sender, mac, Instant::now());
        }
    }

    /// Takes one frame captured below the IP stack, against the ports this
    /// client's endpoints are bound to.
    fn note_link_frame(&self, frame: &[u8]) {
        let Some(conn) = lock(&self.tx).conn() else {
            return;
        };
        let ports: Vec<u16> = std::iter::once(conn.target())
            .chain(conn.secondary_target())
            .map(|target| target.port())
            .collect();
        self.note_link_frame_on(frame, &ports);
    }

    /// Reads captured frames until the client is closing or the capture
    /// fails.
    fn capture_loop(&self, capture: &Capture) {
        let mut buf = vec![0u8; RECV_BUFFER];
        while !self.closing.load(Ordering::SeqCst) {
            match capture.recv(&mut buf) {
                Ok(Some(frame)) => self.note_link_frame(frame),
                Ok(None) => {}
                Err(_) => break,
            }
        }
    }

    /// Sends a frame, refusing one the addressee cannot decode.
    fn send(&self, to: &Addressee, opcode: Opcode, payload: &[u8]) -> Result<usize, SendError> {
        let both = self.broadcast_needed.load(Ordering::SeqCst);
        lock(&self.tx).send(to, self.uid, opcode, payload, both)
    }

    /// Re-judges, as of `now`, whether frames must reach some peer by
    /// broadcast too, and whether this client's own multicast fails, and
    /// returns whether the second changed: a unit announces a change to its
    /// features at once.
    fn judge_paths(&self, state: &State, now: Instant) -> bool {
        self.broadcast_needed
            .store(state.needs_broadcast(now), Ordering::SeqCst);
        let fault = state.multicast_fault(now);
        self.multicast_fault.swap(fault, Ordering::SeqCst) != fault
    }

    fn discover(&self) -> Result<(), SendError> {
        self.discover_at(Instant::now())
    }

    fn discover_at(&self, now: Instant) -> Result<(), SendError> {
        lock(&self.schedule).discovered(now);
        self.send(&Addressee::Broadcast, op::SYS_DISCOVER, &[])?;
        Ok(())
    }

    /// Asks one device for everything it holds, with a discover naming it.
    ///
    /// Skipped while a discover to every device is recent enough to have
    /// covered it. A receiver that predates the named form answers it as a
    /// discover to every device, so each one asked costs the whole mesh's
    /// state on such a network.
    fn request_state(&self, device: DeviceUid) {
        if lock(&self.schedule).discover_covers(Instant::now()) {
            return;
        }
        let _ = self.send(&Addressee::Broadcast, op::SYS_DISCOVER, device.as_bytes());
    }

    /// Announces this client, and re-arms the announcement timer only once the
    /// frame is away.
    ///
    /// The firmware resets its own hello timeout inside the branch where the
    /// transmit succeeded. A send that fails is then retried on the next tick
    /// rather than costing a whole interval of silence, which matters most at
    /// startup and after a network blip: exactly when being heard is worth the
    /// most.
    ///
    /// What it announces is [`Self::features`].
    ///
    /// The hello goes to both endpoints when it is the first since the
    /// sockets opened, or the last one on broadcast went [`HELLO_BROADCAST`]
    /// ago or more, as a unit's does; otherwise as any other frame.
    fn announce(&self) {
        let payload = build_hello(
            PROTOCOL_VERSION,
            &self.name,
            &self.serial,
            &self.version,
            self.features().bits(),
        );
        let now = Instant::now();
        let on_broadcast = hello_due_on_broadcast(*lock(&self.hello_on_broadcast), now)
            || self.broadcast_needed.load(Ordering::SeqCst);
        let sent = lock(&self.tx).send(
            &Addressee::Broadcast,
            self.uid,
            op::SYS_HELLO,
            &payload,
            on_broadcast,
        );
        if on_broadcast {
            *lock(&self.hello_on_broadcast) = Some(now);
        }
        match sent {
            Ok(n) if n > 0 => lock(&self.schedule).announced(now),
            _ => {}
        }
    }

    /// The features this client announces as of now.
    ///
    /// It announces `CONFIG_INITIALISED` because a unit uses the scaling
    /// fields of a 0x3C write only from a sender that does, and this client
    /// zeroes every field it does not set.
    ///
    /// While it holds an overlay sync it also announces itself as the mesh's
    /// overlay sync manager, which is what has the controller leave units'
    /// requests for a new sync to it.
    ///
    /// It announces the mesh controller role as a unit does: only while it is
    /// its own controller and the caller has bays mapped.
    fn features(&self) -> DeviceFeature {
        let mut features = DeviceFeature::CONFIG_INITIALISED;
        features |= if self.mesh_device {
            DeviceFeature::MESH
        } else {
            DeviceFeature::MANAGER
        };
        if lock(&self.osd).holds() {
            features |= DeviceFeature::OSD_SYNC;
        }
        if self.multicast_fault.load(Ordering::SeqCst) {
            features |= DeviceFeature::STATUS_MCAST_FAULT;
        }
        let controller = *lock(&self.mesh_controller);
        if !controller.is_zero() {
            features |= DeviceFeature::MESH_MEMBER;
        }
        if controller == self.uid && self.mesh_bays_mapped.load(Ordering::SeqCst) {
            features |= DeviceFeature::MESH_MASTER;
        }
        features
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
            // Only a client with both endpoints judges paths, as only it has
            // a second one to fall back on.
            let dual = conn.is_dual();
            match conn.recv(&mut buf) {
                Ok(Some((data, from, path))) => {
                    self.process_datagram_on(data, from, dual.then_some(path));
                }
                Ok(None) => {}
                Err(_) => break,
            }
        }
    }

    /// Reads datagrams from the second endpoint until the client is closing,
    /// idling while there is none.
    fn receive_secondary_loop(&self) {
        let mut buf = vec![0u8; RECV_BUFFER];
        while !self.closing.load(Ordering::SeqCst) {
            let Some(conn) = lock(&self.tx).conn() else {
                break;
            };
            if conn.paths().1.is_none() {
                std::thread::sleep(SHUTDOWN_POLL);
                continue;
            }
            match conn.recv_secondary(&mut buf) {
                Ok(Some((data, from, path))) => self.process_datagram_on(data, from, Some(path)),
                Ok(None) => {}
                Err(_) => break,
            }
        }
    }

    /// Runs [`Self::probe_once`] once a second until the client is closing.
    fn probe_loop(&self) {
        while self.sleep_until_next_tick() {
            self.probe_once(Instant::now());
        }
    }

    /// One pass: liveness, completion, the announcement timer and discovery.
    ///
    /// A discover to every device goes only while none is known. A device
    /// that is online and still owes part of its description past
    /// [`CONFIG_GRACE`] is asked by a discover naming it, as
    /// [`OWED_ASK_INTERVAL`] and [`OWED_ASK_LIMIT`] allow. An ask skipped
    /// because a discover to every device covers it is not counted.
    pub(super) fn probe_once(&self, now: Instant) {
        let covered = lock(&self.schedule).discover_covers(now);
        let (none_known, owed, fault_changed) = self.mutate(|state, ev| {
            let mut owed = Vec::new();
            for device in state.devices.values_mut() {
                device.check_online(now, ev);
                // Re-tested here rather than only where a frame lands: a device
                // stops waiting for its links on the clock alone, and no frame
                // arrives to announce that.
                device.check_config_complete(now, ev);
                if device.configuration_complete(now) {
                    device.owed_asks = 0;
                    device.last_owed_ask = None;
                } else if device.online
                    && !covered
                    && now.saturating_duration_since(device.first_seen) > CONFIG_GRACE
                    && device.owed_asks < OWED_ASK_LIMIT
                    && device.last_owed_ask.map_or(true, |last| {
                        now.saturating_duration_since(last) >= OWED_ASK_INTERVAL
                    })
                {
                    device.owed_asks += 1;
                    device.last_owed_ask = Some(now);
                    owed.push(device.uid);
                }
            }
            state.forget_silent(now, ev);
            let fault_changed = self.judge_paths(state, now);
            (state.devices.is_empty(), owed, fault_changed)
        });

        let discover_due = lock(&self.schedule).discover_due(now);
        if fault_changed || self.announce_due(now) {
            self.announce();
        }
        if none_known && discover_due {
            let _ = self.discover_at(now);
        }
        for device in owed {
            let _ = self.send(&Addressee::Broadcast, op::SYS_DISCOVER, device.as_bytes());
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
