//! Spatial rendering of the stereo mix playback renders.
//!
//! Playback feeds the chain a two-channel 48 kHz mix, so nothing here decodes
//! object or discrete surround audio. Each mode builds a virtual speaker
//! layout, derives its channels from the stereo mix by matrix, and renders that
//! layout back to two channels using the cues headphones can actually reproduce:
//! per-speaker level, interaural time delay and head shadowing. There is no
//! personalised HRTF, and rear or height speakers are conveyed by level and
//! spectral tilt rather than by true front/back localisation.
//!
//! Atmos is the exception: it hands the same 7.1.4 layout to Windows Spatial
//! Sound as twelve mono object feeds (`Bed`), so Windows renders it with the
//! spatial format the user selected — Dolby Atmos for Headphones or Home
//! Theater when that is the chosen format. The feeds are still derived from the
//! stereo mix; no Atmos bitstream is decoded here.

use serde::Serialize;

const ITD_SAMPLES: usize = 32;
const RING: usize = ITD_SAMPLES + 1;
const MAX_SPEAKERS: usize = 12;
/// 12 ms of ramping when the mode or its parameters change, so switching never clicks.
const FADE_STEP: f32 = 1.0 / (0.012 * 48_000.0);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpatialMode {
    Stereo,
    Surround51,
    Surround71,
    Atmos,
    /// The app's own derived 7.1.4 render, used when Windows spatial sound is
    /// off or cannot be opened. It is not a selectable mode: the interface's
    /// Atmos position falls back to it for the track.
    DerivedHeights,
    Binaural,
    Custom,
}

impl SpatialMode {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "stereo" => Self::Stereo,
            "surround51" => Self::Surround51,
            "surround71" => Self::Surround71,
            // "heights" is the retired id of the app's derived 7.1.4 mode; an
            // older window or a saved setting must land on the Windows-rendered
            // Atmos bed instead of failing with an unknown mode.
            "atmos" | "heights" => Self::Atmos,
            "binaural" => Self::Binaural,
            "custom" => Self::Custom,
            _ => return None,
        })
    }

}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpatialConfig {
    pub mode: SpatialMode,
    /// Custom only: how far the pair is spread, 0 = narrow, 1 = wide.
    pub width: f32,
    /// Binaural and Custom: how much each ear hears the opposite channel.
    pub crossfeed: f32,
    /// Custom only: how much of the mix is lifted overhead.
    pub height: f32,
    /// Custom only: rotates the whole image, negative turns left.
    pub rotation: f32,
}

impl Default for SpatialConfig {
    fn default() -> Self {
        Self {
            mode: SpatialMode::Stereo,
            width: 0.65,
            crossfeed: 0.35,
            height: 0.4,
            rotation: 0.0,
        }
    }
}

pub fn validate(config: &SpatialConfig) -> Result<(), String> {
    let finite_in = |value: f32, low: f32, high: f32| value.is_finite() && (low..=high).contains(&value);
    if !finite_in(config.width, 0.0, 1.0) {
        return Err("Spatial width must be between 0 and 1".into());
    }
    if !finite_in(config.crossfeed, 0.0, 1.0) {
        return Err("Crossfeed must be between 0 and 1".into());
    }
    if !finite_in(config.height, 0.0, 1.0) {
        return Err("Spatial height must be between 0 and 1".into());
    }
    if !finite_in(config.rotation, -60.0, 60.0) {
        return Err("Spatial rotation must be between -60 and 60 degrees".into());
    }
    Ok(())
}

/// Where a virtual speaker's signal comes from in the stereo mix.
#[derive(Clone, Copy)]
enum Source {
    Left = 0,
    Right = 1,
    Mid = 2,
    Side = 3,
    Low = 4,
    High = 5,
}

impl Source {
    fn name(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Mid => "Mid",
            Self::Side => "Side",
            Self::Low => "Low",
            Self::High => "High",
        }
    }
}

/// The layout as the UI draws it: the same speakers the renderer is using.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerView {
    pub azimuth: f32,
    pub elevation: f32,
    pub gain: f32,
    pub source: &'static str,
}

