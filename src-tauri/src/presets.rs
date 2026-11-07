//! Saved equaliser patches.
//!
//! A patch is a preamp and a band list, and nothing else: the stage's own bypass
//! belongs to the running chain, not to the patch, so loading one never surprises
//! you by making it audible.
//!
//! The built-in patches are constants in this file. They are never written to the
//! store and never mutated, so they cannot be damaged: applying one and editing it
//! saves a copy under a new name, and the original stays exactly as shipped. That
//! fork happens in the engine, on the first edit that actually changes a value, so
//! it happens even if the interface forgets to ask for it.
//!
//! What this is not: a headphone correction database. A built-in patch is a named
//! starting point with a note that says what it does; no patch here claims to match
//! a measured headphone.

use serde::{Deserialize, Serialize};

use crate::autoeq;
use crate::eq::{self, EqConfig, EqFilter, EqKind};

/// Long enough for "Bass shelf (edited) 2", short enough for one row.
pub const MAX_NAME: usize = 40;
/// A ceiling on saved patches, so one runaway loop cannot grow the file forever.
pub const MAX_SAVED: usize = 60;

/// Where a patch's numbers came from, for the patches that were not authored here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    /// Who produced it and from what, in one line.
    pub label: String,
    /// The exact file it was read from.
    pub url: String,
    /// The terms it is used under, so the app never hides an obligation.
    pub terms: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    /// Stable across renames: the interface addresses a patch by this, not its name.
    pub id: String,
    pub name: String,
    /// Shipped patches live in `builtins()` or in the vendored correction set;
    /// only saved copies are in the store.
    pub builtin: bool,
    /// One line about what the patch does, shown beside its name.
    pub note: String,
    pub preamp_db: f32,
    pub filters: Vec<EqFilter>,
    /// Set on a patch whose numbers came from published data; `None` for a patch
    /// written here, and for a copy the user has since edited.
    #[serde(default)]
    pub source: Option<Provenance>,
}

impl Preset {
    /// The band set this patch stands for, for comparison and for loading.
    fn body(&self) -> (f32, &[EqFilter]) {
        (self.preamp_db, &self.filters)
    }

    /// True when the running equaliser already is this patch, so an edit that
    /// changes nothing does not fork a copy.
    pub fn matches(&self, config: &EqConfig) -> bool {
        let (preamp_db, filters) = self.body();
        preamp_db == config.preamp_db && filters == config.filters.as_slice()
    }
}

fn band(kind: EqKind, frequency: f32, gain_db: f32, q: f32) -> EqFilter {
    EqFilter {
        kind,
        frequency,
        gain_db,
        q,
        enabled: true,
        dynamics: None,
    }
}

/// A shipped patch, with its preamp computed from its own curve: the largest boost
/// in the audio band, rounded up to a tenth of a decibel and taken out ahead of the
/// bands. A shipped patch therefore never asks for headroom it does not have, and
/// the number cannot drift away from the bands because it is derived from them. A
/// curve that only cuts, to within a rounding error, carries no preamp at all.
fn built_in(id: &str, name: &str, note: &str, filters: Vec<EqFilter>) -> Preset {
    let boost = eq::response(
        &EqConfig {
            enabled: true,
            preamp_db: 0.0,
            filters: filters.clone(),
        },
        512,
    )
    .max_boost_db;
    Preset {
        id: id.into(),
        name: name.into(),
        builtin: true,
        note: note.into(),
        // A high pass is flat to within a float rounding error up top; that is not
        // a boost and must not cost a whole decibel of level.
        preamp_db: if boost > 0.05 {
            -((boost * 10.0).ceil() / 10.0)
        } else {
            0.0
        },
        filters,
        source: None,
    }
}

/// The nine octave centres every shipped shape is drawn on: the bands a graphic
/// equaliser has had since the 1970s, low to high.
pub const OCTAVES: [f32; 9] = [
    63.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
];
/// One octave of bandwidth, the Q that goes with those centres: a band reaches the
/// middle of its neighbours and no further, so nine bands tile the spectrum.
pub const OCTAVE_Q: f32 = 1.41;

/// One shape, as nine gains in the order of `OCTAVES`.
fn graphic(gains: [f32; 9]) -> Vec<EqFilter> {
    OCTAVES
        .iter()
        .zip(gains)
        .map(|(frequency, gain_db)| band(EqKind::Peaking, *frequency, gain_db, OCTAVE_Q))
        .collect()
}

