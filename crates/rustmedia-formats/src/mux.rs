//! The [`Muxer`] trait: the uniform interface for writing a container.

use rustmedia_core::{Metadata, Packet, Result, Track};

/// A muxer writes tracks and packets into a container format.
///
/// The lifecycle is: [`start`](Muxer::start) with the track list, then
/// [`write_packet`](Muxer::write_packet) for each packet (referencing a track
/// by its [`Track::id`]), then [`finish`](Muxer::finish) to flush the trailer.
///
/// Packets are copied through untouched — a muxer never re-encodes — so the
/// tracks' [`codec_private`](Track::codec_private) data must describe the same
/// coded bitstream the packets carry.
pub trait Muxer {
    /// Begin a file with the given tracks. Must be called exactly once, before
    /// any packets.
    fn start(&mut self, tracks: &[Track]) -> Result<()>;

    /// Attach container-level metadata (tags). Optional; call before `finish`.
    /// The default implementation ignores it.
    fn set_metadata(&mut self, metadata: &Metadata) -> Result<()> {
        let _ = metadata;
        Ok(())
    }

    /// Write one packet. Its [`Packet::track_id`] must match a track passed to
    /// [`start`](Muxer::start).
    fn write_packet(&mut self, packet: &Packet) -> Result<()>;

    /// Finish the file, writing any trailer (for MP4, the `moov` and `mdat`).
    fn finish(&mut self) -> Result<()>;
}
