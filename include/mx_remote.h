// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions


#ifndef MX_REMOTE_H
#define MX_REMOTE_H

/*
 * Generated from the mx-remote-ffi crate by cbindgen. Do not edit: every
 * change belongs in the Rust source, and this file is regenerated and diffed
 * in CI.
 */


#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * Bytes a [`mxr_uid_t`] needs when written as text, the terminator included.
 */
#define MXR_UID_STRING_LEN 36

/**
 * Receives infrared.
 */
#define MXR_FEATURE_IR_RX (1 << 0)

/**
 * Transmits infrared.
 */
#define MXR_FEATURE_IR_TX (1 << 1)

/**
 * Speaks CEC.
 */
#define MXR_FEATURE_CEC (1 << 2)

/**
 * Acts as a V2IP stream source.
 */
#define MXR_FEATURE_V2IP_SOURCE (1 << 3)

/**
 * Acts as a V2IP stream sink.
 */
#define MXR_FEATURE_V2IP_SINK (1 << 4)

/**
 * Routes video.
 */
#define MXR_FEATURE_VIDEO_ROUTING (1 << 5)

/**
 * Routes audio.
 */
#define MXR_FEATURE_AUDIO_ROUTING (1 << 6)

/**
 * Controls volume.
 */
#define MXR_FEATURE_VOLUME_CONTROL (1 << 7)

/**
 * Supports audio return.
 */
#define MXR_FEATURE_AUDIO_RETURN (1 << 8)

/**
 * Passes remote-control commands through.
 */
#define MXR_FEATURE_REMOTE_CONTROL (1 << 9)

/**
 * Installer setup has been completed.
 */
#define MXR_FEATURE_SETUP_COMPLETED (1 << 10)

/**
 * Is the master of its mesh.
 */
#define MXR_FEATURE_MESH_MASTER (1 << 11)

/**
 * Has a notification pending.
 */
#define MXR_FEATURE_STATUS_NOTIFY (1 << 12)

/**
 * Has a warning pending.
 */
#define MXR_FEATURE_STATUS_WARNING (1 << 13)

/**
 * Has an error pending.
 */
#define MXR_FEATURE_STATUS_ERROR (1 << 14)

/**
 * Is about to reboot.
 */
#define MXR_FEATURE_STATUS_REBOOT (1 << 15)

/**
 * Is a member of a mesh.
 */
#define MXR_FEATURE_MESH_MEMBER (1 << 16)

/**
 * Is an audio amplifier.
 */
#define MXR_FEATURE_AUDIO_AMPLIFIER (1 << 17)

/**
 * Is still booting.
 */
#define MXR_FEATURE_BOOTING (1 << 18)

/**
 * Is a management client rather than a device.
 */
#define MXR_FEATURE_MANAGER (1 << 19)

/**
 * Is in power-save mode.
 */
#define MXR_FEATURE_STATUS_POWER_SAVE (1 << 20)

/**
 * Supports meshing.
 */
#define MXR_FEATURE_MESH (1 << 21)

/**
 * Is a multiviewer.
 */
#define MXR_FEATURE_MULTIVIEWER (1 << 22)

/**
 * Has crashed since it last booted.
 */
#define MXR_FEATURE_STATUS_CRASHED (1 << 23)

/**
 * Supports video walls.
 */
#define MXR_FEATURE_VIDEO_WALL (1 << 24)

/**
 * Initialises the configuration it broadcasts.
 *
 * Firmware without this bit sends a device configuration built over
 * uninitialised memory, so fields it did not mean to write carry junk.
 */
#define MXR_FEATURE_CONFIG_INITIALISED (1 << 25)

/**
 * Set while the device is in its boot loader.
 */
#define MXR_FEATURE_BOOT_BIT (1 << 31)

/**
 * HDMI output.
 */
#define MXR_BAY_HDMI_OUT (1 << 0)

/**
 * HDMI input.
 */
#define MXR_BAY_HDMI_IN (1 << 1)

/**
 * Digital audio output.
 */
#define MXR_BAY_AUDIO_DIG_OUT (1 << 2)

/**
 * Digital audio input.
 */
#define MXR_BAY_AUDIO_DIG_IN (1 << 3)

/**
 * Analogue audio output.
 */
#define MXR_BAY_AUDIO_ANA_OUT (1 << 4)

/**
 * Analogue audio input.
 */
#define MXR_BAY_AUDIO_ANA_IN (1 << 5)

/**
 * Infrared input.
 */
#define MXR_BAY_IR_IN (1 << 6)

/**
 * Infrared output.
 */
#define MXR_BAY_IR_OUT (1 << 7)

/**
 * Amplified audio output.
 */
#define MXR_BAY_AUDIO_AMP_OUT (1 << 8)

/**
 * Remote-control output.
 */
#define MXR_BAY_RC_OUT (1 << 9)

/**
 * Remote-control input.
 */
#define MXR_BAY_RC_IN (1 << 10)

/**
 * Dolby decoding.
 */
#define MXR_BAY_DOLBY (1 << 11)

/**
 * Switches itself off when idle.
 */
#define MXR_BAY_AUTO_OFF (1 << 12)

/**
 * Is a remote V2IP source.
 */
#define MXR_BAY_V2IP_SOURCE_REMOTE (1 << 13)

/**
 * Is a remote V2IP sink.
 */
#define MXR_BAY_V2IP_SINK_REMOTE (1 << 14)

/**
 * Is a local V2IP source.
 */
#define MXR_BAY_V2IP_SOURCE_LOCAL (1 << 15)

/**
 * Is a local V2IP sink.
 */
#define MXR_BAY_V2IP_SINK_LOCAL (1 << 16)

/**
 * The bay reports a fault.
 */
#define MXR_BAY_STATUS_FAULT (1 << 0)

/**
 * The bay is hidden from the user interface.
 */
#define MXR_BAY_STATUS_HIDDEN (1 << 1)

/**
 * The bay has power.
 */
#define MXR_BAY_STATUS_POWERED (1 << 2)

/**
 * A signal is present.
 */
#define MXR_BAY_STATUS_SIGNAL_DETECTED (1 << 3)

/**
 * Hot-plug detect is asserted.
 */
#define MXR_BAY_STATUS_HPD_DETECTED (1 << 4)

/**
 * The signal is scrambled.
 */
#define MXR_BAY_STATUS_SIGNAL_SCRAMBLE (1 << 5)

/**
 * An HDBaseT link is up.
 */
#define MXR_BAY_STATUS_HDBT_CONNECTED (1 << 6)

/**
 * A CEC device answered.
 */
#define MXR_BAY_STATUS_CEC_DETECTED (1 << 7)

/**
 * The attached device was powered on.
 */
#define MXR_BAY_STATUS_POWERED_ON (1 << 8)

/**
 * The attached device was powered off.
 */
#define MXR_BAY_STATUS_POWERED_OFF (1 << 9)

/**
 * Audio return over HDMI is active.
 */
#define MXR_BAY_STATUS_AUDIO_ARC_HDMI (1 << 10)

/**
 * Audio return over optical is active.
 */
#define MXR_BAY_STATUS_AUDIO_ARC_OPTIC (1 << 11)

/**
 * Audio return over analogue is active.
 */
#define MXR_BAY_STATUS_AUDIO_ARC_ANALOG (1 << 12)

/**
 * The bay is offline.
 */
#define MXR_BAY_STATUS_OFFLINE (1 << 13)

/**
 * The V2IP decoder is disabled.
 */
#define MXR_BAY_STATUS_DECODER_DISABLE (1 << 14)

/**
 * The V2IP encoder is disabled.
 */
#define MXR_BAY_STATUS_ENCODER_DISABLE (1 << 15)

/**
 * CEC is switched off for this bay.
 */
#define MXR_BAY_STATUS_CEC_DISABLED (1 << 20)

/**
 * The V2IP encoder reports an error.
 */
#define MXR_BAY_STATUS_ENCODER_ERROR (1 << 21)

/**
 * Accepts audio.
 */
#define MXR_AUDIO_INPUT (1 << 0)

/**
 * Produces audio.
 */
#define MXR_AUDIO_OUTPUT (1 << 1)

/**
 * Sends a V2IP audio stream.
 */
#define MXR_AUDIO_V2IP_TX (1 << 2)

/**
 * Receives a V2IP audio stream.
 */
#define MXR_AUDIO_V2IP_RX (1 << 3)

/**
 * Carries HDMI audio.
 */
#define MXR_AUDIO_HDMI (1 << 4)

/**
 * Is an analogue RCA connector.
 */
#define MXR_AUDIO_RCA (1 << 5)

/**
 * Is an S/PDIF connector.
 */
#define MXR_AUDIO_SPDIF (1 << 6)

/**
 * Drives a trigger output.
 */
#define MXR_AUDIO_TRIGGER (1 << 7)

/**
 * Can be muted.
 */
#define MXR_AUDIO_MUTE (1 << 8)

/**
 * Can be routed to as an input.
 */
#define MXR_AUDIO_ROUTE_INPUT (1 << 9)

/**
 * Can be routed from as an output.
 */
#define MXR_AUDIO_ROUTE_OUTPUT (1 << 10)

/**
 * Accepts "no input" as a route.
 */
#define MXR_AUDIO_ROUTE_IN_NONE (1 << 11)

/**
 * Is an amplifier output.
 */
#define MXR_AUDIO_AMP_OUTPUT (1 << 12)

/**
 * Has a volume control.
 */
#define MXR_AUDIO_VOLUME_CONTROL (1 << 13)

/**
 * Has a gain control.
 */
#define MXR_AUDIO_GAIN_CONTROL (1 << 14)

/**
 * Digit 0.
 */
#define MXR_KEY_NUM0 0

/**
 * Digit 1.
 */
#define MXR_KEY_NUM1 1

/**
 * Digit 2.
 */
#define MXR_KEY_NUM2 2

/**
 * Digit 3.
 */
#define MXR_KEY_NUM3 3

/**
 * Digit 4.
 */
#define MXR_KEY_NUM4 4

/**
 * Digit 5.
 */
#define MXR_KEY_NUM5 5

/**
 * Digit 6.
 */
#define MXR_KEY_NUM6 6

/**
 * Digit 7.
 */
#define MXR_KEY_NUM7 7

/**
 * Digit 8.
 */
#define MXR_KEY_NUM8 8

/**
 * Digit 9.
 */
#define MXR_KEY_NUM9 9

/**
 * Confirm the highlighted item.
 */
#define MXR_KEY_SELECT 10

/**
 * Go back one step.
 */
#define MXR_KEY_BACK 11

/**
 * Navigate up.
 */
#define MXR_KEY_UP 12

/**
 * Navigate down.
 */
#define MXR_KEY_DOWN 13

/**
 * Navigate left.
 */
#define MXR_KEY_LEFT 14

/**
 * Navigate right.
 */
#define MXR_KEY_RIGHT 15

/**
 * Open the main menu.
 */
#define MXR_KEY_MENU 16

/**
 * Open the content menu.
 */
#define MXR_KEY_CONTENT_MENU 17

/**
 * Next channel.
 */
#define MXR_KEY_CHANNEL_UP 18

/**
 * Previous channel.
 */
#define MXR_KEY_CHANNEL_DOWN 19

/**
 * Start playback.
 */
#define MXR_KEY_PLAY 20

/**
 * Pause playback.
 */
#define MXR_KEY_PAUSE 21

/**
 * Stop playback.
 */
#define MXR_KEY_STOP 22

/**
 * Start recording.
 */
#define MXR_KEY_RECORD 23

/**
 * Fast forward.
 */
#define MXR_KEY_FAST_FORWARD 24

/**
 * Rewind.
 */
#define MXR_KEY_REWIND 25

/**
 * Red colour key.
 */
#define MXR_KEY_RED 26

/**
 * Green colour key.
 */
#define MXR_KEY_GREEN 27

/**
 * Yellow colour key.
 */
#define MXR_KEY_YELLOW 28

/**
 * Blue colour key.
 */
#define MXR_KEY_BLUE 29

/**
 * Open help.
 */
#define MXR_KEY_HELP 30

/**
 * Show information.
 */
#define MXR_KEY_INFORMATION 31

/**
 * Open teletext.
 */
#define MXR_KEY_TEXT 32

/**
 * Open the programme guide.
 */
#define MXR_KEY_GUIDE 33

/**
 * Open video on demand.
 */
#define MXR_KEY_VIDEO_ON_DEMAND 34

/**
 * Return to the previous channel.
 */
#define MXR_KEY_PREVIOUS_CHANNEL 80

/**
 * Toggle 3D mode.
 */
#define MXR_KEY_MODE_3D 81

/**
 * Toggle subtitles.
 */
#define MXR_KEY_SUBTITLE 82

/**
 * Select an audio track.
 */
#define MXR_KEY_SOUND_SELECT 83

/**
 * Select an input.
 */
#define MXR_KEY_INPUT_SELECT 84

/**
 * Eject the medium.
 */
#define MXR_KEY_EJECT 85

/**
 * Next chapter.
 */
#define MXR_KEY_NEXT_CHAPTER 86

/**
 * Previous chapter.
 */
#define MXR_KEY_PREV_CHAPTER 87

/**
 * Open interactive services.
 */
#define MXR_KEY_INTERACTIVE 128

/**
 * Open search.
 */
#define MXR_KEY_SEARCH 129

/**
 * Sky home key.
 */
#define MXR_KEY_SKY 130

/**
 * Base of the range carrying a raw CEC user-control code.
 */
#define MXR_KEY_CUSTOM_CEC 1280

/**
 * Base of the range carrying a raw Sky key code.
 */
#define MXR_KEY_CUSTOM_SKY 2048

/**
 * The multiviewer reports no window layout.
 */
#define MXR_MV_VIEW_MODE_UNKNOWN 0

/**
 * One full-screen window.
 */
#define MXR_MV_VIEW_MODE_SINGLE 1

/**
 * Picture in picture.
 */
#define MXR_MV_VIEW_MODE_PIP 2