/// The shipped patches: ten named shapes, every one of them nine peaking bands on
/// the octave grid, so any shape can be bent band by band the way a graphic
/// equaliser is used. A band left at 0 dB is a band you have not touched yet, not a
/// missing one.
///
/// They are starting points written here, not measurements. No patch in this list
/// is derived from a headphone or a room, so none of them is a correction, and the
/// picker says so beside them. A patch that claims to correct a specific model has
/// to come from that model's measurement, which is what the importer is for.
pub fn builtins() -> Vec<Preset> {
    vec![
        built_in(
            "builtin:flat",
            "Flat",
            "All nine bands at 0 dB from 63 Hz to 16 kHz: a straight wire with a handle on every octave.",
            graphic([0.0; 9]),
        ),
        built_in(
            "builtin:bass-shelf",
            "Bass shelf",
            "63 Hz +5, 125 Hz +4, 250 Hz +2, flat from 500 Hz: weight under a mix, rolled off instead of thickening it.",
            graphic([5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        ),
        built_in(
            "builtin:treble-trim",
            "Treble trim",
            "2 kHz -1, 4 kHz -2, 8 and 16 kHz -3: the top falls away evenly instead of ending at a shelf edge.",
            graphic([0.0, 0.0, 0.0, 0.0, 0.0, -1.0, -2.0, -3.0, -3.0]),
        ),
        built_in(
            "builtin:presence",
            "Presence",
            "500 Hz -1 with 2 kHz +2, 4 kHz +2.5 and 8 kHz +1.5: leading edges and breath, with the boxiness out of the way.",
            graphic([0.0, 0.0, 0.0, -1.0, 0.0, 2.0, 2.5, 1.5, 1.0]),
        ),
        built_in(
            "builtin:rumble-filter",
            "Rumble filter",
            "63 Hz -12 and 125 Hz -5, nothing above 250 Hz touched: subsonic weight out without thinning the bass.",
            graphic([-12.0, -5.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        ),
        built_in(
            "builtin:vocal",
            "Vocal",
            "63 Hz -4 and 250 Hz -2, with 1 kHz +1, 2 kHz +2.5 and 4 kHz +2: speech forward with the mud taken out.",
            graphic([-4.0, -2.0, -2.0, 0.0, 1.0, 2.5, 2.0, 1.0, 0.0]),
        ),
        built_in(
            "builtin:warm-tilt",
            "Warm tilt",
            "A down-tilt: +3 dB at 63 Hz through to -2.5 dB at 16 kHz, sloping the whole octave grid down.",
            graphic([3.0, 2.0, 1.0, 0.0, 0.0, -0.5, -1.0, -2.0, -2.5]),
        ),
        built_in(
            "builtin:bright-tilt",
            "Bright tilt",
            "An up-tilt: -2.5 dB at 63 Hz through to +2 dB at 16 kHz, the same slope the other way.",
            graphic([-2.5, -2.0, -1.0, 0.0, 0.0, 0.5, 1.0, 1.5, 2.0]),
        ),
        built_in(
            "builtin:butterworth-sub",
            "Butterworth 30 Hz",
            "The fourth-order Butterworth at 30 Hz, drawn on the grid: 63 Hz -24 dB and 125 Hz -24 dB (the alignment is past both, and the bands stop at -24), 250 Hz -9, 500 Hz -3. Its shape, not its two-section filter.",
            graphic([-24.0, -24.0, -9.0, -3.0, -0.5, 0.0, 0.0, 0.0, 0.0]),
        ),
        built_in(
            "builtin:quiet",
            "Quiet level",
            "An equal-loudness-style lift for low-level listening: 63 Hz +5, 125 Hz +3, 8 kHz +1.5, 16 kHz +2. Not a Fletcher-Munson implementation.",
            graphic([5.0, 3.0, 1.0, 0.0, 0.0, 0.0, 0.5, 1.5, 2.0]),
        ),
    ]
}

/// A name a patch may carry: trimmed, non-empty, no control characters, and short
/// enough to sit in one row.
pub fn clean_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("A patch needs a name".into());
    }
    if trimmed.chars().any(|character| character.is_control()) {
        return Err("A patch name cannot contain control characters".into());
    }
    if trimmed.chars().count() > MAX_NAME {
        return Err(format!("A patch name is at most {MAX_NAME} characters"));
    }
    Ok(trimmed.to_string())
}

/// The saved copies and which one the running bands came from. Built-in patches
/// are not stored here: they are constants.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    #[serde(default)]
    pub saved: Vec<Preset>,
    /// `None` when the running bands are a working set that belongs to no patch.
    #[serde(default)]
    pub active: Option<String>,
}

/// What the interface lists: the shipped patches, then the saved ones, and the
/// patch the running bands belong to.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetList {
    pub presets: Vec<Preset>,
    pub active: Option<String>,
}

