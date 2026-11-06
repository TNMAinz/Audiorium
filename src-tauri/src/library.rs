//! Local library browsing.
//!
//! There is no playback engine yet, so this module only walks a folder and reads
//! what a file already says about itself. Tags come from ID3v2/ID3v1 (MP3) and
//! Vorbis comments (FLAC); every other container falls back to the file name.
//! Nothing is copied, uploaded or played.

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const AUDIO_EXTENSIONS: [&str; 9] = ["mp3", "flac", "wav", "m4a", "aac", "ogg", "opus", "aiff", "aif"];
const MAX_TRACKS: usize = 5000;
const MAX_DEPTH: usize = 12;
/// Leading metadata is always at the start of the file, so only the head is read.
const HEAD_BYTES: u64 = 262_144;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    path: String,
    file_name: String,
    title: String,
    artist: String,
    album: String,
    format: String,
    bytes: u64,
    modified: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryScan {
    root: String,
    tracks: Vec<Track>,
    skipped: usize,
    truncated: bool,
}

fn extension_of(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| *byte as char).collect()
}

fn utf16(bytes: &[u8]) -> String {
    let big_endian = matches!(bytes.first(), Some(0xFE));
    let body = if bytes.len() >= 2 { &bytes[2..] } else { bytes };
    let units: Vec<u16> = body
        .chunks_exact(2)
        .map(|pair| {
            if big_endian {
                u16::from_be_bytes([pair[0], pair[1]])
            } else {
                u16::from_le_bytes([pair[0], pair[1]])
            }
        })
        .collect();
    String::from_utf16_lossy(&units)
}

/// Decodes an ID3 text frame body, whose first byte selects the encoding.
fn text_frame(body: &[u8]) -> Option<String> {
    let (encoding, payload) = body.split_first()?;
    let value = match encoding {
        0 => latin1(payload),
        1 => utf16(payload),
        2 => {
            let units: Vec<u16> = payload
                .chunks_exact(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        }
        3 => String::from_utf8_lossy(payload).into_owned(),
        _ => return None,
    };
    let trimmed = value.trim_end_matches('\0').trim().to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn syncsafe(value: &[u8]) -> usize {
    ((value[0] as usize & 0x7F) << 21)
        | ((value[1] as usize & 0x7F) << 14)
        | ((value[2] as usize & 0x7F) << 7)
        | (value[3] as usize & 0x7F)
}

struct Tags {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
}

fn id3v2(bytes: &[u8]) -> Option<Tags> {
    if bytes.len() < 10 || &bytes[0..3] != b"ID3" {
        return None;
    }
    let major = bytes[3];
    if !(3..=4).contains(&major) {
        return None;
    }
    let tag_size = syncsafe(&bytes[6..10]);
    let mut offset = 10;
    if bytes[5] & 0x40 != 0 {
        // Extended header: skip its declared size before the frames.
        if major == 4 && bytes.len() > offset + 4 {
            offset += syncsafe(&bytes[offset..offset + 4]);
        } else if bytes.len() > offset + 4 {
            offset += 4 + u32::from_be_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]) as usize;
        }
    }
    let end = (10 + tag_size).min(bytes.len());
    let mut tags = Tags { title: None, artist: None, album: None };
    while offset + 10 <= end {
        let header = &bytes[offset..offset + 10];
        if header[0] == 0 {
            break;
        }
        let id = std::str::from_utf8(&header[0..4]).unwrap_or_default().to_string();
        let size = if major == 4 {
            syncsafe(&header[4..8])
        } else {
            u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize
        };
        let start = offset + 10;
        let stop = start.saturating_add(size).min(end);
        if size == 0 || start >= stop {
            break;
        }
        let slot = match id.as_str() {
            "TIT2" => Some(&mut tags.title),
            "TPE1" => Some(&mut tags.artist),
            "TALB" => Some(&mut tags.album),
            _ => None,
        };
        if let Some(slot) = slot {
            if slot.is_none() {
                *slot = text_frame(&bytes[start..stop]);
            }
        }
        offset = stop;
    }
    Some(tags)
}

fn id3v1(bytes: &[u8]) -> Option<Tags> {
    if bytes.len() < 128 {
        return None;
    }
    let tail = &bytes[bytes.len() - 128..];
    if &tail[0..3] != b"TAG" {
        return None;
    }
    let field = |range: std::ops::Range<usize>| {
        let value = latin1(&tail[range]);
        let trimmed = value.trim_end_matches(['\0', ' ']).trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    };
    Some(Tags { title: field(3..33), artist: field(33..63), album: field(63..93) })
}

