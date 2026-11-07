//! Writing a render to a file.
//!
//! One 48 kHz stereo file, streamed as it is rendered so a long file never sits in
//! memory, and quantised honestly: TPDF dither on the integer depths, none on the
//! float one, so the noise a 16-bit file carries is noise and not distortion. At 16
//! bits that dither is also noise-shaped: the quantiser's own error is fed back
//! through a first-order difference before the next sample is rounded, which moves
//! the noise up out of the middle of the band and costs no headroom. A WAV
//! patches its two lengths in the header when it finishes, which is the one thing it
//! cannot know before the last frame arrives; a FLAC patches its `STREAMINFO` the same
//! way, because the frame count and the sample digest are only known at the end.
//!
//! A master also carries the description of the file it was made from: a `LIST`/`INFO`
//! chunk for title, artist and album, and an `id3 ` chunk when the source had a cover,
//! because RIFF has no picture chunk and an ID3v2 tag inside a WAV is the only in-file
//! convention that carries one. Both chunks sit before `data`, so a player that skips
//! them never seeks past the audio to find it.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

use flacenc::bitsink::ByteSink;
use flacenc::component::{BitRepr, StreamInfo};
use flacenc::config;
use flacenc::encode_fixed_size_frame;
use flacenc::error::Verify;
use flacenc::error::Verified;
use flacenc::source::{Fill, FrameBuf};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

/// The one rate the whole engine runs at. Only the convenience constructors and
/// the tests still name it: a master carries its own rate.
#[cfg(test)]
const RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
/// Samples are staged here and written in one call, never a syscall per frame.
const CHUNK: usize = 64 * 1024;
/// A FLAC frame holds this many samples per channel: the reference encoder's default,
/// and small enough that a long file is never held anywhere but on disk.
const FLAC_BLOCK: usize = 4_096;
/// The 34 bytes of a `STREAMINFO` body, which is patched in place when a FLAC ends.
const STREAMINFO_LEN: usize = 34;

/// Which container a master is written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Wav,
    Flac,
}

impl Format {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "wav" => Self::Wav,
            "flac" => Self::Flac,
            _ => return None,
        })
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Flac => "flac",
        }
    }
}

/// The rates a master can be written at. The rack always runs at 48 kHz — every
/// processor is tuned for it — so a master at another rate is the same work
/// converted once, after the last stage, by `resample::Resampler`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SampleRate {
    Hz44100,
    Hz48000,
    Hz96000,
}

impl SampleRate {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "44.1" => Self::Hz44100,
            "48" => Self::Hz48000,
            "96" => Self::Hz96000,
            _ => return None,
        })
    }

    pub fn hz(self) -> u32 {
        match self {
            Self::Hz44100 => 44_100,
            Self::Hz48000 => 48_000,
            Self::Hz96000 => 96_000,
        }
    }

    /// "44.1 kHz": the figure a release sheet carries.
    pub fn label(self) -> &'static str {
        match self {
            Self::Hz44100 => "44.1 kHz",
            Self::Hz48000 => "48 kHz",
            Self::Hz96000 => "96 kHz",
        }
    }
}

/// What the render status says a finished master is: the depth and the container.
pub fn describe(depth: BitDepth, format: Format) -> String {
    match format {
        Format::Wav => depth.label().to_string(),
        Format::Flac => format!("{}-bit FLAC", depth.bytes() * 8),
    }
}

/// What a master says about itself, copied from the source it was rendered from.
///
/// An empty field writes no chunk, so a source with no tags produces a master with
/// no metadata rather than a tag full of blanks.
#[derive(Clone, Default, Debug)]
pub struct Tags {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// The cover as the source carried it, and the type that says what the bytes are.
    pub cover: Vec<u8>,
    pub cover_mime: String,
}

impl Tags {
    fn is_empty(&self) -> bool {
        self.title.is_empty() && self.artist.is_empty() && self.album.is_empty() && self.cover.is_empty()
    }
}

/// One RIFF chunk: its four-byte name, its length, its body, and the pad byte RIFF
/// requires when the body is an odd number of bytes.
fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + body.len() + 1);
    out.extend_from_slice(id);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
    out
}

/// The `LIST`/`INFO` body: the fields Windows and every tag-aware player read from a
/// WAV. Text is UTF-8; a reader that assumes Latin-1 shows ASCII names unchanged.
fn info_body(tags: &Tags) -> Option<Vec<u8>> {
    let mut body = b"INFO".to_vec();
    let mut wrote = false;
    for (id, value) in [(b"INAM", &tags.title), (b"IART", &tags.artist), (b"IPRD", &tags.album)] {
        if value.is_empty() {
            continue;
        }
        let mut text = value.as_bytes().to_vec();
        text.push(0);
        body.extend_from_slice(&chunk(id, &text));
        wrote = true;
    }
    if wrote {
        let software = format!("AUDIORIUM {}", env!("CARGO_PKG_VERSION"));
        let mut text = software.into_bytes();
        text.push(0);
        body.extend_from_slice(&chunk(b"ISFT", &text));
    }
    wrote.then_some(body)
}

