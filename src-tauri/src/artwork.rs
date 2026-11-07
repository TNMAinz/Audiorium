//! On-demand embedded covers; never add image bytes to the library scan.
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::{Limit, MetadataOptions, MetadataRevision, StandardVisualKey, Visual};
use symphonia::core::probe::Hint;

const READ_BUDGET: usize = 16 * 1024 * 1024;
const MAX_COVER: usize = 8 * 1024 * 1024;
const THUMBNAIL: u32 = 256;

// A probe may seek to trailing metadata, but cannot read an entire large music file.
struct BoundedSource { file: File, remaining: usize }
impl Read for BoundedSource {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.remaining == 0 { return Ok(0); }
        let limit = buffer.len().min(self.remaining);
        let read = self.file.read(&mut buffer[..limit])?;
        self.remaining -= read;
        Ok(read)
    }
}
impl Seek for BoundedSource {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> { self.file.seek(from) }
}
impl MediaSource for BoundedSource {
    fn is_seekable(&self) -> bool { true }
    fn byte_len(&self) -> Option<u64> { self.file.metadata().ok().map(|meta| meta.len()) }
}

fn cover_in(revision: &MetadataRevision) -> Option<&Visual> {
    revision.visuals().iter()
        .filter(|visual| !visual.data.is_empty() && visual.data.len() <= MAX_COVER)
        .max_by_key(|visual| visual.usage == Some(StandardVisualKey::FrontCover))
}

fn thumbnail(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.is_empty() || bytes.len() > MAX_COVER { return None; }
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    let thumb = if decoded.width() > THUMBNAIL || decoded.height() > THUMBNAIL {
        decoded.thumbnail(THUMBNAIL, THUMBNAIL)
    } else { decoded };
    let mut output = Cursor::new(Vec::new());
    thumb.write_to(&mut output, image::ImageFormat::Png).ok()?;
    Some(output.into_inner())
}