fn flac(bytes: &[u8]) -> Option<Tags> {
    if bytes.len() < 8 || &bytes[0..4] != b"fLaC" {
        return None;
    }
    let mut offset = 4;
    let mut tags = Tags { title: None, artist: None, album: None };
    while offset + 4 <= bytes.len() {
        let header = bytes[offset];
        let last = header & 0x80 != 0;
        let kind = header & 0x7F;
        let length = ((bytes[offset + 1] as usize) << 16)
            | ((bytes[offset + 2] as usize) << 8)
            | bytes[offset + 3] as usize;
        let start = offset + 4;
        let stop = (start + length).min(bytes.len());
        if kind == 4 && stop > start + 8 {
            let block = &bytes[start..stop];
            let vendor = u32::from_le_bytes([block[0], block[1], block[2], block[3]]) as usize;
            let count_at = 4 + vendor;
            if count_at + 4 <= block.len() {
                let count = u32::from_le_bytes([
                    block[count_at],
                    block[count_at + 1],
                    block[count_at + 2],
                    block[count_at + 3],
                ]) as usize;
                let mut cursor = count_at + 4;
                for _ in 0..count {
                    if cursor + 4 > block.len() {
                        break;
                    }
                    let size = u32::from_le_bytes([
                        block[cursor],
                        block[cursor + 1],
                        block[cursor + 2],
                        block[cursor + 3],
                    ]) as usize;
                    cursor += 4;
                    let end = (cursor + size).min(block.len());
                    let entry = String::from_utf8_lossy(&block[cursor..end]).into_owned();
                    cursor = end;
                    if let Some((key, value)) = entry.split_once('=') {
                        let value = value.trim().to_string();
                        if value.is_empty() {
                            continue;
                        }
                        match key.to_ascii_uppercase().as_str() {
                            "TITLE" if tags.title.is_none() => tags.title = Some(value),
                            "ARTIST" if tags.artist.is_none() => tags.artist = Some(value),
                            "ALBUM" if tags.album.is_none() => tags.album = Some(value),
                            _ => {}
                        }
                    }
                }
            }
        }
        if last {
            break;
        }
        offset = start + length;
    }
    Some(tags)
}

fn read_tags(path: &Path) -> Option<Tags> {
    let bytes = read_head(path)?;
    match extension_of(path).as_str() {
        "mp3" => id3v2(&bytes).or_else(|| {
            let tail = fs::read(path).ok()?;
            id3v1(&tail)
        }),
        "flac" => flac(&bytes),
        _ => None,
    }
}

fn read_head(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let file = fs::File::open(path).ok()?;
    let mut buffer = Vec::new();
    file.take(HEAD_BYTES).read_to_end(&mut buffer).ok()?;
    Some(buffer)
}

fn track_from(path: &Path) -> Option<Track> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let file_name = path.file_name()?.to_string_lossy().into_owned();
    let stem = path.file_stem().map(|value| value.to_string_lossy().into_owned()).unwrap_or_else(|| file_name.clone());
    let tags = read_tags(path);
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    Some(Track {
        path: path.to_string_lossy().into_owned(),
        file_name,
        title: tags.as_ref().and_then(|tags| tags.title.clone()).unwrap_or_else(|| stem.clone()),
        artist: tags.as_ref().and_then(|tags| tags.artist.clone()).unwrap_or_default(),
        album: tags.as_ref().and_then(|tags| tags.album.clone()).unwrap_or_default(),
        format: extension_of(path).to_ascii_uppercase(),
        bytes: metadata.len(),
        modified,
    })
}

