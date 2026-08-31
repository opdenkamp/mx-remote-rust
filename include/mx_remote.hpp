// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

#ifndef MX_REMOTE_HPP
#define MX_REMOTE_HPP

/*
 * A C++ face for mx_remote.h. It owns nothing the C API does not, and adds no
 * behaviour: what it adds is that the client handle is released on every path
 * out of a scope, that a uid is a value with an identity rather than a struct
 * to memcmp, and that events arrive as virtual calls on an object rather than
 * as a table of function pointers with a void* to cast back.
 *
 * Requires C++11. Nothing here throws or allocates on the event path.
 *
 * Handlers run on the library's receive thread, one at a time, with no lock
 * held. Calling back into the library from one is safe; blocking in one stalls
 * every device on the network. Pointers a handler is given - strings, blobs,
 * payload structs - are borrowed for the length of that call, so anything
 * needed afterwards must be copied.
 */

#include "mx_remote.h"

#include <cstdint>
#include <cstring>
#include <string>
#include <utility>
#include <vector>

namespace mxr {

/// A device identifier, by value.
struct Uid {
    /// The identifier as the C API carries it.
    mxr_uid_t raw{};

    Uid() = default;
    /// Wraps an identifier from the C API. Deliberately implicit: the two are
    /// the same value, and a conversion at every callback would say nothing.
    Uid(const mxr_uid_t &value) : raw(value) {}
    operator mxr_uid_t() const { return raw; }

    /// Whether this is the empty identifier, which the protocol uses wherever
    /// a device could be named and is not.
    bool empty() const { return mxr_uid_is_zero(raw); }

    /// The dotted-hex form, or an empty string if it could not be written.
    std::string str() const {
        char buf[MXR_UID_STRING_LEN];
        if (mxr_uid_to_string(raw, buf, sizeof buf) != MXR_OK) return std::string();
        return std::string(buf);
    }

    /// Reads the dotted-hex form. Returns the empty identifier on bad input,
    /// which `empty()` reports and which no device ever carries.
    static Uid parse(const char *text) {
        Uid out;
        mxr_uid_from_string(text, &out.raw);
        return out;
    }
};

inline bool operator==(const Uid &a, const Uid &b) {
    return std::memcmp(a.raw.bytes, b.raw.bytes, sizeof a.raw.bytes) == 0;
}
inline bool operator!=(const Uid &a, const Uid &b) { return !(a == b); }
inline bool operator<(const Uid &a, const Uid &b) {
    return std::memcmp(a.raw.bytes, b.raw.bytes, sizeof a.raw.bytes) < 0;
}

/// A bay identifier: a device, and a port on it.
struct BayUid {
    /// The identifier as the C API carries it.
    mxr_bay_uid_t raw{};

    BayUid() = default;
    /// Wraps an identifier from the C API. Implicit for the same reason
    /// `Uid`'s is.
    BayUid(const mxr_bay_uid_t &value) : raw(value) {}
    BayUid(Uid device, uint16_t port) {
        raw.device = device.raw;
        raw.port = port;
    }
    operator mxr_bay_uid_t() const { return raw; }

    /// The device the bay is on.
    Uid device() const { return Uid(raw.device); }
    /// The bay's port number on that device.
    uint16_t port() const { return raw.port; }
    /// Whether this names no bay.
    bool empty() const { return mxr_uid_is_zero(raw.device); }

