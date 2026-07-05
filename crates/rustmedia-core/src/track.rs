//! Track descriptions: the per-stream metadata a demuxer exposes.

use std::time::Duration;

use crate::codec::{Codec, MediaType};
use crate::time::{Rational, Timestamp};

/// A single elementary stream within a container (one video, audio, or
/// subtitle track).
///
/// A `Track` is a pure description — it holds no samples. Use a demuxer to pull
/// [`Packet`](crate::packet::Packet)s for a given [`Track::id`].
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Track {
    /// Container-assigned track identifier. Unique within a file.
    pub id: u32,

    /// The codec carried by this track.
    pub codec: Codec,

    /// The broad kind of this track. Usually derived from [`Track::codec`], but
    /// stored explicitly because containers can disagree with their codecs.
    pub media_type: MediaType,

    /// Ticks per second for this track's timestamps.
    pub timescale: u32,

    /// Track duration in its own [`timescale`](Track::timescale), if known.
    pub duration: Option<Timestamp>,

    /// ISO 639-2/T language code (e.g. `"eng"`), if declared.
    pub language: Option<String>,

    /// Human-readable track name / title, if declared.
    pub name: Option<String>,

    /// Average bitrate in bits per second, if known or derivable.
    pub bitrate: Option<u64>,

    /// Codec initialisation data (e.g. an MP4 `avcC`/`esds` payload or a
    /// Matroska `CodecPrivate`). Required to remux the track into another
    /// container without re-encoding.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub codec_private: Option<Vec<u8>>,

    /// Codec-category-specific parameters.
    pub parameters: TrackParameters,
}

impl Track {
    /// The track's duration as a [`Duration`], if known.
    #[must_use]
    pub fn duration(&self) -> Option<Duration> {
        self.duration.map(Timestamp::to_duration)
    }

    /// `true` if this is a video track.
    #[must_use]
    pub fn is_video(&self) -> bool {
        self.media_type == MediaType::Video
    }

    /// `true` if this is an audio track.
    #[must_use]
    pub fn is_audio(&self) -> bool {
        self.media_type == MediaType::Audio
    }

    /// `true` if this is a subtitle track.
    #[must_use]
    pub fn is_subtitle(&self) -> bool {
        self.media_type == MediaType::Subtitle
    }

    /// Borrow the video parameters, if this is a video track.
    #[must_use]
    pub fn video(&self) -> Option<&VideoParameters> {
        match &self.parameters {
            TrackParameters::Video(v) => Some(v),
            _ => None,
        }
    }

    /// Borrow the audio parameters, if this is an audio track.
    #[must_use]
    pub fn audio(&self) -> Option<&AudioParameters> {
        match &self.parameters {
            TrackParameters::Audio(a) => Some(a),
            _ => None,
        }
    }
}

/// Codec-category-specific track parameters.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "kind", rename_all = "lowercase"))]
pub enum TrackParameters {
    /// Parameters for a video track.
    Video(VideoParameters),
    /// Parameters for an audio track.
    Audio(AudioParameters),
    /// Parameters for a subtitle track.
    Subtitle(SubtitleParameters),
    /// No category-specific parameters are available.
    None,
}

/// Parameters describing a video track.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VideoParameters {
    /// Coded frame width in pixels.
    pub width: u32,
    /// Coded frame height in pixels.
    pub height: u32,
    /// Nominal frame rate, if known.
    pub frame_rate: Option<Rational>,
    /// Display aspect ratio, if it differs from the coded ratio.
    pub display_aspect_ratio: Option<Rational>,
    /// Bits per colour component (e.g. `8`, `10`), if known.
    pub bit_depth: Option<u8>,
}

impl VideoParameters {
    /// The frame rate as an `f64`, if known.
    #[must_use]
    pub fn fps(&self) -> Option<f64> {
        self.frame_rate.map(Rational::as_f64)
    }
}

/// Parameters describing an audio track.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AudioParameters {
    /// Sample rate in hertz.
    pub sample_rate: u32,
    /// Number of channels.
    pub channels: u16,
    /// Bits per sample for PCM formats, if applicable.
    pub bits_per_sample: Option<u16>,
}

/// Parameters describing a subtitle track.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SubtitleParameters {
    /// Whether the subtitle stream carries bitmap (rather than text) cues.
    pub is_bitmap: bool,
}