/**
 * Two windows, large.
 */
#define MXR_MV_VIEW_MODE_TWO_SCREEN_LARGE 3

/**
 * Two windows, small.
 */
#define MXR_MV_VIEW_MODE_TWO_SCREEN_SMALL 4

/**
 * Three windows, large.
 */
#define MXR_MV_VIEW_MODE_THREE_SCREEN_LARGE 5

/**
 * Three windows, small.
 */
#define MXR_MV_VIEW_MODE_THREE_SCREEN_SMALL 6

/**
 * Four windows, equal size.
 */
#define MXR_MV_VIEW_MODE_FOUR_SCREEN_EQUAL 7

/**
 * Four windows, small.
 */
#define MXR_MV_VIEW_MODE_FOUR_SCREEN_SMALL 8

/**
 * The multiviewer reports no picture-in-picture position.
 */
#define MXR_MV_PIP_POSITION_UNKNOWN 0

/**
 * Top left.
 */
#define MXR_MV_PIP_POSITION_LEFT_TOP 1

/**
 * Bottom left.
 */
#define MXR_MV_PIP_POSITION_LEFT_BOTTOM 2

/**
 * Top right.
 */
#define MXR_MV_PIP_POSITION_RIGHT_TOP 3

/**
 * Bottom right.
 */
#define MXR_MV_PIP_POSITION_RIGHT_BOTTOM 4

/**
 * The multiviewer reports no picture-in-picture size.
 */
#define MXR_MV_PIP_SIZE_UNKNOWN 0

/**
 * Small.
 */
#define MXR_MV_PIP_SIZE_SMALL 1

/**
 * Medium.
 */
#define MXR_MV_PIP_SIZE_MEDIUM 2

/**
 * Large.
 */
#define MXR_MV_PIP_SIZE_LARGE 3

/**
 * The multiviewer reports no output mode.
 */
#define MXR_MV_OUTPUT_UNKNOWN 0

/**
 * 4096x2160p60.
 */
#define MXR_MV_OUTPUT_DCI4K_P60 1

/**
 * 4096x2160p50.
 */
#define MXR_MV_OUTPUT_DCI4K_P50 2

/**
 * 3840x2160p60.
 */
#define MXR_MV_OUTPUT_UHD_P60 3

/**
 * 3840x2160p50.
 */
#define MXR_MV_OUTPUT_UHD_P50 4

/**
 * 3840x2160p30.
 */
#define MXR_MV_OUTPUT_UHD_P30 5

/**
 * 3840x2160p25.
 */
#define MXR_MV_OUTPUT_UHD_P25 6

/**
 * 1920x1200p60, reduced blanking.
 */
#define MXR_MV_OUTPUT_WUXGA_P60_RB 7

/**
 * 1920x1080p60.
 */
#define MXR_MV_OUTPUT_HD1080_P60 8

/**
 * 1920x1080p50.
 */
#define MXR_MV_OUTPUT_HD1080_P50 9

/**
 * 1360x768p60.
 */
#define MXR_MV_OUTPUT_WXGA_P60 10

/**
 * 1280x800p60.
 */
#define MXR_MV_OUTPUT_WXGA800_P60 11

/**
 * 1280x720p60.
 */
#define MXR_MV_OUTPUT_HD720_P60 12

/**
 * 1280x720p50.
 */
#define MXR_MV_OUTPUT_HD720_P50 13

/**
 * 1024x768p60.
 */
#define MXR_MV_OUTPUT_XGA_P60 14

/**
 * The multiviewer reports no HDCP mode.
 */
#define MXR_MV_HDCP_UNKNOWN 0

/**
 * HDCP 1.4.
 */
#define MXR_MV_HDCP_V14 1

/**
 * HDCP 2.2.
 */
#define MXR_MV_HDCP_V22 2

/**
 * Content protection off.
 */
#define MXR_MV_HDCP_OFF 3

/**
 * The multiviewer reports no EDID template.
 */
#define MXR_MV_EDID_UNKNOWN 0

/**
 * 4K2K60 4:4:4, stereo 2.0.
 */
#define MXR_MV_EDID_4K2K60_444_STEREO 1

/**
 * 4K2K60 4:4:4, Dolby/DTS 5.1.
 */
#define MXR_MV_EDID_4K2K60_444_DOLBY_DTS_51 2

/**
 * 4K2K60 4:4:4, HD audio 7.1.
 */
#define MXR_MV_EDID_4K2K60_444_HD_AUDIO_71 3

/**
 * 4K2K30 4:4:4, stereo 2.0.
 */
#define MXR_MV_EDID_4K2K30_444_STEREO 4

/**
 * 4K2K30 4:4:4, Dolby/DTS 5.1.
 */
#define MXR_MV_EDID_4K2K30_444_DOLBY_DTS_51 5

/**
 * 4K2K30 4:4:4, HD audio 7.1.
 */
#define MXR_MV_EDID_4K2K30_444_HD_AUDIO_71 6

/**
 * 1080p, stereo 2.0.
 */
#define MXR_MV_EDID_1080P_STEREO 7

/**
 * 1080p, Dolby/DTS 5.1.
 */
#define MXR_MV_EDID_1080P_DOLBY_DTS_51 8

/**
 * 1080p, HD audio 7.1.
 */
#define MXR_MV_EDID_1080P_HD_AUDIO_71 9

/**
 * 1920x1200, stereo 2.0.
 */
#define MXR_MV_EDID_1920X1200_STEREO 10

/**
 * 1680x1050, stereo 2.0.
 */
#define MXR_MV_EDID_1680X1050_STEREO 11

/**
 * 1600x1200, stereo 2.0.
 */
#define MXR_MV_EDID_1600X1200_STEREO 12

/**
 * 1440x900, stereo 2.0.
 */
#define MXR_MV_EDID_1440X900_STEREO 13

/**
 * 1360x768, stereo 2.0.
 */
#define MXR_MV_EDID_1360X768_STEREO 14

/**
 * 1280x1024, stereo 2.0.
 */
#define MXR_MV_EDID_1280X1024_STEREO 15

/**
 * 1024x768, stereo 2.0.
 */
#define MXR_MV_EDID_1024X768_STEREO 16

/**
 * 720p, stereo 2.0.
 */
#define MXR_MV_EDID_720P_STEREO 17

/**
 * Whatever the display connected to the HDMI output presents. The template a
 * multiviewer leaves the factory with.
 */
#define MXR_MV_EDID_COPY_OUTPUT 18

/**
 * The EDID loaded onto the device.
 */
#define MXR_MV_EDID_CUSTOM 19

/**
 * The multiviewer reports no IT-content mode.
 */
#define MXR_MV_ITC_UNKNOWN 0

/**
 * Video content.
 */
#define MXR_MV_ITC_VIDEO 1

/**
 * PC content.
 */
#define MXR_MV_ITC_PC 2

/**
 * The multiviewer reports no aspect ratio.
 */
#define MXR_MV_ASPECT_UNKNOWN 0

/**
 * Fill the window.
 */
#define MXR_MV_ASPECT_FULL 1

/**
 * 16:9.
 */
#define MXR_MV_ASPECT_RATIO_16_9 2

/**
 * Off.
 */
#define MXR_MV_BOOL_OFF 0

/**
 * On.
 */
#define MXR_MV_BOOL_ON 1

/**
 * The multiviewer reports no value.
 */
#define MXR_MV_BOOL_UNKNOWN 255

/**
 * The multiviewer reports no source.
 */
#define MXR_MV_SOURCE_UNKNOWN 0

/**
 * Input 1.
 */
#define MXR_MV_SOURCE_INPUT_1 1

/**
 * Input 2.
 */
#define MXR_MV_SOURCE_INPUT_2 2

/**
 * Input 3.
 */
#define MXR_MV_SOURCE_INPUT_3 3

/**
 * Input 4.
 */
#define MXR_MV_SOURCE_INPUT_4 4

/**
 * A window's horizontal origin must be a multiple of this.
 */
#define MXR_VIDEO_WALL_POS_ALIGN 64

/**
 * A window's width must be a multiple of this.
 */
#define MXR_VIDEO_WALL_WIDTH_ALIGN 4

/**
 * Neither side of a window may be smaller than this.
 */
#define MXR_VIDEO_WALL_MIN_SIZE 64

/**
 * Bytes a device, bay or port name needs, the terminator included.
 *
 * The wire field is 16 bytes wide. The rest is headroom for the names this
 * library derives rather than reads, which that width does not bound.
 */
#define MXR_NAME_LEN 32

/**
 * Bytes a serial number needs, the terminator included.
 */
#define MXR_SERIAL_LEN 32

/**
 * Bytes a model name needs, the terminator included.
 */
#define MXR_MODEL_LEN 48

/**
 * Bytes a firmware version string needs, the terminator included.
 *
 * The wire field is 128 bytes and is not NUL-terminated when full.
 */
#define MXR_VERSION_LEN 129

/**
 * Bytes a signal description such as `1080p60 444 8` needs, the terminator
 * included.
 */
#define MXR_SIGNAL_TYPE_LEN 48

/**
 * Bytes a device's system-status message needs, the terminator included.
 */
#define MXR_MESSAGE_LEN 128

/**
 * Number of EQ bands an amplifier zone carries.
 *
 * Written as a literal because the generated header needs one, and checked
 * against the core crate's value below so the two cannot drift apart.
 */
#define MXR_AMP_EQ_BANDS 5

/**
 * Bytes in one EDID: a base block and exactly one extension block.
 */
#define MXR_EDID_LEN 256

/**
 * Bytes an IPv4 address needs when written as text, the terminator included.
 */
#define MXR_IP_STRING_LEN 16

/**
 * How many inputs a multiviewer has.
 *
 * Written as a literal because the generated header needs one, and checked
 * against the core crate's value below so the two cannot drift apart.
 */
#define MXR_MULTIVIEWER_INPUTS 4

/**
 * How many pairs a UTP cable diagnostic covers.
 */
#define MXR_UTP_PAIRS 4

/**
 * Set when the frame carries a scaling mode and refresh rate.
 */
#define MXR_SCALING_FLAG_MODE_VALID (1 << 0)

/**
 * Set when the frame carries the scaling options.
 */
#define MXR_SCALING_FLAG_OPTIONS_VALID (1 << 1)

/**
 * Set when the output scales automatically.
 */
#define MXR_SCALING_FLAG_AUTO_SCALING (1 << 7)

/**
 * The audio return channel a bay is carrying.
 */
enum mxr_arc_status_t
#ifdef __cplusplus
  : int32_t
#endif // __cplusplus
 {
  /**
   * No audio is being returned.
   */
  MXR_ARC_INACTIVE = 0,
  /**
   * Returned over HDMI.
   */
  MXR_ARC_HDMI = 1,
  /**
   * Returned over optical.
   */
  MXR_ARC_OPTICAL = 2,
  /**
   * Returned over analogue.
   */
  MXR_ARC_ANALOG = 3,
};
#ifndef __cplusplus
typedef int32_t mxr_arc_status_t;
#endif // __cplusplus

/**
 * The high-level state of a device on the network.
 */
enum mxr_device_status_t
#ifdef __cplusplus
  : int32_t
#endif // __cplusplus
 {
  /**
   * Reachable and reporting.
   */
  MXR_DEVICE_ONLINE = 0,
  /**
   * Has stopped answering.
   */
  MXR_DEVICE_OFFLINE = 1,
  /**
   * Announced a reboot.
   */
  MXR_DEVICE_REBOOTING = 2,
  /**
   * Still coming up.
   */
  MXR_DEVICE_BOOTING = 3,
  /**
   * Present but not participating.
   */
  MXR_DEVICE_INACTIVE = 4,
};
#ifndef __cplusplus
typedef int32_t mxr_device_status_t;
#endif // __cplusplus

/**
 * The CEC power state of whatever is connected to a bay.
 */
enum mxr_power_status_t
#ifdef __cplusplus
  : int32_t
#endif // __cplusplus
 {
  /**
   * The bay has not reported a power state.
   */
  MXR_POWER_UNKNOWN = 0,
  /**
   * Powered on.
   */
  MXR_POWER_ON = 1,
  /**
   * Powered off.
   */
  MXR_POWER_OFF = 2,
};
#ifndef __cplusplus
typedef int32_t mxr_power_status_t;
#endif // __cplusplus

/**
 * How a call ended.
 *
 * Everything but [`mxr_result_t::MXR_OK`] is negative, so `if (rc < 0)` is a
 * complete test and a new code cannot turn a failure into a success.
 */
enum mxr_result_t
#ifdef __cplusplus
  : int32_t
#endif // __cplusplus
 {
  /**
   * The call did what was asked.
   */
  MXR_OK = 0,
  /**
   * A pointer was null, a buffer too small, or a string not UTF-8.
   */
  MXR_ERR_INVALID_ARGUMENT = -1,
  /**
   * No device, bay or source by that name has been heard from.
   *
   * A device reports itself when it feels like it, so this is as likely to
   * mean "not yet" as "never": the same call may succeed later.
   */
  MXR_ERR_NOT_FOUND = -2,
  /**
   * The addressed device speaks a protocol older than the command needs.
   *
   * It would discard the frame without answering, so nothing was sent.
   */
  MXR_ERR_PROTOCOL_TOO_OLD = -3,
  /**
   * The client has no socket, because it was never started or was closed.
   */
  MXR_ERR_NOT_CONNECTED = -4,
  /**
   * The socket write failed, or the socket could not be opened.
   */
  MXR_ERR_IO = -5,
  /**
   * The addressee does not do what was asked of it.
   */
  MXR_ERR_UNSUPPORTED = -6,
  /**
   * The device has not reported something the request is assembled from.
   */
  MXR_ERR_NOT_REPORTED = -7,
  /**
   * A panic was caught at the boundary. The library's state is unknown.
   */
  MXR_ERR_PANIC = -8,
};
#ifndef __cplusplus
typedef int32_t mxr_result_t;
#endif // __cplusplus

