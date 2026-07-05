//! Native RIFF/WAVE (`.wav`) demuxer.
//!
//! WAV is a RIFF container: a `WAVE` form holding a `fmt ` chunk (the audio
//! format) and a `data` chunk (the samples), plus optional `LIST`/`INFO`
//! metadata. RustMedia reads the format, exposes a single audio track, and
//! streams the PCM payload as fixed-size packets so it can be remuxed or
//! trimmed like any other source.

use std::io::{Read, Seek};
use std::time::Duration;

use rustmedia_core::metadata::keys;
use rustmedia_core::{
    AudioParameters, Codec, ContainerFormat, Error, Metadata, Packet, Result, Timestamp, Track,
    TrackParameters,
};
use rustmedia_io::{ReadBytes, Source};

use crate::demux::Demuxer;

/// Roughly how many audio frames to return per packet when streaming PCM.
const FRAMES_PER_PACKET: u64 = 4096;

/// A demuxer for RIFF/WAVE audio files.
pub struct WavDemuxer<R: Read + Seek> {
    reader: R,
    track: Track,
    metadata: Metadata,
    duration: Option<Duration>,
    sample_rate: u32,
    block_align: u64,
    data_start: u64,
    data_len: u64,
    /// Bytes of the `data` chunk already served.
    pos: u64,
}

impl<R: Read + Seek> WavDemuxer<R> {
    /// Parse a WAV file's header, leaving it ready to stream audio packets.
    ///
    /// # Errors
    /// Returns [`Error::Malformed`] if the RIFF/WAVE structure or `fmt ` chunk
    /// is invalid, or [`Error::UnexpectedEof`] if the file is truncated.
    pub fn new(mut reader: R) -> Result<Self> {
        let file_size = reader.size()?;
        reader.seek_to(0)?;

        let riff = reader.read_fourcc()?;
        let _riff_size = reader.read_u32_le()?;
        let form = reader.read_fourcc()?;
        if &riff != b"RIFF" || &form != b"WAVE" {
            return Err(Error::malformed("wav", "missing RIFF/WAVE signature"));
        }

        let mut fmt: Option<WaveFormat> = None;
        let mut data_region: Option<(u64, u64)> = None;
        let mut metadata = Metadata::new();

        // Walk the chunks until the file ends.
        loop {
            let pos = reader.stream_position()?;
            if pos + 8 > file_size {
                break;
            }
            let id = reader.read_fourcc()?;
            let size = u64::from(reader.read_u32_le()?);
            let body_start = pos + 8;

            match &id {
                b"fmt " => fmt = Some(WaveFormat::parse(&mut reader, size)?),
                b"data" => {
                    // The declared size can exceed the file for streamed WAVs;
                    // clamp to what is actually present.
                    let available = file_size.saturating_sub(body_start);
                    data_region = Some((body_start, size.min(available)));
                }
                b"LIST" => {
                    let list_type = reader.read_fourcc()?;
                    if &list_type == b"INFO" {
                        parse_info_list(&mut reader, size.saturating_sub(4), &mut metadata)?;
                    }
                }
                _ => {}
            }

            // Chunks are padded to an even number of bytes.
            let advance = size + (size & 1);
            reader.seek_to(body_start + advance)?;
        }

        let fmt = fmt.ok_or_else(|| Error::malformed("wav", "missing 'fmt ' chunk"))?;
        let (data_start, data_len) =
            data_region.ok_or_else(|| Error::malformed("wav", "missing 'data' chunk"))?;

        let block_align = u64::from(fmt.block_align.max(1));
        let sample_rate = fmt.sample_rate.max(1);
        let frame_count = data_len / block_align;
        let duration = Some(Timestamp::new(frame_count as i64, sample_rate).to_duration());
        let bitrate = if fmt.byte_rate > 0 {
            Some(u64::from(fmt.byte_rate) * 8)
        } else {
            Some(u64::from(sample_rate) * block_align * 8)
        };

        let track = Track {
            id: 1,
            codec: fmt.codec.clone(),
            media_type: rustmedia_core::MediaType::Audio,
            timescale: sample_rate,
            duration: Some(Timestamp::new(frame_count as i64, sample_rate)),
            language: None,
            name: None,
            bitrate,
            codec_private: None,
            parameters: TrackParameters::Audio(AudioParameters {
                sample_rate,
                channels: fmt.channels.max(1),
                bits_per_sample: (fmt.bits_per_sample != 0).then_some(fmt.bits_per_sample),
            }),
        };

        Ok(WavDemuxer {
            reader,
            track,
            metadata,
            duration,
            sample_rate,
            block_align,
            data_start,
            data_len,
            pos: 0,
        })
    }
}

impl<R: Read + Seek> Demuxer for WavDemuxer<R> {
    fn format(&self) -> ContainerFormat {
        ContainerFormat::Wav
    }

    fn tracks(&self) -> &[Track] {
        std::slice::from_ref(&self.track)
    }

    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn duration(&self) -> Option<Duration> {
        self.duration
    }

