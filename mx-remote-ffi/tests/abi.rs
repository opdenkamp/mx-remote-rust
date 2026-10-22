// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! What the boundary does, exercised through the boundary.
//!
//! The protocol is the core crate's to test and is tested there. What is only
//! testable here is the translation: that a caller's mistake becomes a code
//! rather than a crash, that a value survives the trip out and back, and that
//! a failure says why.
//!
//! No test opens a socket, and every client is given an identifier so that
//! none reaches the file where one would otherwise be stored.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use mx_remote::{V2ipDecoderDetail, V2ipDecoderFormat, V2ipDecoderReason, V2ipDecoderReport};
use mx_remote_ffi::*;

/// A distinguishable identifier, from one byte.
fn uid_n(n: u8) -> mxr_uid_t {
    let mut bytes = [0u8; 16];
    bytes[0] = n;
    bytes[15] = 0xa5;
    mxr_uid_t { bytes }
}

/// A client that has an identifier of its own and no socket.
fn client(name: &CStr, uid: &CStr) -> *mut mxr_remote_t {
    let config = mxr_config_t {
        target_ip: ptr::null(),
        port: 0,
        broadcast: false,
        local_ip: ptr::null(),
        interface: ptr::null(),
        name: name.as_ptr(),
        uid: uid.as_ptr(),
        uid_path: ptr::null(),
        logo_port: 0,
    };
    // SAFETY: every pointer in the config is a live NUL-terminated string, and
    // a null callback table asks for no callbacks.
    let handle = unsafe { mxr_remote_new(&config, ptr::null(), ptr::null_mut()) };
    assert!(
        !handle.is_null(),
        "the client was not created: {}",
        last_error()
    );
    handle
}

/// The message behind the last failure on this thread.
fn last_error() -> String {
    // SAFETY: mxr_last_error never returns null, and the string it points at
    // lives until the next failure on this thread.
    unsafe { CStr::from_ptr(mxr_last_error()) }
        .to_string_lossy()
        .into_owned()
}

/// Reads a fixed-width field the way C would.
fn field(bytes: &[c_char]) -> String {
    let text: Vec<u8> = bytes
        .iter()
        .take_while(|b| **b != 0)
        .map(|b| *b as u8)
        .collect();
    String::from_utf8_lossy(&text).into_owned()
}

#[test]
fn an_identifier_survives_the_trip_out_to_text_and_back() {
    let uid = uid_n(7);
    let mut text = [0 as c_char; MXR_UID_STRING_LEN];
    // SAFETY: the buffer is MXR_UID_STRING_LEN bytes, which is what is asked for.
    let rc = unsafe { mxr_uid_to_string(uid, text.as_mut_ptr(), text.len()) };
    assert_eq!(rc, mxr_result_t::MXR_OK);

    let mut back = mxr_uid_t::default();
    // SAFETY: text is NUL-terminated by the call above, and back is writable.
    let rc = unsafe { mxr_uid_from_string(text.as_ptr(), &mut back) };
    assert_eq!(rc, mxr_result_t::MXR_OK);
    assert_eq!(
        back,
        uid,
        "{} did not read back as it was written",
        field(&text)
    );
}