/**
 * Which of a V2IP device's streams an address describes.
 */
enum mxr_stream_kind_t
#ifdef __cplusplus
  : int32_t
#endif // __cplusplus
 {
  /**
   * The video stream.
   */
  MXR_STREAM_VIDEO = 0,
  /**
   * The audio stream.
   */
  MXR_STREAM_AUDIO = 1,
  /**
   * The ancillary-data stream.
   */
  MXR_STREAM_ANC = 2,
  /**
   * The audio-return stream.
   */
  MXR_STREAM_ARC = 3,
};
#ifndef __cplusplus
typedef int32_t mxr_stream_kind_t;
#endif // __cplusplus

/**
 * A flag a device may not have reported.
 *
 * Firmware sends only what it has, so "off" and "never said" are different
 * answers and a two-valued flag would have to pick one of them for both.
 */
enum mxr_tribool_t
#ifdef __cplusplus
  : int8_t
#endif // __cplusplus
 {
  /**
   * The device has not reported this.
   */
  MXR_UNKNOWN = -1,
  /**
   * Reported, and false.
   */
  MXR_FALSE = 0,
  /**
   * Reported, and true.
   */
  MXR_TRUE = 1,
};
#ifndef __cplusplus
typedef int8_t mxr_tribool_t;
#endif // __cplusplus

/**
 * A running client. Opaque: everything about it is reached through the
 * functions below.
 */
typedef struct mxr_remote_t mxr_remote_t;

/**
 * The 16-byte identifier of a device on the network.
 *
 * All zero is the empty identifier, which is how the protocol says "no
 * device" - see `mxr_uid_is_zero()`.
 */
typedef struct {
  /**
   * The raw identifier, in wire order.
   */
  uint8_t bytes[16];
} mxr_uid_t;

/**
 * The signal format a device reports, packed into one 16-bit word.
 *
 * Read it with the `mxr_signal_type_*` functions rather than by shifting it
 * directly: the bit depth in it is an index into a table rather than a depth,
 * and two different encodings both mean "nothing configured".
 */
typedef uint16_t mxr_signal_type_t;

/**
 * A single bay: the device it is on, and its port number there.
 */
typedef struct {
  /**
   * The device the bay belongs to.
   */
  mxr_uid_t device;
  /**
   * The bay's port number on that device.
   */
  uint16_t port;
} mxr_bay_uid_t;

/**
 * A stream's sample rate and channel count.
 */
typedef struct {
  /**
   * Sample rate in Hz.
   */
  uint32_t sample_rate;
  /**
   * Channel count.
   */
  uint8_t channels;
} mxr_audio_format_t;

/**
 * One stream of a route the caller assembles.
 */
typedef struct {
  /**
   * The multicast group, as a dotted quad. Null or empty sends the slot
   * zeroed, naming no group for that stream.
   *
   * It is not a way to leave one stream alone. The firmware decides
   * whether a sink has a manual route at all by reading the video and
   * ancillary slots, so an empty one of those disqualifies the whole
   * route rather than preserving anything - see
   * `mxr_select_source_addr()`.
   */
  const char *ip;
  /**
   * The destination UDP port. Zero means the standard port for the stream
   * this slot names.
   */
  uint16_t port;
} mxr_stream_addr_t;

/**
 * The three streams a manual route points a V2IP sink at.
 */
typedef struct {
  /**
   * The video stream, at port 50020 unless the port says otherwise.
   */
  mxr_stream_addr_t video;
  /**
   * The audio stream, at port 50022 unless the port says otherwise.
   */
  mxr_stream_addr_t audio;
  /**
   * The ancillary-data stream, at port 50021 unless the port says
   * otherwise.
   */
  mxr_stream_addr_t anc;
} mxr_v2ip_route_t;

/**
 * A ProAmp8 zone's gain, delay, tone and power settings.
 *
 * Gains and volume limits run 0-248 in 0.5dB steps, with 200 as 0dB. Tone and
 * EQ values are neutral at 128.
 */
typedef struct {
  /**
   * Left channel gain.
   */
  uint8_t gain_left;
  /**
   * Right channel gain.
   */
  uint8_t gain_right;
  /**
   * Lowest volume the zone may be set to.
   */
  uint8_t volume_min;
  /**
   * Highest volume the zone may be set to.
   */
  uint8_t volume_max;
  /**
   * Bass tone control.
   */
  uint8_t bass;
  /**
   * Treble tone control.
   */
  uint8_t treble;
  /**
   * 0 = normal, 1 = bridged.
   */
  uint8_t bridged;
  /**
   * Power-on mode.
   */
  uint8_t power_mode;
  /**
   * Signal level that switches the zone on automatically.
   */
  uint8_t power_level;
  /**
   * Left channel delay, in 1/48000 second increments.
   */
  uint32_t delay_left;
  /**
   * Right channel delay, in 1/48000 second increments.
   */
  uint32_t delay_right;
  /**
   * Idle time before the zone powers down, in seconds.
   */
  uint32_t power_timeout;
  /**
   * Left channel EQ, from 100Hz to 10KHz.
   */
  uint8_t eq_left[MXR_AMP_EQ_BANDS];
  /**
   * Right channel EQ, from 100Hz to 10KHz.
   */
  uint8_t eq_right[MXR_AMP_EQ_BANDS];
} mxr_amp_zone_settings_t;

/**
 * Where a video-wall sink's window sits, and the picture it was measured
 * against.
 *
 * `pos_x` must be a multiple of `MXR_VIDEO_WALL_POS_ALIGN`, `width` a
 * multiple of `MXR_VIDEO_WALL_WIDTH_ALIGN`, both sides at least
 * `MXR_VIDEO_WALL_MIN_SIZE`, and the window must fit inside the raster it
 * names. `pos_y` and `height` have no alignment rule. A zero `width` or
 * `height` clears the wall and is checked against none of this.
 */
typedef struct {
  /**
   * Window origin, horizontal.
   */
  uint16_t pos_x;
  /**
   * Window origin, vertical.
   */
  uint16_t pos_y;
  /**
   * Window width, or zero to clear the wall.
   */
  uint16_t width;
  /**
   * Window height, or zero to clear the wall.
   */
  uint16_t height;
  /**
   * Active picture width the window was measured against.
   */
  uint16_t raster_w;
  /**
   * Active picture height the window was measured against.
   */
  uint16_t raster_h;
} mxr_video_wall_window_t;

/**
 * What a device is, and what it is doing.
 */
typedef struct {
  /**
   * The device's identifier.
   */
  mxr_uid_t uid;
  /**
   * The name the device advertises.
   */
  char name[MXR_NAME_LEN];
  /**
   * Serial number.
   */
  char serial[MXR_SERIAL_LEN];
  /**
   * A friendly model name, derived from the advertised name and the bays
   * the device reports.
   */
  char model[MXR_MODEL_LEN];
  /**
   * Firmware version string from the hello frame.
   */
  char version[MXR_VERSION_LEN];
  /**
   * The highest protocol version the device can decode, or zero before it
   * has said. A command above it is refused rather than sent.
   */
  uint16_t supported_protocol;
  /**
   * What the device says it can do, as `MXR_FEATURE_*` bits.
   */
  uint32_t features;
  /**
   * The address the device was last heard from, empty when never.
   */
  char address[MXR_IP_STRING_LEN];
  /**
   * Online, offline, booting or rebooting.
   */
  mxr_device_status_t status;
  /**
   * Whether the device has been heard from recently enough to count as
   * present.
   */
  bool online;
  /**
   * Whether every part of the device's configuration has arrived.
   */
  bool configuration_complete;
  /**
   * Whether the device's firmware initialises the configuration it
   * broadcasts.
   *
   * Firmware without it builds some frames over uninitialised stack, so
   * those fields carry noise rather than values: the scaling flags and,
   * behind a spuriously set valid bit, the scaling mode and refresh; bay
   * zero's addresses in the V2IP sources frame; and the padding beside the
   * remote-control target.
   */
  bool config_initialised;
  /**
   * The mesh master this device follows, zero when it is in no mesh.
   */
  mxr_uid_t mesh_master;
  /**
   * How many HDBaseT outputs this model has.
   */
  uint8_t hdbt_outputs;
  /**
   * Whether installation was marked complete.
   */
  mxr_tribool_t setup_done;
  /**
   * The installer identifier, or -1 when the device has not reported one.
   */
  int32_t installer_id;
  /**
   * Whether the device has reported a status about itself.
   */
  bool has_system_status;
  /**
   * The status code, meaningful only when `has_system_status` is set.
   */
  uint16_t system_status;
  /**
   * The status message, empty when there is none.
   */
  char system_message[MXR_MESSAGE_LEN];
  /**
   * How many temperatures `mxr_device_temperatures()` would return.
   */
  size_t temperature_count;
  /**
   * How many bays `mxr_device_bays()` would return.
   */
  size_t bay_count;
} mxr_device_info_t;

/**
 * What a bay is, and what is connected to it.
 */
typedef struct {
  /**
   * How the bay is addressed.
   */
  mxr_bay_uid_t uid;
  /**
   * The name the device gives the port, such as `Output 1`.
   */
  char port_name[MXR_NAME_LEN];
  /**
   * The name the installer gave the bay, falling back to the port name.
   */
  char user_name[MXR_NAME_LEN];
  /**
   * The bay number the device's own API and topology use, which is not the
   * port number this library addresses it by.
   */
  uint8_t bay_num;
  /**
   * What the bay is wired for, as `MXR_BAY_*` bits.
   */
  uint32_t features;
  /**
   * Whether the bay takes a signal in.
   */
  bool is_input;
  /**
   * Whether the bay puts a signal out.
   */
  bool is_output;
  /**
   * Whether the bay carries audio and no video.
   */
  bool is_audio;
  /**
   * Whether the bay can decode Dolby.
   */
  bool has_dolby;
  /**
   * Whether the bay is on this device rather than reached through the mesh.
   */
  bool is_local;
  /**
   * The bay routed to this one for video, zero when unrouted.
   */
  mxr_bay_uid_t video_source;
  /**
   * The bay routed to this one for audio, which follows the video source
   * until the bay is told otherwise. Zero when unrouted.
   */
  mxr_bay_uid_t audio_source;
  /**
   * Power state of what is connected.
   */
  mxr_power_status_t power_status;
  /**
   * Whether the bay is hidden from the installation's user interface.
   */
  mxr_tribool_t hidden;
  /**
   * Whether the device reports the bay as faulty.
   */
  mxr_tribool_t faulty;
  /**
   * Whether the bay is delivering power over the link.
   */
  mxr_tribool_t poe_powered;
  /**
   * Whether an HDBaseT link is up.
   */
  mxr_tribool_t hdbt_connected;
  /**
   * Whether a signal is present.
   */
  mxr_tribool_t signal_detected;
  /**
   * Whether hot-plug detect is asserted.
   */
  mxr_tribool_t hpd_detected;
  /**
   * Whether a CEC device answered.
   */
  mxr_tribool_t cec_detected;
  /**
   * Whether the bay's encoder is switched off.
   */
  mxr_tribool_t encoder_disabled;
  /**
   * Whether the bay's decoder is switched off.
   */
  mxr_tribool_t decoder_disabled;
  /**
   * The signal as the device describes it, empty when it has not.
   */
  char signal_type[MXR_SIGNAL_TYPE_LEN];
  /**
   * The signal format the device reports, packed. Read it with the
   * `mxr_signal_type_*` functions.
   */
  mxr_signal_type_t signal_mode;
  /**
   * Whether audio return is active, and over which connector.
   */
  mxr_arc_status_t arc;
  /**
   * Whether the bay has reported a volume.
   */
  bool has_volume;
  /**
   * The combined left/right volume percentage.
   */
  uint8_t volume;
  /**
   * Whether either channel is muted.
   */
  mxr_tribool_t muted;
  /**
   * Whether the bay has reported a remote-control type.
   */
  bool has_rc_type;
  /**
   * The kind of remote control attached, as the wire value.
   */
  uint8_t rc_type;
  /**
   * Whether the bay has reported an EDID profile.
   */
  bool has_edid_profile;
  /**
   * The EDID profile the bay presents.
   */
  uint16_t edid_profile;
  /**
   * The bay this one mirrors, zero when it mirrors nothing.
   */
  mxr_bay_uid_t mirror;
  /**
   * The audio endpoint this bay feeds, or -1 on a device without them.
   */
  int16_t audio_endpoint;
  /**
   * The bay on another device this one is linked to, zero when it is
   * linked to none or that bay is not yet known.
   *
   * The link is mesh configuration, not a route: it names the bay elsewhere
   * that belongs to this one, such as the amplifier zone carrying a OneIP
   * output's volume. `volume` is already read through it, and
   * `mxr_set_volume()` already writes through it.
   */
  mxr_bay_uid_t linked_bay;
  /**
   * The source device a V2IP bay maps to, zero when it maps to none.
   */
  mxr_uid_t v2ip_uid;
  /**
   * How many devices `mxr_bay_filtered()` would return.
   */
  size_t filtered_count;
} mxr_bay_info_t;

/**
 * The signal a bay measures, beyond the description in
 * [`mxr_bay_info_t::signal_type`].
 */
typedef struct {
  /**
   * Frame rate in Hz, already corrected for a 1000/1001 clock.
   */
  double frame_rate;
  /**
   * TMDS clock rate in Hz.
   */
  uint32_t tmds_clock;
  /**
   * Video clock rate in Hz.
   */
  uint32_t clock_rate;
  /**
   * The bay status word from the report's bay block.
   */
  uint32_t status;
  /**
   * The signal type the bay is scaling to.
   */
  mxr_signal_type_t scaling;
} mxr_signal_details_t;

/**
 * The audio a bay signal report describes.
 */