    fn read_packet(&mut self) -> Result<Option<Packet>> {
        if self.pos >= self.data_len {
            return Ok(None);
        }
        let remaining = self.data_len - self.pos;
        let want = (self.block_align * FRAMES_PER_PACKET).min(remaining);
        // Keep packets frame-aligned unless this is the trailing remainder.
        let want = if want < remaining {
            (want - want % self.block_align).max(self.block_align)
        } else {
            want
        };

        let frame_index = self.pos / self.block_align;
        self.reader.seek_to(self.data_start + self.pos)?;
        let data = self.reader.read_vec(want as usize)?;
        self.pos += want;

        Ok(Some(Packet {
            track_id: 1,
            dts: Some(frame_index as i64),
            pts: Some(frame_index as i64),
            duration: Some(want / self.block_align),
            is_keyframe: true,
            data,
        }))
    }

    fn seek(&mut self, target: Duration) -> Result<()> {
        let frame = (target.as_secs_f64() * f64::from(self.sample_rate)) as u64;
        let byte = (frame * self.block_align).min(self.data_len);
        self.pos = byte - byte % self.block_align;
        Ok(())
    }
}

/// The fields RustMedia needs from a `fmt ` chunk.
struct WaveFormat {
    codec: Codec,
    channels: u16,
    sample_rate: u32,
    byte_rate: u32,
    block_align: u16,
    bits_per_sample: u16,
}

impl WaveFormat {
    fn parse<R: Read>(reader: &mut R, size: u64) -> Result<WaveFormat> {
        if size < 16 {
            return Err(Error::malformed("wav", "'fmt ' chunk too small"));
        }
        let mut format_tag = reader.read_u16_le()?;
        let channels = reader.read_u16_le()?;
        let sample_rate = reader.read_u32_le()?;
        let byte_rate = reader.read_u32_le()?;
        let block_align = reader.read_u16_le()?;
        let bits_per_sample = reader.read_u16_le()?;

        let mut consumed = 16u64;
        // WAVE_FORMAT_EXTENSIBLE (0xFFFE) carries the real tag in its sub-format.
        if format_tag == 0xFFFE && size >= 40 {
            let _ext_size = reader.read_u16_le()?; // cbSize
            let _valid_bits = reader.read_u16_le()?;
            let _channel_mask = reader.read_u32_le()?;
            let sub_format = reader.read_u16_le()?;
            reader.skip(14)?; // remainder of the 16-byte sub-format GUID
            format_tag = sub_format;
            consumed = 40;
        }

        // Skip any trailing bytes within the declared chunk size.
        if size > consumed {
            reader.skip(size - consumed)?;
        }

        let codec = codec_for(format_tag, bits_per_sample);
        Ok(WaveFormat {
            codec,
            channels,
            sample_rate,
            byte_rate,
            block_align,
            bits_per_sample,
        })
    }
}

/// Map a WAVE format tag + bit depth to a codec.
fn codec_for(format_tag: u16, bits: u16) -> Codec {
    match format_tag {
        // WAVE_FORMAT_PCM
        0x0001 => match bits {
            8 => Codec::PcmU8,
            16 => Codec::PcmS16Le,
            24 => Codec::PcmS24Le,
            _ => Codec::Other(format!("pcm_s{bits}le")),
        },
        // WAVE_FORMAT_IEEE_FLOAT
        0x0003 => {
            if bits == 32 {
                Codec::PcmF32Le
            } else {
                Codec::Other(format!("pcm_f{bits}le"))
            }
        }
        0x0006 => Codec::Other("pcm_alaw".to_string()),
        0x0007 => Codec::Other("pcm_mulaw".to_string()),
        0x0055 => Codec::Mp3,
        0x2000 => Codec::Ac3,
        other => Codec::Other(format!("wav_0x{other:04x}")),
    }
}

/// Parse a `LIST`/`INFO` block into normalised metadata tags.
fn parse_info_list<R: Read + Seek>(
    reader: &mut R,
    mut remaining: u64,
    metadata: &mut Metadata,
) -> Result<()> {
    while remaining >= 8 {
        let id = reader.read_fourcc()?;
        let size = u64::from(reader.read_u32_le()?);
        let padded = size + (size & 1);
        if padded + 8 > remaining && size > remaining {
            break;
        }
        let bytes = reader.read_vec(size as usize)?;
        if padded > size {
            reader.skip(padded - size)?;
        }
        if let Some(key) = info_key(&id) {
            let text = String::from_utf8_lossy(&bytes);
            let text = text.trim_end_matches(['\0', ' ']);
            if !text.is_empty() {
                metadata.insert(key, text);
            }
        }
        remaining = remaining.saturating_sub(8 + padded);
    }
    Ok(())
}

fn info_key(id: &[u8; 4]) -> Option<&'static str> {
    Some(match id {
        b"INAM" => keys::TITLE,
        b"IART" => keys::ARTIST,
        b"IPRD" => keys::ALBUM,
        b"ICMT" => keys::COMMENT,
        b"ICRD" => keys::DATE,
        b"IGNR" => keys::GENRE,
        b"ISFT" => keys::ENCODER,
        b"ICOP" => keys::COPYRIGHT,
        b"ITRK" | b"IPRT" => keys::TRACK,
        _ => return None,
    })
}