/// An ID3v2.3 tag: text in UTF-16 so a title that is not Latin-1 survives, and the
/// cover as a front-cover `APIC` frame. This is what foobar2000, VLC and MusicBee read
/// from a WAV; a player that ignores it hears the file unchanged.
fn id3_body(tags: &Tags) -> Option<Vec<u8>> {
    fn frame(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(10 + body.len());
        out.extend_from_slice(id);
        // ID3v2.3 frame sizes are plain big-endian, not syncsafe.
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(body);
        out
    }
    let text = |id: &[u8; 4], value: &str| -> Option<Vec<u8>> {
        if value.is_empty() {
            return None;
        }
        let mut body = vec![1u8];
        body.extend_from_slice(&[0xFF, 0xFE]);
        for unit in value.encode_utf16() {
            body.extend_from_slice(&unit.to_le_bytes());
        }
        body.extend_from_slice(&[0, 0]);
        Some(frame(id, &body))
    };
    let mut frames = Vec::new();
    for (id, value) in [(b"TIT2", &tags.title), (b"TPE1", &tags.artist), (b"TALB", &tags.album)] {
        if let Some(bytes) = text(id, value) {
            frames.extend_from_slice(&bytes);
        }
    }
    if !tags.cover.is_empty() {
        let mime = if tags.cover_mime.is_empty() { sniff_mime(&tags.cover) } else { &tags.cover_mime };
        let mut body = vec![0u8]; // Latin-1, so an empty description can be one NUL.
        body.extend_from_slice(mime.as_bytes());
        body.push(0);
        body.push(3); // Front cover.
        body.push(0);
        body.extend_from_slice(&tags.cover);
        frames.extend_from_slice(&frame(b"APIC", &body));
    }
    if frames.is_empty() {
        return None;
    }
    let mut tag = b"ID3".to_vec();
    tag.extend_from_slice(&[3, 0, 0]);
    // The tag header's size is syncsafe in every ID3v2 version.
    let size = frames.len() as u32;
    tag.extend_from_slice(&[
        ((size >> 21) & 0x7F) as u8,
        ((size >> 14) & 0x7F) as u8,
        ((size >> 7) & 0x7F) as u8,
        (size & 0x7F) as u8,
    ]);
    tag.extend_from_slice(&frames);
    Some(tag)
}

/// What the bytes are, when the source did not say.
pub(crate) fn sniff_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else {
        "image/jpeg"
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BitDepth {
    Sixteen,
    TwentyFour,
    Float32,
}

impl BitDepth {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "16" => Self::Sixteen,
            "24" => Self::TwentyFour,
            "32f" => Self::Float32,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Sixteen => "16-bit PCM",
            Self::TwentyFour => "24-bit PCM",
            Self::Float32 => "32-bit float",
        }
    }

    /// Bytes per sample, which is also the block alignment divided by the channels.
    pub fn bytes(self) -> u16 {
        match self {
            Self::Sixteen => 2,
            Self::TwentyFour => 3,
            Self::Float32 => 4,
        }
    }

    /// 1 for PCM, 3 for IEEE float, as a WAV's format chunk names them.
    fn tag(self) -> u16 {
        match self {
            Self::Float32 => 3,
            _ => 1,
        }
    }

    fn max_code(self) -> Option<i32> {
        match self {
            Self::Sixteen => Some(32_767),
            Self::TwentyFour => Some(8_388_607),
            Self::Float32 => None,
        }
    }
}

/// Floor LSB dither: the sum of two independent uniforms, which is triangular and
/// centres the quantisation error instead of leaving it correlated with the signal.
/// At 16 bits the noise is shaped as well: what a sample rounded to is compared with
/// what it should have been, and that error is subtracted from the next sample before
/// it is quantised. The error the listener hears is then the *difference* between two
/// successive errors, a first-order high pass: the noise that would sit in the middle
/// of the band is moved towards the top of it, where it is least audible.
struct Dither {
    state: u32,
    /// Shaping is only worth its cost at 16 bits, where the dither floor sits close
    /// enough to the programme to hear. A 24-bit master is dithered flat.
    shaped: bool,
    /// The error of the last sample, per channel, which the shaper subtracts.
    error: [f32; 2],
}

impl Dither {
    fn new(shaped: bool) -> Self {
        // A fixed seed: the same render is the same file, which is what makes a
        // second render comparable with the first.
        Self { state: 0x1234_5678, shaped, error: [0.0; 2] }
    }

    fn uniform(&mut self) -> f32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        (self.state as f32 / u32::MAX as f32) - 0.5
    }

    /// ±1 LSB, triangular: two uniforms, one LSB apart at their widest.
    fn tpdf(&mut self) -> f32 {
        self.uniform() + self.uniform()
    }
}

pub struct WavWriter {
    file: BufWriter<File>,
    depth: BitDepth,
    /// The rate the header names, which is the rate of the samples written.
    rate: u32,
    dither: Dither,
    staged: Vec<u8>,
    data_bytes: u64,
    /// Where in the file the `data` chunk's length is written when the render ends.
    data_size_at: u64,
    /// How long the header turned out, which depends on the chunks the tags ask for.
    header_bytes: u64,
}

