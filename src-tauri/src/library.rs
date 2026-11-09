//! Local library browsing.
//!
//! There is no playback engine yet, so this module only walks a folder and reads
//! what a file already says about itself. Tags come from ID3v2/ID3v1 (MP3), Vorbis
//! comments (FLAC), and a RIFF `LIST`/`INFO` chunk or an embedded ID3v2 tag (WAV);
//! every other container falls back to the file name. Nothing is copied, uploaded or
//! played.

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const AUDIO_EXTENSIONS: [&str; 9] = ["mp3", "flac", "wav", "m4a", "aac", "ogg", "opus", "aiff", "aif"];
const MAX_TRACKS: usize = 5000;
const MAX_DEPTH: usize = 12;
/// The folder the master writer puts prints in; the scan leaves it alone.
const MASTERED_FOLDER: &str = crate::render::MASTERED_FOLDER;
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

pub(crate) struct Tags {
    pub(crate) title: Option<String>,
    pub(crate) artist: Option<String>,
    pub(crate) album: Option<String>,
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

/// One RIFF chunk body at a time: name, length, and where the next one starts.
/// Chunks are word aligned, so an odd body is followed by one pad byte.
fn riff_chunks(bytes: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut chunks = Vec::new();
    let mut offset = 12;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]) as usize;
        let start = offset + 8;
        let stop = start.saturating_add(size).min(bytes.len());
        chunks.push((id, &bytes[start..stop]));
        if id == b"data" {
            break;
        }
        offset = stop + (size & 1);
    }
    chunks
}

fn is_riff(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE"
}

/// RIFF `LIST`/`INFO` fields: the names Windows shows for a WAV, plus the two
/// spellings other writers use for the same things.
fn riff_info(body: &[u8]) -> Option<Tags> {
    if body.len() < 4 || &body[0..4] != b"INFO" {
        return None;
    }
    let mut tags = Tags { title: None, artist: None, album: None };
    let mut offset = 4;
    while offset + 8 <= body.len() {
        let id = &body[offset..offset + 4];
        let size = u32::from_le_bytes([
            body[offset + 4],
            body[offset + 5],
            body[offset + 6],
            body[offset + 7],
        ]) as usize;
        let start = offset + 8;
        let stop = start.saturating_add(size).min(body.len());
        if stop > start {
            let text = info_text(&body[start..stop]);
            let slot = match id {
                b"INAM" | b"TITL" => Some(&mut tags.title),
                b"IART" | b"ARTIST" => Some(&mut tags.artist),
                b"IPRD" | b"IALB" => Some(&mut tags.album),
                _ => None,
            };
            if let (Some(slot), Some(value)) = (slot, text) {
                if slot.is_none() {
                    *slot = Some(value);
                }
            }
        }
        offset = stop + (size & 1);
    }
    (tags.title.is_some() || tags.artist.is_some() || tags.album.is_some()).then_some(tags)
}