pub fn describe(config: SpatialConfig) -> Vec<SpeakerView> {
    layout(config)
        .into_iter()
        .map(|item| SpeakerView {
            azimuth: item.azimuth,
            elevation: item.elevation,
            gain: item.gain,
            source: item.source.name(),
        })
        .collect()
}

#[derive(Clone, Copy)]
struct Speaker {
    /// Degrees, negative is left of centre, +/-180 is behind.
    azimuth: f32,
    /// Degrees above the listener.
    elevation: f32,
    gain: f32,
    source: Source,
}

const fn speaker(azimuth: f32, elevation: f32, gain: f32, source: Source) -> Speaker {
    Speaker { azimuth, elevation, gain, source }
}

fn layout(config: SpatialConfig) -> Vec<Speaker> {
    let mut list = match config.mode {
        SpatialMode::Stereo => Vec::new(),
        SpatialMode::Binaural => vec![
            speaker(-30.0, 0.0, 1.0, Source::Left),
            speaker(30.0, 0.0, 1.0, Source::Right),
        ],
        SpatialMode::Surround51 => vec![
            speaker(-30.0, 0.0, 1.0, Source::Left),
            speaker(30.0, 0.0, 1.0, Source::Right),
            speaker(0.0, 0.0, 0.8, Source::Mid),
            speaker(0.0, -20.0, 0.7, Source::Low),
            speaker(-110.0, 0.0, 0.75, Source::Side),
            speaker(110.0, 0.0, -0.75, Source::Side),
        ],
        SpatialMode::Surround71 => vec![
            speaker(-30.0, 0.0, 1.0, Source::Left),
            speaker(30.0, 0.0, 1.0, Source::Right),
            speaker(0.0, 0.0, 0.8, Source::Mid),
            speaker(0.0, -20.0, 0.7, Source::Low),
            speaker(-90.0, 0.0, 0.62, Source::Side),
            speaker(90.0, 0.0, -0.62, Source::Side),
            speaker(-150.0, 0.0, 0.55, Source::Side),
            speaker(150.0, 0.0, -0.55, Source::Side),
        ],
        SpatialMode::Atmos | SpatialMode::DerivedHeights => vec![
            speaker(-30.0, 0.0, 1.0, Source::Left),
            speaker(30.0, 0.0, 1.0, Source::Right),
            speaker(0.0, 0.0, 0.8, Source::Mid),
            speaker(0.0, -20.0, 0.7, Source::Low),
            speaker(-90.0, 0.0, 0.62, Source::Side),
            speaker(90.0, 0.0, -0.62, Source::Side),
            speaker(-150.0, 0.0, 0.55, Source::Side),
            speaker(150.0, 0.0, -0.55, Source::Side),
            speaker(-45.0, 45.0, 0.5, Source::High),
            speaker(45.0, 45.0, 0.5, Source::High),
            speaker(-135.0, 45.0, 0.4, Source::High),
            speaker(135.0, 45.0, 0.4, Source::High),
        ],
        SpatialMode::Custom => {
            let azimuth = 15.0 + 55.0 * config.width;
            let mut custom = vec![
                speaker(-azimuth, 0.0, 1.0, Source::Left),
                speaker(azimuth, 0.0, 1.0, Source::Right),
            ];
            if config.height > 0.01 {
                let elevation = 25.0 + 30.0 * config.height;
                let gain = 0.7 * config.height;
                custom.push(speaker(-azimuth * 0.6, elevation, gain, Source::High));
                custom.push(speaker(azimuth * 0.6, elevation, gain, Source::High));
            }
            custom
        }
    };
    // Constant-power normalisation keeps the monitor at a similar loudness across layouts.
    let total: f32 = list.iter().map(|item| item.gain * item.gain).sum();
    let norm = if total > 0.0 { 1.0 / total.sqrt() } else { 1.0 };
    for item in &mut list {
        item.azimuth += config.rotation;
        item.gain *= norm;
    }
    list
}

struct Tap {
    ring: [f32; RING],
    position: usize,
    shadow: f32,
}