    /// The `device:port` form.
    std::string str() const { return device().str() + ":" + std::to_string(raw.port); }
};

inline bool operator==(const BayUid &a, const BayUid &b) {
    return a.raw.port == b.raw.port && Uid(a.raw.device) == Uid(b.raw.device);
}
inline bool operator!=(const BayUid &a, const BayUid &b) { return !(a == b); }
inline bool operator<(const BayUid &a, const BayUid &b) {
    if (Uid(a.raw.device) != Uid(b.raw.device)) return Uid(a.raw.device) < Uid(b.raw.device);
    return a.raw.port < b.raw.port;
}

/// Why the last call on this thread failed. Empty when none has.
inline std::string last_error() { return std::string(mxr_last_error()); }


/*
 * The event set, listed once per shape. Each list is expanded three times
 * below - as the virtual methods on Handler, as the C functions the library
 * calls, and as the assignments that fill the table - so an event added to a
 * list here reaches all three or none.
 */

#define MXR_EVENTS_DEVICE(X) \
    X(on_device_update) \
    X(on_device_config_changed) \
    X(on_device_config_complete) \
    X(on_device_temperature_changed) \
    X(on_firmware_version_changed) \
    X(on_network_status_changed) \
    X(on_v2ip_stats_changed) \
    X(on_v2ip_sources_changed) \
    X(on_v2ip_details_changed) \
    X(on_v2ip_sink_changed) \
    X(on_multiviewer_status_changed) \
    X(on_audio_endpoints_changed) \
    X(on_topology_changed) \
    X(on_amp_dolby_settings_changed) \
    X(on_pdu_state_changed) \
    X(on_tiling_changed) \
    X(on_rc_settings_changed) \
    X(on_discover_request) \
    X(on_monitoring_pulse) \
    X(on_upgrade_fpga_requested) \
    X(on_detect_bays_requested)

#define MXR_EVENTS_DEVICE_BOOL(X) \
    X(on_device_online_changed) \
    X(on_setup_status_changed)

#define MXR_EVENTS_DEVICE_UID(X) \
    X(on_mesh_master_changed) \
    X(on_v2ip_link_changed) \
    X(on_reboot_requested)

#define MXR_EVENTS_DEVICE_U16(X) \
    X(on_installer_id_changed)

#define MXR_EVENTS_ENDPOINT_BOOL(X) \
    X(on_audio_endpoint_mute) \
    X(on_audio_endpoint_trigger)

#define MXR_EVENTS_ENDPOINT_U32(X) \
    X(on_audio_endpoint_volume)

#define MXR_EVENTS_BAY(X) \
    X(on_bay_update) \
    X(on_bay_registered) \
    X(on_amp_zone_settings_changed) \
    X(on_filtered_devices_changed)

#define MXR_EVENTS_BAY_BOOL(X) \
    X(on_signal_detected_changed) \
    X(on_faulty_changed) \
    X(on_hidden_changed) \
    X(on_poe_powered_changed) \
    X(on_hdbt_connected_changed) \
    X(on_hpd_detected_changed) \
    X(on_cec_detected_changed) \
    X(on_volume_step) \
    X(on_encoder_disabled_changed) \
    X(on_decoder_disabled_changed)

#define MXR_EVENTS_BAY_STR(X) \
    X(on_name_changed) \
    X(on_signal_type_changed)

#define MXR_EVENTS_BAY_BAY(X) \
    X(on_video_source_changed) \
    X(on_audio_source_changed) \
    X(on_mirror_status_changed)

#define MXR_EVENTS_BAY_U8(X) \
    X(on_rc_type_changed) \
    X(on_audio_clip) \
    X(on_audio_endpoint_changed)

#define MXR_EVENTS_BAY_U16(X) \
    X(on_edid_profile_changed) \
    X(on_key_pressed) \
    X(on_action_received)

#define MXR_EVENTS_DEVICE_PAYLOAD(X) \
    X(on_multiviewer_command, mxr_multiviewer_command_t) \
    X(on_audio_select_input, mxr_audio_change_source_t) \
    X(on_set_route_requested, mxr_set_route_request_t) \
    X(on_edid_requested, mxr_edid_request_t) \
    X(on_edid_received, mxr_edid_record_t) \
    X(on_bay_name_change_requested, mxr_bay_name_change_t) \
    X(on_edid_profile_change_requested, mxr_edid_profile_change_t) \
    X(on_factory_reset_requested, mxr_factory_reset_request_t) \
    X(on_power_save_requested, mxr_power_save_request_t) \
    X(on_key_transmit_requested, mxr_key_transmit_request_t) \
    X(on_action_transmit_requested, mxr_action_transmit_request_t) \
    X(on_ir_transmit_requested, mxr_ir_transmit_request_t) \
    X(on_blacklist_changed, mxr_blacklist_change_t) \
    X(on_video_wall_command, mxr_video_wall_command_t)

#define MXR_EVENTS_BAY_PAYLOAD(X) \
    X(on_ir_captured, mxr_ir_capture_t)

/// Override what matters and pass the object to `Remote::open`.
///
/// Every method does nothing by default, so a handler names only the events it
/// cares about. Two of them are enough on their own: `on_device_update` fires
/// after every device-level event and `on_bay_update` after every bay-level
/// one, so a program that redraws from the snapshots needs no others.
///
/// An event whose payload is state carries only the identifier, because the
/// snapshot is where that value lives and a copy here could only be staler.
/// An event that is a request or a one-off carries a struct, because nothing
/// stores it.
class Handler {
public:
    virtual ~Handler() = default;

#define MXR_DECL(name) virtual void name(Uid /*device*/) {}
    MXR_EVENTS_DEVICE(MXR_DECL)
#undef MXR_DECL

#define MXR_DECL(name) virtual void name(Uid /*device*/, bool /*value*/) {}
    MXR_EVENTS_DEVICE_BOOL(MXR_DECL)
#undef MXR_DECL

#define MXR_DECL(name) virtual void name(Uid /*device*/, Uid /*other*/) {}
    MXR_EVENTS_DEVICE_UID(MXR_DECL)
#undef MXR_DECL

#define MXR_DECL(name) virtual void name(Uid /*device*/, uint16_t /*value*/) {}
    MXR_EVENTS_DEVICE_U16(MXR_DECL)
#undef MXR_DECL

#define MXR_DECL(name) virtual void name(Uid /*device*/, uint16_t /*endpoint*/, bool /*value*/) {}
    MXR_EVENTS_ENDPOINT_BOOL(MXR_DECL)
#undef MXR_DECL

#define MXR_DECL(name) virtual void name(Uid /*device*/, uint16_t /*endpoint*/, uint32_t /*value*/) {}
    MXR_EVENTS_ENDPOINT_U32(MXR_DECL)
#undef MXR_DECL

#define MXR_DECL(name) virtual void name(BayUid /*bay*/) {}
    MXR_EVENTS_BAY(MXR_DECL)
#undef MXR_DECL

#define MXR_DECL(name) virtual void name(BayUid /*bay*/, bool /*value*/) {}
    MXR_EVENTS_BAY_BOOL(MXR_DECL)
#undef MXR_DECL

#define MXR_DECL(name) virtual void name(BayUid /*bay*/, const char * /*value*/) {}
    MXR_EVENTS_BAY_STR(MXR_DECL)
#undef MXR_DECL

#define MXR_DECL(name) virtual void name(BayUid /*bay*/, BayUid /*other*/) {}
    MXR_EVENTS_BAY_BAY(MXR_DECL)
#undef MXR_DECL

#define MXR_DECL(name) virtual void name(BayUid /*bay*/, uint8_t /*value*/) {}
    MXR_EVENTS_BAY_U8(MXR_DECL)
#undef MXR_DECL

#define MXR_DECL(name) virtual void name(BayUid /*bay*/, uint16_t /*value*/) {}
    MXR_EVENTS_BAY_U16(MXR_DECL)
#undef MXR_DECL

#define MXR_DECL(name, type) virtual void name(Uid /*device*/, const type &) {}
    MXR_EVENTS_DEVICE_PAYLOAD(MXR_DECL)
#undef MXR_DECL

#define MXR_DECL(name, type) virtual void name(BayUid /*bay*/, const type &) {}
    MXR_EVENTS_BAY_PAYLOAD(MXR_DECL)
#undef MXR_DECL