pub fn scan(root: &str) -> Result<LibraryScan, String> {
    let root_path = PathBuf::from(root.trim());
    if root.is_empty() {
        return Err("Choose a folder to scan".into());
    }
    let metadata = fs::metadata(&root_path).map_err(|_| "That folder cannot be read".to_string())?;
    if !metadata.is_dir() {
        return Err("That path is not a folder".into());
    }
    let mut tracks = Vec::new();
    let mut skipped = 0usize;
    let mut truncated = false;
    let mut queue = vec![(root_path.clone(), 0usize)];
    while let Some((directory, depth)) = queue.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            if is_dir {
                if depth < MAX_DEPTH {
                    queue.push((path, depth + 1));
                }
                continue;
            }
            let extension = extension_of(&path);
            if !AUDIO_EXTENSIONS.contains(&extension.as_str()) {
                continue;
            }
            if tracks.len() >= MAX_TRACKS {
                truncated = true;
                break;
            }
            match track_from(&path) {
                Some(track) => tracks.push(track),
                None => skipped += 1,
            }
        }
        if truncated {
            break;
        }
    }
    tracks.sort_by(|a, b| {
        a.artist
            .to_lowercase()
            .cmp(&b.artist.to_lowercase())
            .then_with(|| a.album.to_lowercase().cmp(&b.album.to_lowercase()))
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
    Ok(LibraryScan {
        root: root_path.to_string_lossy().into_owned(),
        tracks,
        skipped,
        truncated,
    })
}

#[tauri::command]
pub fn scan_library(path: String) -> Result<LibraryScan, String> {
    std::thread::spawn(move || scan(&path))
        .join()
        .map_err(|_| "Library scan failed".to_string())?
}

/// Opens the folder picker and waits for it off the app's main thread.
///
/// This must stay `async`: a synchronous command runs inside the webview's IPC
/// handler, which is the main thread, and a modal dialog owned by a blocked
/// thread deadlocks — the dialog asks the owner's thread to disable its window
/// and that thread is not pumping messages, so the app stops responding. Waiting
/// on a blocking task instead leaves the main thread pumping the dialog's
/// messages, which is what lets it be modal to the window.
#[tauri::command]
pub async fn choose_library_folder(window: tauri::Window) -> Result<String, String> {
    let owner = owner_handle(&window);
    tauri::async_runtime::spawn_blocking(move || pick_folder(owner))
        .await
        .map_err(|_| "Folder picker failed".to_string())?
}

/// The app window the dialog belongs to, as a plain integer so it can cross the
/// thread that runs the picker.
#[cfg(windows)]
fn owner_handle(window: &tauri::Window) -> Option<isize> {
    window.hwnd().ok().map(|handle| handle.0 as isize)
}

#[cfg(not(windows))]
fn owner_handle(_window: &tauri::Window) -> Option<isize> {
    None
}

/// Windows' own Explorer-style folder picker, through `IFileOpenDialog`.
///
/// This is the dialog Explorer and every current Windows app use, so it has the
/// breadcrumb bar, the sidebar and search. It is created in this process rather
/// than in a helper, which is also why no console window appears.
#[cfg(windows)]
pub fn pick_folder(owner: Option<isize>) -> Result<String, String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{IShellItem, SIGDN_FILESYSPATH};

    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|error| format!("Cannot start the folder picker: {error}"))?;
        let chosen = (|| -> Result<String, String> {
            let dialog = folder_dialog()?;
            let window = owner.map(|handle| HWND(handle as *mut core::ffi::c_void));
            // A cancelled dialog is a cancel, not a failure: the page keeps its path.
            if let Err(error) = dialog.Show(window) {
                return if error.code().0 as u32 == 0x8007_04C7 {
                    Ok(String::new())
                } else {
                    Err(format!("The folder picker could not open: {error}"))
                };
            }
            let item: IShellItem = dialog
                .GetResult()
                .map_err(|error| format!("The folder picker returned nothing: {error}"))?;
            let path = item
                .GetDisplayName(SIGDN_FILESYSPATH)
                .map_err(|error| format!("That choice has no folder path: {error}"))?;
            let text = path
                .to_string()
                .map_err(|error| format!("That folder path could not be read: {error}"))?;
            CoTaskMemFree(Some(path.0 as *const core::ffi::c_void));
            Ok(text)
        })();
        CoUninitialize();
        chosen
    }
}

/// Creates the picker and asks it for folders only. Split out from [`pick_folder`]
/// so a test can check the setup without opening a window.
#[cfg(windows)]
fn folder_dialog() -> Result<windows::Win32::UI::Shell::IFileOpenDialog, String> {
    use windows::core::{IUnknown, PCWSTR};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, FILEOPENDIALOGOPTIONS, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST,
        FOS_PICKFOLDERS,
    };

    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None::<&IUnknown>, CLSCTX_ALL)
            .map_err(|error| format!("Cannot create the folder picker: {error}"))?;
        let current = dialog
            .GetOptions()
            .map_err(|error| format!("Cannot read the folder picker's options: {error}"))?;
        dialog
            .SetOptions(FILEOPENDIALOGOPTIONS(
                current.0 | FOS_PICKFOLDERS.0 | FOS_FORCEFILESYSTEM.0 | FOS_PATHMUSTEXIST.0,
            ))
            .map_err(|error| format!("Cannot ask the folder picker for folders: {error}"))?;
        let title: Vec<u16> = "Choose a music folder\0".encode_utf16().collect();
        dialog
            .SetTitle(PCWSTR(title.as_ptr()))
            .map_err(|error| format!("Cannot title the folder picker: {error}"))?;
        Ok(dialog)
    }
}