impl WavWriter {
    #[cfg(test)]
    pub fn create(path: &Path, bit_depth: BitDepth, tags: &Tags) -> Result<Self, String> {
        Self::create_at(path, bit_depth, tags, RATE)
    }

    /// The same writer at a chosen rate, for a master that is not 48 kHz.
    pub fn create_at(
        path: &Path,
        bit_depth: BitDepth,
        tags: &Tags,
        rate: u32,
    ) -> Result<Self, String> {
        Self::create_with(path, bit_depth, tags, bit_depth == BitDepth::Sixteen, rate)
    }

    /// The same writer with the shaping chosen, so a test can hold the flat dither to
    /// its own behaviour instead of the shaper's.
    fn create_with(
        path: &Path,
        bit_depth: BitDepth,
        tags: &Tags,
        shaped: bool,
        rate: u32,
    ) -> Result<Self, String> {
        let file = File::create(path)
            .map_err(|error| format!("Cannot write that file: {error}"))?;
        let mut writer = Self {
            file: BufWriter::new(file),
            depth: bit_depth,
            rate,
            dither: Dither::new(shaped),
            staged: Vec::with_capacity(CHUNK),
            data_bytes: 0,
            data_size_at: 0,
            header_bytes: 0,
        };
        writer.write_header(tags)?;
        Ok(writer)
    }

    /// The header, with both lengths left at zero until `finish` knows them. The fmt
    /// chunk always comes first so a reader that stops at the first unknown chunk still
    /// finds the format; the tags follow, then `data`.
    fn write_header(&mut self, tags: &Tags) -> Result<(), String> {
        let block_align = CHANNELS * self.depth.bytes();
        let byte_rate = self.rate * block_align as u32;
        let mut fmt = Vec::with_capacity(16);
        fmt.extend_from_slice(&self.depth.tag().to_le_bytes());
        fmt.extend_from_slice(&CHANNELS.to_le_bytes());
        fmt.extend_from_slice(&self.rate.to_le_bytes());
        fmt.extend_from_slice(&byte_rate.to_le_bytes());
        fmt.extend_from_slice(&block_align.to_le_bytes());
        fmt.extend_from_slice(&(self.depth.bytes() * 8).to_le_bytes());

        let mut header = Vec::with_capacity(64);
        header.extend_from_slice(b"RIFF");
        header.extend_from_slice(&0u32.to_le_bytes());
        header.extend_from_slice(b"WAVE");
        header.extend_from_slice(&chunk(b"fmt ", &fmt));
        if !tags.is_empty() {
            if let Some(body) = info_body(tags) {
                header.extend_from_slice(&chunk(b"LIST", &body));
            }
            if let Some(body) = id3_body(tags) {
                header.extend_from_slice(&chunk(b"id3 ", &body));
            }
        }
        header.extend_from_slice(b"data");
        self.data_size_at = header.len() as u64;
        header.extend_from_slice(&0u32.to_le_bytes());
        self.header_bytes = header.len() as u64;
        self.file
            .write_all(&header)
            .map_err(|error| format!("Cannot write that file: {error}"))
    }

    pub fn write(&mut self, frames: &[[f32; 2]]) -> Result<(), String> {
        for frame in frames {
            for (channel, value) in frame.iter().enumerate() {
                match self.depth.max_code() {
                    Some(max) => {
                        let code = self.quantise(*value, max, channel);
                        match self.depth {
                            BitDepth::Sixteen => {
                                self.staged.extend_from_slice(&(code as i16).to_le_bytes());
                            }
                            _ => {
                                let low = code as u32 & 0x00ff_ffff;
                                self.staged.extend_from_slice(&low.to_le_bytes()[..3]);
                            }
                        }
                    }
                    None => {
                        // Float output is the samples themselves: nothing is added to
                        // them, and the ceiling is already held upstream.
                        let clamped = if value.is_finite() { value.clamp(-1.0, 1.0) } else { 0.0 };
                        self.staged.extend_from_slice(&clamped.to_le_bytes());
                    }
                }
            }
            self.data_bytes += (CHANNELS * self.depth.bytes()) as u64;
        }
        if self.staged.len() >= CHUNK {
            self.flush_staged()?;
        }
        Ok(())
    }

    /// One sample at the depth the file carries: clamp, scale, dither, round.
    fn quantise(&mut self, value: f32, max: i32, channel: usize) -> i32 {
        quantise(value, max, &mut self.dither, channel)
    }

    fn flush_staged(&mut self) -> Result<(), String> {
        if self.staged.is_empty() {
            return Ok(());
        }
        let result = self
            .file
            .write_all(&self.staged)
            .map_err(|error| format!("Cannot write that file: {error}"));
        self.staged.clear();
        result
    }