    /// The device reported a status about itself.
    virtual void on_system_status_changed(Uid /*device*/, uint16_t /*status*/,
                                          const char * /*message*/) {}
    /// The bay's volume or mute state changed.
    virtual void on_volume_changed(BayUid /*bay*/, uint8_t /*volume*/, mxr_tribool_t /*muted*/) {}
    /// The attached device's power state changed.
    virtual void on_power_changed(BayUid /*bay*/, mxr_power_status_t /*power*/) {}
    /// The audio return channel changed.
    virtual void on_arc_changed(BayUid /*bay*/, mxr_arc_status_t /*arc*/) {}
    /// The bay was linked to a bay on another device. Both ends are told, so
    /// both fire: `bay_name` names the bay whose link record changed, which is
    /// this bay on the device reporting the change and the far bay on its peer.
    virtual void on_bay_linked(BayUid /*bay*/, const char * /*linked_serial*/,
                               const char * /*bay_name*/, uint32_t /*features*/) {}
    /// The bay's link to another device was removed. The arguments describe
    /// the link that went, and mean what they do on `on_bay_linked`.
    virtual void on_bay_unlinked(BayUid /*bay*/, const char * /*linked_serial*/,
                                 const char * /*bay_name*/) {}
};

namespace detail {

inline Handler *handler_of(void *userdata) { return static_cast<Handler *>(userdata); }

/*
 * The functions the library calls. They have C language linkage because that
 * is what the table's members are declared with, and a name of their own so
 * that linkage cannot collide with anything else in the program.
 */
extern "C" {

#define MXR_TRAMPOLINE(name) \
    inline void mxr_cxx_##name(void *ud, mxr_uid_t d) { handler_of(ud)->name(Uid(d)); }
MXR_EVENTS_DEVICE(MXR_TRAMPOLINE)
#undef MXR_TRAMPOLINE

#define MXR_TRAMPOLINE(name) \
    inline void mxr_cxx_##name(void *ud, mxr_uid_t d, bool v) { handler_of(ud)->name(Uid(d), v); }
MXR_EVENTS_DEVICE_BOOL(MXR_TRAMPOLINE)
#undef MXR_TRAMPOLINE

#define MXR_TRAMPOLINE(name) \
    inline void mxr_cxx_##name(void *ud, mxr_uid_t d, mxr_uid_t o) { handler_of(ud)->name(Uid(d), Uid(o)); }
MXR_EVENTS_DEVICE_UID(MXR_TRAMPOLINE)
#undef MXR_TRAMPOLINE

#define MXR_TRAMPOLINE(name) \
    inline void mxr_cxx_##name(void *ud, mxr_uid_t d, uint16_t v) { handler_of(ud)->name(Uid(d), v); }
MXR_EVENTS_DEVICE_U16(MXR_TRAMPOLINE)
#undef MXR_TRAMPOLINE

#define MXR_TRAMPOLINE(name) \
    inline void mxr_cxx_##name(void *ud, mxr_uid_t d, uint16_t e, bool v) { handler_of(ud)->name(Uid(d), e, v); }
MXR_EVENTS_ENDPOINT_BOOL(MXR_TRAMPOLINE)
#undef MXR_TRAMPOLINE

#define MXR_TRAMPOLINE(name) \
    inline void mxr_cxx_##name(void *ud, mxr_uid_t d, uint16_t e, uint32_t v) { handler_of(ud)->name(Uid(d), e, v); }
MXR_EVENTS_ENDPOINT_U32(MXR_TRAMPOLINE)
#undef MXR_TRAMPOLINE

#define MXR_TRAMPOLINE(name) \
    inline void mxr_cxx_##name(void *ud, mxr_bay_uid_t b) { handler_of(ud)->name(BayUid(b)); }
MXR_EVENTS_BAY(MXR_TRAMPOLINE)
#undef MXR_TRAMPOLINE

#define MXR_TRAMPOLINE(name) \
    inline void mxr_cxx_##name(void *ud, mxr_bay_uid_t b, bool v) { handler_of(ud)->name(BayUid(b), v); }
MXR_EVENTS_BAY_BOOL(MXR_TRAMPOLINE)
#undef MXR_TRAMPOLINE

#define MXR_TRAMPOLINE(name) \
    inline void mxr_cxx_##name(void *ud, mxr_bay_uid_t b, const char *v) { handler_of(ud)->name(BayUid(b), v); }
MXR_EVENTS_BAY_STR(MXR_TRAMPOLINE)
#undef MXR_TRAMPOLINE

#define MXR_TRAMPOLINE(name) \
    inline void mxr_cxx_##name(void *ud, mxr_bay_uid_t b, mxr_bay_uid_t o) { handler_of(ud)->name(BayUid(b), BayUid(o)); }
MXR_EVENTS_BAY_BAY(MXR_TRAMPOLINE)
#undef MXR_TRAMPOLINE

#define MXR_TRAMPOLINE(name) \
    inline void mxr_cxx_##name(void *ud, mxr_bay_uid_t b, uint8_t v) { handler_of(ud)->name(BayUid(b), v); }
MXR_EVENTS_BAY_U8(MXR_TRAMPOLINE)
#undef MXR_TRAMPOLINE

#define MXR_TRAMPOLINE(name) \
    inline void mxr_cxx_##name(void *ud, mxr_bay_uid_t b, uint16_t v) { handler_of(ud)->name(BayUid(b), v); }
MXR_EVENTS_BAY_U16(MXR_TRAMPOLINE)
#undef MXR_TRAMPOLINE

#define MXR_TRAMPOLINE(name, type) \
    inline void mxr_cxx_##name(void *ud, mxr_uid_t d, const type *p) { \
        handler_of(ud)->name(Uid(d), *p); \
    }
MXR_EVENTS_DEVICE_PAYLOAD(MXR_TRAMPOLINE)
#undef MXR_TRAMPOLINE

#define MXR_TRAMPOLINE(name, type) \
    inline void mxr_cxx_##name(void *ud, mxr_bay_uid_t b, const type *p) { \
        handler_of(ud)->name(BayUid(b), *p); \
    }
MXR_EVENTS_BAY_PAYLOAD(MXR_TRAMPOLINE)
#undef MXR_TRAMPOLINE

inline void mxr_cxx_on_system_status_changed(void *ud, mxr_uid_t d, uint16_t s, const char *m) {
    handler_of(ud)->on_system_status_changed(Uid(d), s, m);
}
inline void mxr_cxx_on_volume_changed(void *ud, mxr_bay_uid_t b, uint8_t v, mxr_tribool_t m) {
    handler_of(ud)->on_volume_changed(BayUid(b), v, m);
}
inline void mxr_cxx_on_power_changed(void *ud, mxr_bay_uid_t b, mxr_power_status_t p) {
    handler_of(ud)->on_power_changed(BayUid(b), p);
}
inline void mxr_cxx_on_arc_changed(void *ud, mxr_bay_uid_t b, mxr_arc_status_t a) {
    handler_of(ud)->on_arc_changed(BayUid(b), a);
}
inline void mxr_cxx_on_bay_linked(void *ud, mxr_bay_uid_t b, const char *serial,
                                  const char *name, uint32_t features) {
    handler_of(ud)->on_bay_linked(BayUid(b), serial, name, features);
}
inline void mxr_cxx_on_bay_unlinked(void *ud, mxr_bay_uid_t b, const char *serial,
                                    const char *name) {
    handler_of(ud)->on_bay_unlinked(BayUid(b), serial, name);
}

} // extern "C"

