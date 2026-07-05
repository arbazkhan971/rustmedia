//! Codec and media-type identification.

use std::fmt;

/// The broad category a track belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum MediaType {
    /// A moving-image (video) track.
    Video,
    /// An audio track.
    Audio,
    /// A timed-text / subtitle track.
    Subtitle,
    /// Timed metadata or other opaque data.
    Data,
    /// The track kind could not be determined.
    Unknown,
}

impl MediaType {
    /// A lowercase, stable string name (`"video"`, `"audio"`, …).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            MediaType::Video => "video",
            MediaType::Audio => "audio",
            MediaType::Subtitle => "subtitle",
            MediaType::Data => "data",
            MediaType::Unknown => "unknown",
        }
    }
}

impl fmt::Display for MediaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A media codec.
///
/// The enum models the codecs RustMedia recognises by name; [`Codec::Other`]
/// carries a container-specific identifier for anything not yet promoted to a
/// dedicated variant, and [`Codec::Unknown`] represents a wholly unrecognised
/// codec. Recognising a codec by name does *not* imply RustMedia can decode it
/// — the toolkit inspects and remuxes coded data, it does not transcode.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Codec {
    // ---- Video ----
    /// H.264 / AVC.
    H264,
    /// H.265 / HEVC.
    H265,
    /// AV1.
    Av1,
    /// VP8.
    Vp8,
    /// VP9.
    Vp9,
    /// MPEG-4 Part 2 Visual.
    Mpeg4Visual,
    /// Apple ProRes.
    ProRes,

    // ---- Audio ----
    /// Advanced Audio Coding.
    Aac,
    /// MPEG-1/2 Audio Layer III.
    Mp3,
    /// Opus.
    Opus,
    /// Vorbis.
    Vorbis,
    /// Free Lossless Audio Codec.
    Flac,
    /// Dolby Digital (AC-3).
    Ac3,
    /// Dolby Digital Plus (E-AC-3).
    Eac3,
    /// Apple Lossless.
    Alac,
    /// Linear PCM, signed 16-bit little-endian.
    PcmS16Le,
    /// Linear PCM, signed 16-bit big-endian.
    PcmS16Be,
    /// Linear PCM, signed 24-bit little-endian.
    PcmS24Le,
    /// Linear PCM, 32-bit float little-endian.
    PcmF32Le,
    /// Linear PCM, unsigned 8-bit.
    PcmU8,

    // ---- Subtitle ----
    /// 3GPP / QuickTime timed text.
    MovText,
    /// WebVTT.
    WebVtt,
    /// SubRip (SRT).
    SubRip,
    /// Advanced SubStation Alpha.
    Ass,

    /// A codec recognised by a container-specific identifier but not modelled
    /// as a dedicated variant (e.g. an uncommon fourcc or Matroska codec ID).
    Other(String),

    /// A wholly unrecognised codec.
    Unknown,
}

impl Codec {
    /// A short, human-friendly name for the codec (`"h264"`, `"aac"`, …).
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Codec::H264 => "h264",
            Codec::H265 => "h265",
            Codec::Av1 => "av1",
            Codec::Vp8 => "vp8",
            Codec::Vp9 => "vp9",
            Codec::Mpeg4Visual => "mpeg4",
            Codec::ProRes => "prores",
            Codec::Aac => "aac",
            Codec::Mp3 => "mp3",
            Codec::Opus => "opus",
            Codec::Vorbis => "vorbis",
            Codec::Flac => "flac",
            Codec::Ac3 => "ac3",
            Codec::Eac3 => "eac3",
            Codec::Alac => "alac",
            Codec::PcmS16Le => "pcm_s16le",
            Codec::PcmS16Be => "pcm_s16be",
            Codec::PcmS24Le => "pcm_s24le",
            Codec::PcmF32Le => "pcm_f32le",
            Codec::PcmU8 => "pcm_u8",
            Codec::MovText => "mov_text",
            Codec::WebVtt => "webvtt",
            Codec::SubRip => "subrip",
            Codec::Ass => "ass",
            Codec::Other(s) => s,
            Codec::Unknown => "unknown",
        }
    }

    /// The [`MediaType`] this codec produces.
    #[must_use]
    pub fn media_type(&self) -> MediaType {
        match self {
            Codec::H264
            | Codec::H265
            | Codec::Av1
            | Codec::Vp8
            | Codec::Vp9
            | Codec::Mpeg4Visual
            | Codec::ProRes => MediaType::Video,
            Codec::Aac
            | Codec::Mp3
            | Codec::Opus
            | Codec::Vorbis
            | Codec::Flac
            | Codec::Ac3
            | Codec::Eac3
            | Codec::Alac
            | Codec::PcmS16Le
            | Codec::PcmS16Be
            | Codec::PcmS24Le
            | Codec::PcmF32Le
            | Codec::PcmU8 => MediaType::Audio,
            Codec::MovText | Codec::WebVtt | Codec::SubRip | Codec::Ass => MediaType::Subtitle,
            Codec::Other(_) | Codec::Unknown => MediaType::Unknown,
        }
    }

    /// `true` if this codec stores linear PCM samples.
    #[must_use]
    pub fn is_pcm(&self) -> bool {
        matches!(
            self,
            Codec::PcmS16Le | Codec::PcmS16Be | Codec::PcmS24Le | Codec::PcmF32Le | Codec::PcmU8
        )
    }
}

impl fmt::Display for Codec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
