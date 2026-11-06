//! The optional Audiorium audio device.
//!
//! The app plays library files to whichever output Windows offers, and records
//! nothing. The device this repository builds is an extra: a user can install it
//! so other applications reach Windows' audio engine through an endpoint named
//! Audiorium, or ignore it and pick any device Windows already lists. This module
//! is the only place that knows the device exists. It answers what the Devices
//! page asks and runs the bundled installer when the user presses the button.

use std::path::{Path, PathBuf};

use serde::Serialize;
use wasapi::{DeviceEnumerator, Direction};

/// The hardware ID the INF matches. The node is root-enumerated, so it sits under
/// `Enum\Root\MEDIA` and stays there until someone removes it.
const HARDWARE_ID: &str = "*AudioriumVirtualAudio";
/// Both endpoints carry the device name, e.g. "Speakers (Audiorium)".
const NAME_MARKER: &str = "audiorium";
/// How long the elevated installer may run before the app stops waiting for it.
const INSTALL_TIMEOUT_MS: u32 = 240_000;
/// How much of the installer's log the interface keeps.
const LOG_LIMIT: usize = 4000;
/// The registry key the firmware's Secure Boot state is published under.
const SECURE_BOOT_KEY: &str = r"HKLM\SYSTEM\CurrentControlSet\Control\SecureBoot\State";

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverStatus {
    /// The device node is registered, whether or not Windows started it.
    pub installed: bool,
    /// Both endpoints enumerate now, which is the only proof the driver loaded.
    pub active: bool,
    /// The names Windows lists the endpoints under, empty when absent.
    pub render_name: String,
    pub capture_name: String,
    /// Firmware Secure Boot is on: Windows refuses a test-signed driver until it is off.
    pub secure_boot: bool,
    /// The installer this build would run, empty when it carries no package.
    pub installer: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallReport {
    /// The installer exited successfully. A reboot may still be outstanding.
    pub ok: bool,
    /// The installer's own output, so a failure is readable where it happened.
    pub log: String,
    pub status: DriverStatus,
}

/// What this machine has, asked without starting anything.
pub fn status(app: Option<&tauri::AppHandle>) -> DriverStatus {
    let (render, capture) = endpoints();
    DriverStatus {
        installed: device_node_registered(),
        active: !render.is_empty() && !capture.is_empty(),
        render_name: render,
        capture_name: capture,
        secure_boot: secure_boot_enabled(),
        installer: package_root(app)
            .and_then(|root| installer_in(&root))
            .map(|installer| installer.display().to_string())
            .unwrap_or_default(),
    }
}

/// Runs the bundled installer elevated and waits for it, because the button is a
/// request for one finished action: either the package is in, or the log says why.
pub fn install(app: &tauri::AppHandle) -> Result<InstallReport, String> {
    let root = package_root(Some(app)).ok_or(
        "This build carries no driver package: install it with driver\\install.cmd.",
    )?;
    let installer = installer_in(&root)
        .ok_or("The driver package in this build is incomplete: driver\\install.cmd or build\\audiorium.sys is missing.")?;
    let log = std::env::temp_dir().join("audiorium-install.log");
    let _ = std::fs::remove_file(&log);
    let output = run_elevated(&installer, &log)?;
    Ok(InstallReport {
        ok: output.ok,
        log: output.log,
        status: status(Some(app)),
    })
}

struct InstallerOutput {
    ok: bool,
    log: String,
}

/// The directory holding `install.cmd` and the package it installs, in the order
/// this build could find it: an explicit path, a bundled resource, the checkout.
fn package_root(app: Option<&tauri::AppHandle>) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(directory) = std::env::var_os("AUDIORIUM_DRIVER_DIR") {
        candidates.push(PathBuf::from(directory));
    }
    if let Some(app) = app {
        use tauri::Manager;
        if let Ok(resources) = app.path().resource_dir() {
            candidates.push(resources.join("driver"));
        }
    }
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("driver"));
    candidates.into_iter().find(|root| installer_in(root).is_some())
}