/// The table every Handler is reached through. One per program, not one per
/// client: it holds no state, only the addresses of the functions above.
inline const mxr_callbacks_t &callbacks() {
    static const mxr_callbacks_t table = [] {
        mxr_callbacks_t t{};

#define MXR_ASSIGN(name) t.name = &mxr_cxx_##name;
        MXR_EVENTS_DEVICE(MXR_ASSIGN)
#undef MXR_ASSIGN
#define MXR_ASSIGN(name) t.name = &mxr_cxx_##name;
        MXR_EVENTS_DEVICE_BOOL(MXR_ASSIGN)
#undef MXR_ASSIGN
#define MXR_ASSIGN(name) t.name = &mxr_cxx_##name;
        MXR_EVENTS_DEVICE_UID(MXR_ASSIGN)
#undef MXR_ASSIGN
#define MXR_ASSIGN(name) t.name = &mxr_cxx_##name;
        MXR_EVENTS_DEVICE_U16(MXR_ASSIGN)
#undef MXR_ASSIGN
#define MXR_ASSIGN(name) t.name = &mxr_cxx_##name;
        MXR_EVENTS_ENDPOINT_BOOL(MXR_ASSIGN)
#undef MXR_ASSIGN
#define MXR_ASSIGN(name) t.name = &mxr_cxx_##name;
        MXR_EVENTS_ENDPOINT_U32(MXR_ASSIGN)
#undef MXR_ASSIGN
#define MXR_ASSIGN(name) t.name = &mxr_cxx_##name;
        MXR_EVENTS_BAY(MXR_ASSIGN)
#undef MXR_ASSIGN
#define MXR_ASSIGN(name) t.name = &mxr_cxx_##name;
        MXR_EVENTS_BAY_BOOL(MXR_ASSIGN)
#undef MXR_ASSIGN
#define MXR_ASSIGN(name) t.name = &mxr_cxx_##name;
        MXR_EVENTS_BAY_STR(MXR_ASSIGN)
#undef MXR_ASSIGN
#define MXR_ASSIGN(name) t.name = &mxr_cxx_##name;
        MXR_EVENTS_BAY_BAY(MXR_ASSIGN)
#undef MXR_ASSIGN
#define MXR_ASSIGN(name) t.name = &mxr_cxx_##name;
        MXR_EVENTS_BAY_U8(MXR_ASSIGN)
#undef MXR_ASSIGN
#define MXR_ASSIGN(name) t.name = &mxr_cxx_##name;
        MXR_EVENTS_BAY_U16(MXR_ASSIGN)
#undef MXR_ASSIGN
#define MXR_ASSIGN(name, type) t.name = &mxr_cxx_##name;
        MXR_EVENTS_DEVICE_PAYLOAD(MXR_ASSIGN)
        MXR_EVENTS_BAY_PAYLOAD(MXR_ASSIGN)
#undef MXR_ASSIGN
        t.on_system_status_changed = &mxr_cxx_on_system_status_changed;
        t.on_volume_changed = &mxr_cxx_on_volume_changed;
        t.on_power_changed = &mxr_cxx_on_power_changed;
        t.on_arc_changed = &mxr_cxx_on_arc_changed;
        t.on_bay_linked = &mxr_cxx_on_bay_linked;
        t.on_bay_unlinked = &mxr_cxx_on_bay_unlinked;
        return t;
    }();
    return table;
}