typedef struct {
  /**
   * How the stream is encoded: 0 unknown, 1 L-PCM, 2 high bit rate.
   */
  uint8_t format;
  /**
   * Channel count.
   */
  uint8_t channels;
  /**
   * Sample rate in Hz.
   */
  uint32_t sample_rate;
  /**
   * Whether the source sent a CTA-861 audio infoframe at all.
   *
   * Zero is a coding type a source can claim, so without this flag a source
   * that said nothing could not be told from one that claimed zero.
   */
  bool has_coding;
  /**
   * The coding type the source claims, meaningful only when `has_coding`
   * is set.
   */
  uint8_t coding;
} mxr_audio_details_t;

/**
 * How a client finds the network.
 *
 * Zeroing the whole struct asks for the default: multicast discovery on
 * whichever interface the host picks, which is the right answer on a machine
 * with one network and an arbitrary one on any other.
 */
typedef struct {
  /**
   * Where to send. Null means the multicast group, or the interface's
   * broadcast address when `broadcast` is set.
   */
  const char *target_ip;
  /**
   * UDP port. Zero means the default for the selected mode.
   */
  uint16_t port;
  /**
   * Use broadcast rather than multicast.
   */
  bool broadcast;
  /**
   * Selects the interface by address, as text. Null lets the host choose.
   *
   * It decides both which interface frames leave by and which one they are
   * accepted on. Getting it wrong on a multi-homed host fails one-sidedly:
   * devices are still discovered, because their broadcasts arrive by any
   * route, while every request this client sends leaves by the wrong
   * interface and is never answered.
   */
  const char *local_ip;
  /**
   * Selects the interface by name, taking precedence over `local_ip`.
   *
   * An interface with no address of its own - a tagged VLAN - can be named
   * only this way, and only on Linux.
   */
  const char *interface;
  /**
   * The name this client advertises to devices. Null means a default.
   */
  const char *name;
  /**
   * This client's identifier, in the form `mxr_uid_to_string()` writes.
   *
   * Null loads it from `uid_path`, generating and storing one on first run.
   * It must be stable across restarts, or every peer counts each run as a
   * new client.
   *
   * `mxr_uid_to_string()`: crate::mxr_uid_to_string
   */
  const char *uid;
  /**
   * Where the identifier is kept. Null means `.mxr-uid` in the user's home
   * directory.
   */
  const char *uid_path;
} mxr_config_t;

/**
 * Names only the device the event concerns.
 */
typedef void (*mxr_device_cb)(void *userdata, mxr_uid_t device);

/**
 * Names only the bay the event concerns.
 */
typedef void (*mxr_bay_cb)(void *userdata, mxr_bay_uid_t bay);

/**
 * Names a device and a flag.
 */
typedef void (*mxr_device_bool_cb)(void *userdata, mxr_uid_t device, bool value);

/**
 * Names a device, a status code and a message.
 */
typedef void (*mxr_system_status_cb)(void *userdata,
                                     mxr_uid_t device,
                                     uint16_t status,
                                     const char *message);

/**
 * Names a device and a second device.
 */
typedef void (*mxr_device_uid_cb)(void *userdata, mxr_uid_t device, mxr_uid_t other);

/**
 * Names a device and a 16-bit value.
 */
typedef void (*mxr_device_u16_cb)(void *userdata, mxr_uid_t device, uint16_t value);

/**
 * A command addressed to a multiviewer.
 *
 * The parameters are raw: the opcode belongs to the multiviewer module rather
 * than to MatrixOS, so there is no firmware source here to pin per-sub-command
 * field semantics against.
 */
typedef struct {
  /**
   * The multiviewer being addressed.
   */
  mxr_uid_t target;
  /**
   * The sub-opcode. A value this library has no name for still arrives.
   */
  uint8_t op;
  /**
   * Everything after the envelope, borrowed for the call.
   */
  const uint8_t *params;
  /**
   * Length of `params`.
   */
  size_t params_len;
} mxr_multiviewer_command_t;

/**
 * Carries a multiviewer command.
 */
typedef void (*mxr_multiviewer_command_cb)(void *userdata,
                                           mxr_uid_t device,
                                           const mxr_multiviewer_command_t *command);

/**
 * Which source endpoint an audio sink endpoint was switched to.
 */
typedef struct {
  /**
   * The device whose endpoint is being listened to.
   */
  mxr_uid_t source_uid;
  /**
   * The endpoint being listened to.
   */
  uint16_t source_id;
  /**
   * The device doing the listening.
   */
  mxr_uid_t target_uid;
  /**
   * The endpoint doing the listening.
   */
  uint16_t target_id;
} mxr_audio_change_source_t;

/**
 * Carries an audio input selection.
 */
typedef void (*mxr_audio_select_cb)(void *userdata,
                                    mxr_uid_t device,
                                    const mxr_audio_change_source_t *change);

/**
 * Names a device, one of its audio endpoints, and a flag.
 */
typedef void (*mxr_endpoint_bool_cb)(void *userdata, mxr_uid_t device, uint16_t endpoint, bool value);

/**
 * Names a device, one of its audio endpoints, and a 32-bit value.
 */
typedef void (*mxr_endpoint_u32_cb)(void *userdata,
                                    mxr_uid_t device,
                                    uint16_t endpoint,
                                    uint32_t value);

/**
 * Asks a device, addressed by serial, to switch a sink.
 */
typedef struct {
  /**
   * Serial of the device to act on, borrowed for the call.
   */
  const char *serial;
  /**
   * Output bay to switch.
   */
  uint16_t sink_bay;
  /**
   * Source bay to switch it to.
   */
  uint16_t source_bay;
  /**
   * Whether to skip the power-on commands that normally accompany a switch.
   */
  bool no_power_on;
  /**
   * Set when the request routes audio only.
   */
  bool audio_only;
} mxr_set_route_request_t;

/**
 * Carries a request addressed to a device.
 */
typedef void (*mxr_set_route_cb)(void *userdata,
                                 mxr_uid_t device,
                                 const mxr_set_route_request_t *request);

/**
 * Asks one device for its EDID.
 */
typedef struct {
  /**
   * The device being asked.
   */
  mxr_uid_t target;
  /**
   * Whether the sink's EDID is wanted rather than the source's.
   */
  bool output;
} mxr_edid_request_t;

/**
 * Carries a request for a device's EDID.
 */
typedef void (*mxr_edid_request_cb)(void *userdata,
                                    mxr_uid_t device,
                                    const mxr_edid_request_t *request);

/**
 * One EDID block from a device's reply.
 */
typedef struct {
  /**
   * True for a sink's EDID, false for a source's.
   */
  bool output;
  /**
   * A base block plus one extension block, borrowed for the call.
   */
  const uint8_t *data;
  /**
   * Length of `data`, normally 256.
   */
  size_t data_len;
} mxr_edid_record_t;

/**
 * Carries one EDID block a device replied with.
 */
typedef void (*mxr_edid_record_cb)(void *userdata, mxr_uid_t device, const mxr_edid_record_t *record);

/**
 * Asks a device to rename one of its bays.
 */
typedef struct {
  /**
   * The device to act on.
   */
  mxr_uid_t target;
  /**
   * The bay to rename.
   */
  uint16_t port;
  /**
   * The new name, borrowed for the call.
   */
  const char *name;
} mxr_bay_name_change_t;

/**
 * Carries a request to rename a bay.
 */
typedef void (*mxr_bay_name_change_cb)(void *userdata,
                                       mxr_uid_t device,
                                       const mxr_bay_name_change_t *change);

/**
 * Asks a device to switch its input EDID profile.
 */
typedef struct {
  /**
   * The device to act on.
   */
  mxr_uid_t target;
  /**
   * The profile to switch to.
   */
  uint16_t profile;
} mxr_edid_profile_change_t;

/**
 * Carries a request to switch an EDID profile.
 */
typedef void (*mxr_edid_profile_change_cb)(void *userdata,
                                           mxr_uid_t device,
                                           const mxr_edid_profile_change_t *change);

/**
 * Asks peers to factory-reset.
 */
typedef struct {
  /**
   * Set by the broadcast form, which targets every peer.
   */
  bool all;
  /**
   * The single device addressed, zero when `all` is set or when the request
   * addresses only its sender.
   */
  mxr_uid_t target;
} mxr_factory_reset_request_t;

/**
 * Carries a factory-reset request.
 */
typedef void (*mxr_factory_reset_cb)(void *userdata,
                                     mxr_uid_t device,
                                     const mxr_factory_reset_request_t *request);

/**
 * Asks a sink to enter or leave power save.
 */
typedef struct {
  /**
   * The sink to act on, zero on the broadcast form.
   */
  mxr_uid_t target;
  /**
   * Whether power save is being entered.
   */
  bool enabled;
} mxr_power_save_request_t;

/**
 * Carries a power-save request.
 */
typedef void (*mxr_power_save_cb)(void *userdata,
                                  mxr_uid_t device,
                                  const mxr_power_save_request_t *request);

/**
 * Asks one device to send a remote-control key on a bay.
 */
typedef struct {
  /**
   * The device to act on.
   */
  mxr_uid_t target;
  /**
   * Bay in the target's own numbering, which is not a port number.
   */
  uint16_t local_bay;
  /**
   * The key to send.
   */
  uint16_t key;
} mxr_key_transmit_request_t;

/**
 * Carries a request to send a remote-control key.
 */
typedef void (*mxr_key_transmit_cb)(void *userdata,
                                    mxr_uid_t device,
                                    const mxr_key_transmit_request_t *request);

/**
 * Asks one device to perform a remote-control action.
 */
typedef struct {
  /**
   * The device to act on.
   */
  mxr_uid_t target;
  /**
   * Bay in the target's own numbering, which is not a port number.
   */
  uint16_t local_bay;
  /**
   * The action to perform.
   */
  uint16_t action;
} mxr_action_transmit_request_t;

/**
 * Carries a request to perform a remote-control action.
 */
typedef void (*mxr_action_transmit_cb)(void *userdata,
                                       mxr_uid_t device,
                                       const mxr_action_transmit_request_t *request);

/**
 * The metadata shared by the raw-IR capture and transmit frames.
 */
typedef struct {
  /**
   * Tick length of the timing values.
   */
  uint16_t timer_resolution;
  /**
   * Carrier frequency in Hz.
   */
  uint16_t frequency;
  /**
   * Number of timing values that follow.
   */
  uint16_t nb_timings;
  /**
   * Index at which the repeat section starts.
   */
  uint16_t repeat_offset;
  /**
   * Capture status.
   */
  uint8_t status;
} mxr_ir_meta_t;

/**
 * Asks one device to blast raw IR on one of its local bays.
 */
typedef struct {
  /**
   * The device to act on.
   */
  mxr_uid_t target;
  /**
   * Bay mode in the target's own numbering, which is not a port number.
   */
  uint8_t local_mode;
  /**
   * Bay number in the target's own numbering, which is not a port number.
   */
  uint8_t local_bay;
  /**
   * Sender clock at send time.
   */
  uint32_t timestamp;
  /**
   * Metadata for the timings.
   */
  mxr_ir_meta_t meta;
  /**
   * The raw on/off timing blob, borrowed for the call.
   */
  const uint8_t *timings;
  /**
   * Length of `timings`.
   */
  size_t timings_len;
} mxr_ir_transmit_request_t;

/**
 * Carries a request to blast raw infrared.
 */
typedef void (*mxr_ir_transmit_cb)(void *userdata,
                                   mxr_uid_t device,
                                   const mxr_ir_transmit_request_t *request);

/**
 * Registers or unregisters a device on the source blacklist.
 */
typedef struct {
  /**
   * The device being listed.
   */
  mxr_uid_t target;
  /**
   * Whether it is being registered rather than removed.
   */
  bool registered;
} mxr_blacklist_change_t;

/**
 * Carries a blacklist change.
 */
typedef void (*mxr_blacklist_cb)(void *userdata,
                                 mxr_uid_t device,
                                 const mxr_blacklist_change_t *change);

/**
 * Asks one sink to crop its source to a wall window.
 *
 * The window replaces the sink's outright: a zero width or height is the wire
 * spelling of "clear the wall and show the full frame", not of "unset". A
 * revert carries no window, and the geometry in it means nothing.
 */
typedef struct {
  /**
   * The sink to act on.
   */
  mxr_uid_t target;
  /**
   * Window origin, horizontal.
   */
  uint16_t pos_x;
  /**
   * Window origin, vertical.
   */
  uint16_t pos_y;
  /**
   * Window width.
   */
  uint16_t width;
  /**
   * Window height.
   */
  uint16_t height;
  /**
   * Active picture width the window was authored against.
   */
  uint16_t raster_w;
  /**
   * Active picture height the window was authored against.
   */
  uint16_t raster_h;
  /**
   * 0 preview, 1 store, 2 revert.
   */
  uint8_t op;
} mxr_video_wall_command_t;

/**
 * Carries a video wall command.
 */
typedef void (*mxr_video_wall_cb)(void *userdata,
                                  mxr_uid_t device,
                                  const mxr_video_wall_command_t *command);

/**
 * Names a bay and another bay, the zero device standing for none.
 */
typedef void (*mxr_bay_bay_cb)(void *userdata, mxr_bay_uid_t bay, mxr_bay_uid_t other);

/**
 * Names a bay, its combined volume percentage and its mute state.
 */
typedef void (*mxr_volume_cb)(void *userdata, mxr_bay_uid_t bay, uint8_t volume, mxr_tribool_t muted);

/**
 * Names a bay and the power state of what is connected to it.
 */
typedef void (*mxr_power_cb)(void *userdata, mxr_bay_uid_t bay, mxr_power_status_t power);

/**
 * Names a bay and a string, borrowed for the call.
 */
typedef void (*mxr_bay_str_cb)(void *userdata, mxr_bay_uid_t bay, const char *value);

/**
 * Names a bay and a flag.
 */
typedef void (*mxr_bay_bool_cb)(void *userdata, mxr_bay_uid_t bay, bool value);

/**
 * Names a bay and its audio return channel.
 */
typedef void (*mxr_arc_cb)(void *userdata, mxr_bay_uid_t bay, mxr_arc_status_t arc);

