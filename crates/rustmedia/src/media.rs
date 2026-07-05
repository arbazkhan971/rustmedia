//! The [`Media`] facade — the ergonomic front door to RustMedia.

use std::path::Path;
use std::time::Duration;

use rustmedia_core::{ContainerFormat, Metadata, Packet, Result, Track};
use rustmedia_formats::{open, Demuxer};

/// An opened media file.
///
/// `Media` wraps a format-specific demuxer behind one small, friendly surface.
/// Open a file, read its tracks and metadata, then stream its packets:
///
/// ```no_run
/// use rustmedia::Media;
///
/// let mut media = Media::open("movie.mp4")?;
/// println!("format: {}", media.format());
/// if let Some(d) = media.duration() {
///     println!("duration: {:.3}s", d.as_secs_f64());
/// }
/// for track in media.tracks() {
///     println!("track #{}: {} ({})", track.id, track.codec, track.media_type);
/// }
///
/// while let Some(packet) = media.read_packet()? {
///     // move `packet` into a muxer, write it out, inspect it, …
///     let _ = packet;
/// }
/// # Ok::<(), rustmedia::Error>(())
/// ```
pub struct Media {
    demuxer: Box<dyn Demuxer>,
    size_bytes: u64,
}

impl Media {
    /// Open a media file by path, detecting its container format automatically.
    ///
    /// # Errors
    /// Returns an error if the file cannot be opened, its format is not
    /// recognised, or the container is malformed.
    pub fn open(path: impl AsRef<Path>) -> Result<Media> {
        let path = path.as_ref();
        let size_bytes = std::fs::metadata(path).map_or(0, |m| m.len());
        let file = std::fs::File::open(path)?;
        let demuxer = open(file)?;
        Ok(Media {
            demuxer,
            size_bytes,
        })
    }

    /// Open media from any seekable reader (for example an in-memory
    /// [`Cursor`](std::io::Cursor)).
    ///
    /// # Errors
    /// Returns an error if the format is not recognised or is malformed.
    pub fn from_reader<R>(reader: R) -> Result<Media>
    where
        R: std::io::Read + std::io::Seek + 'static,
    {
        let demuxer = open(reader)?;
        Ok(Media {
            demuxer,
            size_bytes: 0,
        })
    }

    /// The detected container format.
    #[must_use]
    pub fn format(&self) -> ContainerFormat {
        self.demuxer.format()
    }

    /// The overall media duration, if known.
    #[must_use]
    pub fn duration(&self) -> Option<Duration> {
        self.demuxer.duration()
    }

    /// All tracks in the file.
    #[must_use]
    pub fn tracks(&self) -> &[Track] {
        self.demuxer.tracks()
    }

    /// Container-level metadata (tags and chapters).
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        self.demuxer.metadata()
    }

    /// The size of the source file in bytes (0 if opened from a non-file reader).
    #[must_use]
    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    /// Iterate over the video tracks.
    pub fn video_tracks(&self) -> impl Iterator<Item = &Track> {
        self.tracks().iter().filter(|t| t.is_video())
    }

    /// Iterate over the audio tracks.
    pub fn audio_tracks(&self) -> impl Iterator<Item = &Track> {
        self.tracks().iter().filter(|t| t.is_audio())
    }

    /// The first video track, if any.
    #[must_use]
    pub fn best_video(&self) -> Option<&Track> {
        self.video_tracks().next()
    }

    /// The first audio track, if any.
    #[must_use]
    pub fn best_audio(&self) -> Option<&Track> {
        self.audio_tracks().next()
    }

    /// Read the next packet in file order, or `Ok(None)` at end of stream.
    ///
    /// # Errors
    /// Returns an error if reading the underlying source fails.
    pub fn read_packet(&mut self) -> Result<Option<Packet>> {
        self.demuxer.read_packet()
    }

    /// An iterator over the remaining packets.
    ///
    /// Each item is a `Result<Packet>`; iteration stops at end of stream or on
    /// the first error.
    pub fn packets(&mut self) -> Packets<'_> {
        Packets {
            media: self,
            done: false,
        }
    }

    /// Seek so subsequent reads begin at or before `target`, keyframe-aligned
    /// for video.
    ///
    /// # Errors
    /// Returns [`Error::Unsupported`] if the format cannot seek.
    pub fn seek(&mut self, target: Duration) -> Result<()> {
        self.demuxer.seek(target)
    }

    /// Consume the `Media`, returning the underlying boxed demuxer for advanced
    /// use.
    #[must_use]
    pub fn into_demuxer(self) -> Box<dyn Demuxer> {
        self.demuxer
    }
}

/// An iterator over a [`Media`]'s packets, yielded by [`Media::packets`].
pub struct Packets<'a> {
    media: &'a mut Media,
    done: bool,
}

impl Iterator for Packets<'_> {
    type Item = Result<Packet>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match self.media.read_packet() {
            Ok(Some(packet)) => Some(Ok(packet)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }
}

/// Re-exported so callers can name the error type without a second `use`.
pub use rustmedia_core::Error as MediaError;