/// How many events the lists above cover.
#define MXR_COUNT_ONE(name) +1
#define MXR_COUNT_TWO(name, type) +1
const size_t event_count = 0
    MXR_EVENTS_DEVICE(MXR_COUNT_ONE)
    MXR_EVENTS_DEVICE_BOOL(MXR_COUNT_ONE)
    MXR_EVENTS_DEVICE_UID(MXR_COUNT_ONE)
    MXR_EVENTS_DEVICE_U16(MXR_COUNT_ONE)
    MXR_EVENTS_ENDPOINT_BOOL(MXR_COUNT_ONE)
    MXR_EVENTS_ENDPOINT_U32(MXR_COUNT_ONE)
    MXR_EVENTS_BAY(MXR_COUNT_ONE)
    MXR_EVENTS_BAY_BOOL(MXR_COUNT_ONE)
    MXR_EVENTS_BAY_STR(MXR_COUNT_ONE)
    MXR_EVENTS_BAY_BAY(MXR_COUNT_ONE)
    MXR_EVENTS_BAY_U8(MXR_COUNT_ONE)
    MXR_EVENTS_BAY_U16(MXR_COUNT_ONE)
    MXR_EVENTS_DEVICE_PAYLOAD(MXR_COUNT_TWO)
    MXR_EVENTS_BAY_PAYLOAD(MXR_COUNT_TWO)
    /* on_system_status_changed, on_volume_changed, on_power_changed,
     * on_arc_changed, on_bay_linked, on_bay_unlinked. */
    + 6;
#undef MXR_COUNT_ONE
#undef MXR_COUNT_TWO

/*
 * An event the C table has and the lists above do not would be silently
 * dropped by every C++ handler, and nothing else would say so. The table is
 * function pointers throughout, so its size is the only thing that counts them.
 */
static_assert(sizeof(mxr_callbacks_t) == event_count * sizeof(void (*)(void)),
              "mx_remote.h has events the C++ lists in this header do not cover");

} // namespace detail


/// A running client, released when it goes out of scope.
///
/// Move-only, because there is one socket and two threads behind the handle
/// and nothing sensible for a copy of it to mean.
class Remote {
public:
    Remote() = default;
    ~Remote() { reset(); }

    Remote(const Remote &) = delete;
    Remote &operator=(const Remote &) = delete;

    Remote(Remote &&other) noexcept : h_(other.h_) { other.h_ = nullptr; }
    Remote &operator=(Remote &&other) noexcept {
        if (this != &other) {
            reset();
            h_ = other.h_;
            other.h_ = nullptr;
        }
        return *this;
    }

    /// Creates a client, without opening a socket yet.
    ///
    /// `config` may be null for the defaults: multicast discovery on whichever
    /// interface the host picks. `handler` may be null for a client that only
    /// reads state; when it is not, it must outlive this object, because the
    /// library calls into it from its own threads until the client is freed.
    ///
    /// The result tests false on failure, with the reason in `last_error()`.
    static Remote open(const mxr_config_t *config = nullptr, Handler *handler = nullptr) {
        return Remote(mxr_remote_new(config, handler ? &detail::callbacks() : nullptr, handler));
    }

    /// Whether this holds a client.
    explicit operator bool() const { return h_ != nullptr; }

    /// The underlying handle, for the C functions this class does not wrap.
    /// It stays owned here.
    mxr_remote_t *get() const { return h_; }

    /// Closes and releases the client, and leaves this object empty.
    ///
    /// This waits for the receive and timer threads, so a callback that is
    /// running returns first - which means a handler must not destroy the
    /// Remote it was called from.
    void reset() {
        if (h_ != nullptr) {
            mxr_remote_free(h_);
            h_ = nullptr;
        }
    }

