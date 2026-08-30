// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

/*
 * Discovers MX Remote devices and prints what they report.
 *
 * The shape here is the one most programs want: two callbacks that say only
 * which device or bay moved, and a snapshot read back for the detail. Both run
 * on the library's receive thread, so what they do is kept short.
 *
 * Usage: discover [interface-address]
 */

#include "mx_remote.h"

#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#define MAX_DEVICES 64

static volatile sig_atomic_t running = 1;

static void stop(int signum) {
    (void)signum;
    running = 0;
}

/*
 * What the callbacks are given back. The client cannot be its own userdata,
 * because it does not exist when its table is handed over, so the handle
 * reaches them through a struct that does.
 */
struct app {
    mxr_remote_t *remote;
};

static void print_device(const mxr_remote_t *remote, mxr_uid_t uid) {
    mxr_device_info_t info;
    char text[MXR_UID_STRING_LEN];

    if (mxr_device(remote, uid, &info) != MXR_OK) return;
    mxr_uid_to_string(uid, text, sizeof text);
    printf("  %s  %-16s %-12s %-16s protocol 0x%02x, %zu bays%s\n", text, info.model,
           info.serial, info.name, info.supported_protocol, info.bay_count,
           info.online ? "" : " (offline)");
}

static void on_device_update(void *userdata, mxr_uid_t device) {
    print_device(((struct app *)userdata)->remote, device);
}

static void on_bay_update(void *userdata, mxr_bay_uid_t bay) {
    mxr_bay_info_t info;

    if (mxr_bay(((struct app *)userdata)->remote, bay, &info) != MXR_OK) return;
    printf("    bay %u %-16s %s\n", bay.port, info.user_name,
           info.signal_detected == MXR_TRUE ? "signal" : "no signal");
}

int main(int argc, char **argv) {
    struct app app;
    mxr_config_t config;
    mxr_callbacks_t callbacks;
    mxr_uid_t devices[MAX_DEVICES];
    size_t count, shown, i;
    char ip[MXR_IP_STRING_LEN];
    uint16_t port;

    /* Zeroing asks for every default; only what differs is filled in. */
    memset(&app, 0, sizeof app);
    memset(&config, 0, sizeof config);
    memset(&callbacks, 0, sizeof callbacks);
    config.name = "discover";
    if (argc > 1) config.local_ip = argv[1];

    callbacks.on_device_update = on_device_update;
    callbacks.on_bay_update = on_bay_update;

    app.remote = mxr_remote_new(&config, &callbacks, &app);
    if (app.remote == NULL) {
        fprintf(stderr, "could not create the client: %s\n", mxr_last_error());
        return 1;
    }

    /* Nothing calls back before this, so app.remote is set in time. */
    if (mxr_remote_start(app.remote) != MXR_OK) {
        fprintf(stderr, "could not start: %s\n", mxr_last_error());
        mxr_remote_free(app.remote);
        return 1;
    }

    if (mxr_remote_target(app.remote, ip, sizeof ip, &port) == MXR_OK)
        printf("listening, sending to %s:%u. Ctrl-C to stop.\n", ip, port);

    signal(SIGINT, stop);
    signal(SIGTERM, stop);
    while (running) sleep(1);

    count = mxr_devices(app.remote, devices, MAX_DEVICES);
    shown = count < MAX_DEVICES ? count : MAX_DEVICES;
    printf("\n%zu device(s):\n", count);
    for (i = 0; i < shown; i++) print_device(app.remote, devices[i]);

    mxr_remote_free(app.remote);
    return 0;
}
