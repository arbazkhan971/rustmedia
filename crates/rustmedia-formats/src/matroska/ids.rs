//! Matroska/EBML element IDs used by the demuxer, stored with their length
//! marker bits (as they appear on disk).
#![allow(clippy::unreadable_literal)]

// Top level.
pub(super) const EBML_HEADER: u32 = 0x1A45DFA3;
pub(super) const DOCTYPE: u32 = 0x4282;
pub(super) const SEGMENT: u32 = 0x18538067;

// Info.
pub(super) const INFO: u32 = 0x1549A966;
pub(super) const TIMESTAMP_SCALE: u32 = 0x2AD7B1;
pub(super) const DURATION: u32 = 0x4489;
pub(super) const TITLE: u32 = 0x7BA9;
pub(super) const MUXING_APP: u32 = 0x4D80;
pub(super) const WRITING_APP: u32 = 0x5741;

// Tracks.
pub(super) const TRACKS: u32 = 0x1654AE6B;
pub(super) const TRACK_ENTRY: u32 = 0xAE;
pub(super) const TRACK_NUMBER: u32 = 0xD7;
pub(super) const TRACK_TYPE: u32 = 0x83;
pub(super) const CODEC_ID: u32 = 0x86;
pub(super) const CODEC_PRIVATE: u32 = 0x63A2;
pub(super) const TRACK_NAME: u32 = 0x536E;
pub(super) const LANGUAGE: u32 = 0x22B59C;
pub(super) const DEFAULT_DURATION: u32 = 0x23E383;

// Video sub-element.
pub(super) const VIDEO: u32 = 0xE0;
pub(super) const PIXEL_WIDTH: u32 = 0xB0;
pub(super) const PIXEL_HEIGHT: u32 = 0xBA;

// Audio sub-element.
pub(super) const AUDIO: u32 = 0xE1;
pub(super) const SAMPLING_FREQUENCY: u32 = 0xB5;
pub(super) const CHANNELS: u32 = 0x9F;
pub(super) const BIT_DEPTH: u32 = 0x6264;

// Clusters and blocks.
pub(super) const CLUSTER: u32 = 0x1F43B675;
pub(super) const CLUSTER_TIMESTAMP: u32 = 0xE7;
pub(super) const SIMPLE_BLOCK: u32 = 0xA3;
pub(super) const BLOCK_GROUP: u32 = 0xA0;
pub(super) const BLOCK: u32 = 0xA1;
pub(super) const REFERENCE_BLOCK: u32 = 0xFB;