/// `install.cmd` is only an installer when the package it runs is beside it: the
/// script derives `build\` from its own directory.
fn installer_in(root: &Path) -> Option<PathBuf> {
    let installer = root.join("install.cmd");
    let package = root.join("build").join("audiorium.sys");
    (installer.is_file() && package.is_file()).then_some(installer)
}

/// The endpoints Windows lists, by the names they were given. A driver that is
/// registered but not started has a node and no endpoints, which is the state a
/// pending reboot leaves behind.
fn endpoints() -> (String, String) {
    std::thread::spawn(|| {
        if wasapi::initialize_mta().is_err() {
            return (String::new(), String::new());
        }
        match DeviceEnumerator::new() {
            Ok(enumerator) => (
                named_endpoint(&enumerator, Direction::Render),
                named_endpoint(&enumerator, Direction::Capture),
            ),
            Err(_) => (String::new(), String::new()),
        }
    })
    .join()
    .unwrap_or_default()
}

fn named_endpoint(enumerator: &DeviceEnumerator, direction: Direction) -> String {
    let Ok(collection) = enumerator.get_device_collection(&direction) else {
        return String::new();
    };
    for device in &collection {
        let Ok(device) = device else { continue };
        let Ok(name) = device.get_friendlyname() else { continue };
        if name.to_lowercase().contains(NAME_MARKER) {
            return name;
        }
    }
    String::new()
}

/// Whether the root-enumerated node exists. `reg` is the platform's own reader and
/// needs no elevation for the Enum tree, so nothing here asks for permission to
/// merely look.
fn device_node_registered() -> bool {
    let marker = HARDWARE_ID.trim_start_matches('*');
    reg(&[
        "query",
        r"HKLM\SYSTEM\CurrentControlSet\Enum\Root\MEDIA",
        "/s",
        "/d",
        "/f",
        marker,
    ])
    .map(|found| found.to_lowercase().contains(&marker.to_lowercase()))
    .unwrap_or(false)
}

/// The firmware setting that decides whether a test-signed driver can load at all.
fn secure_boot_enabled() -> bool {
    reg(&["query", SECURE_BOOT_KEY, "/v", "UEFISecureBootEnabled"])
        .map(|value| value.split_whitespace().last() == Some("0x1"))
        .unwrap_or(false)
}

fn reg(args: &[&str]) -> Option<String> {
    use std::os::windows::process::CommandExt;
    // A console window would flash over the app for a registry read.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new("reg")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).to_string())
}

/// `install.cmd` needs administrator rights for `bcdedit`, the certificate stores
/// and the device node. The app asks Windows for them instead of requiring that
/// the app itself was started elevated, and waits for the one process it started.
fn run_elevated(installer: &Path, log: &Path) -> Result<InstallerOutput, String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    // cmd is what carries the redirection; the installer's own output is the log.
    let parameters = HSTRING::from(format!(
        "/c \"\"{}\" > \"{}\" 2>&1\"",
        installer.display(),
        log.display()
    ));
    let verb = HSTRING::from("runas");
    let file = HSTRING::from("cmd.exe");
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        nShow: SW_HIDE.0 as i32,
        ..Default::default()
    };
    // SAFETY: `info` is a fully initialised SHELLEXECUTEINFOW for the duration of
    // the call, and its three strings outlive it.
    unsafe { ShellExecuteExW(&mut info) }.map_err(|error| {
        if error.code() == ERROR_CANCELLED.to_hresult() {
            "Installation was cancelled at the Windows permission prompt.".to_string()
        } else {
            format!("Windows could not start the installer: {error}")
        }
    })?;

    let process = info.hProcess;
    if process.is_invalid() {
        return Err("Windows started the installer but returned no process to wait for.".into());
    }
    // SAFETY: the handle came from ShellExecuteExW with SEE_MASK_NOCLOSEPROCESS.
    let waited = unsafe { WaitForSingleObject(process, INSTALL_TIMEOUT_MS) };
    let mut code = 1u32;
    // SAFETY: the same live process handle.
    let read = unsafe { GetExitCodeProcess(process, &mut code) };
    // SAFETY: this is the only owner of the handle ShellExecuteExW handed back.
    unsafe { let _ = CloseHandle(process); };

    if waited == WAIT_TIMEOUT {
        return Err("The installer is still running, so its result is not known yet.".into());
    }
    if waited != WAIT_OBJECT_0 || read.is_err() {
        return Err("The installer exited before Windows reported its result.".into());
    }
    Ok(InstallerOutput {
        ok: code == 0,
        log: read_log(log),
    })
}

