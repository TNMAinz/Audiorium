//! Audio wiring shared by the app: output enumeration, the spatial renderer and
//! the processing chain.
//!
//! Library playback owns the app's render path: nothing is captured from the
//! system mix into it, so no stage processes the same file twice. The player owns
//! that stream; this module owns the configuration it reads and re-exports the
//! analysis state the same stream publishes into. A capture that the user asks to
//! hear renders through a second stream of its own, built from the same
//! configuration, and never through the player's.

use std::sync::{Arc, Mutex};
use std::thread;

use serde::Serialize;
use wasapi::{DeviceEnumerator, Direction};

use crate::analysis::AnalysisState;
use crate::chain::ChainState;
use crate::spatial::{self, SpatialConfig, SpatialMode};
use crate::store::Store;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    id: String,
    name: String,
    is_default: bool,
}

pub struct AudioState {
    /// The spatial renderer every played frame passes through.
    spatial: Arc<Mutex<SpatialConfig>>,
    /// The panner, room, equaliser and limiter, with the last session's settings.
    chain: Arc<ChainState>,
    /// What the player publishes after the chain, so the panels measure the
    /// stream that is leaving the app.
    analysis: Arc<AnalysisState>,
}
impl Default for AudioState {
    fn default() -> Self {
        Self {
            spatial: Arc::new(Mutex::new(SpatialConfig::default())),
            // The last session's chain and patches, read from disk. If there is no
            // file, or it no longer validates, this is the neutral chain.
            chain: Arc::new(ChainState::load(Store::maybe(crate::store::config_path()))),
            analysis: Arc::new(AnalysisState::default()),
        }
    }
}
impl AudioState {
    pub fn spatial_handle(&self) -> Arc<Mutex<SpatialConfig>> {
        self.spatial.clone()
    }
    /// The panner, room, equaliser and limiter, shared with the playback engine.
    pub fn chain_handle(&self) -> Arc<ChainState> {
        self.chain.clone()
    }
    /// The readings the playback engine publishes and the interface polls.
    pub fn analysis_handle(&self) -> Arc<AnalysisState> {
        self.analysis.clone()
    }
}

fn fail(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// The endpoints Windows lists in one direction, with the default marked. Outputs
/// are what playback renders to; inputs are the devices a capture path could
/// read, including the Audiorium device when it is installed.
fn enumerate(direction: Direction) -> Result<Vec<DeviceInfo>, String> {
    wasapi::initialize_mta().ok().map_err(fail)?;
    let enumerator = DeviceEnumerator::new().map_err(fail)?;
    let default_id = enumerator
        .get_default_device(&direction)
        .map_err(fail)?
        .get_id()
        .map_err(fail)?;
    let collection = enumerator.get_device_collection(&direction).map_err(fail)?;
    let mut result = Vec::new();
    for device in &collection {
        let device = device.map_err(fail)?;
        let id = device.get_id().map_err(fail)?;
        result.push(DeviceInfo {
            name: device.get_friendlyname().map_err(fail)?,
            is_default: id == default_id,
            id,
        });
    }
    Ok(result)
}

#[tauri::command]
pub fn list_audio_outputs() -> Result<Vec<DeviceInfo>, String> {
    thread::spawn(|| enumerate(Direction::Render))
        .join()
        .map_err(|_| "Audio device enumeration failed".to_string())?
}

#[tauri::command]
pub fn list_audio_inputs() -> Result<Vec<DeviceInfo>, String> {
    thread::spawn(|| enumerate(Direction::Capture))
        .join()
        .map_err(|_| "Audio device enumeration failed".to_string())?
}

#[tauri::command]
pub fn set_spatial(
    state: tauri::State<'_, AudioState>,
    mode: String,
    width: f32,
    crossfeed: f32,
    height: f32,
    rotation: f32,
) -> Result<(), String> {
    let mode = SpatialMode::parse(&mode).ok_or("Unknown spatial mode")?;
    let config = SpatialConfig {
        mode,
        width,
        crossfeed,
        height,
        rotation,
    };
    spatial::validate(&config)?;
    let mut current = state.spatial.lock().map_err(fail)?;
    *current = config;
    Ok(())
}

#[tauri::command]
pub fn spatial_layout(state: tauri::State<'_, AudioState>) -> Result<Vec<spatial::SpeakerView>, String> {
    let config = state.spatial.lock().map(|config| *config).map_err(fail)?;
    Ok(spatial::describe(config))
}

/// Whether the selected output can render the Atmos bed, and why not when it
/// cannot. This only probes the endpoint; opening the sink happens at play time.
#[tauri::command]
pub fn spatial_output(device_id: String) -> crate::spatial_out::SpatialOutput {
    thread::spawn(move || crate::spatial_out::probe(&device_id))
        .join()
        .unwrap_or_else(|_| {
            crate::spatial_out::SpatialOutput::unavailable("The spatial sound check failed.")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shared_handles_point_at_one_configuration() {
        let state = AudioState::default();
        let spatial = state.spatial_handle();
        let analysis = state.analysis_handle();
        // The player and the interface must see the same objects, not copies.
        assert!(Arc::ptr_eq(&spatial, &state.spatial));
        assert!(Arc::ptr_eq(&analysis, &state.analysis));
        assert!(Arc::ptr_eq(&state.chain_handle(), &state.chain));
    }

    /// The page's two lists come from these two calls, so the check that they
    /// answer on a real machine is worth having:
    /// `cargo test --bins both_directions -- --ignored --nocapture`.
    #[test]
    #[ignore = "enumerates the real audio endpoints"]
    fn both_directions_enumerate_this_machine() {
        let outputs = enumerate(Direction::Render).expect("output endpoints");
        let inputs = enumerate(Direction::Capture).expect("input endpoints");
        println!("{} outputs, {} inputs", outputs.len(), inputs.len());
        for device in &outputs {
            println!("  out: {}{}", device.name, if device.is_default { " (default)" } else { "" });
        }
        for device in &inputs {
            println!("  in:  {}{}", device.name, if device.is_default { " (default)" } else { "" });
        }
        assert!(!outputs.is_empty(), "Windows listed no output devices");
    }

    #[test]
    fn an_idle_engine_publishes_no_level() {
        let state = AudioState::default();
        let readings = state.analysis_handle().read();
        assert!(!readings.playing);
        assert_eq!(readings.rms, 0.0);
        assert!(readings.spectrum.is_empty());
    }
}