#[test]
fn a_buffer_too_short_for_an_identifier_is_refused_rather_than_filled() {
    let mut text = [0 as c_char; MXR_UID_STRING_LEN - 1];
    // SAFETY: the buffer is as long as the length passed with it.
    let rc = unsafe { mxr_uid_to_string(uid_n(1), text.as_mut_ptr(), text.len()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    assert_eq!(text[0], 0, "a refused call still wrote to the buffer");
}

#[test]
fn text_that_is_not_an_identifier_is_refused_rather_than_read_as_zero() {
    let text = CString::new("not a uid").expect("no NUL");
    let mut out = uid_n(3);
    // SAFETY: both pointers are live, and the text is NUL-terminated.
    let rc = unsafe { mxr_uid_from_string(text.as_ptr(), &mut out) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    assert_eq!(out, uid_n(3), "a refused parse still wrote an identifier");
    assert!(
        last_error().contains("not a uid"),
        "the failure did not say what could not be read: {}",
        last_error()
    );
}

#[test]
fn the_empty_identifier_is_the_one_the_protocol_spells_absence_with() {
    assert!(mxr_uid_is_zero(mxr_uid_t::default()));
    assert!(!mxr_uid_is_zero(uid_n(1)));
}

/// A null handle must reach a result code, never a dereference.
///
/// Every entry point takes one, and they divide into three shapes by what they
/// have to return when there is nothing to work with: a code, a count, or
/// nothing at all. One of each is enough to show the shape is handled; what
/// would not be caught is a single function that forgot, which is why the
/// check lives in one place they all call.
#[test]
fn a_null_handle_is_an_argument_error_rather_than_a_crash() {
    let mut info = std::mem::MaybeUninit::<mxr_device_info_t>::zeroed();
    // SAFETY: a null handle is what is under test; the output is writable.
    let rc = unsafe { mxr_device(ptr::null(), uid_n(1), info.as_mut_ptr()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    assert!(!last_error().is_empty(), "the failure said nothing");

    // SAFETY: a null handle, and a null buffer with a zero capacity to match.
    let count = unsafe { mxr_devices(ptr::null(), ptr::null_mut(), 0) };
    assert_eq!(count, 0);

    // SAFETY: a null handle is what is under test.
    let rc = unsafe { mxr_power_on(ptr::null(), mxr_bay_uid_t::default()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);

    // SAFETY: closing nothing must be a no-op, not a dereference.
    unsafe { mxr_remote_close(ptr::null()) };
    // SAFETY: freeing null is documented as ignored.
    unsafe { mxr_remote_free(ptr::null_mut()) };
}

#[test]
fn a_client_reports_the_identifier_and_name_it_was_given() {
    let name = CString::new("test client").expect("no NUL");
    let uid_text = CString::new("00000007.00000000.00000000.a5000000").expect("no NUL");
    let handle = client(&name, &uid_text);

    let mut uid = mxr_uid_t::default();
    // SAFETY: the handle is live and the output is writable.
    let rc = unsafe { mxr_remote_uid(handle, &mut uid) };
    assert_eq!(rc, mxr_result_t::MXR_OK);
    assert_eq!(uid, uid_n(7), "the configured identifier was not kept");

    let mut buf = [0 as c_char; 32];
    // SAFETY: the handle is live, and the buffer is as long as it is said to be.
    let rc = unsafe { mxr_remote_name(handle, buf.as_mut_ptr(), buf.len()) };
    assert_eq!(rc, mxr_result_t::MXR_OK);
    assert_eq!(field(&buf), "test client");

    // SAFETY: the handle came from mxr_remote_new and is freed once.
    unsafe { mxr_remote_free(handle) };
}

#[test]
fn a_client_that_was_never_started_has_no_target() {
    let name = CString::new("unstarted").expect("no NUL");
    let uid_text = CString::new("00000001.00000000.00000000.00000000").expect("no NUL");
    let handle = client(&name, &uid_text);

    let mut port = 0u16;
    // SAFETY: the handle is live; a null address buffer skips that output.
    let rc = unsafe { mxr_remote_target(handle, ptr::null_mut(), 0, &mut port) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_CONNECTED);

    // SAFETY: the handle came from mxr_remote_new and is freed once.
    unsafe { mxr_remote_free(handle) };
}

#[test]
fn a_command_for_a_device_never_heard_from_is_not_found() {
    let name = CString::new("no devices").expect("no NUL");
    let uid_text = CString::new("00000002.00000000.00000000.00000000").expect("no NUL");
    let handle = client(&name, &uid_text);

    // Not MXR_ERR_NOT_CONNECTED: the gate refuses this before the socket is
    // ever reached, so an unstarted client and a started one answer alike.
    // SAFETY: the handle is live.
    let rc = unsafe { mxr_reboot(handle, uid_n(9)) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);
    assert!(
        last_error().contains("no device"),
        "the failure did not say what was missing: {}",
        last_error()
    );

    // SAFETY: the handle came from mxr_remote_new and is freed once.
    unsafe { mxr_remote_free(handle) };
}

#[test]
fn a_configuration_that_cannot_be_read_yields_no_client() {
    let bad = CString::new("300.1.1.1").expect("no NUL");
    let config = mxr_config_t {
        target_ip: ptr::null(),
        port: 0,
        broadcast: false,
        local_ip: bad.as_ptr(),
        interface: ptr::null(),
        name: ptr::null(),
        uid: ptr::null(),
        uid_path: ptr::null(),
        logo_port: 0,
    };
    // SAFETY: every non-null pointer in the config is a live NUL-terminated
    // string, and a null callback table asks for no callbacks.
    let handle = unsafe { mxr_remote_new(&config, ptr::null(), ptr::null_mut()) };
    assert!(
        handle.is_null(),
        "a client was built from an unreadable address"
    );
    assert!(
        last_error().contains("local_ip"),
        "the failure did not name the field: {}",
        last_error()
    );
}

#[test]
fn a_list_call_reports_its_length_when_given_nowhere_to_write() {
    let name = CString::new("sizing").expect("no NUL");
    let uid_text = CString::new("00000003.00000000.00000000.00000000").expect("no NUL");
    let handle = client(&name, &uid_text);

    // SAFETY: the handle is live; a null buffer with a zero capacity asks only
    // for the count.
    let count = unsafe { mxr_devices(handle, ptr::null_mut(), 0) };
    assert_eq!(count, 0, "a client that has heard nothing listed a device");

    // SAFETY: the handle came from mxr_remote_new and is freed once.
    unsafe { mxr_remote_free(handle) };
}

#[test]
fn the_version_is_the_crate_version() {
    // SAFETY: mxr_version returns a static NUL-terminated string.
    let version = unsafe { CStr::from_ptr(mxr_version()) };
    assert_eq!(version.to_str().expect("ASCII"), env!("CARGO_PKG_VERSION"));
}

/// A subsystem read must say which of the two things went wrong.
///
/// "No such device" and "that device has not sent this" lead somewhere
/// different - the first is a wrong identifier, the second is a wait - and a
/// single code for both would leave a caller unable to tell a typo from a
/// device that has not got round to it. Only the first is reachable from here,
/// because reaching the second needs a device to have said something.
#[test]
fn a_subsystem_of_a_device_never_heard_from_is_not_found() {
    let name = CString::new("subsystems").expect("no NUL");
    let uid_text = CString::new("00000004.00000000.00000000.00000000").expect("no NUL");
    let handle = client(&name, &uid_text);

    let mut stats = std::mem::MaybeUninit::<mxr_v2ip_stats_t>::zeroed();
    // SAFETY: the handle is live and the output is writable.
    let rc = unsafe { mxr_v2ip_stats(handle, uid_n(4), stats.as_mut_ptr()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);
    assert!(
        last_error().contains("no device"),
        "the failure did not say the device was missing: {}",
        last_error()
    );

    // SAFETY: the handle is live; a null buffer with a zero capacity asks only
    // for the count.
    let count = unsafe { mxr_audio_endpoints(handle, uid_n(4), ptr::null_mut(), 0) };
    assert_eq!(count, 0);

    // SAFETY: the handle came from mxr_remote_new and is freed once.
    unsafe { mxr_remote_free(handle) };
}

#[test]
fn a_subsystem_read_without_somewhere_to_write_is_an_argument_error() {
    let name = CString::new("no output").expect("no NUL");
    let uid_text = CString::new("00000005.00000000.00000000.00000000").expect("no NUL");
    let handle = client(&name, &uid_text);

    // The null output is caught before the lookup, so this is an argument
    // error rather than the not-found the same call would otherwise give.
    // SAFETY: the handle is live; a null output is what is under test.
    let rc = unsafe { mxr_multiviewer_status(handle, uid_n(5), ptr::null_mut()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);

    // SAFETY: the handle came from mxr_remote_new and is freed once.
    unsafe { mxr_remote_free(handle) };
}

/// One named bit, as either crate spells it.
struct Bit {
    name: String,
    value: u64,
}

/// Reads `path` relative to the workspace root, failing rather than skipping.
///
/// A test that shrugs at a missing file reports every constant correct when it
/// found none, which is the one answer it must not be able to give.
fn workspace_source(path: &str) -> String {
    let full = format!("{}/../{path}", env!("CARGO_MANIFEST_DIR"));
    let text =
        std::fs::read_to_string(&full).unwrap_or_else(|e| panic!("{full} could not be read: {e}"));
    // The blocks are located by patterns written with "\n", so a checkout that
    // stores line endings the other way would report a missing block rather
    // than a constant that disagrees - a pass this test must not be able to
    // give for a reason that has nothing to do with the constants.
    text.replace("\r\n", "\n")
}

/// Evaluates the expressions these constants are written with: a decimal or
/// hexadecimal literal, or a shift of one. Anything else fails rather than
/// being skipped over.
fn value_of(expr: &str, whose: &str) -> u64 {
    let expr = expr.trim();
    if let Some(hex) = expr.strip_prefix("0x") {
        return u64::from_str_radix(hex, 16)
            .unwrap_or_else(|_| panic!("{whose}: {expr} is not a hexadecimal literal"));
    }
    if let Some((lhs, rhs)) = expr.split_once("<<") {
        let base: u64 = lhs
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("{whose}: {expr}"));
        let shift: u32 = rhs
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("{whose}: {expr}"));
        return base << shift;
    }
    expr.parse()
        .unwrap_or_else(|_| panic!("{whose}: {expr} is neither a literal nor a shift"))
}

/// The named constants of one `bitmask!` or `wire_enum!` block.
fn core_bits(source: &str, block: &str) -> Vec<Bit> {
    let start = source
        .find(&format!("\n    {block} {{"))
        .or_else(|| source.find(&format!("\n    {block}: ")))
        .unwrap_or_else(|| panic!("{block} is not in the core source under that name"));
    let body = &source[start..];
    let end = body
        .find("\n    }\n}")
        .unwrap_or_else(|| panic!("{block} has no end"));
    read_bits(&body[..end], block, |line| {
        let (name, expr) = line.strip_suffix(';')?.split_once(" = ")?;
        Some((name.to_owned(), expr.to_owned()))
    })
}

/// The named constants of an `impl` block that declares them one by one.
fn core_consts(source: &str, ty: &str) -> Vec<Bit> {
    read_bits(source, ty, |line| {
        let rest = line.strip_prefix("pub const ")?;
        let (name, rest) = rest.split_once(": Self = Self(")?;
        Some((name.to_owned(), rest.strip_suffix(");")?.to_owned()))
    })
}

/// The constants of `bits.rs` whose name starts with `prefix`, and not with
/// any of `not`.
fn header_bits(source: &str, prefix: &str, not: &[&str]) -> Vec<Bit> {
    read_bits(source, prefix, |line| {
        let rest = line.strip_prefix("pub const ")?;
        let (name, rest) = rest.split_once(": ")?;
        let expr = rest.split_once(" = ")?.1.strip_suffix(';')?;
        Some((name.to_owned(), expr.to_owned()))
    })
    .into_iter()
    .filter(|b| b.name.starts_with(prefix) && !not.iter().any(|n| b.name.starts_with(n)))
    .collect()
}

fn read_bits(
    source: &str,
    whose: &str,
    parse: impl Fn(&str) -> Option<(String, String)>,
) -> Vec<Bit> {
    source
        .lines()
        .map(str::trim)
        .filter_map(&parse)
        .map(|(name, expr)| Bit {
            value: value_of(&expr, whose),
            name,
        })
        .collect()
}

/// Every bit the core crate names reaches the header, at the same value.
///
/// The header's copies are written out as literals because cbindgen puts a
/// constant's expression into the header verbatim and cannot evaluate a call,
/// so the two lists are genuinely separate and drift is the risk. Both
/// directions are checked: a core bit with no name in the header would leave a
/// C caller writing the number itself, and a header bit with no core bit
/// behind it names something that no longer exists.
///
/// Each list is also held to a minimum length, so a rename that leaves a
/// pattern matching nothing fails here instead of reporting every constant
/// correct.
#[test]
fn every_core_bit_reaches_the_header_at_its_own_value() {
    let enums = workspace_source("mx-remote/src/wire/enums.rs");
    let audio = workspace_source("mx-remote/src/types/audio.rs");
    let header = workspace_source("mx-remote-ffi/src/bits.rs");

    let lists: [(&str, Vec<Bit>, &[&str], usize); 39] = [
        ("MXR_FEATURE_", core_bits(&enums, "DeviceFeature"), &[], 30),
        (
            "MXR_BAY_",
            core_bits(&enums, "BayFeatures"),
            &["MXR_BAY_STATUS_"],
            17,
        ),
        ("MXR_BAY_STATUS_", core_bits(&enums, "BayStatus"), &[], 19),
        ("MXR_KEY_", core_bits(&enums, "RcKey"), &[], 48),
        ("MXR_AUDIO_", core_consts(&audio, "AudioFeatures"), &[], 16),
        (
            "MXR_MV_VIEW_MODE_",
            core_bits(&enums, "MultiviewerViewMode"),
            &[],
            9,
        ),
        (
            "MXR_MV_PIP_POSITION_",
            core_bits(&enums, "MultiviewerPipPosition"),
            &[],
            5,
        ),
        (
            "MXR_MV_PIP_SIZE_",
            core_bits(&enums, "MultiviewerPipSize"),
            &[],
            4,
        ),
        (
            "MXR_MV_OUTPUT_",
            core_bits(&enums, "MultiviewerOutputMode"),
            &[],
            15,
        ),
        (
            "MXR_MV_HDCP_",
            core_bits(&enums, "MultiviewerHdcpMode"),
            &[],
            4,
        ),
        // The EDID names already carry their own word, so they take the bare
        // multiviewer prefix and every other multiviewer list is excluded
        // rather than the names being spelled MXR_MV_EDID_EDID_*.
        (
            "MXR_MV_",
            core_bits(&enums, "MultiviewerEdidTemplate"),
            &[
                "MXR_MV_VIEW_MODE_",
                "MXR_MV_PIP_",
                "MXR_MV_OUTPUT_",
                "MXR_MV_HDCP_",
                "MXR_MV_ITC_",
                "MXR_MV_ASPECT_",
                "MXR_MV_BOOL_",
                "MXR_MV_SOURCE_",
            ],
            20,
        ),
        (
            "MXR_MV_ITC_",
            core_bits(&enums, "MultiviewerItcMode"),
            &[],
            3,
        ),
        (
            "MXR_MV_ASPECT_",
            core_bits(&enums, "MultiviewerAspectRatio"),
            &[],
            3,
        ),
        ("MXR_MV_BOOL_", core_bits(&enums, "MultiviewerBool"), &[], 3),
        ("MXR_V2IP_VLAN_", core_bits(&enums, "V2ipVlanFlag"), &[], 5),
        (
            "MXR_V2IP_TEST_PATTERN_",
            core_bits(&enums, "V2ipTestPattern"),
            &[],
            7,
        ),
        (
            "MXR_V2IP_TONE_MODE_",
            core_bits(&enums, "V2ipToneMode"),
            &[],
            5,
        ),
        (
            "MXR_V2IP_TESTCARD_",
            core_bits(&enums, "V2ipTestcardFlag"),
            &[],
            6,
        ),
        (
            "MXR_V2IP_SDP_FLAG_",
            core_bits(&enums, "V2ipSdpFlag"),
            &[],
            3,
        ),
        (
            "MXR_V2IP_SDP_ENCODING_",
            core_bits(&enums, "V2ipSdpEncoding"),
            &[],
            4,
        ),
        (
            "MXR_V2IP_SDP_MEDIA_CLOCK_",
            core_bits(&enums, "V2ipSdpMediaClock"),
            &[],
            3,
        ),
        (
            "MXR_V2IP_SDP_REF_CLOCK_",
            core_bits(&enums, "V2ipSdpRefClock"),
            &[],
            4,
        ),
        (
            "MXR_V2IP_SDP_CLOCK_MODE_",
            core_bits(&enums, "V2ipSdpClockMode"),
            &[],
            3,
        ),
        (
            "MXR_V2IP_SDP_CLOCK_STATE_",
            core_bits(&enums, "V2ipSdpClockState"),
            &[],
            7,
        ),
        (
            "MXR_UNIT_",
            core_bits(&enums, "UnitFlag"),
            &["MXR_UNIT_POWER_SAVE_"],
            16,
        ),
        (
            "MXR_UNIT_POWER_SAVE_",
            core_bits(&enums, "UnitPowerSave"),
            &[],
            3,
        ),
        (
            "MXR_V2IP_POWER_SAVE_LEVEL_",
            core_bits(&enums, "V2ipPowerSaveLevel"),
            &[],
            4,
        ),
        (
            "MXR_V2IP_PTP_MODE_",
            core_bits(&enums, "V2ipPtpMode"),
            &[],
            3,
        ),
        (
            "MXR_V2IP_PTP_",
            core_bits(&enums, "V2ipPtpFlag"),
            &["MXR_V2IP_PTP_MODE_", "MXR_V2IP_PTP_LOCK_"],
            2,
        ),
        (
            "MXR_V2IP_PTP_LOCK_",
            core_bits(&enums, "V2ipPtpLock"),
            &[],
            3,
        ),
        (
            "MXR_V2IP_SETTING_",
            core_bits(&enums, "V2ipDeviceSetting"),
            &[],
            11,
        ),
        (
            "MXR_MV_SOURCE_",
            core_bits(&enums, "MultiviewerSource"),
            &[],
            5,
        ),
        (
            "MXR_OSD_SYNC_WHAT_",
            core_bits(&enums, "OsdSyncWhat"),
            &[],
            4,
        ),
        (
            "MXR_OSD_SYNC_STATE_",
            core_bits(&enums, "OsdSyncState"),
            &[],
            4,
        ),
        (
            "MXR_OSD_SYNC_SIZE_",
            core_bits(&enums, "OsdSyncSize"),
            &[],
            2,
        ),
        (
            "MXR_OSD_CAPTION_ALIGN_",
            core_bits(&enums, "OsdCaptionAlign"),
            &[],
            4,
        ),
        (
            "MXR_OSD_CAPTION_FORMAT_",
            core_bits(&enums, "OsdCaptionFormat"),
            &[],
            3,
        ),
        (
            "MXR_OSD_CAPTION_FIELD_",
            core_bits(&enums, "OsdCaptionField"),
            &[],
            9,
        ),
        (
            "MXR_OSD_LOGO_FIELD_",
            core_bits(&enums, "OsdLogoField"),
            &[],
            3,
        ),
    ];

    for (prefix, core, not, minimum) in lists {
        assert!(
            core.len() >= minimum,
            "{prefix}: found {} constants in the core crate, expected at least {minimum}; \
             the pattern that reads them has stopped matching",
            core.len()
        );
        let mine = header_bits(&header, prefix, not);
        assert_eq!(
            mine.len(),
            core.len(),
            "{prefix}: the header names {} of the core crate's {}",
            mine.len(),
            core.len()
        );
        for bit in &core {
            let name = format!("{prefix}{}", bit.name);
            let found = mine
                .iter()
                .find(|b| b.name == name)
                .unwrap_or_else(|| panic!("{name} is in the core crate and not in the header"));
            assert_eq!(found.value, bit.value, "{name} differs between the two");
        }
    }
}

/// The unset signal type is not a format, whichever way a sender says so.
#[test]
fn the_signal_type_accessors_read_a_packed_word() {
    // svd 16, colour space 1, bit-depth index 2.
    let word = 16 | (1 << 8) | (2 << 13);
    assert_eq!(mxr_signal_type_svd(word), 16);
    assert_eq!(mxr_signal_type_colour_space(word), 1);
    assert_eq!(mxr_signal_type_bpp_index(word), 2);
    // The wire carries an index into a table of depths, not a depth.
    assert_eq!(mxr_signal_type_bpp(word), 10);
    assert!(mxr_signal_type_is_set(word));

    // A bay with nothing configured, said both ways.
    assert!(!mxr_signal_type_is_set(5 << 13));
    assert!(!mxr_signal_type_is_set(0));
    assert_eq!(mxr_signal_type_bpp(5 << 13), 0);

    // Bits 16-19 and 22-23 of a status word are fields, not flags.
    let status = MXR_BAY_STATUS_SIGNAL_DETECTED | (3 << 16) | (2 << 22);
    assert_eq!(mxr_bay_status_rc_type(status), 3);
    assert_eq!(mxr_bay_status_hdcp(status), 2);
}

/// Every entry point added for a C consumer rejects a null handle rather than
/// dereferencing one, and says why.
#[test]
fn the_new_entry_points_refuse_a_null_handle() {
    let route = mxr_v2ip_route_t {
        video: mxr_stream_addr_t {
            ip: ptr::null(),
            port: 0,
        },
        audio: mxr_stream_addr_t {
            ip: ptr::null(),
            port: 0,
        },
        anc: mxr_stream_addr_t {
            ip: ptr::null(),
            port: 0,
        },
    };
    let mut edid = [0u8; MXR_EDID_LEN];
    let mut audio = std::mem::MaybeUninit::<mxr_audio_details_t>::zeroed();
    let mut frames = 0u64;

    // SAFETY: a null handle is what is under test; every other argument is
    // valid, so an argument error can only be the handle.
    let calls = unsafe {
        [
            mxr_select_source_addr(ptr::null(), mxr_bay_uid_t::default(), &route, ptr::null()),
            mxr_send_key(ptr::null(), mxr_bay_uid_t::default(), MXR_KEY_PLAY),
            mxr_request_edid(ptr::null(), uid_n(1), true),
            mxr_request_signal_status(ptr::null(), uid_n(1)),
            mxr_bay_audio_details(ptr::null(), mxr_bay_uid_t::default(), audio.as_mut_ptr()),
            mxr_device_edid(ptr::null(), uid_n(1), true, edid.as_mut_ptr(), edid.len()),
            mxr_frames_received(ptr::null(), &mut frames),
        ]
    };
    for (n, rc) in calls.into_iter().enumerate() {
        assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT, "call {n}");
    }
    assert!(!last_error().is_empty(), "the failure said nothing");
}

/// The device settings calls answer a caller's mistake as an argument error
/// and a device never heard from as not found, before anything is sent.
#[test]
fn the_device_settings_calls_check_their_arguments_first() {
    let remote = client(c"abi-settings", c"00000022.00000000.00000000.000000a5");

    // SAFETY: a live handle; a null output is what is under test.
    let rc = unsafe { mxr_v2ip_device_settings(remote, uid_n(9), ptr::null_mut()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);

    let mut out = std::mem::MaybeUninit::<mxr_v2ip_device_settings_t>::zeroed();
    // SAFETY: a live handle and a writable struct.
    let rc = unsafe { mxr_v2ip_device_settings(remote, uid_n(9), out.as_mut_ptr()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);

    // A bit that is not an on/off setting, and profiles out of range, are the
    // caller's to fix whatever the device.
    // SAFETY: a live handle for each call.
    let calls = unsafe {
        [
            mxr_set_v2ip_device_setting(remote, uid_n(9), MXR_V2IP_SETTING_IR_PROFILE, true),
            mxr_set_v2ip_ir_profile(remote, uid_n(9), MXR_V2IP_IR_PROFILE_NOT_SET),
            mxr_set_v2ip_sink_ir_profile(remote, uid_n(9), MXR_V2IP_IR_PROFILE_MAX),
        ]
    };
    for (n, rc) in calls.into_iter().enumerate() {
        assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT, "call {n}");
    }
    // SAFETY: a live handle.
    let rc =
        unsafe { mxr_set_v2ip_device_setting(remote, uid_n(9), MXR_V2IP_SETTING_STATUS_LED, true) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);

    // SAFETY: a live handle; a null output is what is under test.
    let rc = unsafe { mxr_v2ip_power_save(remote, uid_n(9), ptr::null_mut()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    // SAFETY: a live handle; a null schedule is what is under test.
    let rc = unsafe { mxr_set_v2ip_power_save_schedule(remote, uid_n(9), ptr::null()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    let mut schedule = mxr_v2ip_power_save_t {
        auto_minutes: 0,
        start: [0; 7],
        end: [0; 7],
    };
    schedule.end[6] = MXR_V2IP_MINUTES_PER_DAY;
    // SAFETY: a live handle and an initialised schedule.
    let rc = unsafe { mxr_set_v2ip_power_save_schedule(remote, uid_n(9), &schedule) };
    assert_eq!(
        rc,
        mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
        "a time past the day"
    );
    schedule.end[6] = 0;
    // SAFETY: a live handle and an initialised schedule.
    let rc = unsafe { mxr_set_v2ip_power_save_schedule(remote, uid_n(9), &schedule) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);
    // SAFETY: a live handle.
    let rc = unsafe { mxr_set_v2ip_auto_power_save(remote, uid_n(9), 30) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);

    let mut all = mxr_v2ip_device_settings_t {
        valid: MXR_V2IP_SETTING_AUTO_POWER_SAVE,
        flags: 0,
        ir_profiles: 0,
        ir_profile: 0,
        ir_profile_sink: 0,
    };
    // SAFETY: a live handle and an initialised settings struct; a null power
    // save is what is under test.
    let rc = unsafe { mxr_set_all_v2ip_device_settings(remote, &all, ptr::null()) };
    assert_eq!(
        rc,
        mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
        "minutes with no values"
    );
    all.valid = MXR_V2IP_SETTING_CLOCK_SET;
    // SAFETY: a live handle and initialised structs.
    let rc = unsafe { mxr_set_all_v2ip_device_settings(remote, &all, &schedule) };
    assert_eq!(
        rc,
        mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
        "a reported-only bit"
    );
    all.valid = MXR_V2IP_SETTING_STATUS_LED;
    // SAFETY: a live handle and an initialised settings struct.
    let rc = unsafe { mxr_set_all_v2ip_device_settings(remote, &all, ptr::null()) };
    assert_eq!(
        rc,
        mxr_result_t::MXR_ERR_NOT_CONNECTED,
        "a switch needs no power save"
    );

    // SAFETY: created above and not yet freed.
    unsafe { mxr_remote_free(remote) };
}

/// The unit calls answer a caller's mistake as an argument error and a device
/// never heard from as not found, before anything is sent.
#[test]
fn the_unit_calls_check_their_arguments_first() {
    let remote = client(c"abi-unit", c"00000022.00000000.00000000.000000a9");

    // SAFETY: a live handle; a null output is what is under test.
    let rc = unsafe { mxr_unit_status(remote, uid_n(9), ptr::null_mut()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    let mut out = std::mem::MaybeUninit::<mxr_unit_status_t>::zeroed();
    // SAFETY: a live handle and a writable struct.
    let rc = unsafe { mxr_unit_status(remote, uid_n(9), out.as_mut_ptr()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);

    // SAFETY: a live handle.
    let calls = unsafe {
        [
            mxr_clear_unit_status(remote, uid_n(9)),
            mxr_upgrade_unit_fpga(remote, uid_n(9), true),
            mxr_set_unit_dhcp(remote, uid_n(9)),
            mxr_confirm_unit_network(remote, uid_n(9)),
        ]
    };
    assert_eq!(calls, [mxr_result_t::MXR_ERR_NOT_FOUND; 4]);

    let net = |ip: &CStr, mask: &CStr, gw: *const c_char| {
        // SAFETY: a live handle and NUL-terminated strings or null.
        unsafe {
            mxr_set_unit_network(
                remote,
                uid_n(9),
                ip.as_ptr(),
                mask.as_ptr(),
                gw,
                ptr::null(),
                ptr::null(),
            )
        }
    };
    assert_eq!(
        net(c"10.1.2", c"255.255.255.0", ptr::null()),
        mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
        "an address that does not parse"
    );
    assert_eq!(
        net(c"10.1.2.3", c"255.0.255.0", ptr::null()),
        mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
        "a netmask with a hole"
    );
    assert_eq!(
        net(c"10.1.2.3", c"255.255.255.0", c"gateway".as_ptr()),
        mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
        "a gateway that does not parse"
    );
    assert_eq!(
        net(c"10.1.2.3", c"255.255.255.0", ptr::null()),
        mxr_result_t::MXR_ERR_NOT_FOUND,
        "a valid network, for a device never heard from"
    );

    // SAFETY: created above and not yet freed.
    unsafe { mxr_remote_free(remote) };
}

/// The power save level calls answer a caller's mistake as an argument error
/// and a device never heard from as not found, before anything is sent, and
/// the mesh-wide settings call leaves the level to its own.
#[test]
fn the_power_save_level_calls_check_their_arguments_first() {
    let remote = client(c"abi-psl", c"00000022.00000000.00000000.000000aa");

    let mut level = 0u8;
    // SAFETY: a live handle; a null output is what is under test.
    let rc = unsafe { mxr_v2ip_power_save_level(remote, uid_n(9), ptr::null_mut()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    // SAFETY: a live handle and a writable byte.
    let rc = unsafe { mxr_v2ip_power_save_level(remote, uid_n(9), &mut level) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);
    let mut offers = std::mem::MaybeUninit::<mxr_unit_power_save_t>::zeroed();
    // SAFETY: a live handle and a writable struct.
    let rc = unsafe { mxr_unit_power_save(remote, uid_n(9), offers.as_mut_ptr()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);

    // SAFETY: a live handle.
    let rc = unsafe { mxr_set_v2ip_power_save_level(remote, uid_n(9), 4) };
    assert_eq!(
        rc,
        mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
        "a level with no name"
    );
    // SAFETY: a live handle.
    let rc = unsafe { mxr_set_all_v2ip_power_save_level(remote, 4) };
    assert_eq!(
        rc,
        mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
        "a level with no name"
    );
    // SAFETY: a live handle.
    let rc = unsafe { mxr_set_all_v2ip_power_save_level(remote, MXR_V2IP_POWER_SAVE_LEVEL_LOW) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_CONNECTED);

    let all = mxr_v2ip_device_settings_t {
        valid: MXR_V2IP_SETTING_POWER_SAVE_LEVEL,
        flags: 0,
        ir_profiles: 0,
        ir_profile: 0,
        ir_profile_sink: 0,
    };
    // SAFETY: a live handle and an initialised settings struct.
    let rc = unsafe { mxr_set_all_v2ip_device_settings(remote, &all, ptr::null()) };
    assert_eq!(
        rc,
        mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
        "a level with nowhere to carry it"
    );

    // SAFETY: created above and not yet freed.
    unsafe { mxr_remote_free(remote) };
}

/// The PTP message calls answer a caller's mistake as an argument error and a
/// device never heard from as not found, before anything is sent, and the
/// mesh-wide settings call leaves them to their own.
#[test]
fn the_ptp_message_calls_check_their_arguments_first() {
    let remote = client(c"abi-ptpm", c"00000022.00000000.00000000.000000ab");

    // SAFETY: a live handle; a null output is what is under test.
    let rc = unsafe { mxr_v2ip_ptp_messages(remote, uid_n(9), ptr::null_mut()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    let mut out = std::mem::MaybeUninit::<mxr_v2ip_ptp_messages_t>::zeroed();
    // SAFETY: a live handle and a writable struct.
    let rc = unsafe { mxr_v2ip_ptp_messages(remote, uid_n(9), out.as_mut_ptr()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);

    let messages = mxr_v2ip_ptp_messages_t {
        log_announce_interval: 1,
        log_sync_interval: -3,
        log_delay_req_interval: 0,
        announce_timeout: 3,
        dscp: 46,
        sap_expire_min: 60,
        sap_interval_s: 30,
        stream_ttl: 0,
        sink_rotation: 360,
        sink_rotation_step: 0,
    };
    let dscp = MXR_V2IP_SETTING_PTP_DSCP;
    // SAFETY: a live handle; a null struct is what is under test.
    let rc = unsafe { mxr_set_v2ip_ptp_messages(remote, uid_n(9), dscp, ptr::null()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT, "no values");
    // SAFETY: a live handle and an initialised struct.
    let rc = unsafe {
        mxr_set_v2ip_ptp_messages(
            remote,
            uid_n(9),
            dscp | MXR_V2IP_SETTING_STATUS_LED,
            &messages,
        )
    };
    assert_eq!(
        rc,
        mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
        "a bit that is not one"
    );
    // SAFETY: a live handle and an initialised struct.
    let rc = unsafe { mxr_set_v2ip_ptp_messages(remote, uid_n(9), dscp, &messages) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);
    // SAFETY: a live handle and an initialised struct.
    let rc = unsafe { mxr_set_all_v2ip_ptp_messages(remote, 0, &messages) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT, "no bit");
    // SAFETY: a live handle and an initialised struct.
    let rc = unsafe { mxr_set_all_v2ip_ptp_messages(remote, dscp, &messages) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_CONNECTED);

    let all = mxr_v2ip_device_settings_t {
        valid: MXR_V2IP_SETTING_PTP_SYNC,
        flags: 0,
        ir_profiles: 0,
        ir_profile: 0,
        ir_profile_sink: 0,
    };
    // SAFETY: a live handle and an initialised settings struct.
    let rc = unsafe { mxr_set_all_v2ip_device_settings(remote, &all, ptr::null()) };
    assert_eq!(
        rc,
        mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
        "a PTP message bit with no values"
    );

    // The SAP timings travel the same way, and the SAP switches do not.
    let sap = MXR_V2IP_SETTING_SAP_INTERVAL;
    // SAFETY: a live handle and an initialised struct.
    let rc = unsafe { mxr_set_all_v2ip_ptp_messages(remote, sap, &messages) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_CONNECTED);
    let rc = unsafe {
        // SAFETY: a live handle and an initialised struct.
        mxr_set_all_v2ip_ptp_messages(remote, MXR_V2IP_SETTING_SAP_LISTEN, &messages)
    };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT, "a switch");

    // So does the stream TTL, which is never 0.
    let ttl = MXR_V2IP_SETTING_STREAM_TTL;
    // SAFETY: a live handle and an initialised struct.
    let rc = unsafe { mxr_set_all_v2ip_ptp_messages(remote, ttl, &messages) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT, "a TTL of 0");
    let one = mxr_v2ip_ptp_messages_t {
        stream_ttl: 1,
        ..messages
    };
    // SAFETY: a live handle and an initialised struct.
    let rc = unsafe { mxr_set_all_v2ip_ptp_messages(remote, ttl, &one) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_CONNECTED);

    // So does the output's rotation, which turns less than a full circle.
    let rotation = MXR_V2IP_SETTING_SINK_ROTATION;
    // SAFETY: a live handle and an initialised struct.
    let rc = unsafe { mxr_set_all_v2ip_ptp_messages(remote, rotation, &messages) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT, "a full circle");
    let most = mxr_v2ip_ptp_messages_t {
        sink_rotation: MXR_V2IP_SINK_ROTATION_MAX,
        ..messages
    };
    // SAFETY: a live handle and an initialised struct.
    let rc = unsafe { mxr_set_all_v2ip_ptp_messages(remote, rotation, &most) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_CONNECTED);
    let all = mxr_v2ip_device_settings_t {
        valid: sap,
        flags: 0,
        ir_profiles: 0,
        ir_profile: 0,
        ir_profile_sink: 0,
    };
    // SAFETY: a live handle and an initialised settings struct.
    let rc = unsafe { mxr_set_all_v2ip_device_settings(remote, &all, ptr::null()) };
    assert_eq!(
        rc,
        mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
        "a SAP timing with no values"
    );

    // SAFETY: created above and not yet freed.
    unsafe { mxr_remote_free(remote) };
}

/// The stream encryption calls answer a caller's mistake as an argument error
/// and a device never heard from as not found, before anything is sent.
#[test]
fn the_privacy_calls_check_their_arguments_first() {
    let remote = client(c"abi-privacy", c"00000022.00000000.00000000.000000ad");

    // SAFETY: a live handle; a null output is what is under test.
    let rc = unsafe { mxr_v2ip_privacy(remote, uid_n(9), ptr::null_mut()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    let mut out = std::mem::MaybeUninit::<mxr_v2ip_privacy_t>::zeroed();
    // SAFETY: a live handle and a writable struct.
    let rc = unsafe { mxr_v2ip_privacy(remote, uid_n(9), out.as_mut_ptr()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);
    // SAFETY: a live handle.
    let rc = unsafe { mxr_request_v2ip_privacy(remote, uid_n(9)) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);

    // SAFETY: created above and not yet freed.
    unsafe { mxr_remote_free(remote) };
}

/// The output message calls answer a caller's mistake as an argument error
/// and a device never heard from as not found, before anything is sent.
#[test]
fn the_message_calls_check_their_arguments_first() {
    let remote = client(c"abi-message", c"00000022.00000000.00000000.000000ae");

    // SAFETY: a live handle; a null output is what is under test.
    let rc = unsafe { mxr_osd_message(remote, uid_n(9), ptr::null_mut()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    let mut out = std::mem::MaybeUninit::<mxr_osd_message_t>::zeroed();
    // SAFETY: a live handle and a writable struct.
    let rc = unsafe { mxr_osd_message(remote, uid_n(9), out.as_mut_ptr()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);

    let normal = MXR_OSD_MESSAGE_PRIORITY_NORMAL;
    // SAFETY: a live handle; a null text is what is under test.
    let rc = unsafe { mxr_show_osd_message(remote, uid_n(9), 0, normal, ptr::null()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT, "no text");
    let long = CString::new("x".repeat(MXR_OSD_MESSAGE_TEXT_MAX + 1)).expect("no NUL");
    // SAFETY: a live handle and NUL-terminated strings.
    let refused = unsafe {
        [
            mxr_show_osd_message(remote, uid_n(9), 6, normal, c"".as_ptr()),
            mxr_show_osd_message(remote, uid_n(9), 0, 3, c"".as_ptr()),
            mxr_show_osd_message(remote, uid_n(9), 0, normal, long.as_ptr()),
            mxr_show_osd_message(remote, uid_n(9), 0, normal, c"a\tb".as_ptr()),
        ]
    };
    assert_eq!(refused, [mxr_result_t::MXR_ERR_INVALID_ARGUMENT; 4]);
    // SAFETY: a live handle and NUL-terminated strings.
    let unknown = unsafe {
        [
            mxr_show_osd_message(remote, uid_n(9), -1, normal, c"hi".as_ptr()),
            mxr_clear_osd_message(remote, uid_n(9)),
            mxr_request_osd_message(remote, uid_n(9)),
        ]
    };
    assert_eq!(unknown, [mxr_result_t::MXR_ERR_NOT_FOUND; 3]);

    // SAFETY: created above and not yet freed.
    unsafe { mxr_remote_free(remote) };
}

/// The power save switch answers a device never heard from as not found, and
/// goes out to the whole mesh as soon as there is a socket to send it on.
#[test]
fn the_power_save_switch_checks_the_device_first() {
    let remote = client(c"abi-power-save", c"00000022.00000000.00000000.000000b0");

    // SAFETY: a live handle.
    let rc = unsafe { mxr_set_v2ip_power_save(remote, uid_n(9), true) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);
    // SAFETY: a live handle.
    let rc = unsafe { mxr_set_all_v2ip_power_save(remote, false) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_CONNECTED);

    // SAFETY: created above and not yet freed.
    unsafe { mxr_remote_free(remote) };
}

/// The downmix calls answer a caller's mistake as an argument error and a
/// device never heard from as not found, before anything is sent.
#[test]
fn the_downmix_calls_check_their_arguments_first() {
    let remote = client(c"abi-downmix", c"00000022.00000000.00000000.000000af");

    // SAFETY: a live handle; a null output is what is under test.
    let rc = unsafe { mxr_v2ip_downmix(remote, uid_n(9), ptr::null_mut()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    let mut out = std::mem::MaybeUninit::<mxr_v2ip_downmix_t>::zeroed();
    // SAFETY: a live handle and a writable struct.
    let rc = unsafe { mxr_v2ip_downmix(remote, uid_n(9), out.as_mut_ptr()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);

    // SAFETY: a live handle; a null downmix is what is under test.
    let rc = unsafe { mxr_set_v2ip_downmix(remote, uid_n(9), ptr::null()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT, "no downmix");
    let most = mxr_v2ip_downmix_t {
        mode: MXR_V2IP_DOWNMIX_ALWAYS,
        level: MXR_V2IP_DOWNMIX_LEVEL_UNSCALED,
        lfe: MXR_V2IP_DOWNMIX_LFE_MAX,
        state: 0xff,
        channels: 0xff,
    };
    let refused = [
        mxr_v2ip_downmix_t {
            mode: MXR_V2IP_DOWNMIX_ALWAYS + 1,
            ..most
        },
        mxr_v2ip_downmix_t {
            level: MXR_V2IP_DOWNMIX_LEVEL_UNSCALED + 1,
            ..most
        },
        mxr_v2ip_downmix_t {
            lfe: MXR_V2IP_DOWNMIX_LFE_MAX + 1,
            ..most
        },
    ]
    // SAFETY: a live handle and initialised structs.
    .map(|d| unsafe { mxr_set_v2ip_downmix(remote, uid_n(9), &d) });
    assert_eq!(refused, [mxr_result_t::MXR_ERR_INVALID_ARGUMENT; 3]);
    // SAFETY: a live handle and an initialised struct.
    let rc = unsafe { mxr_set_v2ip_downmix(remote, uid_n(9), &most) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND, "the most of each");

    // SAFETY: created above and not yet freed.
    unsafe { mxr_remote_free(remote) };
}

/// The session description calls answer a caller's mistake as an argument
/// error and a device never heard from as not found, before anything is sent.
#[test]
fn the_sdp_calls_check_their_arguments_first() {
    let remote = client(c"abi-sdp", c"00000022.00000000.00000000.000000ac");

    // SAFETY: a live handle; a null output is what is under test.
    let rc = unsafe { mxr_v2ip_sink_sdp(remote, uid_n(9), ptr::null_mut()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    let mut out = std::mem::MaybeUninit::<mxr_v2ip_sink_sdp_t>::zeroed();
    // SAFETY: a live handle and a writable struct.
    let rc = unsafe { mxr_v2ip_sink_sdp(remote, uid_n(9), out.as_mut_ptr()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);

    // SAFETY: a live handle; a null description is what is under test.
    let rc = unsafe { mxr_play_v2ip_sdp(remote, uid_n(9), ptr::null()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT, "no description");
    // SAFETY: a live handle and a NUL-terminated string.
    let rc = unsafe { mxr_play_v2ip_sdp(remote, uid_n(9), c"".as_ptr()) };
    assert_eq!(
        rc,
        mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
        "an empty description"
    );
    // SAFETY: a live handle and a NUL-terminated string.
    let calls = unsafe {
        [
            mxr_play_v2ip_sdp(remote, uid_n(9), c"v=0".as_ptr()),
            mxr_stop_v2ip_sdp(remote, uid_n(9)),
            mxr_set_v2ip_sdp_screen(remote, uid_n(9), 1),
            mxr_request_v2ip_sink_sdp(remote, uid_n(9)),
        ]
    };
    assert_eq!(calls, [mxr_result_t::MXR_ERR_NOT_FOUND; 4]);

    // SAFETY: created above and not yet freed.
    unsafe { mxr_remote_free(remote) };
}

/// The PTP calls answer a caller's mistake as an argument error and a device
/// never heard from as not found, before anything is sent, and the mesh-wide
/// settings call leaves PTP to its own.
#[test]
fn the_ptp_calls_check_their_arguments_first() {
    let remote = client(c"abi-ptp", c"00000022.00000000.00000000.000000a8");

    // SAFETY: a live handle; a null output is what is under test.
    let rc = unsafe { mxr_v2ip_ptp_settings(remote, uid_n(9), ptr::null_mut()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    let mut out = std::mem::MaybeUninit::<mxr_v2ip_ptp_settings_t>::zeroed();
    // SAFETY: a live handle and a writable struct.
    let rc = unsafe { mxr_v2ip_ptp_settings(remote, uid_n(9), out.as_mut_ptr()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);

    let mut state = std::mem::MaybeUninit::<mxr_v2ip_ptp_t>::zeroed();
    // SAFETY: a live handle and a writable struct.
    let rc = unsafe { mxr_v2ip_ptp(remote, uid_n(9), state.as_mut_ptr()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);
    // SAFETY: a live handle; a null output is what is under test.
    let rc = unsafe { mxr_v2ip_ptp(remote, uid_n(9), ptr::null_mut()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);

    // SAFETY: a live handle.
    let rc = unsafe { mxr_set_v2ip_ptp_mode(remote, uid_n(9), 3) };
    assert_eq!(
        rc,
        mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
        "an unnamed mode"
    );
    // SAFETY: a live handle.
    let rc = unsafe { mxr_set_v2ip_ptp_domain(remote, uid_n(9), MXR_V2IP_PTP_DOMAIN_MAX + 1) };
    assert_eq!(
        rc,
        mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
        "a domain past the last"
    );
    // SAFETY: a live handle.
    let rc = unsafe { mxr_set_v2ip_ptp_priority1(remote, uid_n(9), 128) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);

    let ptp = mxr_v2ip_ptp_settings_t {
        mode: MXR_V2IP_PTP_MODE_AUTO,
        domain: 0,
        priority1: 128,
    };
    // SAFETY: a live handle; a null struct is what is under test.
    let rc =
        unsafe { mxr_set_all_v2ip_ptp_settings(remote, MXR_V2IP_SETTING_PTP_MODE, ptr::null()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT, "no values");
    // SAFETY: a live handle and an initialised struct.
    let rc = unsafe {
        mxr_set_all_v2ip_ptp_settings(
            remote,
            MXR_V2IP_SETTING_PTP_MODE | MXR_V2IP_SETTING_STATUS_LED,
            &ptp,
        )
    };
    assert_eq!(
        rc,
        mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
        "a bit that is not PTP"
    );
    // SAFETY: a live handle and an initialised struct.
    let rc = unsafe { mxr_set_all_v2ip_ptp_settings(remote, MXR_V2IP_SETTING_PTP_MODE, &ptp) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_CONNECTED, "a PTP bit alone");

    let all = mxr_v2ip_device_settings_t {
        valid: MXR_V2IP_SETTING_PTP_DOMAIN,
        flags: 0,
        ir_profiles: 0,
        ir_profile: 0,
        ir_profile_sink: 0,
    };
    // SAFETY: a live handle and an initialised settings struct.
    let rc = unsafe { mxr_set_all_v2ip_device_settings(remote, &all, ptr::null()) };
    assert_eq!(
        rc,
        mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
        "a PTP bit with no PTP values"
    );

    // SAFETY: created above and not yet freed.
    unsafe { mxr_remote_free(remote) };
}

/// The VLAN calls answer a caller's mistake as an argument error and a device
/// never heard from as not found, before anything is sent.
#[test]
fn the_vlan_calls_check_their_arguments_first() {
    let remote = client(c"abi-vlan", c"00000022.00000000.00000000.000000a6");

    // SAFETY: a live handle; a null output is what is under test.
    let rc = unsafe { mxr_v2ip_vlan(remote, uid_n(9), ptr::null_mut()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);

    let mut out = std::mem::MaybeUninit::<mxr_v2ip_vlan_t>::zeroed();
    // SAFETY: a live handle and a writable struct.
    let rc = unsafe { mxr_v2ip_vlan(remote, uid_n(9), out.as_mut_ptr()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);

    let vlan = |device, uplink| mxr_v2ip_vlan_t {
        flags: MXR_V2IP_VLAN_TRUNK,
        device,
        port: [0; MXR_V2IP_VLAN_PORTS],
        uplink,
        active_uplink: 0,
        revert_s: 0,
    };
    let too_high = vlan(MXR_V2IP_VLAN_ID_MAX + 1, 0);
    let no_port = vlan(1, MXR_V2IP_VLAN_PORTS as u8 + 1);
    let fine = vlan(MXR_V2IP_VLAN_ID_MAX, MXR_V2IP_VLAN_PORTS as u8);
    // SAFETY: a live handle, and each struct null or initialised.
    let calls = unsafe {
        [
            mxr_set_v2ip_vlan(remote, uid_n(9), ptr::null()),
            mxr_set_v2ip_vlan(remote, uid_n(9), &too_high),
            mxr_set_v2ip_vlan(remote, uid_n(9), &no_port),
        ]
    };
    for (n, rc) in calls.into_iter().enumerate() {
        assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT, "call {n}");
    }
    // SAFETY: a live handle and an initialised struct.
    let rc = unsafe { mxr_set_v2ip_vlan(remote, uid_n(9), &fine) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);

    // SAFETY: created above and not yet freed.
    unsafe { mxr_remote_free(remote) };
}

/// The test card calls answer a caller's mistake as an argument error and a
/// device never heard from as not found, before anything is sent.
#[test]
fn the_testcard_calls_check_their_arguments_first() {
    let remote = client(c"abi-testcard", c"00000022.00000000.00000000.000000a7");

    // SAFETY: a live handle; a null output is what is under test.
    let rc = unsafe { mxr_v2ip_testcard(remote, uid_n(9), ptr::null_mut()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    let mut out = std::mem::MaybeUninit::<mxr_v2ip_testcard_t>::zeroed();
    // SAFETY: a live handle and a writable struct.
    let rc = unsafe { mxr_v2ip_testcard(remote, uid_n(9), out.as_mut_ptr()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);

    let loud = mxr_v2ip_test_tone_t {
        mode: MXR_V2IP_TONE_MODE_CONTINUOUS,
        level: 1,
        channels: 2,
        freq: 1000,
        rate: 48000,
    };
    let no_lead = mxr_v2ip_test_sync_t {
        period: 0,
        lead: 1,
        offset: 0,
        beep_ms: MXR_V2IP_SYNC_BEEP_MS_MIN,
    };
    // SAFETY: a live handle, and each struct null or initialised.
    let calls = unsafe {
        [
            mxr_set_v2ip_test_pattern(remote, uid_n(9), MXR_V2IP_TEST_PATTERN_CARD + 1, 0),
            mxr_set_v2ip_test_pattern(remote, uid_n(9), MXR_V2IP_TEST_PATTERN_FLAT, 0x0100_0000),
            mxr_set_v2ip_test_tone(remote, uid_n(9), ptr::null()),
            mxr_set_v2ip_test_tone(remote, uid_n(9), &loud),
            mxr_set_v2ip_test_sync(remote, uid_n(9), ptr::null()),
            mxr_set_v2ip_test_sync(remote, uid_n(9), &no_lead),
        ]
    };
    for (n, rc) in calls.into_iter().enumerate() {
        assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT, "call {n}");
    }
    // SAFETY: a live handle.
    let rc = unsafe { mxr_request_v2ip_testcard(remote, uid_n(9)) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);

    // SAFETY: created above and not yet freed.
    unsafe { mxr_remote_free(remote) };
}

/// The audio lock calls answer a null output as an argument error and a
/// device never heard from as not found, before anything is sent.
#[test]
fn the_audio_lock_calls_check_their_arguments_first() {
    let remote = client(c"abi-audio-lock", c"00000022.00000000.00000000.000000a8");

    // SAFETY: a live handle; a null output is what is under test.
    let rc = unsafe { mxr_audio_endpoint_status(remote, uid_n(9), 1, ptr::null_mut()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    let mut out = 0u32;
    // SAFETY: a live handle and a writable u32.
    let rc = unsafe { mxr_audio_endpoint_status(remote, uid_n(9), 1, &mut out) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);
    // SAFETY: a live handle.
    let rc = unsafe { mxr_set_audio_endpoint_locked(remote, uid_n(9), 1, true) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);

    // SAFETY: created above and not yet freed.
    unsafe { mxr_remote_free(remote) };
}

/// Handing addresses back answers a device never heard from as not found.
#[test]
fn auto_addresses_answers_an_unknown_device_as_not_found() {
    let remote = client(c"abi-auto-addr", c"00000022.00000000.00000000.000000a9");
    // SAFETY: a live handle.
    let rc = unsafe { mxr_auto_assign_v2ip_source_addresses(remote, uid_n(9)) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);
    // SAFETY: created above and not yet freed.
    unsafe { mxr_remote_free(remote) };
}

/// What the new reads answer on a client that has heard from nothing.
///
/// "Not found" and "not reported" are different answers and a caller acts on
/// them differently, so a client with no devices must not give either of them
/// where the other belongs.
#[test]
fn the_new_reads_separate_an_unknown_device_from_an_unreported_value() {
    let remote = client(c"abi-new-reads", c"00000021.00000000.00000000.000000a5");

    let mut frames = 1234u64;
    // SAFETY: a live handle and a writable u64.
    assert_eq!(
        unsafe { mxr_frames_received(remote, &mut frames) },
        mxr_result_t::MXR_OK
    );
    assert_eq!(frames, 0, "a client with no socket has heard nothing");

    let mut edid = [0u8; MXR_EDID_LEN];
    // SAFETY: a live handle and a buffer of exactly the documented size.
    let rc = unsafe { mxr_device_edid(remote, uid_n(9), true, edid.as_mut_ptr(), edid.len()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_REPORTED);

    // A buffer too short is the caller's mistake, not a missing report.
    // SAFETY: a live handle, and a capacity that matches the shortened slice.
    let rc = unsafe { mxr_device_edid(remote, uid_n(9), true, edid.as_mut_ptr(), 8) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);

    let mut audio = std::mem::MaybeUninit::<mxr_audio_details_t>::zeroed();
    // SAFETY: a live handle and a writable struct.
    let rc = unsafe { mxr_bay_audio_details(remote, mxr_bay_uid_t::default(), audio.as_mut_ptr()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND, "no such bay");

    // SAFETY: created above and not yet freed.
    unsafe { mxr_remote_free(remote) };
}

/// A route with an address that is not one is refused before anything is sent.
#[test]
fn a_route_with_an_unparseable_address_is_an_argument_error() {
    let remote = client(c"abi-bad-route", c"00000022.00000000.00000000.000000a5");
    let bad = CString::new("not an address").expect("no NUL");
    let route = mxr_v2ip_route_t {
        video: mxr_stream_addr_t {
            ip: bad.as_ptr(),
            port: 0,
        },
        audio: mxr_stream_addr_t {
            ip: ptr::null(),
            port: 0,
        },
        anc: mxr_stream_addr_t {
            ip: ptr::null(),
            port: 0,
        },
    };
    // SAFETY: a live handle, and a route whose one address is a live string.
    let rc =
        unsafe { mxr_select_source_addr(remote, mxr_bay_uid_t::default(), &route, ptr::null()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    assert!(
        last_error().contains("not an address"),
        "the failure did not name the address it could not read: {}",
        last_error()
    );

    // A null route pointer is the other way to get this wrong.
    // SAFETY: a live handle and a deliberately null route.
    let rc = unsafe {
        mxr_select_source_addr(remote, mxr_bay_uid_t::default(), ptr::null(), ptr::null())
    };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);

    // SAFETY: created above and not yet freed.
    unsafe { mxr_remote_free(remote) };
}

/// Only one of the three decoder states carries a reading, and the other two
/// carry nothing rather than a stale copy of one.
///
/// A C caller has no `Option` to stop it reading the geometry regardless, so
/// what stops it reporting a 4K picture from a sink whose decoder has never
/// answered is that the fields beside `detail` are zero.
#[test]
fn only_an_answered_decoder_carries_a_reading() {
    let answered: mxr_v2ip_decoder_t = V2ipDecoderDetail::Answered(V2ipDecoderReport {
        reason: V2ipDecoderReason::PTP_UNLOCKED,
        blocking: true,
        width: 3840,
        height: 2160,
        format: V2ipDecoderFormat::YCBCR_420,
        updates: 600,
        flags: 1 << 8,
        blocked_count: 100_009,
    })
    .into();
    assert_eq!(
        answered.detail,
        mxr_v2ip_decoder_detail_t::MXR_V2IP_DECODER_ANSWERED
    );
    assert_eq!(answered.reason, 8);
    assert!(answered.blocking);
    assert_eq!((answered.width, answered.height), (3840, 2160));
    assert_eq!(answered.format, 3);
    assert_eq!(answered.updates, 600);
    assert_eq!(answered.flags, 1 << 8);
    assert_eq!(answered.blocked_count, 100_009);

    for (detail, expected) in [
        (
            V2ipDecoderDetail::Absent,
            mxr_v2ip_decoder_detail_t::MXR_V2IP_DECODER_ABSENT,
        ),
        (
            V2ipDecoderDetail::NeverAnswered,
            mxr_v2ip_decoder_detail_t::MXR_V2IP_DECODER_NEVER_ANSWERED,
        ),
    ] {
        let empty: mxr_v2ip_decoder_t = detail.into();
        assert_eq!(empty.detail, expected);
        assert!(!empty.blocking);
        assert_eq!(
            (
                u32::from(empty.reason),
                u32::from(empty.width),
                u32::from(empty.height),
                u32::from(empty.format),
                u32::from(empty.updates),
                empty.flags,
                empty.blocked_count,
            ),
            (0, 0, 0, 0, 0, 0, 0),
            "{expected:?} carried a reading it does not have"
        );
    }
}

/// The time calls check their arguments before anything else.
#[test]
fn the_time_calls_check_their_arguments_first() {
    let remote = client(c"abi-time", c"00000023.00000000.00000000.000000a5");

    // SAFETY: a live handle; null outputs are what is under test.
    unsafe {
        assert_eq!(
            mxr_time_zone(remote, uid_n(9), ptr::null_mut()),
            mxr_result_t::MXR_ERR_INVALID_ARGUMENT
        );
        assert_eq!(
            mxr_device_clock(remote, uid_n(9), ptr::null_mut()),
            mxr_result_t::MXR_ERR_INVALID_ARGUMENT
        );
    }
    let long = CString::new("Z".repeat(MXR_TIME_ZONE_NAME_LEN)).expect("no NUL");
    // SAFETY: a live handle and NUL-terminated strings, or a null one where
    // that is what is under test.
    unsafe {
        assert_eq!(
            mxr_set_mesh_time_zone(remote, c"UTC".as_ptr(), ptr::null()),
            mxr_result_t::MXR_ERR_INVALID_ARGUMENT
        );
        assert_eq!(
            mxr_set_mesh_time_zone(remote, long.as_ptr(), c"UTC0".as_ptr()),
            mxr_result_t::MXR_ERR_INVALID_ARGUMENT
        );
        assert_eq!(
            mxr_set_mesh_time_zone(remote, c"UTC".as_ptr(), c"UTC0".as_ptr()),
            mxr_result_t::MXR_ERR_NOT_CONNECTED
        );
        assert_eq!(
            mxr_set_mesh_time(remote, 1 << 32),
            mxr_result_t::MXR_ERR_INVALID_ARGUMENT
        );
        assert_eq!(
            mxr_set_mesh_time(remote, 0x6ABB_8FC6),
            mxr_result_t::MXR_ERR_NOT_CONNECTED
        );
    }

    // SAFETY: created above and not yet freed.
    unsafe { mxr_remote_free(remote) };
}

#[test]
fn bay_content_of_a_bay_never_heard_from_is_not_found() {
    // SAFETY: a null handle is what is under test; no buffer to write.
    let count =
        unsafe { mxr_bay_content(ptr::null(), mxr_bay_uid_t::default(), ptr::null_mut(), 0) };
    assert_eq!(count, 0);
    assert!(
        last_error().contains("handle is null"),
        "the failure did not name the handle: {}",
        last_error()
    );

    let handle = client(c"abi-content", c"00000023.00000000.00000000.000000ad");
    let mut out = [mxr_bay_content_field_t {
        key: [0; MXR_BAY_CONTENT_KEY_LEN],
        value: [0; MXR_BAY_CONTENT_VALUE_LEN],
    }];
    // SAFETY: a live handle and one writable field.
    let count = unsafe { mxr_bay_content(handle, mxr_bay_uid_t::default(), out.as_mut_ptr(), 1) };
    assert_eq!(count, 0);
    assert!(
        last_error().contains("no bay"),
        "the failure did not say what was missing: {}",
        last_error()
    );

    // SAFETY: the handle came from mxr_remote_new and is freed once.
    unsafe { mxr_remote_free(handle) };
}

#[test]
fn an_overlay_sync_of_a_device_never_heard_from_is_not_found() {
    let remote = client(c"abi-osd", c"00000024.00000000.00000000.000000ae");

    // SAFETY: a live handle; a null output is what is under test.
    let rc = unsafe { mxr_osd_sync(remote, uid_n(9), ptr::null_mut()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    let mut sync = std::mem::MaybeUninit::<mxr_osd_sync_t>::zeroed();
    // SAFETY: a live handle and a writable struct.
    let rc = unsafe { mxr_osd_sync(remote, uid_n(9), sync.as_mut_ptr()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);

    // SAFETY: a live handle; a null output is what is under test.
    let rc = unsafe { mxr_osd_sync_report(remote, uid_n(9), ptr::null_mut()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    let mut report = std::mem::MaybeUninit::<mxr_osd_sync_report_t>::zeroed();
    // SAFETY: a live handle and a writable struct.
    let rc = unsafe { mxr_osd_sync_report(remote, uid_n(9), report.as_mut_ptr()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);

    // SAFETY: the handle came from mxr_remote_new and is freed once.
    unsafe { mxr_remote_free(remote) };
}

#[test]
fn the_overlay_sync_calls_check_their_arguments_first() {
    // SAFETY: a null handle is what is under test.
    let rc =
        unsafe { mxr_send_osd_sync(ptr::null(), 1, ptr::null(), 0, ptr::null(), ptr::null_mut()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);

    let remote = client(c"abi-osd-send", c"00000025.00000000.00000000.000000af");
    let mut seq = 0x5A5Au16;
    let background = mxr_osd_sync_entry_t {
        kind: MXR_OSD_SYNC_TYPE_BACKGROUND,
        size: MXR_OSD_SYNC_SIZE_LARGE,
        layers: 1 << 4,
        set: 1,
        font: ptr::null(),
        font_size: 0,
        x: 0,
        y: 0,
        wrap: 0,
        align: 0,
        format: 0,
        colour: 0x0010_2030,
        text: ptr::null(),
        logo: 0,
    };

    // SAFETY: a live handle, no entries and no images; a sync copying
    // nothing is what is under test.
    let rc = unsafe { mxr_send_osd_sync(remote, 0, ptr::null(), 0, ptr::null(), &mut seq) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    assert!(
        last_error().contains("copying something"),
        "the failure did not say why: {}",
        last_error()
    );
    // SAFETY: a live handle and one entry; a logo of 5 is what is under test.
    let rc = unsafe {
        let mut logo = background;
        logo.kind = MXR_OSD_SYNC_TYPE_LOGO;
        logo.logo = 5;
        mxr_send_osd_sync(
            remote,
            MXR_OSD_SYNC_WHAT_LOGO,
            &logo,
            1,
            ptr::null(),
            &mut seq,
        )
    };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);

    // A valid sync passes every check and fails only for the missing socket.
    // SAFETY: a live handle and one entry with no strings.
    let rc = unsafe {
        mxr_send_osd_sync(
            remote,
            MXR_OSD_SYNC_WHAT_BACKGROUNDS,
            &background,
            1,
            ptr::null(),
            &mut seq,
        )
    };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_CONNECTED);
    assert_eq!(seq, 0x5A5A, "a failed send writes no number");

    // SAFETY: a live handle and a writable number.
    let rc = unsafe { mxr_resend_osd_sync(remote, uid_n(9), &mut seq) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);
    // SAFETY: a live handle.
    let rc = unsafe { mxr_clear_osd_sync(remote) };
    assert_eq!(rc, mxr_result_t::MXR_OK);

    // SAFETY: the handle came from mxr_remote_new and is freed once.
    unsafe { mxr_remote_free(remote) };
}
