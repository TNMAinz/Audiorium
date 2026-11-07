//! Where the running chain and the saved patches live between launches.
//!
//! One small JSON file, written whole and replaced by rename, so a crash halfway
//! through cannot leave a half-valid file in its place. The write happens on a
//! background thread that keeps only the newest state: a knob drag sends a value
//! per pointer move, and none of those may block the audio or the interface.
//!
//! Loading is forgiving in the direction that protects the audio path: a file that
//! cannot be read, or a stage that no longer validates, falls back to the defaults
//! instead of running something half-understood.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::chain::ChainConfig;
use crate::presets::Library;

pub const VERSION: u32 = 1;
/// Long enough that a drag writes a handful of times, short enough that a setting
/// changed just before quitting still reaches the disk.
const COALESCE: Duration = Duration::from_millis(90);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stored {
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default)]
    pub chain: ChainConfig,
    #[serde(default)]
    pub library: Library,
}

fn version() -> u32 {
    VERSION
}

impl Default for Stored {
    fn default() -> Self {
        Self {
            version: VERSION,
            chain: ChainConfig::default(),
            library: Library::default(),
        }
    }
}

/// `%APPDATA%\AUDIORIUM\state.json` on Windows. `AUDIORIUM_CONFIG_DIR` overrides
/// it, which is how a test run or a gate script keeps its own file.
pub fn config_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("AUDIORIUM_CONFIG_DIR") {
        if !dir.trim().is_empty() {
            return Some(PathBuf::from(dir).join("state.json"));
        }
    }
    #[cfg(windows)]
    if let Ok(appdata) = std::env::var("APPDATA") {
        if !appdata.trim().is_empty() {
            return Some(PathBuf::from(appdata).join("AUDIORIUM").join("state.json"));
        }
    }
    std::env::var("HOME")
        .ok()
        .filter(|home| !home.trim().is_empty())
        .map(|home| PathBuf::from(home).join(".config").join("audiorium").join("state.json"))
}

fn write(path: &Path, state: &Stored) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{error}"))?;
    }
    let text = serde_json::to_string_pretty(state).map_err(|error| format!("{error}"))?;
    // A temporary beside the real file, so the rename is a same-volume replace.
    let mut temporary = path.to_path_buf();
    temporary.set_extension("json.tmp");
    fs::write(&temporary, text).map_err(|error| format!("{error}"))?;
    fs::rename(&temporary, path).map_err(|error| format!("{error}"))
}

pub fn read(path: &Path) -> Option<Stored> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str::<Stored>(&text).ok()
}

/// The handle the chain keeps. With no path it is a no-op, which is what a unit
/// test wants and what the app gets if no config directory can be found.
pub struct Store {
    sender: Option<Sender<Stored>>,
    path: Option<PathBuf>,
}

impl Default for Store {
    fn default() -> Self {
        Self::off()
    }
}

impl Store {
    pub fn off() -> Self {
        Self {
            sender: None,
            path: None,
        }
    }

    /// A store at `path`, or no store at all when there is nowhere to write.
    pub fn maybe(path: Option<PathBuf>) -> Self {
        path.map(Self::at).unwrap_or_else(Self::off)
    }

    pub fn at(path: PathBuf) -> Self {
        let (sender, receiver) = mpsc::channel::<Stored>();
        let target = path.clone();
        thread::Builder::new()
            .name("audiorium-store".into())
            .spawn(move || {
                while let Ok(mut latest) = receiver.recv() {
                    thread::sleep(COALESCE);
                    // Only the newest value matters: the ones behind it are superseded.
                    while let Ok(next) = receiver.try_recv() {
                        latest = next;
                    }
                    if let Err(error) = write(&target, &latest) {
                        eprintln!("AUDIORIUM: settings not saved: {error}");
                    }
                }
            })
            .ok();
        Self {
            sender: Some(sender),
            path: Some(path),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Never blocks: the callers are the audio setup and the command handlers.
    pub fn save(&self, state: Stored) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eq::{EqConfig, EqFilter, EqKind};

    fn scratch(name: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("audiorium-store-{name}-{stamp}"));
        fs::create_dir_all(&dir).expect("a scratch directory");
        dir.join("state.json")
    }

    fn state() -> Stored {
        let mut library = Library::default();
        library.apply("builtin:bass-shelf").unwrap();
        Stored {
            version: VERSION,
            chain: ChainConfig {
                eq: EqConfig {
                    enabled: true,
                    preamp_db: -2.0,
                    filters: vec![EqFilter {
                        kind: EqKind::Lowshelf,
                        frequency: 105.0,
                        gain_db: 4.0,
                        q: 0.7,
                        enabled: true,
                        dynamics: None,
                    }],
                },
                ..ChainConfig::default()
            },
            library,
        }
    }

    #[test]
    fn what_is_saved_is_what_is_read_back() {
        let path = scratch("round-trip");
        write(&path, &state()).expect("the file is written");
        let loaded = read(&path).expect("the file is read");
        assert_eq!(loaded.chain, state().chain);
        assert_eq!(loaded.library.active, Some("builtin:bass-shelf".into()));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_missing_or_unreadable_file_is_not_an_error() {
        let path = scratch("missing");
        assert!(read(&path).is_none());
        fs::write(&path, "{ not json").expect("a broken file");
        assert!(read(&path).is_none(), "a broken file was accepted");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_file_with_one_missing_field_still_loads_the_rest() {
        let path = scratch("partial");
        fs::write(&path, r#"{"version":1,"library":{"saved":[],"active":null}}"#)
            .expect("a partial file");
        let loaded = read(&path).expect("a partial file is still a file");
        assert_eq!(loaded.chain, ChainConfig::default());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_continuous_edit_coalesces_instead_of_writing_once_per_value() {
        let path = scratch("coalesce");
        let store = Store::at(path.clone());
        for step in 0..40 {
            let mut next = state();
            next.chain.eq.preamp_db = -(step as f32) / 4.0;
            store.save(next);
        }
        // The writer sleeps before taking the newest value, so this is the value on disk.
        thread::sleep(COALESCE * 6);
        let loaded = read(&path).expect("the file is written");
        assert!(
            (loaded.chain.eq.preamp_db + 9.75).abs() < 1e-3,
            "the last value did not reach the disk: {}",
            loaded.chain.eq.preamp_db
        );
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_store_without_a_path_keeps_quiet() {
        let store = Store::off();
        assert!(store.path().is_none());
        store.save(state());
    }
}