/// An INFO value: NUL terminated, written as UTF-8 here and as Latin-1 by older tools.
fn info_text(bytes: &[u8]) -> Option<String> {
    let trimmed = bytes.strip_suffix(&[0]).unwrap_or(bytes);
    let value = match std::str::from_utf8(trimmed) {
        Ok(text) => text.to_string(),
        Err(_) => latin1(trimmed),
    };
    let value = value.trim_end_matches('\0').trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// A WAV's own tags: `LIST`/`INFO` first, then an embedded ID3v2 tag, which is where
/// a cover lives because RIFF has no chunk for one.
fn riff_tags(bytes: &[u8]) -> Option<Tags> {
    if !is_riff(bytes) {
        return None;
    }
    let mut info = None;
    let mut id3 = None;
    for (id, body) in riff_chunks(bytes) {
        match id {
            b"LIST" => info = info.or_else(|| riff_info(body)),
            b"id3 " | b"ID3 " => id3 = id3.or_else(|| id3v2(body)),
            _ => {}
        }
    }
    match (info, id3) {
        (Some(mut info), Some(id3)) => {
            info.title = info.title.or(id3.title);
            info.artist = info.artist.or(id3.artist);
            info.album = info.album.or(id3.album);
            Some(info)
        }
        (Some(info), None) => Some(info),
        (None, Some(id3)) => Some(id3),
        (None, None) => None,
    }
}

/// The embedded ID3v2 tag of a RIFF file, for the cover reader.
pub(crate) fn riff_id3(bytes: &[u8]) -> Option<&[u8]> {
    if !is_riff(bytes) {
        return None;
    }
    riff_chunks(bytes)
        .into_iter()
        .find(|(id, body)| (*id == b"id3 " || *id == b"ID3 ") && !body.is_empty())
        .map(|(_, body)| body)
}

/// The first front-cover picture in an ID3v2 tag, as the bytes the tag carries.
pub(crate) fn id3v2_picture(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < 10 || &bytes[0..3] != b"ID3" {
        return None;
    }
    let major = bytes[3];
    if !(3..=4).contains(&major) {
        return None;
    }
    let tag_size = syncsafe(&bytes[6..10]);
    let end = (10 + tag_size).min(bytes.len());
    let mut offset = 10;
    while offset + 10 <= end {
        let header = &bytes[offset..offset + 10];
        if header[0] == 0 {
            break;
        }
        let id = &header[0..4];
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
        if id == b"APIC" {
            if let Some(picture) = apic_picture(&bytes[start..stop]) {
                return Some(picture);
            }
        }
        offset = stop;
    }
    None
}

/// `APIC`: encoding, MIME (Latin-1, NUL terminated), picture type, description in that
/// encoding, then the image itself.
fn apic_picture(body: &[u8]) -> Option<Vec<u8>> {
    let (encoding, rest) = body.split_first()?;
    let mime_end = rest.iter().position(|byte| *byte == 0)?;
    let after_mime = rest.get(mime_end + 1..)?;
    let (_, after_type) = after_mime.split_first()?;
    let description_end = match encoding {
        // UTF-16 descriptions end on an even pair of NULs.
        1 | 2 => {
            let mut at = 0;
            while at + 1 < after_type.len() {
                if after_type[at] == 0 && after_type[at + 1] == 0 {
                    break;
                }
                at += 2;
            }
            at + 2
        }
        _ => after_type.iter().position(|byte| *byte == 0)? + 1,
    };
    let data = after_type.get(description_end..)?;
    (!data.is_empty()).then(|| data.to_vec())
}

fn read_tags(path: &Path) -> Option<Tags> {
    let bytes = read_head(path)?;
    match extension_of(path).as_str() {
        "mp3" => id3v2(&bytes).or_else(|| {
            let tail = fs::read(path).ok()?;
            id3v1(&tail)
        }),
        "flac" => flac(&bytes),
        "wav" => riff_tags(&bytes),
        _ => None,
    }
}

/// What a file says about itself, for a module that needs the tags without the scan.
pub(crate) fn tags_of(path: &Path) -> Option<Tags> {
    read_tags(path)
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
                // A master has a folder of its own. Scanning it would put prints back in
                // the library and let one be mastered again, which is not what the
                // library is: it is the music that came into the app.
                if name.eq_ignore_ascii_case(MASTERED_FOLDER) {
                    continue;
                }
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

/// Windows' own file picker, for the one file a panel asks the user to bring: a
/// wavetable, or a Scala tuning. The title says what is being chosen, and the
/// filter is the type the reader can actually parse, so a wrong file is not offered.
#[cfg(windows)]
pub fn pick_file(owner: Option<isize>, title: &str, name: &str, extensions: &[&str]) -> Result<String, String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, IShellItem, SIGDN_FILESYSPATH,
    };

    let pattern: Vec<u16> = extensions
        .iter()
        .map(|extension| format!("*.{extension}"))
        .collect::<Vec<_>>()
        .join(";")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let name: Vec<u16> = format!("{name} ").encode_utf16().collect();
    let title: Vec<u16> = format!("{title} ").encode_utf16().collect();

    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|error| format!("Cannot start the file picker: {error}"))?;
        let chosen = (|| -> Result<String, String> {
            let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None::<&windows::core::IUnknown>, CLSCTX_ALL)
                .map_err(|error| format!("Cannot create the file picker: {error}"))?;
            let filter = [COMDLG_FILTERSPEC {
                pszName: PCWSTR(name.as_ptr()),
                pszSpec: PCWSTR(pattern.as_ptr()),
            }];
            dialog
                .SetFileTypes(&filter)
                .map_err(|error| format!("Cannot filter the file picker: {error}"))?;
            dialog
                .SetTitle(PCWSTR(title.as_ptr()))
                .map_err(|error| format!("Cannot title the file picker: {error}"))?;
            let window = owner.map(|handle| HWND(handle as *mut core::ffi::c_void));
            if let Err(error) = dialog.Show(window) {
                return if error.code().0 as u32 == 0x8007_04C7 {
                    Ok(String::new())
                } else {
                    Err(format!("The file picker could not open: {error}"))
                };
            }
            let item: IShellItem = dialog
                .GetResult()
                .map_err(|error| format!("The file picker returned nothing: {error}"))?;
            let path = item
                .GetDisplayName(SIGDN_FILESYSPATH)
                .map_err(|error| format!("That choice has no file path: {error}"))?;
            let text = path
                .to_string()
                .map_err(|error| format!("That file path could not be read: {error}"))?;
            CoTaskMemFree(Some(path.0 as *const core::ffi::c_void));
            Ok(text)
        })();
        CoUninitialize();
        chosen
    }
}