impl Tap {
    fn new() -> Self {
        Self { ring: [0.0; RING], position: 0, shadow: 0.0 }
    }

    fn write(&mut self, value: f32) {
        self.position = (self.position + 1) % RING;
        self.ring[self.position] = value;
    }

    fn read(&self, delay: usize) -> f32 {
        self.ring[(self.position + RING - delay.min(ITD_SAMPLES)) % RING]
    }

    fn shadowed(&mut self, value: f32, coefficient: f32) -> f32 {
        self.shadow += coefficient * (value - self.shadow);
        self.shadow
    }
}

#[derive(Default)]
struct Filters {
    low: f32,
    high: f32,
    crossfeed_left: f32,
    crossfeed_right: f32,
}

/// The twelve channels the Atmos mode hands to Windows as a 7.1.4 static bed.
/// The order is the renderer's own layout order, and `spatial_out::BED_TYPES`
/// maps each slot to its Windows static object type.
pub const BED_CHANNELS: usize = 12;

/// Derives the Atmos bed from the processed stereo mix, one mono feed per
/// static channel. The filters keep their state across frames, so the low and
/// high feeds are continuous instead of restarting on every buffer.
pub struct Bed {
    config: SpatialConfig,
    layout: Vec<Speaker>,
    low: f32,
    high: f32,
}

impl Bed {
    pub fn new(config: SpatialConfig) -> Self {
        Self {
            config,
            layout: layout(config),
            low: 0.0,
            high: 0.0,
        }
    }

    /// A layout change rebuilds the bed only when it really changed.
    pub fn set_config(&mut self, config: SpatialConfig) {
        if config != self.config {
            self.config = config;
            self.layout = layout(config);
        }
    }

    /// Twelve feeds in layout order, from one stereo frame.
    pub fn feeds(&mut self, frame: [f32; 2]) -> [f32; BED_CHANNELS] {
        let [left, right] = frame;
        let mid = (left + right) * 0.5;
        let side = (left - right) * 0.5;
        self.low += 0.016 * (mid - self.low);
        self.high += 0.22 * (mid - self.high);
        let sources = [left, right, mid, side, self.low, mid - self.high];
        let mut out = [0.0f32; BED_CHANNELS];
        for (slot, item) in self.layout.iter().take(BED_CHANNELS).enumerate() {
            out[slot] = sources[item.source as usize] * item.gain;
        }
        out
    }
}

pub struct SpatialRenderer {
    config: SpatialConfig,
    pending: Option<SpatialConfig>,
    layout: Vec<Speaker>,
    taps: Vec<Tap>,
    filters: Filters,
    fade: f32,
}

impl SpatialRenderer {
    pub fn new(config: SpatialConfig) -> Self {
        Self {
            config,
            pending: None,
            layout: layout(config),
            taps: (0..MAX_SPEAKERS).map(|_| Tap::new()).collect(),
            filters: Filters::default(),
            fade: 1.0,
        }
    }

    fn adopt(&mut self, config: SpatialConfig) {
        self.config = config;
        self.layout = layout(config);
    }

    /// One stereo frame in, one spatialised stereo frame out.
    pub fn process(&mut self, frame: [f32; 2], config: SpatialConfig) -> [f32; 2] {
        if config != self.config && self.pending.is_none() {
            self.pending = Some(config);
        }
        if let Some(next) = self.pending {
            self.fade = (self.fade - FADE_STEP).max(0.0);
            if self.fade == 0.0 {
                self.adopt(next);
                self.pending = None;
            }
        } else if self.fade < 1.0 {
            self.fade = (self.fade + FADE_STEP).min(1.0);
        }
        let rendered = match self.config.mode {
            // Atmos does not render here: the bed is handed to Windows after the
            // rest of the chain, so the spatial stage stays a pass-through.
            SpatialMode::Stereo | SpatialMode::Atmos => frame,
            _ => self.render(frame),
        };
        [
            (rendered[0] * self.fade).clamp(-1.0, 1.0),
            (rendered[1] * self.fade).clamp(-1.0, 1.0),
        ]
    }

