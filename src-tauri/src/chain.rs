//! The monitor and playback processing chain.
//!
//! Every frame that reaches a device passes through the same five stages in the
//! same order: the spatial renderer, the binaural panner, the delay-network
//! room, the parametric equaliser, then the look-ahead limiter. Capture
//! monitoring and library playback build the same `Chain` from the same shared
//! configuration, so what you hear on the monitor is the chain you see.
//!
//! The equaliser sits next to the transducer, where a headphone correction
//! belongs, and the limiter follows it to guard the headroom a boost takes.
//! The chain does not add loudness metering or a compressor. Each stage can be
//! switched off, and a stage that is off is bit-exact pass-through.

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

use crate::eq::{self, Eq, EqConfig, EqFilter, EqResponse};
use crate::presets::{Library, PresetList};
use crate::store::{self, Store, Stored};
use crate::limiter::{self, Limiter, LimiterConfig};
use crate::panner::{self, Panner, PannerConfig};
use crate::reverb::{self, Reverb, ReverbConfig};
use crate::spatial::{SpatialConfig, SpatialRenderer};

/// Everything the three processors need, in one value the interface can read and
/// replace atomically.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ChainConfig {
    pub panner: PannerConfig,
    pub reverb: ReverbConfig,
    pub eq: EqConfig,
    pub limiter: LimiterConfig,
}

impl Default for ChainConfig {
    fn default() -> Self {
        Self {
            panner: PannerConfig::default(),
            reverb: ReverbConfig::default(),
            eq: EqConfig::default(),
            limiter: LimiterConfig::default(),
        }
    }
}

impl ChainConfig {
    pub fn validate(&self) -> Result<(), String> {
        panner::validate(&self.panner)?;
        reverb::validate(&self.reverb)?;
        eq::validate(&self.eq)?;
        limiter::validate(&self.limiter)
    }
}

/// What the running chain is actually doing, read from the processors themselves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainMeters {
    pub gain_reduction_db: f32,
    pub output_peak_db: f32,
    pub wet_mix: f32,
    pub panner_enabled: bool,
    pub reverb_enabled: bool,
    pub eq_enabled: bool,
    pub limiter_enabled: bool,
}

/// An equaliser edit and the patch list it produced, in one answer: the interface
/// cannot show a fork that the engine did not make.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EqUpdate {
    pub chain: ChainConfig,
    pub presets: PresetList,
}

pub struct Chain {
    spatial: SpatialRenderer,
    panner: Panner,
    reverb: Reverb,
    eq: Eq,
    limiter: Limiter,
}

impl Chain {
    pub fn new(spatial: SpatialConfig, config: ChainConfig) -> Self {
        Self {
            spatial: SpatialRenderer::new(spatial),
            panner: Panner::new(config.panner),
            reverb: Reverb::new(config.reverb),
            eq: Eq::new(&config.eq),
            limiter: Limiter::new(config.limiter),
        }
    }

    /// One stereo frame in, one processed frame out. The configuration is a
    /// reference so the realtime path never copies the room's network string.
    pub fn process(
        &mut self,
        frame: [f32; 2],
        spatial: SpatialConfig,
        config: &ChainConfig,
    ) -> [f32; 2] {
        let placed = self.spatial.process(frame, spatial);
        let positioned = self.panner.process(placed, config.panner);
        let roomed = self.reverb.process(positioned, &config.reverb);
        let corrected = self.eq.process(roomed, &config.eq);
        self.limiter.process(corrected, config.limiter)
    }

    pub fn meters(&self, config: &ChainConfig) -> ChainMeters {
        ChainMeters {
            gain_reduction_db: self.limiter.reduction_db(),
            output_peak_db: self.limiter.peak_db(),
            wet_mix: if config.reverb.enabled {
                config.reverb.wet
            } else {
                0.0
            },
            panner_enabled: config.panner.enabled,
            reverb_enabled: config.reverb.enabled,
            eq_enabled: config.eq.is_active(),
            limiter_enabled: config.limiter.enabled,
        }
    }
}

/// The shared chain state both audio threads read and the commands write.
pub struct ChainState {
    config: Mutex<ChainConfig>,
    meters: Mutex<ChainMeters>,
    library: Mutex<Library>,
    store: Store,
}

impl Default for ChainState {
    /// In memory only: what a test wants, and what the app runs on if there is
    /// nowhere to write.
    fn default() -> Self {
        Self::memory()
    }
}