fn read_log(log: &Path) -> String {
    let text = std::fs::read(log)
        .map(|bytes| String::from_utf8_lossy(&bytes).replace('\r', ""))
        .unwrap_or_default();
    let trimmed = text.trim();
    if trimmed.len() <= LOG_LIMIT {
        return trimmed.to_string();
    }
    // The failure is at the end of an installer log, so the tail is what is kept,
    // starting at a character boundary so a log with non-ASCII output stays valid.
    let mut start = trimmed.len() - LOG_LIMIT;
    while start < trimmed.len() && !trimmed.is_char_boundary(start) {
        start += 1;
    }
    trimmed[start..].to_string()
}

#[tauri::command]
pub fn audiorium_device_status(app: tauri::AppHandle) -> DriverStatus {
    status(Some(&app))
}

#[tauri::command]
pub async fn install_audiorium_device(app: tauri::AppHandle) -> Result<InstallReport, String> {
    // Waiting on an elevation prompt must not hold the interface's own thread.
    tauri::async_runtime::spawn_blocking(move || install(&app))
        .await
        .map_err(|error| format!("The installer task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("audiorium-driver-{name}"));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn a_package_is_only_found_when_the_installer_and_the_driver_are_both_there() {
        let root = scratch("package");
        fs::write(root.join("install.cmd"), "").unwrap();
        // The script derives build\ from its own directory, so the pair is required.
        assert!(installer_in(&root).is_none());
        fs::create_dir_all(root.join("build")).unwrap();
        fs::write(root.join("build").join("audiorium.sys"), "").unwrap();
        assert_eq!(installer_in(&root), Some(root.join("install.cmd")));
    }

    #[test]
    fn status_reads_this_machine_without_requiring_the_device() {
        // The device is optional, so the only promise is a coherent answer: the two
        // endpoints are both there or both absent, and an installer path is a file
        // that exists. This runs the registry reads and the endpoint enumeration for
        // real, which is what the page's button and its status line depend on.
        let current = status(None);
        assert_eq!(current.active, !current.render_name.is_empty() && !current.capture_name.is_empty());
        assert!(current.installer.is_empty() || Path::new(&current.installer).is_file());
        assert!(!current.active || current.installed);
        // A checkout that carries the package finds it without being told where.
        let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("driver").join("install.cmd");
        if checkout.is_file() {
            assert!(current.installer.replace('\\', "/").ends_with("driver/install.cmd"), "{}", current.installer);
        }
        let _ = current.secure_boot;
    }

    #[test]
    fn an_installer_log_keeps_its_end_and_never_splits_a_character() {
        let path = std::env::temp_dir().join("audiorium-driver-log.txt");
        fs::write(&path, format!("start\r\n{}\r\nfailure", "a".repeat(LOG_LIMIT))).unwrap();
        let log = read_log(&path);
        assert!(log.ends_with("failure"));
        assert!(log.len() <= LOG_LIMIT);
        assert!(!log.contains('\r'));
        let _ = fs::remove_file(&path);
        assert_eq!(read_log(Path::new("no-such-audiorium-log")), "");
    }
}