    /// The lengths are known now, so they are written where the header left blanks.
    pub fn finish(mut self) -> Result<u64, String> {
        self.flush_staged()?;
        // The buffer has to reach the file before a seek can put the lengths behind it.
        self.file
            .flush()
            .map_err(|error| format!("Cannot finish that file: {error}"))?;
        if self.header_bytes + self.data_bytes - 8 > u32::MAX as u64 {
            return Err("That render is longer than a WAV file can hold".into());
        }
        let riff = (self.header_bytes - 8 + self.data_bytes) as u32;
        let data = self.data_bytes as u32;
        let data_size_at = self.data_size_at;
        let file = self.file.get_mut();
        file.seek(SeekFrom::Start(4))
            .and_then(|_| file.write_all(&riff.to_le_bytes()))
            .and_then(|_| file.seek(SeekFrom::Start(data_size_at)))
            .and_then(|_| file.write_all(&data.to_le_bytes()))
            .map_err(|error| format!("Cannot finish that file: {error}"))?;
        self.file
            .flush()
            .map_err(|error| format!("Cannot finish that file: {error}"))?;
        Ok(self.header_bytes + self.data_bytes)
    }
}

/// One sample at the depth the file carries: clamp, scale, shape the error, dither,
/// round. Shared by both containers, so a FLAC and a WAV of the same master carry the
/// same codes. `channel` picks which shaper state the sample belongs to: shaping both
/// channels through one error would feed the left channel's mistake into the right
/// one, which is a stereo noise floor rather than a shaped one.
fn quantise(value: f32, max: i32, dither: &mut Dither, channel: usize) -> i32 {
    let clamped = if value.is_finite() { value.clamp(-1.0, 1.0) } else { 0.0 };
    let scaled = clamped * max as f32;
    // What this sample should have been, once the previous error is taken out of it.
    // Subtracting that error is what turns a flat noise floor into a tilted one.
    let target = if dither.shaped { scaled - dither.error[channel] } else { scaled };
    let dithered = target + dither.tpdf();
    // The negative side reaches one code further than the positive side, as every
    // two's-complement PCM file does.
    let low = -(max as f32) - 1.0;
    let code = dithered.round().clamp(low, max as f32) as i32;
    // The residual includes the dither itself: that is the noise the shaper works on,
    // and leaving it out would leave the dither flat and only tilt the signal error.
    let ceiling = 4.0 * max as f32;
    dither.error[channel] = (code as f32 - target).clamp(-ceiling, ceiling);
    code
}

// ---------- FLAC ----------

/// One metadata block: the last-block flag, the type, and a 24-bit big-endian length.
fn flac_block(last: bool, kind: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + body.len());
    out.push(if last { 0x80 | kind } else { kind });
    let length = body.len() as u32;
    out.extend_from_slice(&length.to_be_bytes()[1..]);
    out.extend_from_slice(body);
    out
}

/// Vorbis comments: the fields a FLAC already carries, so a master is titled, credited
/// and filed like the file it came from.
fn vorbis_comment(tags: &Tags) -> Option<Vec<u8>> {
    let mut fields: Vec<(&str, &str)> = Vec::new();
    for (name, value) in [
        ("TITLE", tags.title.as_str()),
        ("ARTIST", tags.artist.as_str()),
        ("ALBUM", tags.album.as_str()),
    ] {
        if !value.is_empty() {
            fields.push((name, value));
        }
    }
    if fields.is_empty() {
        return None;
    }
    let vendor = b"AUDIORIUM";
    let mut body = Vec::new();
    body.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    body.extend_from_slice(vendor);
    body.extend_from_slice(&((fields.len() + 1) as u32).to_le_bytes());
    let encoder = format!("ENCODER=AUDIORIUM {}", env!("CARGO_PKG_VERSION"));
    for entry in fields
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .chain(std::iter::once(encoder))
    {
        body.extend_from_slice(&(entry.len() as u32).to_le_bytes());
        body.extend_from_slice(entry.as_bytes());
    }
    Some(body)
}

/// A front-cover `PICTURE` block, which is how a FLAC carries an image.
fn flac_picture(tags: &Tags) -> Option<Vec<u8>> {
    if tags.cover.is_empty() {
        return None;
    }
    let mime = if tags.cover_mime.is_empty() {
        sniff_mime(&tags.cover)
    } else {
        tags.cover_mime.as_str()
    };
    // A picture block states its own dimensions; the bytes are the source's own.
    let (width, height) = image::load_from_memory(&tags.cover)
        .map(|picture| (picture.width(), picture.height()))
        .unwrap_or((0, 0));
    let mut body = Vec::new();
    body.extend_from_slice(&3u32.to_be_bytes()); // Front cover.
    body.extend_from_slice(&(mime.len() as u32).to_be_bytes());
    body.extend_from_slice(mime.as_bytes());
    body.extend_from_slice(&0u32.to_be_bytes()); // No description.
    for value in [width, height, 24, 0] {
        body.extend_from_slice(&value.to_be_bytes());
    }
    body.extend_from_slice(&(tags.cover.len() as u32).to_be_bytes());
    body.extend_from_slice(&tags.cover);
    Some(body)
}

/// The 34-byte `STREAMINFO` body of a stream description.
fn stream_info_body(info: &StreamInfo) -> Result<Vec<u8>, String> {
    let mut sink = ByteSink::new();
    info.write(&mut sink)
        .map_err(|error| format!("Cannot describe the FLAC stream: {error:?}"))?;
    Ok(sink.as_slice().to_vec())
}