impl Library {
    /// Everything the picker shows: the measured corrections first, because they
    /// are the ones with evidence behind them, then the shapes written here, then
    /// the copies saved on this machine.
    pub fn list(&self) -> PresetList {
        let mut presets = autoeq::presets();
        presets.extend(builtins());
        presets.extend(self.saved.iter().cloned());
        PresetList {
            presets,
            active: self.active.clone(),
        }
    }

    pub fn find(&self, id: &str) -> Option<Preset> {
        autoeq::presets()
            .into_iter()
            .chain(builtins())
            .chain(self.saved.iter().cloned())
            .find(|preset| preset.id == id)
    }

    fn is_builtin(id: &str) -> bool {
        // Both kinds of correction are shipped: AutoEq's files, and the profiles fitted here.
        id.starts_with("autoeq:")
            || id.starts_with("fit:")
            || builtins().iter().any(|preset| preset.id == id)
    }

    /// The next free `custom:n` id. Counting from the highest number in the store
    /// keeps ids unique across restarts without a timestamp or a random source.
    fn next_id(&self) -> String {
        let highest = self
            .saved
            .iter()
            .filter_map(|preset| preset.id.strip_prefix("custom:"))
            .filter_map(|suffix| suffix.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("custom:{}", highest + 1)
    }

    /// A copy name that is not already taken, and still fits one row.
    fn copy_name(&self, original: &str) -> String {
        let base = format!("{original} (edited)");
        let fit = |value: String| -> String {
            if value.chars().count() <= MAX_NAME {
                return value;
            }
            value.chars().take(MAX_NAME).collect()
        };
        let taken = |name: &str| self.saved.iter().any(|preset| preset.name == name);
        let first = fit(base.clone());
        if !taken(&first) {
            return first;
        }
        for round in 2.. {
            let candidate = fit(format!("{base} {round}"));
            if !taken(&candidate) {
                return candidate;
            }
        }
        first
    }

    /// Called after the running bands changed. A saved patch follows its own edits;
    /// a shipped patch is copied first, so editing it can never alter it. A working
    /// set that belongs to no patch stays a working set.
    pub fn after_edit(&mut self, config: &EqConfig) -> Option<String> {
        // Neutral is the absence of a patch, not a patch with no bands in it: it
        // unlinks the bands and saves nothing, so clearing a patch cannot leave an
        // empty copy behind under a name that promised something.
        if config.filters.is_empty() && config.preamp_db == 0.0 {
            self.active = None;
            return None;
        }
        let active = self.active.clone()?;
        let Some(origin) = self.find(&active) else {
            // The patch was deleted: the bands become a working set.
            self.active = None;
            return None;
        };
        if origin.matches(config) {
            return Some(active);
        }
        if origin.builtin {
            // At the ceiling the edit stays a working set rather than silently
            // dropping a patch the user saved earlier.
            if self.saved.len() >= MAX_SAVED {
                return Some(active);
            }
            // The copy is the user's, so it carries their numbers and not the
            // original's provenance; the note says where it began.
            let note = if origin.id.starts_with("fit:") {
                format!("Edited from this app's own fit for {}.", origin.name)
            } else if origin.source.is_some() {
                format!("Edited from the AutoEq correction for {}.", origin.name)
            } else {
                format!("Edited from the built-in {}.", origin.name)
            };
            let copy = Preset {
                id: self.next_id(),
                name: self.copy_name(&origin.name),
                builtin: false,
                note,
                preamp_db: config.preamp_db,
                filters: config.filters.clone(),
                source: None,
            };
            self.active = Some(copy.id.clone());
            self.saved.push(copy);
            return self.active.clone();
        }
        if let Some(saved) = self.saved.iter_mut().find(|preset| preset.id == active) {
            saved.preamp_db = config.preamp_db;
            saved.filters = config.filters.clone();
        }
        Some(active)
    }

    /// Load a patch into the running chain. The stage is switched on, because a
    /// patch you just chose and cannot hear is a patch that did nothing.
    pub fn apply(&mut self, id: &str) -> Result<EqConfig, String> {
        let preset = self.find(id).ok_or("That patch is not in the library")?;
        self.active = Some(preset.id.clone());
        Ok(EqConfig {
            enabled: true,
            preamp_db: preset.preamp_db,
            filters: preset.filters.clone(),
        })
    }

    /// Only saved copies can be renamed: a shipped patch keeps its name so the
    /// note that describes it stays true.
    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), String> {
        if Self::is_builtin(id) {
            return Err("A built-in patch keeps its name. Rename the copy instead.".into());
        }
        let name = clean_name(name)?;
        let saved = self
            .saved
            .iter_mut()
            .find(|preset| preset.id == id)
            .ok_or("That patch is not in the library")?;
        saved.name = name;
        Ok(())
    }

