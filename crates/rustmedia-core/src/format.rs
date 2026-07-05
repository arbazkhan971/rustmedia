//! Container format identification.

use std::fmt;

/// A media container format.
///
/// This identifies the *container* (how streams are wrapped), not the codecs
/// inside it. Detection from bytes lives in `rustmedia-formats`; this enum is
/// the shared vocabulary the whole toolkit uses to name a format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
#[non_exhaustive]
pub enum ContainerFormat {
    /// ISO Base Media File Format (MP4, `.mp4`/`.m4a`/`.m4v`).
    Mp4,
    /// QuickTime File Format (`.mov`) — an ISO-BMFF sibling.
    Mov,
    /// Matroska (`.mkv`).
    Matroska,
    /// WebM — a Matroska subset (`.webm`).
    WebM,
    /// RIFF WAVE (`.wav`).
    Wav,
    /// MPEG-1/2 Audio Layer III elementary stream (`.mp3`).
    Mp3,
    /// Free Lossless Audio Codec native stream (`.flac`).
    Flac,
    /// Ogg (`.ogg`/`.oga`/`.opus`).
    Ogg,
}

impl ContainerFormat {
    /// A short, stable lowercase name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            ContainerFormat::Mp4 => "mp4",
            ContainerFormat::Mov => "mov",
            ContainerFormat::Matroska => "matroska",
            ContainerFormat::WebM => "webm",
            ContainerFormat::Wav => "wav",
            ContainerFormat::Mp3 => "mp3",
            ContainerFormat::Flac => "flac",
            ContainerFormat::Ogg => "ogg",
        }
    }

    /// The canonical file extension (without the dot).
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            ContainerFormat::Mp4 => "mp4",
            ContainerFormat::Mov => "mov",
            ContainerFormat::Matroska => "mkv",
            ContainerFormat::WebM => "webm",
            ContainerFormat::Wav => "wav",
            ContainerFormat::Mp3 => "mp3",
            ContainerFormat::Flac => "flac",
            ContainerFormat::Ogg => "ogg",
        }
    }

    /// The most common MIME type for the format.
    #[must_use]
    pub fn mime_type(self) -> &'static str {
        match self {
            ContainerFormat::Mp4 => "video/mp4",
            ContainerFormat::Mov => "video/quicktime",
            ContainerFormat::Matroska => "video/x-matroska",
            ContainerFormat::WebM => "video/webm",
            ContainerFormat::Wav => "audio/wav",
            ContainerFormat::Mp3 => "audio/mpeg",
            ContainerFormat::Flac => "audio/flac",
            ContainerFormat::Ogg => "audio/ogg",
        }
    }

    /// Guess a format from a file-name extension (case-insensitive, with or
    /// without a leading dot). Returns `None` for unrecognised extensions.
    ///
    /// This is only a hint — real detection reads the file's magic bytes.
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        let ext = ext.trim_start_matches('.').to_ascii_lowercase();
        Some(match ext.as_str() {
            "mp4" | "m4a" | "m4v" | "m4b" => ContainerFormat::Mp4,
            "mov" | "qt" => ContainerFormat::Mov,
            "mkv" | "mka" => ContainerFormat::Matroska,
            "webm" => ContainerFormat::WebM,
            "wav" | "wave" => ContainerFormat::Wav,
            "mp3" => ContainerFormat::Mp3,
            "flac" => ContainerFormat::Flac,
            "ogg" | "oga" | "opus" => ContainerFormat::Ogg,
            _ => return None,
        })
    }
}

impl fmt::Display for ContainerFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