#[cfg(not(windows))]
pub fn pick_folder(_owner: Option<isize>) -> Result<String, String> {
    Err("The native folder picker is only available on Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("audiorium-library-{name}"));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn id3v2_bytes(title: &str, artist: &str, album: &str) -> Vec<u8> {
        let mut frames = Vec::new();
        for (id, value) in [("TIT2", title), ("TPE1", artist), ("TALB", album)] {
            let mut body = vec![0u8];
            body.extend_from_slice(value.as_bytes());
            frames.extend_from_slice(id.as_bytes());
            frames.extend_from_slice(&(body.len() as u32).to_be_bytes());
            frames.extend_from_slice(&[0, 0]);
            frames.extend_from_slice(&body);
        }
        let size = frames.len() as u32;
        let syncsafe = [
            ((size >> 21) & 0x7F) as u8,
            ((size >> 14) & 0x7F) as u8,
            ((size >> 7) & 0x7F) as u8,
            (size & 0x7F) as u8,
        ];
        let mut out = b"ID3\x03\x00\x00".to_vec();
        out.extend_from_slice(&syncsafe);
        out.extend_from_slice(&frames);
        out
    }

    fn write_file(directory: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = directory.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(bytes).unwrap();
        path
    }

    #[test]
    fn id3v2_tags_are_read_from_an_mp3() {
        let directory = scratch("id3v2");
        write_file(&directory, "song.mp3", &id3v2_bytes("A Moment Apart", "ODESZA", "A Moment Apart"));
        let tags = read_tags(&directory.join("song.mp3")).expect("tags");
        assert_eq!(tags.title.as_deref(), Some("A Moment Apart"));
        assert_eq!(tags.artist.as_deref(), Some("ODESZA"));
        assert_eq!(tags.album.as_deref(), Some("A Moment Apart"));
    }

    #[test]
    fn id3v1_is_used_when_there_is_no_id3v2() {
        let directory = scratch("id3v1");
        let field = |value: &str| {
            let mut buffer = vec![0u8; 30];
            buffer[..value.len()].copy_from_slice(value.as_bytes());
            buffer
        };
        let mut bytes = vec![0u8; 256];
        let mut tag = Vec::from(b"TAG".as_slice());
        tag.extend(field("Late Night"));
        tag.extend(field("Kiasmos"));
        tag.extend(field("Looped"));
        tag.extend_from_slice(b"2014");
        tag.extend(field(""));
        tag.push(0);
        assert_eq!(tag.len(), 128, "ID3v1 is a fixed 128-byte trailer");
        bytes.extend(tag);
        write_file(&directory, "legacy.mp3", &bytes);
        let tags = read_tags(&directory.join("legacy.mp3")).expect("tags");
        assert_eq!(tags.title.as_deref(), Some("Late Night"));
        assert_eq!(tags.artist.as_deref(), Some("Kiasmos"));
    }

    #[test]
    fn flac_vorbis_comments_are_read() {
        let directory = scratch("flac");
        let mut block = Vec::new();
        block.extend_from_slice(&3u32.to_le_bytes());
        block.extend_from_slice(b"ref");
        block.extend_from_slice(&2u32.to_le_bytes());
        for entry in ["TITLE=Says", "ARTIST=Nils Frahm"] {
            block.extend_from_slice(&(entry.len() as u32).to_le_bytes());
            block.extend_from_slice(entry.as_bytes());
        }
        let mut bytes = b"fLaC".to_vec();
        bytes.push(0x84);
        let length = block.len() as u32;
        bytes.extend_from_slice(&[((length >> 16) & 0xFF) as u8, ((length >> 8) & 0xFF) as u8, (length & 0xFF) as u8]);
        bytes.extend_from_slice(&block);
        write_file(&directory, "spaces.flac", &bytes);
        let tags = read_tags(&directory.join("spaces.flac")).expect("tags");
        assert_eq!(tags.title.as_deref(), Some("Says"));
        assert_eq!(tags.artist.as_deref(), Some("Nils Frahm"));
    }

    #[test]
    fn scanning_walks_subfolders_and_ignores_other_files() {
        let directory = scratch("walk");
        write_file(&directory, "one.mp3", &id3v2_bytes("One", "Artist A", "Album"));
        write_file(&directory, "notes.txt", b"not audio");
        write_file(&directory, "nested/two.flac", b"fLaC");
        write_file(&directory, "nested/three.wav", b"RIFF");
        let result = scan(directory.to_str().unwrap()).unwrap();
        assert_eq!(result.tracks.len(), 3, "{:?}", result.tracks.iter().map(|t| &t.file_name).collect::<Vec<_>>());
        let tagged = result.tracks.iter().find(|track| track.file_name == "one.mp3").unwrap();
        assert_eq!(tagged.title, "One");
        assert_eq!(tagged.artist, "Artist A");
        assert_eq!(tagged.format, "MP3");
        let nested = result.tracks.iter().find(|track| track.file_name == "two.flac").unwrap();
        assert_eq!(nested.format, "FLAC");
        let untagged = result.tracks.iter().find(|track| track.file_name == "three.wav").unwrap();
        assert_eq!(untagged.title, "three");
        assert!(!result.truncated);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_missing_or_file_path_is_rejected() {
        assert!(scan("").is_err());
        assert!(scan("Z:/definitely/not/here").is_err());
        let directory = scratch("notdir");
        let file = write_file(&directory, "single.mp3", b"ID3");
        assert!(scan(file.to_str().unwrap()).is_err());
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn tracks_sort_by_artist_then_album_then_title() {
        let directory = scratch("sort");
        write_file(&directory, "a.mp3", &id3v2_bytes("Zebra", "Beta", "X"));
        write_file(&directory, "b.mp3", &id3v2_bytes("Apple", "Beta", "X"));
        write_file(&directory, "c.mp3", &id3v2_bytes("Middle", "Alpha", "Y"));
        let result = scan(directory.to_str().unwrap()).unwrap();
        let order: Vec<&str> = result.tracks.iter().map(|track| track.title.as_str()).collect();
        assert_eq!(order, vec!["Middle", "Apple", "Zebra"]);
        let _ = fs::remove_dir_all(&directory);
    }

    /// The picker must be the current Windows dialog, asked for folders only. It
    /// is built but never shown here, so this runs without a desktop.
    #[cfg(windows)]
    #[test]
    fn the_folder_picker_asks_for_folders_only() {
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
        use windows::Win32::UI::Shell::{FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS};

        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .expect("COM for the picker");
            let dialog = folder_dialog().expect("a folder dialog");
            let options = dialog.GetOptions().expect("the dialog's options").0;
            assert_eq!(
                options & FOS_PICKFOLDERS.0,
                FOS_PICKFOLDERS.0,
                "the picker must select folders, not files"
            );
            assert_eq!(options & FOS_PATHMUSTEXIST.0, FOS_PATHMUSTEXIST.0);
            assert_eq!(options & FOS_FORCEFILESYSTEM.0, FOS_FORCEFILESYSTEM.0);
            drop(dialog);
            CoUninitialize();
        }
    }

    /// The whole picker, end to end: it must put a window on screen, and closing
    /// that window must come back as an empty path rather than an error.
    #[cfg(windows)]
    #[test]
    #[ignore = "opens a real folder dialog and closes it again"]
    fn the_folder_picker_shows_a_window_and_a_cancel_returns_nothing() {
        use std::time::{Duration, Instant};
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_CLOSE};

        let picker = std::thread::spawn(|| pick_folder(None));
        let title: Vec<u16> = "Choose a music folder\0".encode_utf16().collect();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut window = None;
        while window.is_none() && Instant::now() < deadline {
            unsafe {
                window = FindWindowW::<PCWSTR, _>(PCWSTR::null(), PCWSTR(title.as_ptr())).ok();
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let window = window.expect("the folder dialog should be on screen");
        unsafe {
            PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)).expect("close the dialog");
        }
        let chosen = picker
            .join()
            .expect("the picker thread")
            .expect("the picker's result");
        assert!(chosen.is_empty(), "a cancelled picker returned {chosen:?}");
    }
}