/**
 * Names a bay and a 16-bit value.
 */
typedef void (*mxr_bay_u16_cb)(void *userdata, mxr_bay_uid_t bay, uint16_t value);

/**
 * Names a bay and an 8-bit value.
 */
typedef void (*mxr_bay_u8_cb)(void *userdata, mxr_bay_uid_t bay, uint8_t value);

/**
 * Raw IR captured on a bay of the sending device.
 */
typedef struct {
  /**
   * Sender clock at capture time.
   */
  uint32_t timestamp;
  /**
   * Sender clock at the last signal change.
   */
  uint32_t last_change;
  /**
   * Metadata for the timings.
   */
  mxr_ir_meta_t meta;
  /**
   * The raw on/off timing blob, borrowed for the call.
   */
  const uint8_t *timings;
  /**
   * Length of `timings`.
   */
  size_t timings_len;
} mxr_ir_capture_t;

/**
 * Carries raw infrared captured on a bay.
 */
typedef void (*mxr_ir_capture_cb)(void *userdata, mxr_bay_uid_t bay, const mxr_ir_capture_t *capture);

/**
 * Names a bay and the link that was made to it.
 */
typedef void (*mxr_bay_linked_cb)(void *userdata,
                                  mxr_bay_uid_t bay,
                                  const char *linked_serial,
                                  const char *bay_name,
                                  uint32_t features);

/**
 * Names a bay and the link that was removed from it.
 */
typedef void (*mxr_bay_unlinked_cb)(void *userdata,
                                    mxr_bay_uid_t bay,
                                    const char *linked_serial,
                                    const char *bay_name);

/**
 * What to call when something happens.
 *
 * Zero the whole struct and fill in only what is wanted: a null member drops
 * its event. `userdata` is whatever was passed to
 * `mxr_remote_new()` and is never examined here.
 */
typedef struct {
  /**
   * Fires after every device-level event below.
   */
  mxr_device_cb on_device_update;
  /**
   * Fires after every bay-level event below.
   */
  mxr_bay_cb on_bay_update;
  /**
   * The device's configuration changed.
   */
  mxr_device_cb on_device_config_changed;
  /**
   * The device has reported every part of its configuration.
   */
  mxr_device_cb on_device_config_complete;
  /**
   * The device started or stopped answering.
   */
  mxr_device_bool_cb on_device_online_changed;
  /**
   * The device reported new temperatures; read them with
   * `mxr_device_temperatures()`.
   */
  mxr_device_cb on_device_temperature_changed;
  /**
   * A firmware component reported its version; read it with
   * `mxr_device_firmware()`.
   */
  mxr_device_cb on_firmware_version_changed;
  /**
   * The device reported a status about itself.
   */
  mxr_system_status_cb on_system_status_changed;
  /**
   * A network port reported its link state; read it with
   * `mxr_network_status()`.
   */
  mxr_device_cb on_network_status_changed;
  /**
   * The device reported V2IP statistics; read them with
   * `mxr_v2ip_stats()`.
   */
  mxr_device_cb on_v2ip_stats_changed;
  /**
   * The streams the device's source bays advertise changed; read them with
   * `mxr_v2ip_sources()`.
   */
  mxr_device_cb on_v2ip_sources_changed;
  /**
   * The device's V2IP encoder configuration changed; read it with
   * `mxr_v2ip_details()`.
   */
  mxr_device_cb on_v2ip_details_changed;
  /**
   * The streams the device's sink is subscribed to changed; read them with
   * `mxr_v2ip_sink()`.
   *
   * A route request addressed to the device fires this as soon as it is
   * seen, so this reports what the mesh now believes rather than what the
   * device confirmed - it acknowledges nothing, and only its own
   * configuration report, sent on its own schedule, settles a route.
   */
  mxr_device_cb on_v2ip_sink_changed;
  /**
   * A multiviewer reported its state; read it with
   * `mxr_multiviewer_status()`.
   */
  mxr_device_cb on_multiviewer_status_changed;
  /**
   * The device reported its audio endpoint tree; read it with
   * `mxr_audio_endpoints()`.
   */
  mxr_device_cb on_audio_endpoints_changed;
  /**
   * The device reported its mesh master.
   */
  mxr_device_uid_cb on_mesh_master_changed;
  /**
   * The device reported its view of the mesh topology; read it with
   * `mxr_topology()`.
   */
  mxr_device_cb on_topology_changed;
  /**
   * A ProAmp8 reported its Dolby settings; read them with
   * `mxr_dolby_settings()`.
   */
  mxr_device_cb on_amp_dolby_settings_changed;
  /**
   * Installer setup was completed or cleared.
   */
  mxr_device_bool_cb on_setup_status_changed;
  /**
   * The installer identifier changed.
   */
  mxr_device_u16_cb on_installer_id_changed;
  /**
   * The sink was told to show a window; read it with
   * `mxr_v2ip_tiling()`.
   */
  mxr_device_cb on_tiling_changed;
  /**
   * A source bay's remote-control configuration changed; read it with
   * `mxr_rc_settings()`.
   */
  mxr_device_cb on_rc_settings_changed;
  /**
   * A V2IP device was linked to a remote peer.
   */
  mxr_device_uid_cb on_v2ip_link_changed;
  /**
   * A multiviewer command arrived.
   */
  mxr_multiviewer_command_cb on_multiviewer_command;
  /**
   * An audio endpoint was switched to a new source.
   */
  mxr_audio_select_cb on_audio_select_input;
  /**
   * An audio endpoint was muted or unmuted.
   */
  mxr_endpoint_bool_cb on_audio_endpoint_mute;
  /**
   * An audio endpoint's trigger changed.
   */
  mxr_endpoint_bool_cb on_audio_endpoint_trigger;
  /**
   * An audio endpoint's volume changed.
   */
  mxr_endpoint_u32_cb on_audio_endpoint_volume;
  /**
   * A peer asked every device to announce itself.
   */
  mxr_device_cb on_discover_request;
  /**
   * A peer asked a device to switch a sink.
   */
  mxr_set_route_cb on_set_route_requested;
  /**
   * A peer asked a device for its EDID.
   */
  mxr_edid_request_cb on_edid_requested;
  /**
   * A device answered with its EDID.
   */
  mxr_edid_record_cb on_edid_received;
  /**
   * A peer asked a device to rename a bay.
   */
  mxr_bay_name_change_cb on_bay_name_change_requested;
  /**
   * A peer asked a device to switch its EDID profile.
   */
  mxr_edid_profile_change_cb on_edid_profile_change_requested;
  /**
   * A peer asked a device to reboot. The second identifier is the device
   * being asked, which is not always the sender.
   */
  mxr_device_uid_cb on_reboot_requested;
  /**
   * A peer asked devices to factory-reset.
   */
  mxr_factory_reset_cb on_factory_reset_requested;
  /**
   * A device sent its monitoring pulse.
   */
  mxr_device_cb on_monitoring_pulse;
  /**
   * A peer asked a device to upgrade its FPGA.
   */
  mxr_device_cb on_upgrade_fpga_requested;
  /**
   * A peer asked a device to re-detect its bays.
   */
  mxr_device_cb on_detect_bays_requested;
  /**
   * A peer asked a sink to enter or leave power save.
   */
  mxr_power_save_cb on_power_save_requested;
  /**
   * A peer asked a device to send a remote-control key.
   */
  mxr_key_transmit_cb on_key_transmit_requested;
  /**
   * A peer asked a device to perform a remote-control action.
   */
  mxr_action_transmit_cb on_action_transmit_requested;
  /**
   * A peer asked a device to blast raw infrared.
   */
  mxr_ir_transmit_cb on_ir_transmit_requested;
  /**
   * A device was added to or removed from the source blacklist.
   */
  mxr_blacklist_cb on_blacklist_changed;
  /**
   * A video wall command arrived.
   */
  mxr_video_wall_cb on_video_wall_command;
  /**
   * A bay was seen for the first time.
   */
  mxr_bay_cb on_bay_registered;
  /**
   * The bay's routed video source changed, zero when it was unrouted.
   */
  mxr_bay_bay_cb on_video_source_changed;
  /**
   * The bay's routed audio source changed, zero when it was unrouted.
   */
  mxr_bay_bay_cb on_audio_source_changed;
  /**
   * The bay's volume or mute state changed.
   */
  mxr_volume_cb on_volume_changed;
  /**
   * The attached device's power state changed.
   */
  mxr_power_cb on_power_changed;
  /**
   * The bay was renamed.
   */
  mxr_bay_str_cb on_name_changed;
  /**
   * A signal appeared or disappeared.
   */
  mxr_bay_bool_cb on_signal_detected_changed;
  /**
   * The bay started or stopped reporting a fault.
   */
  mxr_bay_bool_cb on_faulty_changed;
  /**
   * The bay was hidden or shown.
   */
  mxr_bay_bool_cb on_hidden_changed;
  /**
   * Power over Ethernet started or stopped supplying the bay.
   */
  mxr_bay_bool_cb on_poe_powered_changed;
  /**
   * The HDBaseT link came up or went down.
   */
  mxr_bay_bool_cb on_hdbt_connected_changed;
  /**
   * The signal format description changed.
   */
  mxr_bay_str_cb on_signal_type_changed;
  /**
   * Hot-plug detect was asserted or released.
   */
  mxr_bay_bool_cb on_hpd_detected_changed;
  /**
   * A CEC device answered or stopped answering.
   */
  mxr_bay_bool_cb on_cec_detected_changed;
  /**
   * The audio return channel changed.
   */
  mxr_arc_cb on_arc_changed;
  /**
   * The input's EDID profile changed.
   */
  mxr_bay_u16_cb on_edid_profile_changed;
  /**
   * The input's remote-control type changed.
   */
  mxr_bay_u8_cb on_rc_type_changed;
  /**
   * A remote-control key was pressed on the bay.
   */
  mxr_bay_u16_cb on_key_pressed;
  /**
   * A remote-control action was received on the bay.
   */
  mxr_bay_u16_cb on_action_received;
  /**
   * The bay started or stopped mirroring another output, zero when it
   * stopped.
   */
  mxr_bay_bay_cb on_mirror_status_changed;
  /**
   * A ProAmp8 zone's settings changed; read them with
   * `mxr_bay_amp_settings()`.
   */
  mxr_bay_cb on_amp_zone_settings_changed;
  /**
   * A volume step was requested on the bay.
   */
  mxr_bay_bool_cb on_volume_step;
  /**
   * The bay detected audio clipping, at the reported level.
   */
  mxr_bay_u8_cb on_audio_clip;
  /**
   * Raw infrared was captured on the bay.
   */
  mxr_ir_capture_cb on_ir_captured;
  /**
   * The devices filtered out of this sink's picker changed; read them with
   * `mxr_bay_filtered()`.
   */
  mxr_bay_cb on_filtered_devices_changed;
  /**
   * The audio endpoint the bay carries changed.
   */
  mxr_bay_u8_cb on_audio_endpoint_changed;
  /**
   * The bay's V2IP encoder was enabled or disabled.
   */
  mxr_bay_bool_cb on_encoder_disabled_changed;
  /**
   * The bay's V2IP decoder was enabled or disabled.
   */
  mxr_bay_bool_cb on_decoder_disabled_changed;
  /**
   * The bay was linked to a bay on another device. Both ends are told, so
   * both fire: `bay_name` names the bay whose link record changed, which is
   * this bay on the device that reported the change and the far bay on its
   * peer.
   */
  mxr_bay_linked_cb on_bay_linked;
  /**
   * The bay's link to another device was removed. The arguments describe
   * the link that went, and mean what they do on `on_bay_linked`.
   */
  mxr_bay_unlinked_cb on_bay_unlinked;
} mxr_callbacks_t;

/**
 * Transmitter stream statistics.
 */
typedef struct {
  /**
   * Video packets sent.
   */
  uint32_t video;
  /**
   * Audio packets sent.
   */
  uint32_t audio;
  /**
   * Ancillary-data packets sent.
   */
  uint32_t anc;
  /**
   * Times the stream went down.
   */
  uint32_t stream_down;
  /**
   * Transmit overflows.
   */
  uint32_t overflow;
} mxr_v2ip_tx_stats_t;

/**
 * Receiver stream statistics.
 */
typedef struct {
  /**
   * Video packets received.
   */
  uint32_t video_total;
  /**
   * Video packets dropped.
   */
  uint32_t video_dropped;
  /**
   * Video sequence errors.
   */
  uint32_t video_seq_errors;
  /**
   * Watchdog timeouts.
   */
  uint32_t wdt_timeout;
  /**
   * Audio packets received.
   */
  uint32_t audio_total;
  /**
   * Audio packets dropped.
   */
  uint32_t audio_dropped;
  /**
   * Audio sequence errors.
   */
  uint32_t audio_seq_errors;
  /**
   * Ancillary-data packets received.
   */
  uint32_t anc_total;
  /**
   * Ancillary-data packets dropped.
   */
  uint32_t anc_dropped;
  /**
   * Ancillary-data sequence errors.
   */
  uint32_t anc_seq_errors;
  /**
   * The decoder's health state: 0 unknown, 1 healthy, 2 bad, 3 starting.
   *
   * Only healthy and bad are verdicts. Reading failure as "not healthy"
   * counts a decoder that is merely coming up as one that failed, which is
   * what every sink reports for a moment after a route change.
   */
  uint8_t decoder_state;
} mxr_v2ip_rx_stats_t;

/**
 * A device's V2IP statistics, cumulative and over the last minute.
 */
typedef struct {
  /**
   * Transmit totals since boot.
   */
  mxr_v2ip_tx_stats_t tx;
  /**
   * Transmit counts over the last minute.
   */
  mxr_v2ip_tx_stats_t tx_per_minute;
  /**
   * Receive totals since boot.
   */
  mxr_v2ip_rx_stats_t rx;
  /**
   * Receive counts over the last minute.
   */
  mxr_v2ip_rx_stats_t rx_per_minute;
} mxr_v2ip_stats_t;

