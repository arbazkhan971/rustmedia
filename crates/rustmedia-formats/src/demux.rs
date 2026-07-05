//! The [`Demuxer`] trait: the uniform interface every container parser exposes.

use std::time::Duration;

use rustmedia_core::{ContainerFormat, Metadata, Packet, Result, Track};

/// A demuxer reads a container and yields its tracks, metadata, and packets.
///
/// All of RustMedia's format parsers implement this trait, so higher layers
/// (the `rustmedia` facade, the CLI) can treat every container uniformly. The
/// trait is object-safe: a parser can be held as `Box<dyn Demuxer>`.
///
/// # Packet ordering
/// [`read_packet`](Demuxer::read_packet) yields packets in the order they are
/// laid out in the file (interleaved across tracks), which is the order most
/// efficient to read and the order a muxer wants them back.
pub trait Demuxer {
    /// The container format this demuxer is reading.
    fn format(&self) -> ContainerFormat;

    /// The tracks declared by the container.
    fn tracks(&self) -> &[Track];

    /// Container-level metadata (tags and chapters).
    fn metadata(&self) -> &Metadata;

    /// The overall media duration, if known — the longest track's duration.
    fn duration(&self) -> Option<Duration>;

    /// Read the next packet in file order, or `Ok(None)` at end of stream.
    fn read_packet(&mut self) -> Result<Option<Packet>>;

    /// Position the demuxer so that subsequent [`read_packet`](Demuxer::read_packet)
    /// calls begin at or before `target`, aligned to a keyframe for video.
    ///
    /// The default implementation returns [`Unsupported`](rustmedia_core::Error::Unsupported);
    /// seekable formats override it.
    fn seek(&mut self, target: Duration) -> Result<()> {
        let _ = target;
        Err(rustmedia_core::Error::unsupported(
            "this format does not support seeking",
        ))
    }
}
