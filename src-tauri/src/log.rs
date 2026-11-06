//! What a release build can say after something has gone wrong.
//!
//! The release process is a window and nothing else: no console, so `println!`,
//! `eprintln!` and a panic's message have nowhere to go. Everything here appends to
//! one small file instead — `%APPDATA%\AUDIORIUM\audiorium.log` — so a crash on
//! someone else's machine leaves a line they can send back.
//!
//! The file is deliberately not a flight recorder: it is capped, rotated when it
//! fills, and every entry is truncated, because the failure that fills a disk while
//! the app is broken is worse than the missing detail.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The whole file stops growing here. Two hundred and fifty kilobytes is a few
/// thousand lines, far more than a startup note and a panic need.
const MAX_BYTES: u64 = 256 * 1024;
/// One entry is cut here, so a huge panic payload cannot fill the file in one line.
const MAX_ENTRY: usize = 8 * 1024;

/// `AUDIORIUM_LOG_PATH` overrides the file, which is how a test keeps its own.
pub fn path() -> Option<PathBuf> {
    if let Ok(chosen) = std::env::var("AUDIORIUM_LOG_PATH") {
        if !chosen.trim().is_empty() {
            return Some(PathBuf::from(chosen));
        }
    }
    crate::store::config_path().map(|state| state.with_file_name("audiorium.log"))
}

/// Appends one entry, starting the file over when it has grown past the cap. A
/// logger that cannot write stays quiet: it is the last thing that should take the
/// app down.
pub fn append_to(target: &Path, entry: &str) {
    let Some(parent) = target.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let full = fs::metadata(target).map(|file| file.len() > MAX_BYTES).unwrap_or(false);
    let mut text = String::new();
    if full {
        text.push_str(&format!(
            "{} AUDIORIUM: log rotated, the previous {} bytes were dropped\n",
            stamp(),
            MAX_BYTES
        ));
    }
    text.push_str(&format!("{} {}\n", stamp(), cut(entry)));
    let opened = if full {
        fs::write(target, &text)
    } else {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(target)
            .and_then(|mut file| file.write_all(text.as_bytes()))
    };
    let _ = opened;
}

/// Notes one line in the log, if there is a log to note it in.
pub fn note(entry: &str) {
    if let Some(target) = path() {
        append_to(&target, entry);
    }
}

/// Records a launch line and makes every panic leave a trace before the process
/// goes. The previous hook is kept, so a terminal build still prints its panic.
pub fn install() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        note(&describe(payload_of(info), location_of(info)));
        previous(info);
    }));
    note(&format!(
        "AUDIORIUM {} started ({} build)",
        env!("CARGO_PKG_VERSION"),
        if cfg!(debug_assertions) { "debug" } else { "release" }
    ));
}

fn payload_of(info: &std::panic::PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "panic with a payload that is not text".to_string()
    }
}

fn location_of(info: &std::panic::PanicHookInfo<'_>) -> String {
    info.location()
        .map(|place| format!("{}:{}:{}", place.file(), place.line(), place.column()))
        .unwrap_or_else(|| "an unknown place".into())
}

/// The one line a panic leaves behind.
fn describe(payload: String, location: String) -> String {
    format!("panic at {location}: {payload}")
}

fn cut(entry: &str) -> String {
    if entry.len() <= MAX_ENTRY {
        return entry.replace('\n', " ⏎ ");
    }
    let mut end = MAX_ENTRY;
    while !entry.is_char_boundary(end) {
        end -= 1;
    }
    format!("{} … (truncated)", entry[..end].replace('\n', " ⏎ "))
}

/// The time an entry was made, in UTC: `2026-11-06 14:05:11Z`.
fn stamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}Z",
        rest / 3_600,
        (rest % 3_600) / 60,
        rest % 60
    )
}

/// Days since 1970-01-01 to a calendar date. Howard Hinnant's civil calendar, which
/// is exact for every year this app will see and needs no dependency.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("audiorium-log-{name}-{stamp}"));
        fs::create_dir_all(&dir).expect("a scratch directory");
        dir.join("audiorium.log")
    }

    #[test]
    fn entries_are_appended_in_order_and_dated() {
        let path = scratch("append");
        append_to(&path, "first");
        append_to(&path, "second");
        let text = fs::read_to_string(&path).expect("the log is written");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("first"), "{:?}", lines[0]);
        assert!(lines[1].ends_with("second"));
        // `2026-11-06 14:05:11Z first`
        let stamp = &lines[0][..lines[0].len() - "first".len()];
        assert_eq!(stamp.trim_end().len(), "2026-11-06 14:05:11Z".len(), "{stamp:?}");
        assert!(stamp.trim_end().ends_with('Z'));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_long_entry_is_cut_and_keeps_the_file_usable() {
        let path = scratch("cut");
        append_to(&path, &"x".repeat(MAX_ENTRY * 2));
        let text = fs::read_to_string(&path).expect("the log is written");
        assert!(text.contains("(truncated)"), "a long entry was not marked");
        assert!(text.len() < MAX_ENTRY * 2, "the entry was not cut");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_file_past_the_cap_starts_over_instead_of_growing_forever() {
        let path = scratch("rotate");
        fs::write(&path, vec![b'.'; (MAX_BYTES + 1) as usize]).expect("a full file");
        append_to(&path, "after rotation");
        let text = fs::read_to_string(&path).expect("the log is written");
        assert!(text.contains("log rotated"));
        assert!(text.ends_with("after rotation\n"));
        assert!(text.len() < 1_000, "rotation kept the old bytes: {}", text.len());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_panic_leaves_a_line_with_its_place_and_message() {
        assert_eq!(
            describe("boom".into(), "src/main.rs:12:5".into()),
            "panic at src/main.rs:12:5: boom"
        );
    }

    /// The wiring, not just the formatting: install the hook, panic, and look in
    /// the file. This is the path a release build takes with no console at all.
    #[test]
    fn an_installed_hook_writes_the_panic_to_the_file() {
        let path = scratch("hook");
        std::env::set_var("AUDIORIUM_LOG_PATH", &path);
        install();
        let outcome = std::panic::catch_unwind(|| panic!("hook probe"));
        assert!(outcome.is_err(), "the probe panic did not unwind");
        let text = fs::read_to_string(&path).expect("the log is written");
        std::env::remove_var("AUDIORIUM_LOG_PATH");
        assert!(text.contains("started"), "no launch line: {text}");
        assert!(text.contains("panic at "), "no panic line: {text}");
        assert!(text.contains("hook probe"), "the payload is missing: {text}");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_calendar_matches_known_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_671), (2026, 8, 6));
        // A leap day, so the month arithmetic is exercised.
        assert_eq!(civil_from_days(18_321), (2020, 2, 29));
    }
}