/**
 * One multicast stream address.
 */
typedef struct {
  /**
   * Which stream this address is for.
   */
  mxr_stream_kind_t kind;
  /**
   * The multicast group, as a dotted quad.
   */
  char ip[MXR_IP_STRING_LEN];
  /**
   * The destination UDP port.
   */
  uint16_t port;
  /**
   * Whether this carries a usable address: a multicast group and a non-zero
   * port, both. A slot a device has not filled in is not an error, so this
   * is what separates an address from an empty slot.
   */
  bool valid;
} mxr_stream_source_t;

/**
 * A V2IP device's own encoder configuration.
 */
typedef struct {
  /**
   * The video stream this device sources.
   */
  mxr_stream_source_t video;
  /**
   * The audio stream this device sources.
   */
  mxr_stream_source_t audio;
  /**
   * The ancillary-data stream this device sources.
   */
  mxr_stream_source_t anc;
  /**
   * The audio-return stream this device sources.
   */
  mxr_stream_source_t arc;
  /**
   * Encoder rate in units of 10Mb/s, or -1 when no rate has been reported.
   */
  int16_t tx_rate;
  /**
   * DSCP marking for the video stream, or -1 when unmarked.
   */
  int16_t dscp_video;
  /**
   * DSCP marking for the audio stream, or -1 when unmarked.
   */
  int16_t dscp_audio;
  /**
   * DSCP marking for the ancillary-data stream, or -1 when unmarked.
   */
  int16_t dscp_anc;
  /**
   * The signal type the output scales to.
   */
  uint16_t scaling_mode;
  /**
   * Refresh rate in Hz.
   */
  uint16_t scaling_refresh;
  /**
   * `MXR_SCALING_FLAG_*` bits. Bits outside those are undefined and are not
   * reliably zero: firmware predating the fix builds this frame over an
   * uninitialised stack local.
   */
  uint8_t scaling_flags;
} mxr_v2ip_details_t;

/**
 * The streams one V2IP source advertises.
 */
typedef struct {
  /**
   * The originating device, zero when it is not known.
   */
  mxr_uid_t uid;
  /**
   * The video stream.
   */
  mxr_stream_source_t video;
  /**
   * The audio stream.
   */
  mxr_stream_source_t audio;
  /**
   * The ancillary-data stream.
   */
  mxr_stream_source_t anc;
  /**
   * Whether an audio-return stream is advertised.
   */
  bool has_arc;
  /**
   * The audio-return stream, meaningful only when `has_arc` is set.
   */
  mxr_stream_source_t arc;
} mxr_stream_sources_t;

/**
 * The streams a V2IP sink is subscribed to.
 */
typedef struct {
  /**
   * The streams the sink subscribes to.
   */
  mxr_stream_sources_t addresses;
  /**
   * Whether the sender reported a resolved audio format.
   */
  bool has_audio_format;
  /**
   * The audio format, meaningful only when `has_audio_format` is set.
   */
  mxr_audio_format_t audio_format;
} mxr_v2ip_sink_t;

/**
 * The window a sink is currently told to show.
 *
 * This is the pollable view of a sink's window, not the persisted video wall
 * setting: on a sink running the wall module a write here is transient,
 * because that module pushes its own target window back within about a
 * second.
 */
typedef struct {
  /**
   * The sink this window belongs to.
   */
  mxr_uid_t target;
  /**
   * Window origin, horizontal.
   */
  uint16_t pos_x;
  /**
   * Window origin, vertical.
   */
  uint16_t pos_y;
  /**
   * Window width.
   */
  uint16_t width;
  /**
   * Window height.
   */
  uint16_t height;
} mxr_tiling_config_t;

/**
 * What a multiviewer reports about itself.
 */
typedef struct {
  /**
   * The multiviewer.
   */
  mxr_uid_t uid;
  /**
   * The source device mapped to each input.
   */
  mxr_uid_t mappings[MXR_MULTIVIEWER_INPUTS];
  /**
   * The MCU firmware version.
   */
  char mcu_version[MXR_NAME_LEN];
  /**
   * The scaler firmware version.
   */
  char scaler_version[MXR_NAME_LEN];
  /**
   * The view mode the hardware reports, which is its own numbering rather
   * than `view_mode`'s.
   */
  uint8_t hw_view_mode;
  /**
   * The window layout.
   */
  uint8_t view_mode;
  /**
   * Which corner the picture-in-picture window sits in.
   */
  uint8_t pip_position;
  /**
   * The size of the picture-in-picture window.
   */
  uint8_t pip_size;
  /**
   * The output resolution.
   */
  uint8_t output_mode;
  /**
   * The HDCP mode.
   */
  uint8_t hdcp_mode;
  /**
   * The IT content flag.
   */
  uint8_t output_itc;
  /**
   * The EDID presented to sources.
   */
  uint8_t edid_template;
  /**
   * How a source is fitted into its window.
   */
  uint8_t aspect_ratio;
  /**
   * Whether automatic source switching is on.
   */
  uint8_t auto_switch;
  /**
   * Which window the audio is taken from.
   */
  uint8_t audio_source;
  /**
   * Whether a volume has been reported.
   */
  bool has_audio_volume;
  /**
   * The output volume.
   */
  uint8_t audio_volume;
  /**
   * Whether the output is muted.
   */
  uint8_t audio_muted;
  /**
   * The source shown in each window.
   */
  uint8_t video_sources[MXR_MULTIVIEWER_INPUTS];
  /**
   * Which window remote control is forwarded to.
   */
  uint8_t remote_control;
} mxr_multiviewer_status_t;

/**
 * A ProAmp8's Dolby settings.
 */
typedef struct {
  /**
   * 0 = standard, 1 = 3-zone Dolby, 2 = 4-zone Dolby.
   */
  uint8_t mode;
  /**
   * Whether PCM is up-mixed to 5.1 rather than passed through.
   */
  bool pcm_upmix;
  /**
   * Whether a Dolby stream was detected.
   */
  bool dolby_detected;
  /**
   * Whether up-mixing is currently running.
   */
  bool pcm_upmix_active;
} mxr_dolby_settings_t;

/**
 * The remote-control configuration of a source bay.
 */
typedef struct {
  /**
   * The device this configuration belongs to.
   */
  mxr_uid_t target;
  /**
   * The control method, as the wire value.
   *
   * Zero is infrared, a method a bay really uses, so it is not a stand-in
   * for "not reported". Check that `mxr_rc_settings()` returned `MXR_OK`
   * before reading this: a device that has not sent its settings yet
   * leaves the struct as the caller allocated it, and a zeroed one then
   * reads as a bay set to infrared. `mxr_bay_info_t` answers the same
   * question with a `has_rc_type` flag beside its `rc_type`.
   */
  uint8_t rc_target;
  /**
   * The control target's address, empty when unset.
   */
  char ip[MXR_IP_STRING_LEN];
  /**
   * Whether CEC is enabled.
   */
  bool cec_enabled;
  /**
   * Whether CEC powers the sink on automatically.
   */
  bool cec_auto_on;
  /**
   * Whether remote-control commands are forwarded.
   */
  bool forward_rc;
  /**
   * Whether infrared is forwarded.
   */
  bool forward_ir;
  /**
   * The driver state on the source, as the wire value. One above the last
   * this library knows is passed through as it arrived.
   */
  uint8_t rc_status;
  /**
   * The driver-reported status string, empty when unknown.
   */
  char status_name[MXR_NAME_LEN];
} mxr_rc_settings_t;

/**
 * The diagnostic result for one UTP cable pair.
 */
typedef struct {
  /**
   * Whether the pair is wired with normal polarity.
   */
  bool polarity;
  /**
   * Which pair this describes.
   */
  uint8_t pair;
  /**
   * Measured skew.
   */
  uint32_t skew;
  /**
   * Measured length.
   */
  uint32_t length;
} mxr_cable_status_t;

/**
 * The link state and diagnostics of one network port.
 */
typedef struct {
  /**
   * Port number.
   */
  uint16_t port;
  /**
   * Port name.
   */
  char name[MXR_NAME_LEN];
  /**
   * Negotiated link speed.
   */
  uint8_t link_speed;
  /**
   * Whether the link negotiated full duplex.
   */
  bool link_full_duplex;
  /**
   * The port's own address, empty when it has not reported one.
   */
  char ip[MXR_IP_STRING_LEN];
  /**
   * The IGMP querier the port sees, empty when it sees none.
   */
  char querier[MXR_IP_STRING_LEN];
  /**
   * Whether the port reported a hardware address.
   */
  bool has_mac_address;
  /**
   * The hardware address, meaningful only when `has_mac_address` is set.
   */
  uint8_t mac_address[6];
  /**
   * Whether the port reported link errors.
   */
  bool has_errors;
  /**
   * Input errors.
   */
  bool in_error;
  /**
   * Input frame check errors.
   */
  bool in_fcs_error;
  /**
   * Input collisions.
   */
  bool in_collision;
  /**
   * Deferred transmissions.
   */
  bool out_deferred;
  /**
   * Excessive transmissions.
   */
  bool out_excessive;
  /**
   * Polarity errors.
   */
  bool polarity_error;
  /**
   * Skew warning.
   */
  bool skew_warning;
  /**
   * Length warning.
   */
  bool length_warning;
  /**
   * Whether the port reported a virtual cable test.
   */
  bool has_vct_status;
  /**
   * Whether each pair raised a warning, meaningful only when
   * `has_vct_status` is set.
   */
  bool vct_warning[MXR_UTP_PAIRS];
  /**
   * How many entries of `cable_status` the port filled in.
   */
  size_t cable_status_count;
  /**
   * Cable diagnostics per pair.
   */
  mxr_cable_status_t cable_status[MXR_UTP_PAIRS];
} mxr_network_port_t;

/**
 * One device in a topology report.
 */
typedef struct {
  /**
   * The device this entry describes.
   */
  mxr_uid_t uid;
  /**
   * Bitmask of the devices it is connected to.
   */
  uint32_t mask;
} mxr_topology_entry_t;

/**
 * One firmware component a device reports.
 */
typedef struct {
  /**
   * Which component this describes.
   */
  uint8_t firmware_type;
  /**
   * Build timestamp, in seconds since the Unix epoch.
   */
  uint32_t timestamp;
  /**
   * Source revision hash.
   */
  uint32_t hash;
  /**
   * Human-readable version string.
   */
  char version[MXR_VERSION_LEN];
} mxr_firmware_version_t;

/**
 * One node of a device's audio tree.
 */
typedef struct {
  /**
   * The endpoint's identifier on its device.
   */
  uint8_t id;
  /**
   * What the endpoint can do, as `MXR_AUDIO_*` bits.
   */
  uint32_t features;
  /**
   * Whether the endpoint carries a stream address.
   */
  bool has_address;
  /**
   * The stream address, meaningful only when `has_address` is set.
   */
  mxr_stream_source_t address;
  /**
   * The endpoint this one hangs off, or -1 at a root.
   */
  int16_t parent;
  /**
   * How many children this endpoint has; read them with
   * `mxr_audio_endpoint_children()`.
   */
  size_t child_count;
  /**
   * Whether the device reported which inputs are selectable.
   */
  bool has_inputs_available;
  /**
   * Bitmask of the endpoints this one may be switched to.
   */
  uint32_t inputs_available;
  /**
   * Whether the device reported which input is selected.
   */
  bool has_inputs_routed;
  /**
   * Bitmask of the endpoint this one is listening to.
   */
  uint32_t inputs_routed;
  /**
   * The device at the other end of the link, zero when unlinked.
   */
  mxr_uid_t linked_device;
  /**
   * The endpoint at the other end of the link, or -1 when unlinked.
   */
  int16_t linked_endpoint;
} mxr_audio_endpoint_t;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * This library's version, as `MAJOR.MINOR.PATCH`.
 *
 * The returned pointer is static and always valid.
 */
const char *mxr_version(void);

/**
 * Why the last call on this thread failed, or an empty string.
 *
 * The text is owned by the library and is replaced by the next failure on
 * this thread, so a caller that keeps it copies it first. It describes the
 * failure; the result code classifies it, and only the code should be
 * branched on.
 *
 * Never returns null.
 */
const char *mxr_last_error(void);

/**
 * Reports whether `uid` is the empty identifier.
 *
 * The protocol uses it wherever a device could be named and is not, so this
 * is the test for "no device" rather than a comparison against a constant.
 */
bool mxr_uid_is_zero(mxr_uid_t uid);

/**
 * Writes `uid` as dotted hex into `out`, which needs
 * [`MXR_UID_STRING_LEN`] bytes.
 *
 * # Safety
 *
 * `out` points at `cap` writable bytes.
 */
mxr_result_t mxr_uid_to_string(mxr_uid_t uid, char *out, size_t cap);

/**
 * Reads the dotted-hex form `mxr_uid_to_string()` writes.
 *
 * # Safety
 *
 * `text` points at a NUL-terminated string and `out` at a writable
 * [`mxr_uid_t`].
 */
mxr_result_t mxr_uid_from_string(const char *text, mxr_uid_t *out);

/**
 * The remote-control type a bay status word carries in bits 16-19.
 */
uint8_t mxr_bay_status_rc_type(uint32_t status);

/**
 * The HDCP version a bay status word carries in bits 22-23.
 */
uint8_t mxr_bay_status_hdcp(uint32_t status);

/**
 * The CTA-861 short video descriptor, zero when the signal is not HDMI.
 */
uint8_t mxr_signal_type_svd(mxr_signal_type_t signal_type);

/**
 * The colour space: 0 RGB, 1 4:4:4, 2 4:2:2, 3 4:2:0.
 */
uint8_t mxr_signal_type_colour_space(mxr_signal_type_t signal_type);

/**
 * Whether the frame rate carries a 1000/1001 clock.
 */
bool mxr_signal_type_non_integer(mxr_signal_type_t signal_type);

