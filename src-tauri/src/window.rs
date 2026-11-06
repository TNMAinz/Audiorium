//! Where the window was left.
//!
//! The window opens where it was last closed instead of at the middle of the
//! screen, because it is dragged somewhere specific and that place is part of how
//! the desk is set up. The position is a physical pixel top-left, kept in its own
//! small file beside the settings so that the chain's saves can never overwrite it.
//!
//! Two things are deliberately not remembered. A maximized window's rect is the
//! screen, not a place, so a move is ignored while the window is maximized. And a
//! position that no longer lands on any monitor — the screen it was on was
//! unplugged, or the layout changed — is dropped rather than restored, because a
//! window outside every screen cannot be dragged back.

use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{PhysicalPosition, WebviewWindow};

/// How far into the window the check for a monitor looks: its title bar, which is
/// what a user would grab to drag it back.
const GRAB_X: i32 = 80;
const GRAB_Y: i32 = 8;
/// How often the window is asked where it is. A quarter of a second is invisible
/// next to a drag, and it catches every way a window moves — the title bar, a snap
/// layout, another tool placing it — not just the ones this app performed.
const POLL: Duration = Duration::from_millis(400);

/// A window's top-left in physical pixels.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
}

/// `window.json` beside the settings file, or nothing when there is nowhere to
/// write. `AUDIORIUM_CONFIG_DIR` therefore covers this too, which is how a test
/// keeps its own file.
fn path() -> Option<PathBuf> {
    crate::store::config_path().map(|state| state.with_file_name("window.json"))
}

fn read(path: &std::path::Path) -> Option<Placement> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str::<Placement>(&text).ok()
}

fn write(path: &std::path::Path, placement: Placement) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{error}"))?;
    }
    let text = serde_json::to_string_pretty(&placement).map_err(|error| format!("{error}"))?;
    // A temporary beside the real file, so the rename is a same-volume replace.
    let mut temporary = path.to_path_buf();
    temporary.set_extension("json.tmp");
    fs::write(&temporary, text).map_err(|error| format!("{error}"))?;
    fs::rename(&temporary, path).map_err(|error| format!("{error}"))
}

/// Whether that position still has a monitor under its title bar.
fn on_a_monitor(placement: Placement, monitors: &[(i32, i32, u32, u32)]) -> bool {
    monitors.iter().any(|(x, y, width, height)| {
        let (grab_x, grab_y) = (placement.x + GRAB_X, placement.y + GRAB_Y);
        grab_x >= *x
            && grab_x < x + *width as i32
            && grab_y >= *y
            && grab_y < y + *height as i32
    })
}

/// Puts the window back where it was, when that place still exists. A missing file,
/// an unreadable one, or a position on no monitor leaves the window where the
/// configuration put it.
pub fn restore(window: &WebviewWindow) {
    let Some(path) = path() else {
        return;
    };
    let Some(placement) = read(&path) else {
        return;
    };
    let monitors: Vec<(i32, i32, u32, u32)> = window
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|monitor| {
            let at = monitor.position();
            let size = monitor.size();
            (at.x, at.y, size.width, size.height)
        })
        .collect();
    if monitors.is_empty() || !on_a_monitor(placement, &monitors) {
        return;
    }
    let _ = window.set_position(PhysicalPosition::new(placement.x, placement.y));
}

/// The window's position, kept current while the app runs.
pub struct Watcher {
    window: WebviewWindow,
    path: Option<PathBuf>,
}

impl Watcher {
    pub fn start(window: &WebviewWindow) -> Self {
        let path = path();
        let watch = window.clone();
        let target = path.clone();
        // The file is the starting point, so a position that has not changed is not
        // written again on every launch.
        let mut last = path.as_ref().and_then(|path| read(path));
        let _ = thread::Builder::new()
            .name("audiorium-window".into())
            .spawn(move || loop {
                thread::sleep(POLL);
                // A maximized window's rect is the screen, not a place the user chose.
                if watch.is_maximized().unwrap_or(false) {
                    continue;
                }
                let Ok(position) = watch.outer_position() else {
                    continue;
                };
                let placement = Placement {
                    x: position.x,
                    y: position.y,
                };
                if last == Some(placement) {
                    continue;
                }
                last = Some(placement);
                if let Some(path) = target.as_ref() {
                    if let Err(error) = write(path, placement) {
                        eprintln!("AUDIORIUM: window position not saved: {error}");
                    }
                }
            });
        Self {
            window: window.clone(),
            path,
        }
    }

    /// Writes the position now: the move just before quitting is the one the user
    /// means, and the polling thread may not have seen it yet.
    pub fn flush(&self) {
        let Some(path) = self.path.as_ref() else {
            return;
        };
        if self.window.is_maximized().unwrap_or(false) {
            return;
        }
        if let Ok(position) = self.window.outer_position() {
            let _ = write(
                path,
                Placement {
                    x: position.x,
                    y: position.y,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("audiorium-window-{name}-{stamp}"));
        fs::create_dir_all(&dir).expect("a scratch directory");
        dir.join("window.json")
    }

    #[test]
    fn a_position_is_written_and_read_back() {
        let path = scratch("round-trip");
        write(&path, Placement { x: 240, y: -180 }).expect("the file is written");
        assert_eq!(read(&path), Some(Placement { x: 240, y: -180 }));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_broken_or_missing_file_is_not_a_position() {
        let path = scratch("broken");
        assert!(read(&path).is_none());
        fs::write(&path, "{ not json").expect("a broken file");
        assert!(read(&path).is_none(), "a broken file was accepted");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_place_on_a_screen_is_restored_and_a_place_off_them_is_not() {
        let monitors = [(0, 0, 1920, 1080), (1920, 0, 2560, 1440)];
        assert!(on_a_monitor(Placement { x: 100, y: 100 }, &monitors));
        // The second screen starts where the first ends.
        assert!(on_a_monitor(Placement { x: 2600, y: 40 }, &monitors));
        // A screen that is no longer plugged in, and a place on no screen at all.
        assert!(!on_a_monitor(Placement { x: -3000, y: 100 }, &monitors));
        assert!(!on_a_monitor(Placement { x: 5000, y: 5000 }, &monitors));
    }
}