    /// Saved copies only. Deleting the patch the bands came from leaves the bands
    /// alone and unlinks them, so nothing audible changes.
    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        if Self::is_builtin(id) {
            return Err("A built-in patch cannot be deleted".into());
        }
        let before = self.saved.len();
        self.saved.retain(|preset| preset.id != id);
        if self.saved.len() == before {
            return Err("That patch is not in the library".into());
        }
        if self.active.as_deref() == Some(id) {
            self.active = None;
        }
        Ok(())
    }

    /// Keep the active link only when the running bands really are that patch.
    ///
    /// A shipped shape can change between versions, and a saved copy's numbers can be
    /// edited in the file; either way the picker must not claim the running bands came
    /// from a patch they no longer match. Nothing is rewritten — the bands stay, they
    /// simply stop being attributed to a patch and become a working set again.
    pub fn reconcile(&mut self, config: &EqConfig) {
        if let Some(active) = self.active.clone() {
            let matches = self
                .find(&active)
                .map(|preset| preset.matches(config))
                .unwrap_or(false);
            if !matches {
                self.active = None;
            }
        }
    }

    /// A well-formed library, with anything unreadable dropped rather than loaded
    /// into the audio path.
    pub fn checked(mut self) -> Self {
        self.saved.retain(|preset| {
            !preset.builtin
                && !preset.id.trim().is_empty()
                && !Self::is_builtin(&preset.id)
                && clean_name(&preset.name).is_ok()
                && eq::validate(&EqConfig {
                    enabled: true,
                    preamp_db: preset.preamp_db,
                    filters: preset.filters.clone(),
                })
                .is_ok()
        });
        self.saved.truncate(MAX_SAVED);
        if let Some(active) = self.active.clone() {
            if self.find(&active).is_none() {
                self.active = None;
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(frequency: f32, gain_db: f32) -> EqFilter {
        band(EqKind::Peaking, frequency, gain_db, 1.4)
    }

    fn config(preamp_db: f32, filters: Vec<EqFilter>) -> EqConfig {
        EqConfig {
            enabled: true,
            preamp_db,
            filters,
        }
    }

    #[test]
    fn every_builtin_is_a_usable_patch_with_a_note_that_explains_it() {
        let presets = builtins();
        assert!(presets.len() >= 8, "only {} shipped patches", presets.len());
        let mut ids: Vec<&str> = presets.iter().map(|preset| preset.id.as_str()).collect();
        ids.sort_unstable();
        let unique = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), unique, "two built-in patches share an id");
        for preset in &presets {
            assert!(preset.builtin, "{} is not marked built-in", preset.id);
            assert!(!preset.note.trim().is_empty(), "{} has no note", preset.id);
            assert_eq!(
                clean_name(&preset.name).as_deref(),
                Ok(preset.name.as_str()),
                "{} has a name the interface could not show",
                preset.id
            );
            eq::validate(&config(preset.preamp_db, preset.filters.clone()))
                .unwrap_or_else(|error| panic!("{} is not a valid band set: {error}", preset.id));
        }
    }

    #[test]
    fn every_shipped_shape_is_nine_bands_on_the_octave_grid() {
        for preset in builtins() {
            let frequencies: Vec<f32> = preset
                .filters
                .iter()
                .map(|filter| filter.frequency)
                .collect();
            assert_eq!(
                frequencies,
                OCTAVES.to_vec(),
                "{} is not drawn on the nine octave centres",
                preset.id
            );
            for filter in &preset.filters {
                assert_eq!(filter.kind, EqKind::Peaking, "{} is not a bank of bands", preset.id);
                assert!(
                    (filter.q - OCTAVE_Q).abs() < 1e-6,
                    "{} carries a band that is not one octave wide",
                    preset.id
                );
                assert!(filter.enabled, "{} ships a band switched out", preset.id);
            }
        }
    }

    #[test]
    fn the_subsonic_shape_falls_away_where_it_should_and_leaves_the_rest_alone() {
        let patch = builtins()
            .into_iter()
            .find(|preset| preset.id == "builtin:butterworth-sub")
            .expect("the subsonic patch is gone");
        let at = |hz: f32| {
            patch
                .filters
                .iter()
                .map(|filter| {
                    eq::Biquad::new(filter.kind, filter.frequency, filter.gain_db, filter.q)
                        .magnitude_db(hz)
                })
                .sum::<f32>()
        };
        // The alignment's shape on the grid: a hard floor at 63 Hz, well down by the
        // bass, and flat where the music lives. Not its two-section filter any more.
        assert!(at(63.0) <= -20.0, "63 Hz reads {:.2} dB", at(63.0));
        assert!(at(250.0) <= -5.0, "250 Hz reads {:.2} dB", at(250.0));
        assert!(at(500.0) > at(250.0), "the roll-off is not monotonic");
        // Above the bass it is flat to within the half-decibel the 1 kHz band leans by.
        assert!(at(2_000.0).abs() < 1.0, "2 kHz reads {:.2} dB", at(2_000.0));
        assert_eq!(patch.preamp_db, 0.0, "a shape that only cuts needs no preamp");
    }

    #[test]
    fn no_shipped_patch_asks_for_headroom_it_does_not_have() {
        for preset in builtins() {
            let curve = eq::response(&config(preset.preamp_db, preset.filters.clone()), 512);
            assert!(
                curve.headroom_db <= 0.05,
                "{} would run {:.2} dB past full scale",
                preset.id,
                curve.headroom_db
            );
            // The preamp is the boost rounded up, so a patch that only cuts carries none.
            if preset.preamp_db != 0.0 {
                assert!(preset.preamp_db < 0.0, "{} has a boosting preamp", preset.id);
                assert!(
                    (preset.preamp_db + curve.max_boost_db).abs() < 0.15,
                    "{} leaves more than 0.1 dB of headroom unused",
                    preset.id
                );
            }
        }
    }

    #[test]
    fn editing_a_builtin_saves_a_copy_and_never_touches_the_original() {
        let shipped = builtins();
        let mut library = Library::default();
        let loaded = library.apply("builtin:bass-shelf").expect("the patch loads");
        assert_eq!(loaded.filters.len(), 9, "the shelf patch is not nine bands");

        // One edit: the band moves. The copy appears and becomes the active patch.
        let edited = config(0.0, vec![band(EqKind::Peaking, 60.0, 6.0, 1.0)]);
        let id = library.after_edit(&edited).expect("an edit names a patch");
        assert!(id.starts_with("custom:"));
        assert_eq!(library.active.as_deref(), Some(id.as_str()));
        assert_eq!(library.saved.len(), 1);
        let copy = &library.saved[0];
        assert_eq!(copy.name, "Bass shelf (edited)");
        assert!(!copy.builtin);
        assert_eq!(copy.filters, edited.filters);
        assert!(copy.note.contains("Bass shelf"));

        // The shipped patch is byte-for-byte what it was.
        assert_eq!(builtins(), shipped);
        let again = library.find("builtin:bass-shelf").unwrap();
        assert_eq!(again.filters, shipped[1].filters);
        assert_eq!(again.name, "Bass shelf");
    }

    #[test]
    fn editing_a_fit_copies_it_without_crediting_autoeq() {
        let mut library = Library::default();
        let loaded = library.apply("fit:moondrop-lan-2-ref").expect("the fit loads");
        assert_eq!(loaded.filters.len(), 10, "the LAN 2 fit is not ten bands");
        let mut edited = loaded.clone();
        edited.filters[0].gain_db += 1.0;
        let id = library.after_edit(&edited).expect("an edit names a patch");
        assert!(id.starts_with("custom:"));
        let copy = &library.saved[0];
        assert_eq!(copy.name, "Moondrop LAN 2 (Ref) (edited)");
        // The copy carries the user's numbers, so it inherits no evidence at all.
        assert!(copy.source.is_none());
        assert!(copy.note.contains("own fit"), "{}", copy.note);
        assert!(!copy.note.contains("AutoEq"), "{}", copy.note);
        // Both kinds of shipped correction are protected the way a shape is.
        assert!(library.rename("fit:moondrop-lan-2-ref", "Mine").is_err());
        assert!(library.remove("fit:moondrop-lan-2-ref").is_err());
    }

    #[test]
    fn a_second_edit_follows_the_copy_instead_of_piling_up_new_ones() {
        let mut library = Library::default();
        library.apply("builtin:treble-trim").unwrap();
        library.after_edit(&config(0.0, vec![peak(4_000.0, -2.0)]));
        let id = library.active.clone().unwrap();
        let second = config(-1.5, vec![peak(5_000.0, -3.5)]);
        library.after_edit(&second);
        assert_eq!(library.saved.len(), 1, "a second edit saved another patch");
        assert_eq!(library.active.as_deref(), Some(id.as_str()));
        assert_eq!(library.saved[0].filters, second.filters);
        assert_eq!(library.saved[0].preamp_db, -1.5);
    }

    #[test]
    fn an_edit_that_changes_nothing_does_not_save_a_copy() {
        let mut library = Library::default();
        let loaded = library.apply("builtin:presence").unwrap();
        library.after_edit(&loaded);
        assert!(library.saved.is_empty(), "an untouched patch was forked");
        assert_eq!(library.active.as_deref(), Some("builtin:presence"));
    }

    #[test]
    fn neutral_unlinks_the_bands_and_saves_no_empty_copy() {
        let mut library = Library::default();
        library.apply("builtin:presence").unwrap();
        // The Neutral button sends exactly the default configuration.
        assert_eq!(library.after_edit(&EqConfig::default()), None);
        assert!(library.saved.is_empty(), "neutral saved a copy");
        assert_eq!(library.active, None, "neutral kept pointing at a patch");

        // Clearing every band by hand is the same thing: no patch, no copy.
        library.apply("builtin:bass-shelf").unwrap();
        let emptied = EqConfig {
            enabled: true,
            preamp_db: 0.0,
            filters: Vec::new(),
        };
        assert_eq!(library.after_edit(&emptied), None);
        assert!(library.saved.is_empty(), "an emptied patch saved an empty copy");
    }

    #[test]
    fn a_working_set_with_no_patch_stays_a_working_set() {
        let mut library = Library::default();
        assert_eq!(library.after_edit(&config(-3.0, vec![peak(1_000.0, 4.0)])), None);
        assert!(library.saved.is_empty());
        assert_eq!(library.active, None);
    }

    #[test]
    fn a_copy_is_renamed_and_the_builtin_refuses() {
        let mut library = Library::default();
        library.apply("builtin:bass-shelf").unwrap();
        let id = library
            .after_edit(&config(0.0, vec![peak(80.0, 5.0)]))
            .unwrap();
        library.rename(&id, "  Ananda, warm  ").unwrap();
        assert_eq!(library.find(&id).unwrap().name, "Ananda, warm");
        assert_eq!(library.find(&id).unwrap().filters.len(), 1, "renaming lost the bands");
        assert!(library.rename("builtin:bass-shelf", "Mine").is_err());
        assert!(library.rename(&id, "   ").is_err());
        assert!(library.rename(&id, &"x".repeat(MAX_NAME + 1)).is_err());
        assert!(library.rename("custom:99", "Mine").is_err());
        // The refused renames left the name it had.
        assert_eq!(library.find(&id).unwrap().name, "Ananda, warm");
    }

    #[test]
    fn a_copy_is_deleted_and_the_builtin_remains() {
        let mut library = Library::default();
        library.apply("builtin:bass-shelf").unwrap();
        let id = library
            .after_edit(&config(0.0, vec![peak(80.0, 5.0)]))
            .unwrap();
        library.remove(&id).unwrap();
        assert!(library.saved.is_empty());
        assert_eq!(library.active, None, "the deleted patch is still active");
        assert!(library.find("builtin:bass-shelf").is_some());
        assert!(library.remove("builtin:bass-shelf").is_err());
        assert!(library.remove(&id).is_err());
    }

    #[test]
    fn applying_a_patch_engages_the_stage_and_links_it() {
        let mut library = Library::default();
        let loaded = library.apply("builtin:rumble-filter").unwrap();
        assert!(loaded.enabled);
        // The grid's lowest band carries the deepest cut: 63 Hz, 12 dB down.
        assert_eq!(loaded.filters[0].frequency, 63.0);
        assert_eq!(loaded.filters[0].gain_db, -12.0);
        assert_eq!(library.active.as_deref(), Some("builtin:rumble-filter"));
        assert!(library.apply("builtin:nope").is_err());
    }

    #[test]
    fn a_second_copy_of_the_same_builtin_gets_its_own_name() {
        let mut library = Library::default();
        library.apply("builtin:presence").unwrap();
        library.after_edit(&config(0.0, vec![peak(2_000.0, 2.0)]));
        library.apply("builtin:presence").unwrap();
        library.after_edit(&config(0.0, vec![peak(3_800.0, -2.0)]));
        let names: Vec<&str> = library.saved.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["Presence (edited)", "Presence (edited) 2"]);
        assert_ne!(library.saved[0].id, library.saved[1].id);
    }

    #[test]
    fn ids_stay_unique_across_reloads() {
        let mut library = Library::default();
        library.apply("builtin:flat").unwrap();
        library.after_edit(&config(0.0, vec![peak(100.0, 3.0)]));
        let reloaded = library.clone().checked();
        let mut next = reloaded;
        next.apply("builtin:flat").unwrap();
        let id = next.after_edit(&config(0.0, vec![peak(200.0, 3.0)])).unwrap();
        assert_ne!(id, next.saved[0].id, "a reloaded library reused an id");
        assert_eq!(next.saved.len(), 2);
    }

    #[test]
    fn a_saved_name_that_is_too_long_is_cut_to_one_row() {
        let mut library = Library::default();
        library.apply("builtin:bass-shelf").unwrap();
        // A long built-in name is not possible today; the cut is what a future one gets.
        assert!(library.copy_name("x".repeat(MAX_NAME).as_str()).chars().count() <= MAX_NAME);
    }

    #[test]
    fn a_stored_link_that_the_bands_do_not_match_is_dropped() {
        let mut library = Library::default();
        let loaded = library.apply("builtin:bass-shelf").unwrap();
        // The bands really are the shipped shape: the link stands.
        library.reconcile(&loaded);
        assert_eq!(library.active.as_deref(), Some("builtin:bass-shelf"));

        // The same id pointing at bands the shape no longer has — an older version's
        // two-band shelf, say — is not that patch, so the running bands are unlinked.
        let older = config(0.0, vec![band(EqKind::Lowshelf, 80.0, 4.0, 0.7)]);
        library.reconcile(&older);
        assert_eq!(library.active, None, "the picker still claims a patch it is not running");
        assert!(library.saved.is_empty(), "reconciling saved a copy");
        // And a neutral chain is a working set, not a patch.
        library.apply("builtin:presence").unwrap();
        library.reconcile(&EqConfig::default());
        assert_eq!(library.active, None);
    }

    #[test]
    fn a_corrupt_library_drops_only_the_patch_it_cannot_read() {
        let good = Preset {
            id: "custom:1".into(),
            name: "Good".into(),
            builtin: false,
            note: String::new(),
            preamp_db: 0.0,
            filters: vec![peak(1_000.0, 3.0)],
            source: None,
        };
        let mut broken = good.clone();
        broken.id = "custom:2".into();
        broken.filters[0].frequency = 0.0;
        let mut usurper = good.clone();
        usurper.id = "builtin:flat".into();
        let library = Library {
            saved: vec![good.clone(), broken, usurper],
            active: Some("custom:2".into()),
        }
        .checked();
        assert_eq!(library.saved, vec![good]);
        assert_eq!(library.active, None, "an unlinked active patch was kept");
    }
}