/**
 * The bit depth in bits per component, zero where the word names none.
 *
 * The field on the wire is an index into a table of four depths, so it is not
 * the depth: a signal at 12 bits reads 3 there. This converts it.
 */
uint8_t mxr_signal_type_bpp(mxr_signal_type_t signal_type);

/**
 * The raw bit-depth index, for a caller that wants the wire value.
 *
 * See `mxr_signal_type_bpp()` for the depth it stands for.
 */
uint8_t mxr_signal_type_bpp_index(mxr_signal_type_t signal_type);

/**
 * Whether the word names a signal format at all.
 *
 * A bay with nothing configured says so two ways: a sender that zeroes the
 * word and stamps the unset bit-depth index, and one that writes a plain
 * zero. Neither is a format, and the svd and colour space beside them are not
 * answers either - both read as zero, which is what this word says for "not
 * HDMI" and "RGB" when it is set.
 */
bool mxr_signal_type_is_set(mxr_signal_type_t signal_type);

/**
 * Routes a V2IP sink's video to the stream a source port advertises.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_select_video_source(const mxr_remote_t *remote,
                                     mxr_bay_uid_t sink,
                                     uint16_t source_port);

/**
 * Routes a V2IP sink's audio to the stream a source port advertises,
 * leaving its video where it is.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_select_audio_source(const mxr_remote_t *remote,
                                     mxr_bay_uid_t sink,
                                     uint16_t source_port);

/**
 * Routes a V2IP sink's video to the source bay with this user-assigned name.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `name` is a NUL-terminated string.
 */
mxr_result_t mxr_select_video_source_by_name(const mxr_remote_t *remote,
                                             mxr_bay_uid_t sink,
                                             const char *name);

/**
 * Routes a V2IP sink's audio to the source bay with this user-assigned name.
 *
 * `format` may be null to leave the sink's audio format alone.
 *
 * # Safety
 *
 * `remote` is null or a live handle, `name` is a NUL-terminated string, and
 * `format` is null or points at an initialised [`mxr_audio_format_t`].
 */
mxr_result_t mxr_select_audio_source_by_name(const mxr_remote_t *remote,
                                             mxr_bay_uid_t sink,
                                             const char *name,
                                             const mxr_audio_format_t *format);

/**
 * Routes a V2IP sink's audio to a multicast group directly, for a source this
 * client has not heard advertise it.
 *
 * `audio_port` may be zero for the default, and `format` may be null to leave
 * the sink's audio format alone.
 *
 * # Safety
 *
 * `remote` is null or a live handle, `audio_ip` is a NUL-terminated dotted
 * quad, and `format` is null or points at an initialised
 * [`mxr_audio_format_t`].
 */
mxr_result_t mxr_select_audio_source_addr(const mxr_remote_t *remote,
                                          mxr_bay_uid_t sink,
                                          const char *audio_ip,
                                          uint16_t audio_port,
                                          const mxr_audio_format_t *format);

/**
 * Routes a V2IP sink's video, audio and ancillary streams to multicast groups
 * the caller names.
 *
 * This is the only way to reach a stream no device on the mesh advertises,
 * such as one the calling program is transmitting itself; the routes by
 * source port and by name can only name a stream some bay has announced.
 *
 * Set all three groups. The firmware decides whether a sink has a manual
 * route by looking at the video and ancillary groups, so a route that leaves
 * either unset does not register as one and the sink falls back to the audio
 * source its mesh picks.
 *
 * A null `format` sends 48kHz stereo rather than omitting the field. The
 * firmware stores whatever the frame carries and hands it to the FPGA
 * unexamined, so a frame without a format leaves a zero sample rate there,
 * which the FPGA rejects and which takes the switch down with it.
 *
 * # Safety
 *
 * `remote` is null or a live handle, `route` points at an initialised
 * [`mxr_v2ip_route_t`] whose addresses are null or NUL-terminated strings,
 * and `format` is null or points at an initialised [`mxr_audio_format_t`].
 */
mxr_result_t mxr_select_source_addr(const mxr_remote_t *remote,
                                    mxr_bay_uid_t sink,
                                    const mxr_v2ip_route_t *route,
                                    const mxr_audio_format_t *format);

/**
 * Renames a bay. The device stores the first 16 bytes.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `name` is a NUL-terminated string.
 */
mxr_result_t mxr_set_bay_name(const mxr_remote_t *remote, mxr_bay_uid_t bay, const char *name);

/**
 * Hides a bay from the installation's user interface, or shows it again.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_set_bay_hidden(const mxr_remote_t *remote, mxr_bay_uid_t bay, bool hidden);

/**
 * Switches an input bay's EDID profile.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_select_edid_profile(const mxr_remote_t *remote,
                                     mxr_bay_uid_t bay,
                                     uint16_t profile);

/**
 * Sends a remote-control action to whatever is attached to a bay.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_send_action(const mxr_remote_t *remote, mxr_bay_uid_t bay, uint16_t action);

/**
 * Sends a remote-control key press to whatever is attached to a bay.
 *
 * The device forwards it over CEC, infrared or IP, whichever that bay is
 * configured for; the caller does not choose. `key` is one of the `MXR_KEY_*`
 * values, or a raw code above `MXR_KEY_CUSTOM_CEC` or `MXR_KEY_CUSTOM_SKY`.
 * A value this library has no name for is sent as it was given.
 *
 * `mxr_send_action()` names an outcome instead, and lets the device decide
 * which keys reach it.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_send_key(const mxr_remote_t *remote, mxr_bay_uid_t bay, uint16_t key);

/**
 * Powers on what is attached to a bay.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_power_on(const mxr_remote_t *remote, mxr_bay_uid_t bay);

/**
 * Powers off what is attached to a bay.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_power_off(const mxr_remote_t *remote, mxr_bay_uid_t bay);

/**
 * Sets a bay's volume percentage, and its mute state when `muted` is not
 * `MXR_UNKNOWN`.
 *
 * A bay with no volume control of its own is set through its `linked_bay`,
 * so an output wired to an amplifier zone reaches that zone.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_set_volume(const mxr_remote_t *remote,
                            mxr_bay_uid_t bay,
                            uint8_t volume,
                            mxr_tribool_t muted);

/**
 * Asks a bay to step its volume up.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_volume_up(const mxr_remote_t *remote, mxr_bay_uid_t bay);

/**
 * Asks a bay to step its volume down.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_volume_down(const mxr_remote_t *remote, mxr_bay_uid_t bay);

/**
 * Mutes or unmutes a bay, leaving its volume alone.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_set_muted(const mxr_remote_t *remote, mxr_bay_uid_t bay, bool muted);

/**
 * Writes an amplifier zone's gain, delay, tone and power settings.
 *
 * This replaces every setting at once, so a caller changing one reads the
 * current set with `mxr_bay_amp_settings()`
 * first.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `settings` points at an initialised
 * [`mxr_amp_zone_settings_t`].
 */
mxr_result_t mxr_set_amp_zone_settings(const mxr_remote_t *remote,
                                       mxr_bay_uid_t bay,
                                       const mxr_amp_zone_settings_t *settings);

/**
 * Mutes or unmutes one of a device's audio endpoints.
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_set_audio_endpoint_muted(const mxr_remote_t *remote,
                                          mxr_uid_t device,
                                          uint16_t endpoint,
                                          bool muted);

/**
 * Activates or clears an audio endpoint's trigger.
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_set_audio_endpoint_trigger(const mxr_remote_t *remote,
                                            mxr_uid_t device,
                                            uint16_t endpoint,
                                            bool active);

/**
 * Sets an audio endpoint's volume.
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_set_audio_endpoint_volume(const mxr_remote_t *remote,
                                           mxr_uid_t device,
                                           uint16_t endpoint,
                                           uint32_t volume);

/**
 * Points one device's audio endpoint at another device's.
 *
 * `sink` is the end doing the listening and `source` the end being
 * listened to.
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_select_audio_endpoint_input(const mxr_remote_t *remote,
                                             mxr_uid_t sink,
                                             uint16_t sink_endpoint,
                                             mxr_uid_t source,
                                             uint16_t source_endpoint);

/**
 * Asks a device for an EDID: the one the display on its output publishes, or
 * the one the device presents to the source on its input.
 *
 * The device answers a moment later. The bytes reach `on_edid_received` and
 * stay readable through `mxr_device_edid()`.
 *
 * Only V2IP hardware handles this opcode. A matrix or an amplifier accepts
 * the frame and answers nothing, at any protocol version, so the silence that
 * follows is permanent rather than a reply still to come. `MXR_OK` here means
 * the frame was sent, and nothing more; a caller polling for an EDID should
 * ask a device that can answer rather than wait on one that cannot.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_request_edid(const mxr_remote_t *remote, mxr_uid_t device, bool output);

/**
 * Asks for a detailed signal report from every bay of one device, or - with
 * the zero uid - from every bay on the network.
 *
 * Devices report on their own when a signal changes, so this is what a client
 * that has just started needs: without it, a bay that has been showing the
 * same picture for an hour says nothing until it changes.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_request_signal_status(const mxr_remote_t *remote, mxr_uid_t device);

/**
 * Subscribes to, or unsubscribes from, a device's V2IP statistics.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_subscribe_v2ip_stats(const mxr_remote_t *remote, mxr_uid_t device, bool subscribe);

/**
 * Reboots a device.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_reboot(const mxr_remote_t *remote, mxr_uid_t device);

/**
 * Sends the monitoring pulse that tells devices this client is watching.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_send_monitoring_pulse(const mxr_remote_t *remote);

/**
 * Shows a window on a sink's video wall without storing it.
 *
 * The window lasts until the sink is told otherwise or restarts;
 * `mxr_revert_video_wall()` puts back whatever it has stored. A zero width or
 * height shows the whole frame again.
 *
 * The geometry is checked here and `MXR_ERR_INVALID_ARGUMENT` returned
 * without sending anything, because the sink is not guaranteed to check it
 * itself.
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `window` points at an initialised
 * [`mxr_video_wall_window_t`].
 */
mxr_result_t mxr_preview_video_wall(const mxr_remote_t *remote,
                                    mxr_uid_t sink,
                                    const mxr_video_wall_window_t *window);

/**
 * Stores a window as a sink's video wall.
 *
 * The geometry is checked here and `MXR_ERR_INVALID_ARGUMENT` returned
 * without sending anything. That matters more than a refused frame would: a
 * sink running a video-wall module older than 2026083100 writes the window to
 * its configuration before asking its video processor to apply it, and the
 * processor's refusal does not undo the write, so an out-of-spec window
 * survives a reboot and is re-offered on every stream restart until something
 * else replaces it. A power cycle does not clear it.
 *
 * A zero width or height stores "show the whole frame".
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `window` points at an initialised
 * [`mxr_video_wall_window_t`].
 */
mxr_result_t mxr_store_video_wall(const mxr_remote_t *remote,
                                  mxr_uid_t sink,
                                  const mxr_video_wall_window_t *window);

/**
 * Restores the window a sink has stored, discarding a preview.
 *
 * Carries no window: the sink already holds the one this puts back.
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_revert_video_wall(const mxr_remote_t *remote, mxr_uid_t sink);

/**
 * Switches a multiviewer's window layout.
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_set_multiviewer_view_mode(const mxr_remote_t *remote,
                                           mxr_uid_t device,
                                           uint8_t mode);

/**
 * Puts a source in one of a multiviewer's windows.
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_set_multiviewer_video_source(const mxr_remote_t *remote,
                                              mxr_uid_t device,
                                              uint8_t screen,
                                              uint8_t source);

/**
 * Chooses which window a multiviewer takes its audio from.
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_set_multiviewer_audio_source(const mxr_remote_t *remote,
                                              mxr_uid_t device,
                                              uint8_t source);

/**
 * Sets a multiviewer's output volume and mute state.
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_set_multiviewer_audio_volume(const mxr_remote_t *remote,
                                              mxr_uid_t device,
                                              uint8_t volume,
                                              bool muted);

/**
 * Switches the EDID a multiviewer presents to its sources.
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_set_multiviewer_edid_template(const mxr_remote_t *remote,
                                               mxr_uid_t device,
                                               uint8_t template_);

/**
 * Chooses which window a multiviewer forwards remote control to.
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_set_multiviewer_remote_control(const mxr_remote_t *remote,
                                                mxr_uid_t device,
                                                uint8_t source);

/**
 * Sets the size of a multiviewer's picture-in-picture window.
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_set_multiviewer_pip_size(const mxr_remote_t *remote,
                                          mxr_uid_t device,
                                          uint8_t size);

/**
 * Sets which corner a multiviewer's picture-in-picture window sits in.
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_set_multiviewer_pip_position(const mxr_remote_t *remote,
                                              mxr_uid_t device,
                                              uint8_t position);

/**
 * Sets how a multiviewer fits a source into its window.
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_set_multiviewer_aspect_ratio(const mxr_remote_t *remote,
                                              mxr_uid_t device,
                                              uint8_t aspect);

/**
 * Turns a multiviewer's automatic source switching on or off.
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_set_multiviewer_auto_switch(const mxr_remote_t *remote,
                                             mxr_uid_t device,
                                             bool enable);

/**
 * Switches a multiviewer's output resolution.
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_set_multiviewer_output_mode(const mxr_remote_t *remote,
                                             mxr_uid_t device,
                                             uint8_t mode);

/**
 * Sets a multiviewer's IT content flag.
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_set_multiviewer_output_itc(const mxr_remote_t *remote,
                                            mxr_uid_t device,
                                            uint8_t mode);

/**
 * Switches a multiviewer's HDCP mode.
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_set_multiviewer_hdcp_mode(const mxr_remote_t *remote,
                                           mxr_uid_t device,
                                           uint8_t mode);

/**
 * Maps one of a multiviewer's inputs to a source device.
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_set_multiviewer_input_source(const mxr_remote_t *remote,
                                              mxr_uid_t device,
                                              uint8_t input,
                                              mxr_uid_t source);

/**
 * Asks a multiviewer to map its inputs to the sources it can see.
 *
 * A loadable module serves this, not the device firmware, and a model may
 * not have it. Nothing answers either way, so `MXR_OK` means the frame was
 * sent and not that anything acted on it.
 *
 * # Safety
 *
 * `remote` is null or a live handle from `mxr_remote_new()`.
 */