/// A FLAC written as it is rendered: `STREAMINFO` first with its totals blank, the tags
/// beside it, then one frame per 4096 samples. The encoder compresses every block as it
/// arrives, so nothing but the block being coded is held in memory, and the stream
/// description is patched in place at the end with the frame count and sample digest.
pub struct FlacWriter {
    file: BufWriter<File>,
    depth: BitDepth,
    dither: Dither,
    config: Verified<config::Encoder>,
    stream_info: StreamInfo,
    block: Vec<i32>,
    filled: usize,
    frame_number: usize,
    total_frames: u64,
    min_frame_bytes: u32,
    max_frame_bytes: u32,
    digest: Md5,
    bytes_written: u64,
    stream_info_at: u64,
}

impl FlacWriter {
    /// The stream at a chosen rate, which `STREAMINFO` states and every player reads
    /// back.
    pub fn create_at(
        path: &Path,
        bit_depth: BitDepth,
        tags: &Tags,
        rate: u32,
    ) -> Result<Self, String> {
        Self::create_with(path, bit_depth, tags, bit_depth == BitDepth::Sixteen, rate)
    }

    fn create_with(
        path: &Path,
        bit_depth: BitDepth,
        tags: &Tags,
        shaped: bool,
        rate: u32,
    ) -> Result<Self, String> {
        let bits = match bit_depth {
            BitDepth::Sixteen => 16,
            BitDepth::TwentyFour => 24,
            BitDepth::Float32 => {
                return Err("FLAC carries 16- or 24-bit samples, not 32-bit float".into())
            }
        };
        let file = File::create(path)
            .map_err(|error| format!("Cannot write that file: {error}"))?;
        let stream_info = StreamInfo::new(rate as usize, 2, bits)
            .map_err(|error| format!("Cannot describe the FLAC stream: {error:?}"))?;
        let config = config::Encoder::default()
            .into_verified()
            .map_err(|error| format!("Cannot configure the FLAC encoder: {error:?}"))?;
        let mut writer = Self {
            file: BufWriter::new(file),
            depth: bit_depth,
            dither: Dither::new(shaped),
            config,
            stream_info,
            block: Vec::with_capacity(FLAC_BLOCK * 2),
            filled: 0,
            frame_number: 0,
            total_frames: 0,
            min_frame_bytes: 0,
            max_frame_bytes: 0,
            digest: Md5::new(),
            bytes_written: 0,
            stream_info_at: 0,
        };
        writer.write_header(tags)?;
        Ok(writer)
    }

    /// `fLaC`, the stream description with its totals still blank, whatever metadata the
    /// tags ask for, and then nothing but frames.
    fn write_header(&mut self, tags: &Tags) -> Result<(), String> {
        let stream_info = stream_info_body(&self.stream_info)?;
        if stream_info.len() != STREAMINFO_LEN {
            return Err("The FLAC stream description is not the length the format says".into());
        }
        let picture = flac_picture(tags);
        let comment = vorbis_comment(tags);
        let mut header = b"fLaC".to_vec();
        // STREAMINFO is not the last block when anything follows it.
        header.extend_from_slice(&flac_block(comment.is_none() && picture.is_none(), 0, &stream_info));
        self.stream_info_at = 8;
        if let Some(body) = comment {
            header.extend_from_slice(&flac_block(picture.is_none(), 4, &body));
        }
        if let Some(body) = picture {
            header.extend_from_slice(&flac_block(true, 6, &body));
        }
        self.file
            .write_all(&header)
            .map_err(|error| format!("Cannot write that file: {error}"))?;
        self.bytes_written = header.len() as u64;
        Ok(())
    }

    pub fn write(&mut self, frames: &[[f32; 2]]) -> Result<(), String> {
        let max = self
            .depth
            .max_code()
            .ok_or("FLAC carries 16- or 24-bit samples, not 32-bit float")?;
        let width = self.depth.bytes() as usize;
        for frame in frames {
            for (channel, value) in frame.iter().enumerate() {
                let code = quantise(*value, max, &mut self.dither, channel);
                self.block.push(code);
                // The stream digest covers the samples as little-endian bytes.
                self.digest.update(&code.to_le_bytes()[..width]);
            }
            self.filled += 1;
            self.total_frames += 1;
            if self.filled == FLAC_BLOCK {
                self.flush_block()?;
            }
        }
        Ok(())
    }

    /// Codes the block that is filled and appends its frame to the file.
    fn flush_block(&mut self) -> Result<(), String> {
        if self.filled == 0 {
            return Ok(());
        }
        let mut framebuf = FrameBuf::with_size(2, FLAC_BLOCK)
            .map_err(|error| format!("Cannot size a FLAC block: {error:?}"))?;
        framebuf
            .fill_interleaved(&self.block[..self.filled * 2])
            .map_err(|error| format!("Cannot fill a FLAC block: {error:?}"))?;
        let frame = encode_fixed_size_frame(
            &self.config,
            &framebuf,
            self.frame_number,
            &self.stream_info,
        )
        .map_err(|error| format!("Cannot encode a FLAC frame: {error:?}"))?;
        let mut sink = ByteSink::new();
        frame
            .write(&mut sink)
            .map_err(|error| format!("Cannot write a FLAC frame: {error:?}"))?;
        let bytes = sink.as_slice();
        self.file
            .write_all(bytes)
            .map_err(|error| format!("Cannot write that file: {error}"))?;
        self.bytes_written += bytes.len() as u64;
        let length = bytes.len() as u32;
        if self.min_frame_bytes == 0 || length < self.min_frame_bytes {
            self.min_frame_bytes = length;
        }
        self.max_frame_bytes = self.max_frame_bytes.max(length);
        self.frame_number += 1;
        self.filled = 0;
        self.block.clear();
        Ok(())
    }