fn extension(path: &Path) -> String {
    path.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// A WAV's own cover: the APIC frame of the ID3v2 tag the exporter writes, because
/// RIFF has no chunk for a picture. The tag sits before `data`, so the head is enough.
fn riff_cover(path: &Path) -> Option<Vec<u8>> {
    let mut head = Vec::new();
    File::open(path).ok()?.take(READ_BUDGET as u64).read_to_end(&mut head).ok()?;
    crate::library::id3v2_picture(crate::library::riff_id3(&head)?)
}

/// The cover exactly as the file carries it, with what that data is.
fn cover_visual(path: &Path) -> Option<(Vec<u8>, String)> {
    if !std::fs::metadata(path).ok()?.is_file() { return None; }
    if extension(path) == "wav" {
        if let Some(bytes) = riff_cover(path) {
            return Some((bytes.clone(), crate::export::sniff_mime(&bytes).to_string()));
        }
    }
    let file = File::open(path).ok()?;
    let stream = MediaSourceStream::new(Box::new(BoundedSource { file, remaining: READ_BUDGET }), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) { hint.with_extension(ext); }
    let mut probe = symphonia::default::get_probe().format(
        &hint, stream, &FormatOptions::default(),
        &MetadataOptions { limit_metadata_bytes: Limit::Maximum(1024 * 1024), limit_visual_bytes: Limit::Maximum(MAX_COVER) },
    ).ok()?;
    // Some containers keep pictures in their own metadata, others in a leading ID3 tag.
    if let Some(revision) = probe.format.metadata().current() {
        if let Some(visual) = cover_in(revision) {
            return Some((visual.data.to_vec(), visual.media_type.clone()));
        }
    }
    if let Some(metadata) = probe.metadata.get() {
        if let Some(revision) = metadata.current() {
            if let Some(visual) = cover_in(revision) {
                return Some((visual.data.to_vec(), visual.media_type.clone()));
            }
        }
    }
    None
}

fn embedded(path: &Path) -> Option<Vec<u8>> {
    cover_visual(path).and_then(|(bytes, _)| thumbnail(&bytes))
}

/// The cover as the source stores it, for a master to carry unchanged: no thumbnail,
/// no re-encode. An unreadable or oversized picture is nothing, not an error.
pub(crate) fn original_cover(path: &Path) -> (Vec<u8>, String) {
    cover_visual(path)
        .filter(|(bytes, _)| !bytes.is_empty() && bytes.len() <= MAX_COVER)
        .unwrap_or_default()
}

pub(crate) fn load(root: &str, path: &str) -> Result<Vec<u8>, String> {
    let root = Path::new(root).canonicalize().map_err(|_| "The library folder is unavailable".to_string())?;
    let path = Path::new(path).canonicalize().map_err(|_| "That music file is unavailable".to_string())?;
    if !root.is_dir() || !path.starts_with(&root) || !path.is_file() {
        return Err("Artwork must belong to a file in the library folder".into());
    }
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase();
    if !["mp3", "flac", "wav", "m4a", "aac", "ogg", "opus", "aiff", "aif"].contains(&extension.as_str()) {
        return Err("Artwork is read only from music files".into());
    }
    Ok(embedded(&path).unwrap_or_default())
}

#[tauri::command]
pub async fn track_artwork(root: String, path: String) -> Result<tauri::ipc::Response, String> {
    let bytes = tauri::async_runtime::spawn_blocking(move || load(&root, &path)).await
        .map_err(|_| "Artwork reading failed".to_string())??;
    Ok(tauri::ipc::Response::new(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphonia::core::meta::MetadataBuilder;
    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbImage::from_pixel(width, height, image::Rgb([32, 140, 220]));
        let mut output = Cursor::new(Vec::new());
        image.write_to(&mut output, image::ImageFormat::Png).unwrap();
        output.into_inner()
    }
    fn visual(usage: StandardVisualKey, data: Vec<u8>) -> Visual {
        Visual { media_type: "image/png".into(), dimensions: None, bits_per_pixel: None, color_mode: None,
            usage: Some(usage), tags: vec![], data: data.into_boxed_slice() }
    }
    #[test]
    fn front_cover_wins_and_oversized_pictures_are_ignored() {
        let mut builder = MetadataBuilder::new();
        builder.add_visual(visual(StandardVisualKey::BackCover, vec![1]));
        builder.add_visual(visual(StandardVisualKey::FrontCover, vec![2]));
        builder.add_visual(visual(StandardVisualKey::FrontCover, vec![0; MAX_COVER + 1]));
        let metadata = builder.metadata();
        assert_eq!(&*cover_in(&metadata).unwrap().data, &[2]);
    }
    #[test]
    fn decode_is_bounded_and_preserves_aspect() {
        let bytes = thumbnail(&png(1000, 500)).unwrap();
        let image = image::load_from_memory(&bytes).unwrap();
        assert_eq!((image.width(), image.height()), (256, 128));
        assert!(thumbnail(b"not an image").is_none());
        assert!(thumbnail(&vec![0; MAX_COVER + 1]).is_none());
        assert!(thumbnail(&png(8193, 1)).is_none());
    }
    #[test]
    fn path_must_be_music_inside_the_library() {
        let root = std::env::temp_dir().join(format!("audiorium-artwork-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let bad = root.join("not-music.txt");
        std::fs::write(&bad, b"not music").unwrap();
        assert!(load(root.to_str().unwrap(), bad.to_str().unwrap()).is_err());
        assert!(load(root.to_str().unwrap(), file!()).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn flac_picture_is_extracted_and_missing_picture_is_empty() {
        let root = std::env::temp_dir().join(format!("audiorium-flac-cover-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("cover.flac");
        // A minimal FLAC streaminfo followed by a standard PICTURE block.
        let mut picture = Vec::new();
        picture.extend(3u32.to_be_bytes());
        picture.extend(9u32.to_be_bytes()); picture.extend(b"image/png");
        picture.extend(0u32.to_be_bytes());
        for value in [64u32, 64, 24, 0] { picture.extend(value.to_be_bytes()); }
        let pixels = png(64, 64);
        picture.extend((pixels.len() as u32).to_be_bytes()); picture.extend(pixels);
        let mut flac = b"fLaC".to_vec();
        flac.extend([0, 0, 0, 34]);
        let mut info = [0u8; 34];
        info[0..4].copy_from_slice(&[0x10, 0, 0x10, 0]);
        let sample_info = (44100u64 << 44) | (1 << 41) | (15 << 36) | 44100;
        info[10..18].copy_from_slice(&sample_info.to_be_bytes());
        flac.extend(info);
        flac.push(0x86); flac.extend(&(picture.len() as u32).to_be_bytes()[1..]); flac.extend(picture);
        let mut frame = vec![0xff, 0xf8, 0x69, 0x18, 0, 255];
        let mut crc = 0u8;
        for byte in &frame {
            crc ^= byte;
            for _ in 0..8 { crc = if crc & 0x80 != 0 { (crc << 1) ^ 7 } else { crc << 1 }; }
        }
        frame.push(crc);
        frame.extend([0, 0, 0, 0, 0, 0]); // two constant-zero subframes
        let mut crc16 = 0u16;
        for byte in &frame {
            crc16 ^= (*byte as u16) << 8;
            for _ in 0..8 { crc16 = if crc16 & 0x8000 != 0 { (crc16 << 1) ^ 0x8005 } else { crc16 << 1 }; }
        }
        frame.extend(crc16.to_be_bytes());
        flac.extend(frame);
        std::fs::write(&path, flac).unwrap();
        let data = load(root.to_str().unwrap(), path.to_str().unwrap()).unwrap();
        assert!(!data.is_empty(), "embedded FLAC picture must survive the real probe");
        assert_eq!(image::load_from_memory(&data).unwrap().width(), 64);
        std::fs::write(&path, b"broken audio").unwrap();
        assert!(load(root.to_str().unwrap(), path.to_str().unwrap()).unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
