// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Data structures carried in frame payloads.

mod amp;
mod audio;
mod commands;
mod multiviewer;
mod network;
mod status;
mod v2ip;

pub use amp::{
    AmpDolbySettings, AmpZoneSettings, AMP_EQ_BANDS, AMP_TONE_FLAT, AMP_TONE_HTTP_MAX,
    AMP_TONE_HTTP_MIN,
};
pub use audio::{AudioEndpoint, AudioEndpoints, AudioFeatures, AudioLink};
pub use commands::{
    ActionTransmitRequest, AudioChangeSource, AudioClip, BayNameChange, EdidProfileChange,
    EdidRecord, EdidRequest, FactoryResetRequest, IrCapture, IrMeta, IrTransmitRequest,
    KeyTransmitRequest, MultiviewerCommand, RcSettings, RebootRequest, SetRouteRequest,
    V2ipBlacklistChange, V2ipPowerSaveRequest, V2ipTilingConfig, VideoWallCommand, VideoWallOp,
    VideoWallWindow, VIDEO_WALL_CLEARED, VIDEO_WALL_MIN_SIZE, VIDEO_WALL_POS_ALIGN,
    VIDEO_WALL_WIDTH_ALIGN,
};
pub use multiviewer::{MultiviewerStatus, MULTIVIEWER_INPUTS};
pub use network::{MacAddress, NetworkPortStatus, UtpCableStatus, UtpLinkErrors, VctStatus};
pub use status::{
    ArcStatus, BayAudioDetails, BayMirrorStatus, BaySignalDetails, ConnectStatus, DeviceStatus,
    FirmwareVersion, HiddenStatus, MuteStatus, PowerStatus, TopologyEntry, VolumeMuteStatus,
    VOLUME_UNCHANGED,
};
pub use v2ip::{
    DeviceV2ipDetails, DeviceV2ipSink, StreamKind, V2ipAudioFormat, V2ipDecoderState,
    V2ipDeviceStats, V2ipDscpConfig, V2ipRoute, V2ipRouteTarget, V2ipRxStats, V2ipScalingSettings,
    V2ipStreamSource, V2ipStreamSources, V2ipTxStats, SCALING_FLAGS_DEFINED,
    SCALING_FLAG_AUTO_SCALING, SCALING_FLAG_MODE_VALID, SCALING_FLAG_OPTIONS_VALID,
};

pub(crate) use v2ip::parse_dscp;