    /// The last block, then the description the header left blank.
    pub fn finish(mut self) -> Result<u64, String> {
        self.flush_block()?;
        let digest: [u8; 16] = self.digest.finalize().into();
        self.stream_info.set_total_samples(self.total_frames as usize);
        self.stream_info.set_md5_digest(&digest);
        self.stream_info
            .set_block_sizes(FLAC_BLOCK, FLAC_BLOCK)
            .map_err(|error| format!("Cannot describe the FLAC blocks: {error:?}"))?;
        self.stream_info
            .set_frame_sizes(
                self.min_frame_bytes.max(1) as usize,
                self.max_frame_bytes.max(1) as usize,
            )
            .map_err(|error| format!("Cannot describe the FLAC frames: {error:?}"))?;
        let body = stream_info_body(&self.stream_info)?;
        self.file
            .flush()
            .map_err(|error| format!("Cannot finish that file: {error}"))?;
        let at = self.stream_info_at;
        let file = self.file.get_mut();
        file.seek(SeekFrom::Start(at))
            .and_then(|_| file.write_all(&body))
            .map_err(|error| format!("Cannot finish that file: {error}"))?;
        self.file
            .flush()
            .map_err(|error| format!("Cannot finish that file: {error}"))?;
        Ok(self.bytes_written)
    }
}

/// The writer a render asked for. Both containers take the same frames and the same
/// tags; only the bytes underneath differ.
pub enum Writer {
    Wav(WavWriter),
    Flac(FlacWriter),
}

impl Writer {
    pub fn create(
        path: &Path,
        format: Format,
        depth: BitDepth,
        tags: &Tags,
        rate: u32,
    ) -> Result<Self, String> {
        Ok(match format {
            Format::Wav => Self::Wav(WavWriter::create_at(path, depth, tags, rate)?),
            Format::Flac => Self::Flac(FlacWriter::create_at(path, depth, tags, rate)?),
        })
    }

    pub fn write(&mut self, frames: &[[f32; 2]]) -> Result<(), String> {
        match self {
            Self::Wav(writer) => writer.write(frames),
            Self::Flac(writer) => writer.write(frames),
        }
    }