impl ChainState {
    pub fn memory() -> Self {
        Self::from(Stored::default(), Store::off())
    }

    /// The last session, with anything that no longer validates left behind.
    pub fn load(store: Store) -> Self {
        let stored = store.path().and_then(store::read).unwrap_or_default();
        Self::from(stored, store)
    }

    fn from(stored: Stored, store: Store) -> Self {
        let mut chain = stored.chain;
        // A room saved when dry and wet were one crossfade takes its balance from it
        // once, here, so the file is rewritten with the two gains it became.
        chain.reverb.settle();
        let chain = if chain.validate().is_ok() {
            chain
        } else {
            ChainConfig::default()
        };
        let mut library = stored.library.checked();
        // A saved file can name a patch whose bands have changed since — a shipped
        // shape that was rebuilt, or a copy edited by hand. The link is only kept if
        // the running bands are still exactly that patch's bands.
        library.reconcile(&chain.eq);
        Self {
            config: Mutex::new(chain),
            meters: Mutex::new(ChainMeters::default()),
            library: Mutex::new(library),
            store,
        }
    }

    /// A full copy, so a render block never holds the lock while it processes.
    pub fn config(&self) -> ChainConfig {
        self.config
            .lock()
            .map(|config| config.clone())
            .unwrap_or_default()
    }

    pub fn meters(&self) -> ChainMeters {
        self.meters.lock().map(|meters| *meters).unwrap_or_default()
    }

    /// The shipped patches, the saved copies, and what the running bands belong to.
    pub fn presets(&self) -> PresetList {
        self.library
            .lock()
            .map(|library| library.list())
            .unwrap_or_else(|_| Library::default().list())
    }

    /// One write carries a whole state, so the file never holds a patch list from
    /// after an edit next to a chain from before it. Called with no lock held.
    fn persist(&self, chain: &ChainConfig) {
        let library = self
            .library
            .lock()
            .map(|library| library.clone())
            .unwrap_or_default();
        self.store.save(Stored {
            version: store::VERSION,
            chain: chain.clone(),
            library,
        });
    }

    /// True when no stage is doing anything, so the chain can be skipped safely.
    fn idle(config: &ChainConfig) -> bool {
        !config.panner.enabled
            && !config.reverb.enabled
            && !config.eq.is_active()
            && !config.limiter.enabled
    }

    pub fn update(
        &self,
        change: impl FnOnce(&mut ChainConfig),
    ) -> Result<ChainConfig, String> {
        let next = {
            let mut current = self.config.lock().map_err(|e| e.to_string())?;
            let mut next = current.clone();
            change(&mut next);
            next.validate()?;
            *current = next.clone();
            next
        };
        self.persist(&next);
        Ok(next)
    }

    /// The whole equaliser in one call, with the patch bookkeeping that belongs to
    /// it: the engine decides whether the edit belongs to a saved patch and forks
    /// a copy the first time a shipped one is changed.
    pub fn set_eq(
        &self,
        enabled: bool,
        preamp_db: f32,
        filters: Vec<EqFilter>,
    ) -> Result<EqUpdate, String> {
        let next = EqConfig {
            enabled,
            preamp_db,
            filters,
        };
        eq::validate(&next)?;
        let presets = {
            let mut library = self.library.lock().map_err(|e| e.to_string())?;
            let _ = library.after_edit(&next);
            library.list()
        };
        let chain = {
            let mut current = self.config.lock().map_err(|e| e.to_string())?;
            let previous = current.clone();
            *current = ChainConfig {
                eq: next,
                ..previous
            };
            current.clone()
        };
        self.persist(&chain);
        Ok(EqUpdate { chain, presets })
    }

    /// Load a patch and switch the stage on, in one write.
    pub fn apply_preset(&self, id: &str) -> Result<EqUpdate, String> {
        let eq = {
            let mut library = self.library.lock().map_err(|e| e.to_string())?;
            library.apply(id)?
        };
        let chain = self.update(|config| config.eq = eq)?;
        Ok(EqUpdate {
            chain,
            presets: self.presets(),
        })
    }

    pub fn rename_preset(&self, id: &str, name: &str) -> Result<PresetList, String> {
        let list = {
            let mut library = self.library.lock().map_err(|e| e.to_string())?;
            library.rename(id, name)?;
            library.list()
        };
        self.persist(&self.config());
        Ok(list)
    }