    /// Opens the socket and starts the receive and timer threads.
    mxr_result_t start() const { return mxr_remote_start(h_); }

    /// Stops the threads and closes the socket. A client that has been closed
    /// can be destroyed but not restarted.
    void close() const { mxr_remote_close(h_); }

    /// This client's own identifier.
    Uid uid() const {
        Uid out;
        mxr_remote_uid(h_, &out.raw);
        return out;
    }

    /// The name this client advertises to devices.
    std::string name() const {
        char buf[MXR_NAME_LEN];
        if (mxr_remote_name(h_, buf, sizeof buf) != MXR_OK) return std::string();
        return std::string(buf);
    }

    /// The address this client sends to. False before `start()`.
    bool target(std::string &ip, uint16_t &port) const {
        char buf[MXR_IP_STRING_LEN];
        if (mxr_remote_target(h_, buf, sizeof buf, &port) != MXR_OK) return false;
        ip = buf;
        return true;
    }

    /// Every device heard from.
    std::vector<Uid> devices() const {
        std::vector<mxr_uid_t> raw(mxr_devices(h_, nullptr, 0));
        trim(raw, raw.empty() ? 0 : mxr_devices(h_, raw.data(), raw.size()));
        return std::vector<Uid>(raw.begin(), raw.end());
    }

    /// A device's bays, in port order.
    std::vector<BayUid> bays(Uid device) const {
        std::vector<mxr_bay_uid_t> raw(mxr_device_bays(h_, device, nullptr, 0));
        trim(raw, raw.empty() ? 0 : mxr_device_bays(h_, device, raw.data(), raw.size()));
        return std::vector<BayUid>(raw.begin(), raw.end());
    }

    /// The temperatures a device reports, in its own order, in degrees Celsius.
    std::vector<uint8_t> temperatures(Uid device) const {
        std::vector<uint8_t> raw(mxr_device_temperatures(h_, device, nullptr, 0));
        trim(raw, raw.empty() ? 0 : mxr_device_temperatures(h_, device, raw.data(), raw.size()));
        return raw;
    }

    /// The devices whose signals a bay refuses.
    std::vector<Uid> filtered(BayUid bay) const {
        std::vector<mxr_uid_t> raw(mxr_bay_filtered(h_, bay, nullptr, 0));
        trim(raw, raw.empty() ? 0 : mxr_bay_filtered(h_, bay, raw.data(), raw.size()));
        return std::vector<Uid>(raw.begin(), raw.end());
    }

    /// The streams a device's source bays advertise.
    std::vector<mxr_stream_sources_t> v2ip_sources(Uid device) const {
        std::vector<mxr_stream_sources_t> raw(mxr_v2ip_sources(h_, device, nullptr, 0));
        trim(raw, raw.empty() ? 0 : mxr_v2ip_sources(h_, device, raw.data(), raw.size()));
        return raw;
    }

    /// A device's network ports, in its own order.
    std::vector<mxr_network_port_t> network_status(Uid device) const {
        std::vector<mxr_network_port_t> raw(mxr_network_status(h_, device, nullptr, 0));
        trim(raw, raw.empty() ? 0 : mxr_network_status(h_, device, raw.data(), raw.size()));
        return raw;
    }

    /// What a device reports about the devices it can see.
    std::vector<mxr_topology_entry_t> topology(Uid device) const {
        std::vector<mxr_topology_entry_t> raw(mxr_topology(h_, device, nullptr, 0));
        trim(raw, raw.empty() ? 0 : mxr_topology(h_, device, raw.data(), raw.size()));
        return raw;
    }

    /// The firmware versions a device reports, one per component.
    std::vector<mxr_firmware_version_t> firmware(Uid device) const {
        std::vector<mxr_firmware_version_t> raw(mxr_device_firmware(h_, device, nullptr, 0));
        trim(raw, raw.empty() ? 0 : mxr_device_firmware(h_, device, raw.data(), raw.size()));
        return raw;
    }

    /// A device's audio endpoints.
    std::vector<mxr_audio_endpoint_t> audio_endpoints(Uid device) const {
        std::vector<mxr_audio_endpoint_t> raw(mxr_audio_endpoints(h_, device, nullptr, 0));
        trim(raw, raw.empty() ? 0 : mxr_audio_endpoints(h_, device, raw.data(), raw.size()));
        return raw;
    }

    /// The endpoints one audio endpoint is made of.
    std::vector<uint8_t> audio_endpoint_children(Uid device, uint8_t endpoint) const {
        std::vector<uint8_t> raw(mxr_audio_endpoint_children(h_, device, endpoint, nullptr, 0));
        trim(raw, raw.empty() ? 0
                              : mxr_audio_endpoint_children(h_, device, endpoint, raw.data(),
                                                            raw.size()));
        return raw;
    }