    pub fn finish(self) -> Result<u64, String> {
        match self {
            Self::Wav(writer) => writer.finish(),
            Self::Flac(writer) => writer.finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "audiorium-export-{name}-{}-{stamp}.wav",
            std::process::id()
        ))
    }

    fn sine(frames: usize) -> Vec<[f32; 2]> {
        (0..frames)
            .map(|index| {
                let value = (2.0 * std::f32::consts::PI * 440.0 * index as f32 / RATE as f32).sin() * 0.5;
                [value, -value]
            })
            .collect()
    }

    fn read(path: &std::path::Path) -> Vec<u8> {
        std::fs::read(path).expect("the written file")
    }

    fn u16_at(bytes: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([bytes[at], bytes[at + 1]])
    }

    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    }

    #[test]
    fn the_header_describes_the_file_that_was_written() {
        let path = scratch("header");
        let frames = sine(500);
        let mut writer = WavWriter::create(&path, BitDepth::Sixteen, &Tags::default()).unwrap();
        writer.write(&frames).unwrap();
        let length = writer.finish().unwrap();
        let bytes = read(&path);
        let _ = std::fs::remove_file(&path);

        assert_eq!(length, bytes.len() as u64);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(u32_at(&bytes, 16), 16, "fmt chunk length");
        assert_eq!(u16_at(&bytes, 20), 1, "PCM tag");
        assert_eq!(u16_at(&bytes, 22), 2, "channels");
        assert_eq!(u32_at(&bytes, 24), RATE);
        assert_eq!(u32_at(&bytes, 28), RATE * 4, "byte rate");
        assert_eq!(u16_at(&bytes, 32), 4, "block alignment");
        assert_eq!(u16_at(&bytes, 34), 16, "bits per sample");
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(u32_at(&bytes, 40) as usize, bytes.len() - 44, "data length");
        assert_eq!(u32_at(&bytes, 4) as usize, bytes.len() - 8, "riff length");
        assert_eq!(bytes.len(), 44 + frames.len() * 4);
    }

    #[test]
    fn float_output_is_the_samples_that_went_in() {
        let path = scratch("float");
        let frames = sine(64);
        let mut writer = WavWriter::create(&path, BitDepth::Float32, &Tags::default()).unwrap();
        writer.write(&frames).unwrap();
        writer.finish().unwrap();
        let bytes = read(&path);
        let _ = std::fs::remove_file(&path);

        assert_eq!(u16_at(&bytes, 20), 3, "float tag");
        assert_eq!(u16_at(&bytes, 34), 32, "bits per sample");
        for (index, frame) in frames.iter().enumerate() {
            for channel in 0..2 {
                let at = 44 + (index * 2 + channel) * 4;
                let value = f32::from_le_bytes([
                    bytes[at],
                    bytes[at + 1],
                    bytes[at + 2],
                    bytes[at + 3],
                ]);
                assert_eq!(value.to_bits(), frame[channel].to_bits(), "sample {index}/{channel}");
            }
        }
    }

    #[test]
    fn integer_output_stays_within_a_code_and_the_dither_centres_the_error() {
        let path = scratch("pcm");
        let frames = sine(20_000);
        // The flat writer on purpose: this is the TPDF property, and the shaper
        // deliberately trades that per-sample bound for a tilted spectrum.
        let mut writer =
            WavWriter::create_with(&path, BitDepth::Sixteen, &Tags::default(), false, RATE).unwrap();
        writer.write(&frames).unwrap();
        writer.finish().unwrap();
        let bytes = read(&path);
        let _ = std::fs::remove_file(&path);

        let max = 32_767.0_f32;
        let mut sum = 0.0_f64;
        let mut count = 0_u64;
        let mut flat = Vec::with_capacity(frames.len() * 2);
        for index in 0..frames.len() {
            for channel in 0..2 {
                let at = 44 + (index * 2 + channel) * 2;
                let code = i16::from_le_bytes([bytes[at], bytes[at + 1]]) as f32;
                flat.push(code);
                let error = code - frames[index][channel] * max;
                // One code of dither plus half a code of rounding is the widest the
                // error can be.
                assert!(error.abs() <= 1.5, "sample {index} was {error} codes out");
                sum += error as f64;
                count += 1;
            }
        }
        let mean = sum / count as f64;
        assert!(mean.abs() < 0.01, "mean error {mean} codes: the dither is not centring it");
        assert!(
            flat.iter().any(|code| code.abs() > 100.0),
            "the file carried no signal"
        );
    }

    /// The quantisation error of the left channel, in codes, for a run of frames.
    fn left_errors(bytes: &[u8], frames: &[[f32; 2]]) -> Vec<f32> {
        let max = 32_767.0f32;
        (0..frames.len())
            .map(|index| {
                let at = 44 + (index * 2) * 2;
                let code = i16::from_le_bytes([bytes[at], bytes[at + 1]]) as f32;
                code - frames[index][0].clamp(-1.0, 1.0) * max
            })
            .collect()
    }

    #[test]
    fn sixteen_bit_dither_shapes_the_noise_out_of_the_middle_of_the_band() {
        let frames = sine(40_000);
        let flat_path = scratch("flat-dither");
        let shaped_path = scratch("shaped-dither");
        let mut flat =
            WavWriter::create_with(&flat_path, BitDepth::Sixteen, &Tags::default(), false, RATE).unwrap();
        flat.write(&frames).unwrap();
        flat.finish().unwrap();
        let mut shaped =
            WavWriter::create_with(&shaped_path, BitDepth::Sixteen, &Tags::default(), true, RATE).unwrap();
        shaped.write(&frames).unwrap();
        shaped.finish().unwrap();
        let flat_bytes = read(&flat_path);
        let shaped_bytes = read(&shaped_path);
        let _ = std::fs::remove_file(&flat_path);
        let _ = std::fs::remove_file(&shaped_path);

        // One pole at about 500 Hz: how much of the error survives in the part of the
        // band where a listener hears a noise floor.
        let low = |errors: &[f32]| {
            let a = 1.0 - (-2.0 * std::f32::consts::PI * 500.0 / RATE as f32).exp();
            let mut y = 0.0f32;
            let mut sum = 0.0f64;
            for error in errors {
                y += a * (error - y);
                sum += (y as f64).powi(2);
            }
            sum
        };
        let total = |errors: &[f32]| errors.iter().map(|e| (*e as f64).powi(2)).sum::<f64>();
        let flat_errors = left_errors(&flat_bytes, &frames);
        let shaped_errors = left_errors(&shaped_bytes, &frames);
        let flat_low = low(&flat_errors);
        let shaped_low = low(&shaped_errors);
        assert!(
            shaped_low < flat_low * 0.25,
            "the shaped dither kept {:.1} % of the low-band error power",
            100.0 * shaped_low / flat_low
        );
        // The noise was moved, not removed: total power rises while the audible part
        // falls, which is the whole trade.
        assert!(
            total(&shaped_errors) > total(&flat_errors),
            "the shaping hid the noise instead of tilting it"
        );
        // The tone is still there and still within a code of where it should be in
        // the audible sense: no sample ran away from the signal.
        let worst = shaped_errors
            .iter()
            .fold(0.0f32, |worst, error| worst.max(error.abs()));
        assert!(worst < 4.0, "a sample landed {worst} codes from the signal");

        // A shaped master is reproducible: the same render is the same bytes.
        let again_path = scratch("shaped-again");
        let mut again =
            WavWriter::create_with(&again_path, BitDepth::Sixteen, &Tags::default(), true, RATE).unwrap();
        again.write(&frames).unwrap();
        again.finish().unwrap();
        assert_eq!(read(&again_path), shaped_bytes, "the shaper is not deterministic");
        let _ = std::fs::remove_file(&again_path);
    }

    #[test]
    fn impossible_samples_become_silence_rather_than_a_broken_file() {
        let path = scratch("hostile");
        let mut writer = WavWriter::create(&path, BitDepth::Float32, &Tags::default()).unwrap();
        writer
            .write(&[[f32::NAN, 2.0], [f32::INFINITY, -3.0], [0.5, -0.5]])
            .unwrap();
        writer.finish().unwrap();
        let bytes = read(&path);
        let _ = std::fs::remove_file(&path);

        for index in 0..3 {
            for channel in 0..2 {
                let at = 44 + (index * 2 + channel) * 4;
                let value =
                    f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
                assert!(value.is_finite(), "sample {index}/{channel} is not finite");
                assert!((-1.0..=1.0).contains(&value), "sample {index}/{channel} = {value}");
            }
        }
    }

    #[test]
    fn an_unwritable_path_is_reported() {
        let path = std::env::temp_dir()
            .join("audiorium-export-missing")
            .join("nested")
            .join("file.wav");
        let reason = WavWriter::create(&path, BitDepth::Sixteen, &Tags::default())
            .err()
            .expect("a missing directory should be refused");
        assert!(reason.contains("Cannot write"), "{reason}");
    }

    #[test]
    fn a_master_carries_its_title_artist_album_and_cover() {
        let path = scratch("tags");
        // A one-pixel PNG, so the picture bytes are real image data and not a marker.
        let cover = {
            let image = image::RgbImage::from_pixel(1, 1, image::Rgb([9, 9, 9]));
            let mut output = std::io::Cursor::new(Vec::new());
            image.write_to(&mut output, image::ImageFormat::Png).unwrap();
            output.into_inner()
        };
        let tags = Tags {
            title: "First Light".into(),
            artist: "Nova".into(),
            album: "Night".into(),
            cover: cover.clone(),
            cover_mime: "image/png".into(),
        };
        let mut writer = WavWriter::create(&path, BitDepth::TwentyFour, &tags).unwrap();
        writer.write(&sine(64)).unwrap();
        let length = writer.finish().unwrap();
        let bytes = read(&path);
        let _ = std::fs::remove_file(&path);

        assert_eq!(length, bytes.len() as u64);
        // The text chunk Windows and tag-aware players read from a WAV.
        assert!(bytes.windows(4).any(|window| window == b"LIST"));
        assert!(bytes.windows(4).any(|window| window == b"INAM"));
        assert!(bytes.windows(4).any(|window| window == b"IART"));
        assert!(bytes.windows(9).any(|window| window == b"First Lig"));
        // And an ID3v2 tag for the cover, which RIFF has no chunk for.
        assert!(bytes.windows(4).any(|window| window == b"id3 "));
        assert!(bytes.windows(4).any(|window| window == b"TIT2"));
        assert!(bytes.windows(4).any(|window| window == b"APIC"));
        assert!(bytes.windows(9).any(|window| window == b"image/png"));
        assert!(bytes.windows(cover.len()).any(|window| window == cover.as_slice()), "the cover bytes are not in the file");
        // The audio still starts where the header says, and the lengths describe it.
        let data_at = bytes.windows(4).position(|window| window == b"data").unwrap();
        let data_len = u32_at(&bytes, data_at + 4) as usize;
        assert_eq!(data_len, bytes.len() - data_at - 8, "the data chunk does not describe the audio");
        assert_eq!(u32_at(&bytes, 4) as usize, bytes.len() - 8, "the RIFF length is wrong");
    }

    #[test]
    fn a_source_with_no_tags_writes_no_tag_chunks() {
        let path = scratch("plain");
        let mut writer = WavWriter::create(&path, BitDepth::Sixteen, &Tags::default()).unwrap();
        writer.write(&sine(32)).unwrap();
        writer.finish().unwrap();
        let bytes = read(&path);
        let _ = std::fs::remove_file(&path);
        assert!(!bytes.windows(4).any(|window| window == b"LIST"));
        assert!(!bytes.windows(4).any(|window| window == b"id3 "));
        assert_eq!(&bytes[36..40], b"data", "the audio no longer follows the format chunk");
    }

    #[test]
    fn the_depth_names_are_the_ones_the_interface_offers() {
        assert_eq!(BitDepth::parse("16"), Some(BitDepth::Sixteen));
        assert_eq!(BitDepth::parse("24"), Some(BitDepth::TwentyFour));
        assert_eq!(BitDepth::parse("32f"), Some(BitDepth::Float32));
        assert_eq!(BitDepth::parse("32"), None);
        assert_eq!(BitDepth::TwentyFour.bytes(), 3);
        assert_eq!(BitDepth::Float32.label(), "32-bit float");
    }
}