    fn render(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let [left, right] = frame;
        let mid = (left + right) * 0.5;
        let side = (left - right) * 0.5;
        self.filters.low += 0.016 * (mid - self.filters.low);
        self.filters.high += 0.22 * (mid - self.filters.high);
        let sources = [left, right, mid, side, self.filters.low, mid - self.filters.high];

        let mut out = [0.0f32; 2];
        for (index, item) in self.layout.iter().enumerate() {
            let tap = &mut self.taps[index];
            let signal = sources[item.source as usize] * item.gain;
            tap.write(signal);
            let lateral = item.azimuth.to_radians().sin().clamp(-1.0, 1.0);
            let delay = (lateral.abs() * ITD_SAMPLES as f32).round() as usize;
            let coefficient = (0.92 - 0.55 * lateral.abs()) * (1.0 - 0.006 * item.elevation);
            let far_dry = tap.read(delay);
            let far = tap.shadowed(far_dry, coefficient);
            let left_gain = ((1.0 - lateral) * 0.5).sqrt();
            let right_gain = ((1.0 + lateral) * 0.5).sqrt();
            let elevation = 1.0 - 0.0025 * item.elevation;
            if lateral <= 0.0 {
                out[0] += signal * left_gain * elevation;
                out[1] += far * right_gain * elevation;
            } else {
                out[0] += far * left_gain * elevation;
                out[1] += signal * right_gain * elevation;
            }
        }

        let crossfeed = match self.config.mode {
            SpatialMode::Binaural | SpatialMode::Custom => self.config.crossfeed * 0.5,
            _ => 0.0,
        };
        if crossfeed > 0.0 {
            self.filters.crossfeed_left += 0.12 * (out[0] - self.filters.crossfeed_left);
            self.filters.crossfeed_right += 0.12 * (out[1] - self.filters.crossfeed_right);
            out[0] += self.filters.crossfeed_right * crossfeed;
            out[1] += self.filters.crossfeed_left * crossfeed;
        }
        out
    }