    mxr_result_t select_video_source(BayUid sink, uint16_t source_port) const {
        return mxr_select_video_source(h_, sink, source_port);
    }
    mxr_result_t select_audio_source(BayUid sink, uint16_t source_port) const {
        return mxr_select_audio_source(h_, sink, source_port);
    }
    mxr_result_t select_video_source_by_name(BayUid sink, const char *name) const {
        return mxr_select_video_source_by_name(h_, sink, name);
    }
    mxr_result_t select_audio_source_by_name(BayUid sink, const char *name, const mxr_audio_format_t *format = nullptr) const {
        return mxr_select_audio_source_by_name(h_, sink, name, format);
    }
    mxr_result_t select_audio_source_addr(BayUid sink, const char *audio_ip, uint16_t audio_port, const mxr_audio_format_t *format = nullptr) const {
        return mxr_select_audio_source_addr(h_, sink, audio_ip, audio_port, format);
    }
    mxr_result_t select_source_addr(BayUid sink, const mxr_v2ip_route_t &route, const mxr_audio_format_t *format = nullptr) const {
        return mxr_select_source_addr(h_, sink, &route, format);
    }
    mxr_result_t set_bay_name(BayUid bay, const char *name) const {
        return mxr_set_bay_name(h_, bay, name);
    }
    mxr_result_t set_bay_hidden(BayUid bay, bool hidden) const {
        return mxr_set_bay_hidden(h_, bay, hidden);
    }
    mxr_result_t select_edid_profile(BayUid bay, uint16_t profile) const {
        return mxr_select_edid_profile(h_, bay, profile);
    }
    mxr_result_t send_action(BayUid bay, uint16_t action) const {
        return mxr_send_action(h_, bay, action);
    }
    mxr_result_t send_key(BayUid bay, uint16_t key) const {
        return mxr_send_key(h_, bay, key);
    }
    mxr_result_t power_on(BayUid bay) const {
        return mxr_power_on(h_, bay);
    }
    mxr_result_t power_off(BayUid bay) const {
        return mxr_power_off(h_, bay);
    }
    mxr_result_t set_volume(BayUid bay, uint8_t volume, mxr_tribool_t muted) const {
        return mxr_set_volume(h_, bay, volume, muted);
    }
    mxr_result_t volume_up(BayUid bay) const {
        return mxr_volume_up(h_, bay);
    }
    mxr_result_t volume_down(BayUid bay) const {
        return mxr_volume_down(h_, bay);
    }
    mxr_result_t set_muted(BayUid bay, bool muted) const {
        return mxr_set_muted(h_, bay, muted);
    }
    mxr_result_t set_amp_zone_settings(BayUid bay, const mxr_amp_zone_settings_t *settings) const {
        return mxr_set_amp_zone_settings(h_, bay, settings);
    }
    mxr_result_t set_audio_endpoint_muted(Uid device, uint16_t endpoint, bool muted) const {
        return mxr_set_audio_endpoint_muted(h_, device, endpoint, muted);
    }
    mxr_result_t set_audio_endpoint_trigger(Uid device, uint16_t endpoint, bool active) const {
        return mxr_set_audio_endpoint_trigger(h_, device, endpoint, active);
    }
    mxr_result_t set_audio_endpoint_volume(Uid device, uint16_t endpoint, uint32_t volume) const {
        return mxr_set_audio_endpoint_volume(h_, device, endpoint, volume);
    }
    mxr_result_t select_audio_endpoint_input(Uid sink, uint16_t sink_endpoint, Uid source, uint16_t source_endpoint) const {
        return mxr_select_audio_endpoint_input(h_, sink, sink_endpoint, source, source_endpoint);
    }
    mxr_result_t request_edid(Uid device, bool output) const {
        return mxr_request_edid(h_, device, output);
    }
    /// Asks one device, or - with a zero uid - the whole network.
    mxr_result_t request_signal_status(Uid device = Uid()) const {
        return mxr_request_signal_status(h_, device);
    }
    mxr_result_t subscribe_v2ip_stats(Uid device, bool subscribe) const {
        return mxr_subscribe_v2ip_stats(h_, device, subscribe);
    }
    mxr_result_t reboot(Uid device) const {
        return mxr_reboot(h_, device);
    }
    mxr_result_t send_monitoring_pulse() const {
        return mxr_send_monitoring_pulse(h_);
    }
    mxr_result_t set_multiviewer_view_mode(Uid device, uint8_t mode) const {
        return mxr_set_multiviewer_view_mode(h_, device, mode);
    }
    mxr_result_t set_multiviewer_video_source(Uid device, uint8_t screen, uint8_t source) const {
        return mxr_set_multiviewer_video_source(h_, device, screen, source);
    }
    mxr_result_t set_multiviewer_audio_source(Uid device, uint8_t source) const {
        return mxr_set_multiviewer_audio_source(h_, device, source);
    }
    mxr_result_t set_multiviewer_audio_volume(Uid device, uint8_t volume, bool muted) const {
        return mxr_set_multiviewer_audio_volume(h_, device, volume, muted);
    }
    mxr_result_t set_multiviewer_edid_template(Uid device, uint8_t template_) const {
        return mxr_set_multiviewer_edid_template(h_, device, template_);
    }
    mxr_result_t set_multiviewer_remote_control(Uid device, uint8_t source) const {
        return mxr_set_multiviewer_remote_control(h_, device, source);
    }
    mxr_result_t set_multiviewer_pip_size(Uid device, uint8_t size) const {
        return mxr_set_multiviewer_pip_size(h_, device, size);
    }
    mxr_result_t set_multiviewer_pip_position(Uid device, uint8_t position) const {
        return mxr_set_multiviewer_pip_position(h_, device, position);
    }
    mxr_result_t set_multiviewer_aspect_ratio(Uid device, uint8_t aspect) const {
        return mxr_set_multiviewer_aspect_ratio(h_, device, aspect);
    }
    mxr_result_t set_multiviewer_auto_switch(Uid device, bool enable) const {
        return mxr_set_multiviewer_auto_switch(h_, device, enable);
    }
    mxr_result_t set_multiviewer_output_mode(Uid device, uint8_t mode) const {
        return mxr_set_multiviewer_output_mode(h_, device, mode);
    }
    mxr_result_t set_multiviewer_output_itc(Uid device, uint8_t mode) const {
        return mxr_set_multiviewer_output_itc(h_, device, mode);
    }
    mxr_result_t set_multiviewer_hdcp_mode(Uid device, uint8_t mode) const {
        return mxr_set_multiviewer_hdcp_mode(h_, device, mode);
    }
    mxr_result_t set_multiviewer_input_source(Uid device, uint8_t input, Uid source) const {
        return mxr_set_multiviewer_input_source(h_, device, input, source);
    }
    mxr_result_t multiviewer_auto_route(Uid device) const {
        return mxr_multiviewer_auto_route(h_, device);
    }
    mxr_result_t device(Uid uid, mxr_device_info_t &out) const {
        return mxr_device(h_, uid, &out);
    }
    mxr_result_t bay(BayUid bay, mxr_bay_info_t &out) const {
        return mxr_bay(h_, bay, &out);
    }
    mxr_result_t bay_signal_details(BayUid bay, mxr_signal_details_t &out) const {
        return mxr_bay_signal_details(h_, bay, &out);
    }
    mxr_result_t bay_audio_details(BayUid bay, mxr_audio_details_t &out) const {
        return mxr_bay_audio_details(h_, bay, &out);
    }
    /// The EDID a device last reported, empty until one has arrived.
    std::vector<uint8_t> device_edid(Uid device, bool output) const {
        std::vector<uint8_t> out(MXR_EDID_LEN);
        if (mxr_device_edid(h_, device, output, out.data(), out.size()) != MXR_OK) out.clear();
        return out;
    }
    mxr_result_t bay_amp_settings(BayUid bay, mxr_amp_zone_settings_t &out) const {
        return mxr_bay_amp_settings(h_, bay, &out);
    }
    mxr_result_t v2ip_stats(Uid device, mxr_v2ip_stats_t &out) const {
        return mxr_v2ip_stats(h_, device, &out);
    }
    mxr_result_t v2ip_details(Uid device, mxr_v2ip_details_t &out) const {
        return mxr_v2ip_details(h_, device, &out);
    }
    mxr_result_t v2ip_sink(Uid device, mxr_v2ip_sink_t &out) const {
        return mxr_v2ip_sink(h_, device, &out);
    }
    mxr_result_t v2ip_tiling(Uid device, mxr_tiling_config_t &out) const {
        return mxr_v2ip_tiling(h_, device, &out);
    }
    mxr_result_t multiviewer_status(Uid device, mxr_multiviewer_status_t &out) const {
        return mxr_multiviewer_status(h_, device, &out);
    }
    mxr_result_t dolby_settings(Uid device, mxr_dolby_settings_t &out) const {
        return mxr_dolby_settings(h_, device, &out);
    }
    mxr_result_t pdu_state(Uid device, mxr_pdu_state_t &out) const {
        return mxr_pdu_state(h_, device, &out);
    }
    mxr_result_t rc_settings(Uid device, mxr_rc_settings_t &out) const {
        return mxr_rc_settings(h_, device, &out);
    }
    mxr_result_t device_by_serial(const char *serial, Uid &out) const {
        return mxr_device_by_serial(h_, serial, &out.raw);
    }
    mxr_result_t resolve_device(const char *name, Uid &out) const {
        return mxr_resolve_device(h_, name, &out.raw);
    }
    mxr_result_t bay_by_name(Uid device, const char *port_name, BayUid &out) const {
        return mxr_bay_by_name(h_, device, port_name, &out.raw);
    }
    mxr_result_t bay_by_stream_ip(const char *ip, bool audio, BayUid &out) const {
        return mxr_bay_by_stream_ip(h_, ip, audio, &out.raw);
    }
    mxr_result_t remote_update_config(const char *local_ip, bool broadcast) const {
        return mxr_remote_update_config(h_, local_ip, broadcast);
    }
    mxr_result_t discover() const {
        return mxr_discover(h_);
    }
    /// How many frames from other senders have parsed since `start()`.
    ///
    /// No device found and a count that is climbing says the traffic is
    /// arriving on an interface this client cannot get answers back from.
    uint64_t frames_received() const {
        uint64_t out = 0;
        mxr_frames_received(h_, &out);
        return out;
    }

private:
    explicit Remote(mxr_remote_t *handle) : h_(handle) {}

    /// Cuts a sized-then-filled buffer down to what was written.
    ///
    /// The list can grow between the call that sizes it and the call that
    /// fills it, so the second answer is a count of what was written only up
    /// to the room there was for it.
    template <typename T>
    static void trim(std::vector<T> &items, size_t written) {
        if (written < items.size()) items.resize(written);
    }

    mxr_remote_t *h_ = nullptr;
};

} // namespace mxr

#endif /* MX_REMOTE_HPP */
