//! Container-level metadata: tags and chapters.

use std::collections::BTreeMap;

use crate::time::Timestamp;

/// Well-known metadata tag keys.
///
/// RustMedia normalises the many container-specific spellings (iTunes atoms,
/// Matroska tag names, ID3 frames, …) onto these lowercase keys so callers do
/// not have to know which container a file came from. Unrecognised tags are
/// preserved under their original key.
pub mod keys {
    /// Work title.
    pub const TITLE: &str = "title";
    /// Primary artist / performer.
    pub const ARTIST: &str = "artist";
    /// Album / collection name.
    pub const ALBUM: &str = "album";
    /// Album artist.
    pub const ALBUM_ARTIST: &str = "album_artist";
    /// Composer.
    pub const COMPOSER: &str = "composer";
    /// Genre.
    pub const GENRE: &str = "genre";
    /// Release date or year.
    pub const DATE: &str = "date";
    /// Track number (optionally `n/total`).
    pub const TRACK: &str = "track";
    /// Disc number (optionally `n/total`).
    pub const DISC: &str = "disc";
    /// Free-form comment.
    pub const COMMENT: &str = "comment";
    /// Encoder / muxing application.
    pub const ENCODER: &str = "encoder";
    /// Copyright notice.
    pub const COPYRIGHT: &str = "copyright";
}

/// A named chapter marking a point (and optional span) in a timeline.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Chapter {
    /// Start of the chapter.
    pub start: Timestamp,
    /// End of the chapter, if the container declares one.
    pub end: Option<Timestamp>,
    /// Chapter title.
    pub title: String,
}

/// Container-level metadata: a bag of string tags plus chapter markers.
///
/// Tag keys are normalised (see [`keys`]); values are UTF-8 strings. Insertion
/// order is not preserved — tags are stored in a [`BTreeMap`] so iteration is
/// deterministic (alphabetical), which keeps CLI and `--json` output stable.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Metadata {
    tags: BTreeMap<String, String>,
    /// Chapter markers, in timeline order.
    pub chapters: Vec<Chapter>,
}

impl Metadata {
    /// An empty metadata set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace a tag. The `key` is lowercased for consistency with
    /// [`keys`]; the value is stored verbatim.
    pub fn insert(&mut self, key: impl AsRef<str>, value: impl Into<String>) {
        self.tags
            .insert(key.as_ref().to_ascii_lowercase(), value.into());
    }

    /// Look up a tag value by key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.tags.get(key).map(String::as_str)
    }

    /// The work title, if present.
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.get(keys::TITLE)
    }

    /// The primary artist, if present.
    #[must_use]
    pub fn artist(&self) -> Option<&str> {
        self.get(keys::ARTIST)
    }

    /// The album name, if present.
    #[must_use]
    pub fn album(&self) -> Option<&str> {
        self.get(keys::ALBUM)
    }

    /// Iterate over all `(key, value)` tag pairs in alphabetical key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.tags.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// The number of tags (excluding chapters).
    #[must_use]
    pub fn len(&self) -> usize {
        self.tags.len()
    }

    /// `true` if there are no tags and no chapters.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty() && self.chapters.is_empty()
    }
}