    #[cfg(test)]
    fn fade(&self) -> f32 {
        self.fade
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(mode: SpatialMode) -> SpatialConfig {
        SpatialConfig { mode, ..SpatialConfig::default() }
    }

    fn run(mode: SpatialMode, input: impl Fn(usize) -> [f32; 2], frames: usize) -> Vec<[f32; 2]> {
        let mut renderer = SpatialRenderer::new(config(mode));
        (0..frames).map(|index| renderer.process(input(index), config(mode))).collect()
    }

    #[test]
    fn every_mode_parses_and_labels_itself() {
        for (id, mode) in [
            ("stereo", SpatialMode::Stereo),
            ("surround51", SpatialMode::Surround51),
            ("surround71", SpatialMode::Surround71),
            ("atmos", SpatialMode::Atmos),
            ("binaural", SpatialMode::Binaural),
            ("custom", SpatialMode::Custom),
        ] {
            assert_eq!(SpatialMode::parse(id), Some(mode));
        }
        assert_eq!(SpatialMode::parse("dolby-atmos-decode"), None);
        // The retired id maps to the Atmos bed rather than being rejected.
        assert_eq!(SpatialMode::parse("heights"), Some(SpatialMode::Atmos));
    }

    #[test]
    fn stereo_mode_is_exactly_transparent() {
        let input = [(0.5f32, -0.25f32), (0.1, 0.9), (-0.4, 0.0)];
        let output = run(SpatialMode::Stereo, |index| [input[index].0, input[index].1], input.len());
        for (index, frame) in output.iter().enumerate() {
            assert_eq!(*frame, [input[index].0, input[index].1]);
        }
    }

    #[test]
    fn layouts_have_the_speaker_counts_their_modes_name() {
        assert_eq!(layout(config(SpatialMode::Stereo)).len(), 0);
        assert_eq!(layout(config(SpatialMode::Binaural)).len(), 2);
        assert_eq!(layout(config(SpatialMode::Surround51)).len(), 6);
        assert_eq!(layout(config(SpatialMode::Surround71)).len(), 8);
        assert_eq!(layout(config(SpatialMode::Atmos)).len(), 12);
    }

    #[test]
    fn binaural_shadows_and_delays_the_far_ear() {
        // Crossfeed is disabled here so the interaural delay is measured on its own.
        let settings = SpatialConfig { mode: SpatialMode::Binaural, crossfeed: 0.0, ..SpatialConfig::default() };
        let mut renderer = SpatialRenderer::new(settings);
        for _ in 0..64 {
            let _ = renderer.process([0.0, 0.0], settings);
        }
        let output: Vec<[f32; 2]> = (0..96)
            .map(|index| {
                let frame = if index == 0 { [1.0f32, 0.0f32] } else { [0.0, 0.0] };
                renderer.process(frame, settings)
            })
            .collect();
        let left_energy: f32 = output.iter().map(|frame| frame[0].abs()).sum();
        let right_energy: f32 = output.iter().map(|frame| frame[1].abs()).sum();
        assert!(left_energy > right_energy * 1.4, "left {left_energy} right {right_energy}");
        let first_left = output.iter().position(|frame| frame[0].abs() > 1e-6).unwrap();
        let first_right = output.iter().position(|frame| frame[1].abs() > 1e-6).unwrap();
        assert!(first_right >= first_left + 8, "left {first_left} right {first_right}");
    }

    #[test]
    fn a_centred_mono_mix_stays_centred_in_every_surround_layout() {
        for mode in [SpatialMode::Surround51, SpatialMode::Surround71] {
            let mut renderer = SpatialRenderer::new(config(mode));
            let mut left = 0.0f32;
            let mut right = 0.0f32;
            for index in 0..2048 {
                let sample = (index as f32 * 0.05).sin() * 0.5;
                let frame = renderer.process([sample, sample], config(mode));
                left += frame[0].abs();
                right += frame[1].abs();
            }
            let difference = (left - right).abs() / left.max(1e-6);
            assert!(difference < 0.02, "{mode:?} drifted left {left} right {right}");
        }
    }

    #[test]
    fn spatial_output_never_leaves_full_scale() {
        for mode in [
            SpatialMode::Binaural,
            SpatialMode::Surround51,
            SpatialMode::Surround71,
            SpatialMode::Atmos,
            SpatialMode::Custom,
        ] {
            let output = run(mode, |index| if index % 2 == 0 { [1.0, 1.0] } else { [-1.0, -1.0] }, 512);
            assert!(output.iter().flatten().all(|value| value.abs() <= 1.0), "{mode:?} clipped range");
        }
    }

    #[test]
    fn custom_parameters_move_the_image() {
        let narrow = layout(SpatialConfig { mode: SpatialMode::Custom, width: 0.0, ..SpatialConfig::default() });
        let wide = layout(SpatialConfig { mode: SpatialMode::Custom, width: 1.0, ..SpatialConfig::default() });
        assert!(wide[0].azimuth.abs() > narrow[0].azimuth.abs());
        let base = SpatialConfig { mode: SpatialMode::Custom, width: 0.8, ..SpatialConfig::default() };
        let straight = layout(base);
        let rotated = layout(SpatialConfig { rotation: 30.0, ..base });
        assert!((rotated[1].azimuth - (straight[1].azimuth + 30.0)).abs() < 1e-4);
        let quiet = layout(SpatialConfig { mode: SpatialMode::Custom, height: 0.0, ..SpatialConfig::default() });
        let tall = layout(SpatialConfig { mode: SpatialMode::Custom, height: 1.0, ..SpatialConfig::default() });
        assert_eq!(quiet.len(), 2);
        assert_eq!(tall.len(), 4);
        assert!(tall.iter().any(|item| item.elevation > 40.0));
    }

    #[test]
    fn the_reported_layout_matches_the_renderer() {
        let five = describe(config(SpatialMode::Surround51));
        assert_eq!(five.len(), 6);
        assert_eq!(five[0].source, "Left");
        assert_eq!(five[1].source, "Right");
        assert!(five.iter().any(|item| item.source == "Low"));
        assert!(describe(config(SpatialMode::Stereo)).is_empty());
        let atmos = describe(config(SpatialMode::Atmos));
        assert_eq!(atmos.iter().filter(|item| item.elevation > 1.0).count(), 4);
        assert!(atmos.iter().all(|item| item.gain.abs() > 0.0));
    }

    #[test]
    fn atmos_is_a_pass_through_at_the_spatial_stage() {
        // Windows renders the bed after the rest of the chain, so the app's own
        // spatial stage must not binauralise the mix a second time.
        let input = [[0.4f32, -0.2f32], [0.1, 0.7]];
        let output = run(SpatialMode::Atmos, |index| input[index], input.len());
        assert_eq!(output, input.to_vec());
    }

    #[test]
    fn the_atmos_layout_is_the_static_bed_order() {
        let speakers = layout(config(SpatialMode::Atmos));
        let expected = [
            (-30.0, 0.0),
            (30.0, 0.0),
            (0.0, 0.0),
            (0.0, -20.0),
            (-90.0, 0.0),
            (90.0, 0.0),
            (-150.0, 0.0),
            (150.0, 0.0),
            (-45.0, 45.0),
            (45.0, 45.0),
            (-135.0, 45.0),
            (135.0, 45.0),
        ];
        assert_eq!(speakers.len(), BED_CHANNELS);
        for (speaker, (azimuth, elevation)) in speakers.iter().zip(expected) {
            assert!((speaker.azimuth - azimuth).abs() < 1e-4, "azimuth {}", speaker.azimuth);
            assert!((speaker.elevation - elevation).abs() < 1e-4, "elevation {}", speaker.elevation);
        }
    }

    #[test]
    fn the_atmos_bed_derives_one_feed_per_static_channel() {
        let settings = config(SpatialMode::Atmos);
        let mut bed = Bed::new(settings);
        let feeds = bed.feeds([1.0, 0.0]);
        assert_eq!(feeds.len(), BED_CHANNELS);
        // A hard-left frame puts energy in the left front, side and top feeds
        // and none in the right front; the low band still reaches the LFE.
        assert!(feeds[0] > 0.0);
        assert_eq!(feeds[1], 0.0);
        assert!(feeds[4] > 0.0);
        assert!(feeds[8] > 0.0);
        assert!(feeds[3] > 0.0);
        assert!(feeds.iter().all(|feed| feed.is_finite()));
        // A centred frame is symmetric across the pair.
        let mut bed = Bed::new(settings);
        let centred = bed.feeds([0.5, 0.5]);
        assert!((centred[0] - centred[1]).abs() < 1e-6);
        assert!(centred[3] > 0.0);
    }

    #[test]
    fn invalid_parameters_are_rejected() {
        let base = SpatialConfig::default();
        assert!(validate(&base).is_ok());
        for bad in [
            SpatialConfig { width: 1.5, ..base },
            SpatialConfig { width: f32::NAN, ..base },
            SpatialConfig { crossfeed: -0.1, ..base },
            SpatialConfig { height: f32::INFINITY, ..base },
            SpatialConfig { rotation: 61.0, ..base },
        ] {
            assert!(validate(&bad).is_err());
        }
    }

    #[test]
    fn switching_modes_ramps_instead_of_jumping() {
        let stereo = config(SpatialMode::Stereo);
        let surround = config(SpatialMode::Surround71);
        let mut renderer = SpatialRenderer::new(stereo);
        for _ in 0..32 {
            let _ = renderer.process([0.5, 0.5], stereo);
        }
        assert_eq!(renderer.fade(), 1.0);
        let _ = renderer.process([0.5, 0.5], surround);
        assert!(renderer.fade() < 1.0, "fade should leave unity during a mode change");
        for _ in 0..4096 {
            let _ = renderer.process([0.5, 0.5], surround);
        }
        assert_eq!(renderer.fade(), 1.0);
        // It really adopted the new layout, not just the new fade: the frame matches
        // a renderer that was built for the surround layout from the start.
        let mut fresh = SpatialRenderer::new(surround);
        for _ in 0..4096 {
            let _ = fresh.process([0.5, 0.5], surround);
        }
        assert_eq!(renderer.process([0.4, 0.1], surround), fresh.process([0.4, 0.1], surround));
    }
}