    pub fn delete_preset(&self, id: &str) -> Result<PresetList, String> {
        let list = {
            let mut library = self.library.lock().map_err(|e| e.to_string())?;
            library.remove(id)?;
            library.list()
        };
        self.persist(&self.config());
        Ok(list)
    }

    pub fn publish(&self, meters: ChainMeters) {
        if let Ok(mut slot) = self.meters.lock() {
            *slot = meters;
        }
    }

    /// The current configuration with every stage that does nothing removed, so a
    /// bypassed chain costs one branch per frame.
    pub fn effective(&self) -> ChainConfig {
        let config = self.config();
        if Self::idle(&config) {
            ChainConfig::default()
        } else {
            config
        }
    }
}

// ---------- commands ----------

/// The five reflection timbres the panner can name, in millisecond order.
#[tauri::command]
pub fn panner_timbres() -> [&'static str; 5] {
    panner::timbres()
}

#[tauri::command]
pub fn chain_config(state: tauri::State<'_, Arc<ChainState>>) -> ChainConfig {
    state.config()
}

#[tauri::command]
pub fn chain_meters(state: tauri::State<'_, Arc<ChainState>>) -> ChainMeters {
    state.meters()
}

#[tauri::command]
pub fn set_panner(
    state: tauri::State<'_, Arc<ChainState>>,
    enabled: bool,
    azimuth: f32,
    elevation: f32,
    width: f32,
    room: bool,
    reflection_length: f32,
    reflection_level: f32,
    timbre: usize,
    focus: f32,
) -> Result<ChainConfig, String> {
    state.update(|config| {
        config.panner = PannerConfig {
            enabled,
            azimuth,
            elevation,
            width,
            room,
            reflection_length,
            reflection_level,
            timbre,
            focus,
        };
    })
}

#[tauri::command]
pub fn set_reverb(
    state: tauri::State<'_, Arc<ChainState>>,
    enabled: bool,
    decay: f32,
    dry: f32,
    wet: f32,
    network: String,
) -> Result<ChainConfig, String> {
    state.update(|config| {
        config.reverb = ReverbConfig {
            enabled,
            decay,
            dry,
            wet,
            network,
            legacy_mix: None,
        };
    })
}

#[tauri::command]
pub fn set_limiter(
    state: tauri::State<'_, Arc<ChainState>>,
    enabled: bool,
    threshold_db: f32,
    ceiling_db: f32,
    release: String,
    detector: String,
    soft_clip: bool,
    auto_makeup: bool,
) -> Result<ChainConfig, String> {
    let release = limiter::ReleaseMode::parse(&release).ok_or("Unknown limiter release")?;
    let detector = limiter::Detector::parse(&detector).ok_or("Unknown limiter detector")?;
    state.update(|config| {
        config.limiter = LimiterConfig {
            enabled,
            threshold_db,
            ceiling_db,
            release,
            detector,
            soft_clip,
            auto_makeup,
        };
    })
}

/// The whole equaliser in one call: the band list replaces the running one, so
/// the interface can never describe a cascade the engine is not running.
#[tauri::command]
pub fn set_eq(
    state: tauri::State<'_, Arc<ChainState>>,
    enabled: bool,
    preamp_db: f32,
    filters: Vec<EqFilter>,
) -> Result<EqUpdate, String> {
    state.set_eq(enabled, preamp_db, filters)
}

/// The patch picker: shipped patches, saved copies, and the active one.
#[tauri::command]
pub fn eq_presets(state: tauri::State<'_, Arc<ChainState>>) -> PresetList {
    state.presets()
}

/// Load a patch into the running cascade and switch the stage on.
#[tauri::command]
pub fn apply_eq_preset(
    state: tauri::State<'_, Arc<ChainState>>,
    id: String,
) -> Result<EqUpdate, String> {
    state.apply_preset(&id)
}

/// Rename a saved copy. A shipped patch keeps its name: the note beside it
/// describes what it does, and a renamed copy of it would make that note a lie.
#[tauri::command]
pub fn rename_eq_preset(
    state: tauri::State<'_, Arc<ChainState>>,
    id: String,
    name: String,
) -> Result<PresetList, String> {
    state.rename_preset(&id, &name)
}

