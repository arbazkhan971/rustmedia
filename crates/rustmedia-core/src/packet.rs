//! Coded media packets — the unit a demuxer yields and a muxer consumes.

/// A single coded, undecoded unit of media data (one video frame, one audio
/// frame/chunk, or one subtitle cue) belonging to a track.
///
/// Timestamps are in the owning track's
/// [`timescale`](crate::track::Track::timescale). A packet is deliberately just
/// data plus timing: RustMedia moves packets between containers without ever
/// decoding them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    /// The [`Track::id`](crate::track::Track::id) this packet belongs to.
    pub track_id: u32,

    /// Decode timestamp in the track timescale, if known.
    pub dts: Option<i64>,

    /// Presentation timestamp in the track timescale, if known.
    pub pts: Option<i64>,

    /// Duration of this packet in the track timescale, if known.
    pub duration: Option<u64>,

    /// Whether this packet is a keyframe / sync sample — a valid random-access
    /// point. Trimming and seeking rely on this.
    pub is_keyframe: bool,

    /// The coded payload bytes.
    pub data: Vec<u8>,
}

impl Packet {
    /// Create a new packet for `track_id` carrying `data`.
    #[must_use]
    pub fn new(track_id: u32, data: Vec<u8>) -> Self {
        Packet {
            track_id,
            dts: None,
            pts: None,
            duration: None,
            is_keyframe: false,
            data,
        }
    }

    /// Builder-style setter for the presentation timestamp.
    #[must_use]
    pub fn with_pts(mut self, pts: i64) -> Self {
        self.pts = Some(pts);
        self
    }

    /// Builder-style setter for the decode timestamp.
    #[must_use]
    pub fn with_dts(mut self, dts: i64) -> Self {
        self.dts = Some(dts);
        self
    }

    /// Builder-style setter for the packet duration.
    #[must_use]
    pub fn with_duration(mut self, duration: u64) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Builder-style setter for the keyframe flag.
    #[must_use]
    pub fn keyframe(mut self, is_keyframe: bool) -> Self {
        self.is_keyframe = is_keyframe;
        self
    }

    /// The size of the packet payload in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// `true` if the packet carries no payload bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}