mxr_result_t mxr_multiviewer_auto_route(const mxr_remote_t *remote, mxr_uid_t device);

/**
 * Fills `out` with what is known about a device.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` points at a writable
 * [`mxr_device_info_t`].
 */
mxr_result_t mxr_device(const mxr_remote_t *remote, mxr_uid_t uid, mxr_device_info_t *out);

/**
 * Fills `out` with what is known about a bay.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` points at a writable
 * [`mxr_bay_info_t`].
 */
mxr_result_t mxr_bay(const mxr_remote_t *remote, mxr_bay_uid_t bay, mxr_bay_info_t *out);

/**
 * Fills `out` with the signal a bay measures.
 *
 * Fails with `MXR_ERR_NOT_REPORTED` on a bay that has sent no signal report.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` points at a writable
 * [`mxr_signal_details_t`].
 */
mxr_result_t mxr_bay_signal_details(const mxr_remote_t *remote,
                                    mxr_bay_uid_t bay,
                                    mxr_signal_details_t *out);

/**
 * Fills `out` with the audio a bay's signal report describes.
 *
 * Separate from `mxr_bay_signal_details()` because a report can carry video
 * and no audio: the video block is filled in whenever there is a signal,
 * while the audio block appears only once the source claims one. Fails with
 * `MXR_ERR_NOT_REPORTED` where the report carried none.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` points at a writable
 * [`mxr_audio_details_t`].
 */
mxr_result_t mxr_bay_audio_details(const mxr_remote_t *remote,
                                   mxr_bay_uid_t bay,
                                   mxr_audio_details_t *out);

/**
 * Copies the EDID a device last reported into `out`.
 *
 * `output` picks the EDID of the display on the device's output over the one
 * the device presents to the source on its input. Ask for one with
 * `mxr_request_edid()`; until a device has answered, or been overheard
 * answering a peer, this fails with `MXR_ERR_NOT_REPORTED`.
 *
 * `cap` must be at least `MXR_EDID_LEN`.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` points at `cap` writable
 * bytes.
 */
mxr_result_t mxr_device_edid(const mxr_remote_t *remote,
                             mxr_uid_t device,
                             bool output,
                             uint8_t *out,
                             size_t cap);

/**
 * Fills `out` with an amplifier zone's settings.
 *
 * Fails with `MXR_ERR_NOT_REPORTED` on a bay that is not an amplifier zone or
 * has not reported its settings.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` points at a writable
 * [`mxr_amp_zone_settings_t`].
 */
mxr_result_t mxr_bay_amp_settings(const mxr_remote_t *remote,
                                  mxr_bay_uid_t bay,
                                  mxr_amp_zone_settings_t *out);

/**
 * Writes a device's bays in port order, and returns how many there are.
 *
 * Returns the full count even when it exceeds `cap`, so calling with `cap`
 * zero sizes the buffer. Returns zero for a device never heard from.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` is null or points at `cap`
 * writable [`mxr_bay_uid_t`].
 */
size_t mxr_device_bays(const mxr_remote_t *remote, mxr_uid_t uid, mxr_bay_uid_t *out, size_t cap);

/**
 * Writes the temperatures a device reports, in its own order, in degrees
 * Celsius, and returns how many there are.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` is null or points at `cap`
 * writable bytes.
 */
size_t mxr_device_temperatures(const mxr_remote_t *remote, mxr_uid_t uid, uint8_t *out, size_t cap);

/**
 * Writes the devices whose signals a bay refuses, and returns how many there
 * are.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` is null or points at `cap`
 * writable [`mxr_uid_t`].
 */
size_t mxr_bay_filtered(const mxr_remote_t *remote, mxr_bay_uid_t bay, mxr_uid_t *out, size_t cap);

/**
 * Creates a client, without opening a socket yet.
 *
 * `config` may be null for the defaults. `callbacks` may be null, and so may
 * any member of it: an event with no function pointer is dropped. `userdata`
 * is passed back to every callback and is never examined.
 *
 * Returns null on failure, with the reason in
 * `mxr_last_error()`. The client must be released with
 * `mxr_remote_free()`.
 *
 * # Safety
 *
 * `config` and `callbacks` are null or point at initialised structs that
 * outlive the call, and every string in them is NUL-terminated. `userdata`
 * must remain valid, and safe to use from the library's own threads, until
 * `mxr_remote_free()` returns.
 */
mxr_remote_t *mxr_remote_new(const mxr_config_t *config,
                             const mxr_callbacks_t *callbacks,
                             void *userdata);

/**
 * Opens the socket and starts the receive and timer threads.
 *
 * # Safety
 *
 * `remote` is null or a handle from `mxr_remote_new()` that has not been
 * freed.
 */
mxr_result_t mxr_remote_start(const mxr_remote_t *remote);

/**
 * Stops the threads and closes the socket. Idempotent.
 *
 * A handle that has been closed can be freed but not restarted.
 *
 * # Safety
 *
 * `remote` is null or a handle from `mxr_remote_new()` that has not been
 * freed.
 */
void mxr_remote_close(const mxr_remote_t *remote);

/**
 * Closes the client and releases it. Null is ignored.
 *
 * This waits for the receive and timer threads to finish, so a callback
 * running when it is called returns before it does - which means calling it
 * from inside a callback would deadlock.
 *
 * # Safety
 *
 * `remote` is null or a handle from `mxr_remote_new()` that has not already
 * been freed, and no other thread is using it.
 */
void mxr_remote_free(mxr_remote_t *remote);

/**
 * Writes this client's own identifier.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` points at a writable
 * [`mxr_uid_t`].
 */
mxr_result_t mxr_remote_uid(const mxr_remote_t *remote, mxr_uid_t *out);

/**
 * Writes the name this client advertises.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` points at `cap` writable bytes.
 */
mxr_result_t mxr_remote_name(const mxr_remote_t *remote, char *out, size_t cap);

/**
 * Writes the address this client sends to.
 *
 * Fails with `MXR_ERR_NOT_CONNECTED` before `mxr_remote_start()`. `ip` needs
 * [`MXR_IP_STRING_LEN`] bytes; either output may be null to skip it.
 *
 * # Safety
 *
 * `remote` is null or a live handle, `ip` is null or points at `cap` writable
 * bytes, and `port` is null or points at a writable `uint16_t`.
 */
mxr_result_t mxr_remote_target(const mxr_remote_t *remote, char *ip, size_t cap, uint16_t *port);

/**
 * Writes how many frames from other senders have parsed since
 * `mxr_remote_start()`.
 *
 * It separates a mesh with nothing on it from an interface nothing is on: a
 * client that has discovered no device but is counting frames is hearing
 * traffic it cannot get answers from, which on a multi-homed host is what a
 * wrong `mxr_config_t::local_ip` looks like. Frames this client sent are not
 * counted, because the host loops its own multicast back whichever interface
 * was selected.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` points at a writable
 * `uint64_t`.
 */
mxr_result_t mxr_frames_received(const mxr_remote_t *remote, uint64_t *out);

/**
 * Writes every device heard from, and returns how many there are.
 *
 * Returns the full count even when it exceeds `cap`, so calling with `cap`
 * zero sizes the buffer. Returns zero on a null handle.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` is null or points at `cap`
 * writable [`mxr_uid_t`].
 */
size_t mxr_devices(const mxr_remote_t *remote, mxr_uid_t *out, size_t cap);

/**
 * Finds a device by its serial number.
 *
 * # Safety
 *
 * `remote` is null or a live handle, `serial` is a NUL-terminated string, and
 * `out` points at a writable [`mxr_uid_t`].
 */
mxr_result_t mxr_device_by_serial(const mxr_remote_t *remote, const char *serial, mxr_uid_t *out);

/**
 * Finds a device by serial number, name or identifier, in that order.
 *
 * # Safety
 *
 * `remote` is null or a live handle, `name` is a NUL-terminated string, and
 * `out` points at a writable [`mxr_uid_t`].
 */
mxr_result_t mxr_resolve_device(const mxr_remote_t *remote, const char *name, mxr_uid_t *out);

/**
 * Finds a bay on a device by the name the device gives its port.
 *
 * # Safety
 *
 * `remote` is null or a live handle, `port_name` is a NUL-terminated string,
 * and `out` points at a writable [`mxr_bay_uid_t`].
 */
mxr_result_t mxr_bay_by_name(const mxr_remote_t *remote,
                             mxr_uid_t device,
                             const char *port_name,
                             mxr_bay_uid_t *out);

/**
 * Finds the source bay advertising a multicast group.
 *
 * `audio` picks which of the bay's two streams the address is matched
 * against.
 *
 * # Safety
 *
 * `remote` is null or a live handle, `ip` is a NUL-terminated string, and
 * `out` points at a writable [`mxr_bay_uid_t`].
 */
mxr_result_t mxr_bay_by_stream_ip(const mxr_remote_t *remote,
                                  const char *ip,
                                  bool audio,
                                  mxr_bay_uid_t *out);

/**
 * Reopens the socket on a different interface, or in the other mode.
 *
 * `local_ip` may be null to let the host choose again.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `local_ip` is null or a
 * NUL-terminated string.
 */
mxr_result_t mxr_remote_update_config(const mxr_remote_t *remote,
                                      const char *local_ip,
                                      bool broadcast);

/**
 * Asks every device on the network to announce itself.
 *
 * # Safety
 *
 * `remote` is null or a live handle.
 */
mxr_result_t mxr_discover(const mxr_remote_t *remote);

/**
 * Fills `out` with a device's V2IP statistics.
 *
 * A device sends these only while subscribed; see
 * `mxr_subscribe_v2ip_stats()`.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` points at a writable
 * [`mxr_v2ip_stats_t`].
 */
mxr_result_t mxr_v2ip_stats(const mxr_remote_t *remote, mxr_uid_t uid, mxr_v2ip_stats_t *out);

/**
 * Fills `out` with a V2IP device's own encoder configuration.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` points at a writable
 * [`mxr_v2ip_details_t`].
 */
mxr_result_t mxr_v2ip_details(const mxr_remote_t *remote, mxr_uid_t uid, mxr_v2ip_details_t *out);

/**
 * Fills `out` with the streams a V2IP sink is subscribed to.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` points at a writable
 * [`mxr_v2ip_sink_t`].
 */
mxr_result_t mxr_v2ip_sink(const mxr_remote_t *remote, mxr_uid_t uid, mxr_v2ip_sink_t *out);

/**
 * Fills `out` with the window a sink is told to show.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` points at a writable
 * [`mxr_tiling_config_t`].
 */
mxr_result_t mxr_v2ip_tiling(const mxr_remote_t *remote, mxr_uid_t uid, mxr_tiling_config_t *out);

/**
 * Fills `out` with what a multiviewer reports about itself.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` points at a writable
 * [`mxr_multiviewer_status_t`].
 */
mxr_result_t mxr_multiviewer_status(const mxr_remote_t *remote,
                                    mxr_uid_t uid,
                                    mxr_multiviewer_status_t *out);

/**
 * Fills `out` with a ProAmp8's Dolby settings.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` points at a writable
 * [`mxr_dolby_settings_t`].
 */
mxr_result_t mxr_dolby_settings(const mxr_remote_t *remote,
                                mxr_uid_t uid,
                                mxr_dolby_settings_t *out);

/**
 * Fills `out` with a source bay's remote-control configuration.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` points at a writable
 * [`mxr_rc_settings_t`].
 */
mxr_result_t mxr_rc_settings(const mxr_remote_t *remote, mxr_uid_t uid, mxr_rc_settings_t *out);

/**
 * Writes the streams a device's source bays advertise, and returns how many
 * there are.
 *
 * Returns the full count even when it exceeds `cap`, so calling with `cap`
 * zero sizes the buffer.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` is null or points at `cap`
 * writable [`mxr_stream_sources_t`].
 */
size_t mxr_v2ip_sources(const mxr_remote_t *remote,
                        mxr_uid_t uid,
                        mxr_stream_sources_t *out,
                        size_t cap);

/**
 * Writes a device's network ports, and returns how many there are.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` is null or points at `cap`
 * writable [`mxr_network_port_t`].
 */
size_t mxr_network_status(const mxr_remote_t *remote,
                          mxr_uid_t uid,
                          mxr_network_port_t *out,
                          size_t cap);

/**
 * Writes a device's view of the mesh topology, and returns how many entries
 * there are.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` is null or points at `cap`
 * writable [`mxr_topology_entry_t`].
 */
size_t mxr_topology(const mxr_remote_t *remote,
                    mxr_uid_t uid,
                    mxr_topology_entry_t *out,
                    size_t cap);

/**
 * Writes the firmware versions a device reports, and returns how many there
 * are.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` is null or points at `cap`
 * writable [`mxr_firmware_version_t`].
 */
size_t mxr_device_firmware(const mxr_remote_t *remote,
                           mxr_uid_t uid,
                           mxr_firmware_version_t *out,
                           size_t cap);

/**
 * Writes a device's audio endpoints, in the order it reported them, and
 * returns how many there are.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` is null or points at `cap`
 * writable [`mxr_audio_endpoint_t`].
 */
size_t mxr_audio_endpoints(const mxr_remote_t *remote,
                           mxr_uid_t uid,
                           mxr_audio_endpoint_t *out,
                           size_t cap);

/**
 * Writes the endpoints hanging off one audio endpoint, and returns how many
 * there are.
 *
 * # Safety
 *
 * `remote` is null or a live handle, and `out` is null or points at `cap`
 * writable bytes.
 */
size_t mxr_audio_endpoint_children(const mxr_remote_t *remote,
                                   mxr_uid_t uid,
                                   uint8_t endpoint,
                                   uint8_t *out,
                                   size_t cap);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* MX_REMOTE_H */