#[cfg(not(windows))]
pub fn pick_file(
    _owner: Option<isize>,
    _title: &str,
    _name: &str,
    _extensions: &[&str],
) -> Result<String, String> {
    Err("The native file picker is only available on Windows".into())
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

    /// A RIFF file with the chunks a master is written with, and nothing else in them.
    fn wav_with(chunks: &[(&[u8], Vec<u8>)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (id, bytes) in chunks {
            body.extend_from_slice(id);
            body.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            body.extend_from_slice(bytes);
            if bytes.len() % 2 == 1 {
                body.push(0);
            }
        }
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&((4 + body.len()) as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&[0u8; 16]);
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn a_wav_master_is_read_back_from_its_own_chunks() {
        let directory = scratch("wav-tags");
        let mut info = b"INFO".to_vec();
        for (id, value) in [(b"INAM", "First Light"), (b"IART", "Nova"), (b"IPRD", "Night")] {
            let mut text = value.as_bytes().to_vec();
            text.push(0);
            info.extend_from_slice(id);
            info.extend_from_slice(&(text.len() as u32).to_le_bytes());
            info.extend_from_slice(&text);
            if text.len() % 2 == 1 {
                info.push(0);
            }
        }
        let tagged = wav_with(&[(b"LIST", info), (b"data", vec![0; 8])]);
        let path = write_file(&directory, "tagged.wav", &tagged);
        let tags = read_tags(&path).expect("the INFO chunk");
        assert_eq!(tags.title.as_deref(), Some("First Light"));
        assert_eq!(tags.artist.as_deref(), Some("Nova"));
        assert_eq!(tags.album.as_deref(), Some("Night"));

        // An ID3v2 tag inside the RIFF is read as well, which is where the cover lives.
        let id3 = id3v2_bytes("Ada", "Grace", "Notes");
        let nested = wav_with(&[(b"id3 ", id3.clone()), (b"data", vec![0; 8])]);
        let path = write_file(&directory, "nested.wav", &nested);
        let tags = read_tags(&path).expect("the embedded tag");
        assert_eq!(tags.title.as_deref(), Some("Ada"));
        assert_eq!(tags.album.as_deref(), Some("Notes"));
        assert_eq!(riff_id3(&nested), Some(id3.as_slice()));

        // A WAV with neither produces no tags, rather than an empty set of them.
        let plain = wav_with(&[(b"data", vec![0; 8])]);
        let path = write_file(&directory, "plain.wav", &plain);
        assert!(read_tags(&path).is_none());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn the_picture_of_an_embedded_tag_is_extracted() {
        // TIT2 then APIC: encoding, MIME, front cover, empty description, the image.
        let mut frames = Vec::new();
        for (id, body) in [
            (&b"TIT2"[..], {
                let mut body = vec![0u8];
                body.extend_from_slice(b"A song");
                body
            }),
            (&b"APIC"[..], {
                let mut body = vec![0u8];
                body.extend_from_slice(b"image/png\0");
                body.push(3);
                body.push(0);
                body.extend_from_slice(&[0x89, b'P', b'N', b'G', 1, 2, 3]);
                body
            }),
        ] {
            frames.extend_from_slice(id);
            frames.extend_from_slice(&(body.len() as u32).to_be_bytes());
            frames.extend_from_slice(&[0, 0]);
            frames.extend_from_slice(&body);
        }
        let size = frames.len() as u32;
        let mut tag = b"ID3\x03\x00\x00".to_vec();
        tag.extend_from_slice(&[
            ((size >> 21) & 0x7F) as u8,
            ((size >> 14) & 0x7F) as u8,
            ((size >> 7) & 0x7F) as u8,
            (size & 0x7F) as u8,
        ]);
        tag.extend_from_slice(&frames);
        assert_eq!(
            id3v2_picture(&tag).as_deref(),
            Some(&[0x89, b'P', b'N', b'G', 1, 2, 3][..])
        );
        assert!(id3v2_picture(b"not a tag").is_none());
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
    fn the_mastered_folder_is_not_part_of_the_library() {
        let directory = scratch("mastered-skip");
        write_file(&directory, "one.mp3", &id3v2_bytes("One", "Artist A", "Album"));
        write_file(&directory, "Mastered/one (mastered).mp3", &id3v2_bytes("One", "Artist A", "Album"));
        // The name is the folder's, not a case it has to match exactly.
        write_file(&directory, "Elsewhere/MASTERED/two (mastered).mp3", &id3v2_bytes("Two", "Artist B", "Album"));
        let result = scan(directory.to_str().unwrap()).unwrap();
        let names: Vec<&str> = result.tracks.iter().map(|track| track.file_name.as_str()).collect();
        assert_eq!(names, vec!["one.mp3"], "{names:?}");
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