/// Delete a saved copy. The bands the stage is running are never touched.
#[tauri::command]
pub fn delete_eq_preset(
    state: tauri::State<'_, Arc<ChainState>>,
    id: String,
) -> Result<PresetList, String> {
    state.delete_preset(&id)
}

/// The response of the bands the engine is running, from their own coefficients.
#[tauri::command]
pub fn eq_response(
    state: tauri::State<'_, Arc<ChainState>>,
    points: Option<usize>,
) -> EqResponse {
    eq::response(&state.config().eq, points.unwrap_or(96))
}

/// A fresh room for the reverb's randomize buttons. The interface supplies a
/// changing seed, so the network is reproducible when the same seed is reused.
/// `structure_only` keeps the current structure and only reseeds its primes, the
/// way Protoverb's two randomize buttons work on its two text parts.
#[tauri::command]
pub fn random_reverb_network(seed: u32, current: String, structure_only: bool) -> String {
    if !structure_only {
        return reverb::random_network(seed);
    }
    // Reseed keeps the routing and picks new prime lengths, the way Protoverb's
    // second button leaves the network part of the string alone.
    let (structure, inner) = reverb::parse_network(&current).unwrap_or(("woven", 4211));
    let reseeded = inner.wrapping_mul(1_664_525).wrapping_add(seed | 1);
    format!("{structure}:{reseeded}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAMES: usize = 4_800;

    fn render(config: ChainConfig, input: impl Fn(usize) -> [f32; 2]) -> Vec<[f32; 2]> {
        let mut chain = Chain::new(SpatialConfig::default(), config.clone());
        (0..FRAMES)
            .map(|index| chain.process(input(index), SpatialConfig::default(), &config))
            .collect()
    }

    fn tone(index: usize) -> [f32; 2] {
        let value = (2.0 * std::f32::consts::PI * 440.0 * index as f32 / 48_000.0).sin() * 0.5;
        [value, value]
    }

    fn tone_at(index: usize, frequency: f32) -> [f32; 2] {
        let value =
            (2.0 * std::f32::consts::PI * frequency * index as f32 / 48_000.0).sin() * 0.5;
        [value, value]
    }

    fn band(frequency: f32, gain_db: f32) -> EqFilter {
        EqFilter {
            kind: eq::EqKind::Peaking,
            frequency,
            gain_db,
            q: 1.4,
            enabled: true,
        }
    }

    fn peak(frames: &[[f32; 2]]) -> f32 {
        frames
            .iter()
            .fold(0.0f32, |acc, frame| acc.max(frame[0].abs()).max(frame[1].abs()))
    }

    fn db(value: f32) -> f32 {
        20.0 * value.max(1e-12).log10()
    }

    #[test]
    fn an_idle_chain_is_bit_exact() {
        let config = ChainConfig::default();
        assert!(ChainState::idle(&config));
        let out = render(config, tone);
        for (index, frame) in out.iter().enumerate() {
            assert_eq!(*frame, tone(index), "frame {index}");
        }
    }

    #[test]
    fn the_limiter_stage_holds_the_ceiling_on_its_own() {
        let config = ChainConfig {
            limiter: LimiterConfig {
                enabled: true,
                threshold_db: -6.0,
                auto_makeup: false,
                ..LimiterConfig::default()
            },
            ..ChainConfig::default()
        };
        let out = render(config, tone);
        assert!(db(peak(&out)) <= -5.9, "peak {:.2} dBFS", db(peak(&out)));
    }

    #[test]
    fn the_reverb_stage_leaves_a_tail_after_a_burst() {
        let config = ChainConfig {
            reverb: ReverbConfig {
                enabled: true,
                decay: 0.7,
                dry: 1.0,
                wet: 0.5,
                network: "woven:4211".to_string(),
                legacy_mix: None,
            },
            ..ChainConfig::default()
        };
        let burst = |index: usize| if index < 1_000 { tone(index) } else { [0.0; 2] };
        let out = render(config, burst);
        let late = &out[4_000..];
        assert!(peak(late) > 1e-4, "tail was silent: {}", peak(late));
    }

    #[test]
    fn the_panner_stage_moves_the_image() {
        let config = ChainConfig {
            panner: PannerConfig {
                enabled: true,
                azimuth: 90.0,
                ..PannerConfig::default()
            },
            ..ChainConfig::default()
        };
        let out = render(config, tone);
        let energy = |channel: usize| {
            out.iter().map(|frame| frame[channel].powi(2)).sum::<f32>()
        };
        assert!(
            energy(1) > energy(0) * 1.5,
            "right {:.3} vs left {:.3}",
            energy(1),
            energy(0)
        );
    }

    #[test]
    fn meters_describe_the_running_stages() {
        let config = ChainConfig {
            limiter: LimiterConfig {
                enabled: true,
                threshold_db: -12.0,
                ..LimiterConfig::default()
            },
            reverb: ReverbConfig {
                enabled: true,
                wet: 0.4,
                ..ReverbConfig::default()
            },
            ..ChainConfig::default()
        };
        let mut chain = Chain::new(SpatialConfig::default(), config.clone());
        for index in 0..FRAMES {
            chain.process(tone(index), SpatialConfig::default(), &config);
        }
        let meters = chain.meters(&config);
        assert!(meters.gain_reduction_db < -1.0, "gr {}", meters.gain_reduction_db);
        assert!((meters.wet_mix - 0.4).abs() < 1e-6);
        assert!(meters.limiter_enabled && meters.reverb_enabled && !meters.panner_enabled);
        assert!(!meters.eq_enabled, "an empty equaliser reported itself as running");
        assert!(meters.output_peak_db <= -0.2, "peak {}", meters.output_peak_db);

        let idle = ChainConfig::default();
        let mut chain = Chain::new(SpatialConfig::default(), idle.clone());
        chain.process([0.1, 0.1], SpatialConfig::default(), &idle);
        let meters = chain.meters(&idle);
        assert_eq!(meters.wet_mix, 0.0);
        assert_eq!(meters.gain_reduction_db, 0.0);
    }

    #[test]
    fn invalid_settings_are_rejected_before_they_replace_a_working_chain() {
        let state = ChainState::default();
        assert!(state.update(|config| config.reverb.wet = 2.0).is_err());
        assert!(state.update(|config| config.panner.azimuth = 200.0).is_err());
        assert!(state.update(|config| config.limiter.threshold_db = 12.0).is_err());
        assert!(state
            .update(|config| config.eq.filters.push(band(0.0, 6.0)))
            .is_err());
        // A rejected change leaves the previous configuration in place.
        assert_eq!(state.config(), ChainConfig::default());
        assert!(state.update(|config| config.reverb.wet = 0.6).is_ok());
        assert!((state.config().reverb.wet - 0.6).abs() < 1e-6);
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("audiorium-chain-{name}-{stamp}"));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir.join("state.json")
    }

    /// The writer coalesces on its own thread, so a test waits for the value it
    /// expects rather than for a duration it hopes is long enough.
    fn wait_for(path: &std::path::Path, wanted: &str) -> bool {
        for _ in 0..120 {
            if std::fs::read_to_string(path)
                .map(|text| text.contains(wanted))
                .unwrap_or(false)
            {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn an_edit_to_a_shipped_patch_becomes_a_copy_that_survives_a_restart() {
        let path = scratch("restart");
        let state = ChainState::load(Store::at(path.clone()));

        let applied = state.apply_preset("builtin:bass-shelf").unwrap();
        assert!(applied.chain.eq.enabled, "loading a patch left the stage bypassed");
        assert_eq!(applied.chain.eq.filters.len(), 9, "the shelf patch lost a band");
        assert_eq!(applied.presets.active.as_deref(), Some("builtin:bass-shelf"));

        // One change on the plot: the engine forks a copy and follows it.
        let edited = EqFilter {
            kind: eq::EqKind::Lowshelf,
            frequency: 105.0,
            gain_db: 8.0,
            q: 0.7,
            enabled: true,
        };
        let update = state.set_eq(true, -4.0, vec![edited.clone()]).unwrap();
        let copy = update
            .presets
            .presets
            .iter()
            .find(|preset| !preset.builtin)
            .expect("the edit saved no copy")
            .clone();
        assert_eq!(copy.name, "Bass shelf (edited)");
        assert_eq!(update.presets.active.as_deref(), Some(copy.id.as_str()));
        let shipped = update
            .presets
            .presets
            .iter()
            .find(|preset| preset.id == "builtin:bass-shelf")
            .expect("the shipped patch disappeared");
        assert_eq!(shipped.filters[0].gain_db, 5.0, "the built-in patch was edited");

        // Renaming is the only thing a copy needs from the user.
        let renamed = state.rename_preset(&copy.id, "Ananda warm").unwrap();
        assert!(renamed
            .presets
            .iter()
            .any(|preset| preset.name == "Ananda warm" && !preset.builtin));
        assert!(state.rename_preset("builtin:bass-shelf", "Mine").is_err());

        assert!(wait_for(&path, "Ananda warm"), "the rename never reached the disk");

        // A new process reading the same file gets the patch, its name and the bands.
        let restarted = ChainState::load(Store::at(path.clone()));
        let restored = restarted.presets();
        assert_eq!(restored.active.as_deref(), Some(copy.id.as_str()));
        assert!(restored
            .presets
            .iter()
            .any(|preset| preset.name == "Ananda warm" && preset.filters == vec![edited.clone()]));
        assert_eq!(restarted.config().eq.filters, vec![edited]);
        assert_eq!(restarted.config().eq.preamp_db, -4.0);
        assert_eq!(
            restarted
                .presets()
                .presets
                .iter()
                .find(|preset| preset.id == "builtin:bass-shelf")
                .unwrap()
                .filters[0]
                .gain_db,
            5.0
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_refused_edit_changes_neither_the_patch_list_nor_the_chain() {
        let state = ChainState::default();
        state.apply_preset("builtin:presence").unwrap();
        // A preamp above unity is not a setting the engine will run.
        assert!(state.set_eq(true, 6.0, vec![band(1_000.0, 3.0)]).is_err());
        let presets = state.presets();
        assert_eq!(presets.active.as_deref(), Some("builtin:presence"));
        assert!(
            presets.presets.iter().all(|preset| preset.builtin),
            "a refused edit saved a copy"
        );
        assert_eq!(state.config().eq.filters[0].frequency, 63.0);
    }

    #[test]
    fn the_neutral_button_unlinks_the_patch_instead_of_saving_an_empty_copy() {
        let state = ChainState::default();
        state.apply_preset("builtin:presence").unwrap();
        let neutral = state.set_eq(false, 0.0, Vec::new()).unwrap();
        assert_eq!(neutral.presets.active, None, "neutral kept pointing at a patch");
        assert!(
            neutral.presets.presets.iter().all(|preset| preset.builtin),
            "neutral saved an empty copy"
        );
        assert_eq!(neutral.chain.eq, EqConfig::default());
    }

    #[test]
    fn deleting_the_active_copy_leaves_the_bands_running() {
        let state = ChainState::default();
        state.apply_preset("builtin:treble-trim").unwrap();
        let update = state.set_eq(true, 0.0, vec![band(6_000.0, -6.0)]).unwrap();
        let copy = update
            .presets
            .presets
            .iter()
            .find(|preset| !preset.builtin)
            .unwrap()
            .clone();
        let after = state.delete_preset(&copy.id).unwrap();
        assert!(after.presets.iter().all(|preset| preset.id != copy.id));
        assert_eq!(after.active, None);
        assert_eq!(state.config().eq.filters, vec![band(6_000.0, -6.0)]);
        assert!(state.delete_preset("builtin:treble-trim").is_err());
    }

    #[test]
    fn a_stored_file_naming_a_patch_whose_bands_changed_comes_back_unlinked() {
        // What an older build wrote: the two-band Bass shelf, still selected.
        let path = scratch("relinked");
        std::fs::write(
            &path,
            r#"{"version":1,"chain":{"eq":{"enabled":true,"preampDb":0.0,"filters":[
                {"kind":"lowshelf","frequency":80.0,"gainDb":4.0,"q":0.7,"enabled":true},
                {"kind":"peaking","frequency":180.0,"gainDb":-1.5,"q":1.2,"enabled":true}
            ]}},"library":{"saved":[],"active":"builtin:bass-shelf"}}"#,
        )
        .expect("a stored file");
        let state = ChainState::load(Store::at(path.clone()));
        // The bands are kept as they are, but the picker no longer claims they are
        // the shipped shape, which now has nine bands.
        assert_eq!(state.config().eq.filters.len(), 2, "the running bands were rewritten");
        assert_eq!(state.presets().active, None, "a patch the bands do not match is still active");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_chain_file_that_no_longer_validates_falls_back_to_neutral() {
        let path = scratch("stale");
        std::fs::write(
            &path,
            r#"{"version":1,"chain":{"reverb":{"decay":7.5}},"library":{"saved":[],"active":null}}"#,
        )
        .expect("a stale file");
        let state = ChainState::load(Store::at(path.clone()));
        assert_eq!(state.config(), ChainConfig::default());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_room_saved_as_one_crossfade_keeps_its_balance() {
        let path = scratch("legacy-room");
        std::fs::write(
            &path,
            r#"{"version":1,"chain":{"reverb":{"enabled":true,"mix":0.45}},"library":{"saved":[],"active":null}}"#,
        )
        .expect("a saved file");
        let state = ChainState::load(Store::at(path.clone()));
        let room = state.config().reverb;
        assert!(room.enabled, "the saved room was thrown away");
        assert!((room.dry - 0.55).abs() < 1e-6, "dry {}", room.dry);
        assert!((room.wet - 0.45).abs() < 1e-6, "wet {}", room.wet);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_equaliser_stage_raises_the_band_it_names() {
        let config = ChainConfig {
            eq: EqConfig {
                enabled: true,
                preamp_db: 0.0,
                filters: vec![band(1_000.0, 6.0)],
            },
            ..ChainConfig::default()
        };
        let mut chain = Chain::new(SpatialConfig::default(), config.clone());
        let quiet = render(ChainConfig::default(), |index| tone_at(index, 1_000.0));
        let boosted: Vec<[f32; 2]> = (0..FRAMES)
            .map(|index| chain.process(tone_at(index, 1_000.0), SpatialConfig::default(), &config))
            .collect();
        let gain = db(peak(&boosted[2_400..])) - db(peak(&quiet[2_400..]));
        assert!((gain - 6.0).abs() < 0.4, "band measured {gain:.2} dB");
    }

    #[test]
    fn the_limiter_follows_the_equaliser_so_a_boost_cannot_overshoot() {
        let config = ChainConfig {
            eq: EqConfig {
                enabled: true,
                preamp_db: 0.0,
                filters: vec![band(1_000.0, 18.0)],
            },
            limiter: LimiterConfig {
                enabled: true,
                threshold_db: -6.0,
                auto_makeup: false,
                ..LimiterConfig::default()
            },
            ..ChainConfig::default()
        };
        let mut chain = Chain::new(SpatialConfig::default(), config.clone());
        let out: Vec<[f32; 2]> = (0..FRAMES)
            .map(|index| chain.process(tone_at(index, 1_000.0), SpatialConfig::default(), &config))
            .collect();
        assert!(
            db(peak(&out[2_400..])) <= -5.9,
            "peak {:.2} dBFS",
            db(peak(&out[2_400..]))
        );
        let meters = chain.meters(&config);
        assert!(meters.eq_enabled && meters.limiter_enabled);
    }

    #[test]
    fn an_equaliser_with_no_bands_is_a_bypass_even_when_switched_on() {
        let config = ChainConfig {
            eq: EqConfig {
                enabled: true,
                preamp_db: 0.0,
                filters: Vec::new(),
            },
            ..ChainConfig::default()
        };
        assert!(ChainState::idle(&config));
        let out = render(config, tone);
        for (index, frame) in out.iter().enumerate() {
            assert_eq!(*frame, tone(index), "frame {index}");
        }
    }

    #[test]
    fn every_output_stays_finite_with_every_stage_running() {
        let config = ChainConfig {
            panner: PannerConfig {
                enabled: true,
                azimuth: -135.0,
                elevation: 60.0,
                room: true,
                reflection_level: 1.0,
                timbre: 4,
                ..PannerConfig::default()
            },
            reverb: ReverbConfig {
                enabled: true,
                decay: 1.0,
                dry: 1.0,
                wet: 1.0,
                network: "series:99".to_string(),
                legacy_mix: None,
            },
            limiter: LimiterConfig {
                enabled: true,
                threshold_db: -18.0,
                soft_clip: true,
                ..LimiterConfig::default()
            },
            eq: EqConfig {
                enabled: true,
                preamp_db: -6.0,
                filters: vec![band(120.0, 12.0), band(6_000.0, -9.0)],
            },
        };
        let out = render(config, |index| [tone(index)[0] * 1.4, -tone(index)[0] * 1.4]);
        assert!(out.iter().all(|frame| frame[0].is_finite()
            && frame[1].is_finite()
            && frame[0].abs() <= 1.0
            && frame[1].abs() <= 1.0));
    }
}
