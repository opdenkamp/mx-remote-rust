// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

/*
 * Discovers MX Remote devices and prints what they report.
 *
 * The same program as examples/c/discover.c, through the C++ header: the
 * handle is released by leaving main, and the callbacks are virtual methods on
 * an object that already holds what they need.
 *
 * Usage: discover [interface-address]
 */

#include "mx_remote.hpp"

#include <atomic>
#include <chrono>
#include <csignal>
#include <cstdio>
#include <thread>

namespace {

std::atomic<bool> running(true);

void stop(int) { running = false; }

class Printer : public mxr::Handler {
public:
    /// The client the snapshots are read from. Set after it is created, which
    /// is in time because nothing calls back before it is started.
    mxr::Remote *remote = nullptr;

    void on_device_update(mxr::Uid device) override {
        mxr_device_info_t info;
        if (remote == nullptr || mxr_device(remote->get(), device, &info) != MXR_OK) return;
        std::printf("  %s  %-16s %-12s %-16s protocol 0x%02x, %zu bays%s\n",
                    device.str().c_str(), info.model, info.serial, info.name,
                    info.supported_protocol, info.bay_count, info.online ? "" : " (offline)");
    }

    void on_bay_update(mxr::BayUid bay) override {
        mxr_bay_info_t info;
        if (remote == nullptr || mxr_bay(remote->get(), bay, &info) != MXR_OK) return;
        std::printf("    bay %u %-16s %s\n", bay.port(), info.user_name,
                    info.signal_detected == MXR_TRUE ? "signal" : "no signal");
    }
};

} // namespace

int main(int argc, char **argv) {
    Printer printer;
    mxr_config_t config{};
    config.name = "discover";
    if (argc > 1) config.local_ip = argv[1];

    mxr::Remote remote = mxr::Remote::open(&config, &printer);
    if (!remote) {
        std::fprintf(stderr, "could not create the client: %s\n", mxr::last_error().c_str());
        return 1;
    }
    printer.remote = &remote;

    if (remote.start() != MXR_OK) {
        std::fprintf(stderr, "could not start: %s\n", mxr::last_error().c_str());
        return 1;
    }

    std::string ip;
    uint16_t port = 0;
    if (remote.target(ip, port))
        std::printf("listening, sending to %s:%u. Ctrl-C to stop.\n", ip.c_str(), port);

    std::signal(SIGINT, stop);
    std::signal(SIGTERM, stop);
    while (running) std::this_thread::sleep_for(std::chrono::seconds(1));

    std::vector<mxr::Uid> devices = remote.devices();
    std::printf("\n%zu device(s):\n", devices.size());
    for (std::vector<mxr::Uid>::const_iterator it = devices.begin(); it != devices.end(); ++it)
        printer.on_device_update(*it);

    /* remote closes and frees itself here, before printer goes out of scope. */
    return 0;
}
