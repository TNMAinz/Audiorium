//! Binaural panning for headphone listening.
//!
//! One stereo image is placed anywhere around the listener: azimuth, elevation,
//! width and, optionally, four early reflections from a virtual room. The cues
//! are the ones a spherical head model can actually produce — interaural time
//! difference up to 0.7 ms, a one-pole head shadow on the far ear, a small
//! elevation tilt, and tapped early reflections with per-material damping.
//!
//! This is not a licensed or measured HRTF, it does not personalise anything,
//! and it does not add a reverb tail: rear and height placement is conveyed by
//! level and spectral tilt, and the room is four early taps, not a reverberator.
//! `focus` trades externalisation against tonal fidelity by scaling those cues.

use serde::{Deserialize, Serialize};

const RATE: f32 = 48_000.0;
/// 0.7 ms at 48 kHz, about the largest interaural delay a human head produces.
const MAX_ITD: usize = 34;
const ITD_RING: usize = MAX_ITD + 1;
const REFLECTIONS: usize = 4;
/// 25 ms of history covers the longest early reflection.
const ROOM_RING: usize = 1_200;
/// Everything that moves uses a 12 ms ramp, so dragging a control never clicks.
const RAMP: f32 = 1.0 / (0.012 * RATE);
/// Early reflection arrivals, in milliseconds, before the length control scales them.
const REFLECTION_MS: [f32; REFLECTIONS] = [7.0, 11.5, 17.0, 23.0];
/// How dark each reflection material is: a one-pole coefficient per timbre. A higher
/// coefficient keeps more of the high band.
const TIMBRE_DAMPING: [f32; 5] = [0.55, 0.28, 0.85, 0.14, 0.50];
/// How much of each tap the material gives back: a hard surface returns more of it than a
/// soft one. This is the half of the character that carries, because the four taps sit well
/// under the direct sound — a filter change alone measures about half a decibel in the mix
/// at the room level the panner ships with.
const TIMBRE_ABSORPTION: [f32; 5] = [0.90, 0.55, 1.85, 0.95, 1.45];

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PannerConfig {
    pub enabled: bool,
    /// Degrees, -180.0 ..= 180.0. Zero is straight ahead.
    pub azimuth: f32,
    /// Degrees, -90.0 ..= 90.0. Zero is ear level.
    pub elevation: f32,
    /// Stereo width of the source before it is placed, 0.0 ..= 1.0.
    pub width: f32,
    pub room: bool,
    /// Scales the early reflection delays, 0.0 ..= 1.0.
    pub reflection_length: f32,
    /// Level of the early reflections, 0.0 ..= 1.0.
    pub reflection_level: f32,
    /// Index into [`timbres`], 0 ..= 4.
    pub timbre: usize,
    /// 0 keeps the source's tone, 1 maximises the spatial cue.
    pub focus: f32,
}

impl Default for PannerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            azimuth: 0.0,
            elevation: 0.0,
            width: 0.25,
            room: false,
            reflection_length: 0.35,
            reflection_level: 0.2,
            timbre: 0,
            focus: 0.5,
        }
    }
}

pub fn validate(config: &PannerConfig) -> Result<(), String> {
    let finite_in =
        |value: f32, low: f32, high: f32| value.is_finite() && (low..=high).contains(&value);
    if !finite_in(config.azimuth, -180.0, 180.0) {
        return Err("Azimuth must be between -180 and 180 degrees".into());
    }
    if !finite_in(config.elevation, -90.0, 90.0) {
        return Err("Elevation must be between -90 and 90 degrees".into());
    }
    if !finite_in(config.width, 0.0, 1.0) {
        return Err("Panner width must be between 0 and 1".into());
    }
    if !finite_in(config.focus, 0.0, 1.0) {
        return Err("Panner focus must be between 0 and 1".into());
    }
    if !finite_in(config.reflection_length, 0.0, 1.0) {
        return Err("Reflection length must be between 0 and 1".into());
    }
    if !finite_in(config.reflection_level, 0.0, 1.0) {
        return Err("Reflection level must be between 0 and 1".into());
    }
    if config.timbre >= TIMBRE_DAMPING.len() {
        return Err(format!(
            "Reflection material must be 0 to {}",
            TIMBRE_DAMPING.len() - 1
        ));
    }
    Ok(())
}

/// The five reflection materials, in index order, for the interface to name.
pub fn timbres() -> [&'static str; 5] {
    ["Studio", "Wood", "Glass", "Stone", "Hall"]
}

/// The reflection level is tapered, not linear. The taps sit under the direct sound, so a
/// linear knob is inaudible through the bottom half of its travel — which made the material
/// switch silent at the 20% the panner ships with. At 20% the taper arrives at about half of
/// the tap gain (a material choice is audible there) and at 100% it changes nothing.
fn tap_level(level: f32) -> f32 {
    level.clamp(0.0, 1.0).powf(0.45)
}

fn ramp(current: &mut f32, target: f32, step: f32) {
    let delta = target - *current;
    if delta.abs() <= step {
        *current = target;
    } else {
        *current += step * delta.signum();
    }
}

pub struct Panner {
    config: PannerConfig,
    enabled: bool,
    /// Ramped values, so parameter moves are smooth even mid-frame.
    lateral: f32,
    elevation: f32,
    width_scale: f32,
    focus: f32,
    reflections: f32,
    itd: [f32; ITD_RING],
    itd_pos: usize,
    room: [f32; ROOM_RING],
    room_pos: usize,
    low_left: f32,
    low_right: f32,
    high_left: f32,
    high_right: f32,
    damp: [f32; REFLECTIONS],
}

impl Panner {
    pub fn new(config: PannerConfig) -> Self {
        let mut panner = Self {
            config,
            enabled: config.enabled,
            lateral: 0.0,
            elevation: 0.0,
            width_scale: 1.0,
            focus: 0.5,
            reflections: 0.0,
            itd: [0.0; ITD_RING],
            itd_pos: 0,
            room: [0.0; ROOM_RING],
            room_pos: 0,
            low_left: 0.0,
            low_right: 0.0,
            high_left: 0.0,
            high_right: 0.0,
            damp: [0.0; REFLECTIONS],
        };
        panner.snap(config);
        panner
    }

    /// Jumps the ramped values to their targets. Used on enable, never mid-move.
    fn snap(&mut self, config: PannerConfig) {
        self.lateral = config.azimuth.to_radians().sin();
        self.elevation = (config.elevation / 90.0).clamp(-1.0, 1.0);
        self.width_scale = config.width * 2.0;
        self.focus = config.focus.clamp(0.0, 1.0);
        self.reflections = if config.room {
            tap_level(config.reflection_level)
        } else {
            0.0
        };
    }

    pub fn reset(&mut self) {
        self.itd = [0.0; ITD_RING];
        self.itd_pos = 0;
        self.room = [0.0; ROOM_RING];
        self.room_pos = 0;
        self.low_left = 0.0;
        self.low_right = 0.0;
        self.high_left = 0.0;
        self.high_right = 0.0;
        self.damp = [0.0; REFLECTIONS];
        self.snap(self.config);
    }

    /// One stereo frame in, one binaural stereo frame out.
    pub fn process(&mut self, frame: [f32; 2], config: PannerConfig) -> [f32; 2] {
        if !config.enabled {
            if self.enabled {
                self.enabled = false;
                self.config = config;
                self.reset();
            }
            return frame;
        }
        self.config = config;
        if !self.enabled {
            self.enabled = true;
            self.reset();
        }

        let targets = (
            config.azimuth.to_radians().sin(),
            (config.elevation / 90.0).clamp(-1.0, 1.0),
            config.width * 2.0,
            config.focus.clamp(0.0, 1.0),
            if config.room {
                tap_level(config.reflection_level)
            } else {
                0.0
            },
        );
        ramp(&mut self.lateral, targets.0, RAMP);
        ramp(&mut self.elevation, targets.1, RAMP);
        ramp(&mut self.width_scale, targets.2, RAMP * 2.0);
        ramp(&mut self.focus, targets.3, RAMP);
        ramp(&mut self.reflections, targets.4, RAMP);

        // Width first: scale the side component, and keep it so the placed image
        // still carries the source's own width. The panner itself works on the mid.
        let mid = (frame[0] + frame[1]) * 0.5;
        let side = (frame[0] - frame[1]) * 0.5 * self.width_scale;
        let side_term = side * 0.6;
        let mono = mid;

        self.itd_pos = (self.itd_pos + 1) % ITD_RING;
        self.itd[self.itd_pos] = mono;
        self.room_pos = (self.room_pos + 1) % ROOM_RING;
        self.room[self.room_pos] = mono;

        let lateral = self.lateral.clamp(-1.0, 1.0);
        let cue = 0.35 + 0.65 * self.focus;
        let delay = (MAX_ITD as f32 * lateral.abs() * (1.0 - 0.25 * self.elevation.abs()) * cue)
            .round()
            .clamp(0.0, MAX_ITD as f32) as usize;
        let delay_left = if lateral > 0.0 { delay } else { 0 };
        let delay_right = if lateral < 0.0 { delay } else { 0 };

        let gain_left = ((1.0 - lateral) * 0.5).sqrt();
        let gain_right = ((1.0 + lateral) * 0.5).sqrt();
        let near_left = (1.0 - lateral) * 0.5;
        let near_right = (1.0 + lateral) * 0.5;

        let tap_left =
            self.itd[(self.itd_pos + ITD_RING - delay_left) % ITD_RING] * gain_left;
        let tap_right =
            self.itd[(self.itd_pos + ITD_RING - delay_right) % ITD_RING] * gain_right;

        // Head shadow: the far ear loses high frequency as the source moves aside.
        let depth = 0.55 * cue;
        let coefficient_left = (0.98 - depth * lateral.max(0.0)).clamp(0.02, 0.99);
        let coefficient_right = (0.98 - depth * (-lateral).max(0.0)).clamp(0.02, 0.99);
        self.low_left += coefficient_left * (tap_left - self.low_left);
        self.low_right += coefficient_right * (tap_right - self.low_right);

        // Elevation tilt: a small high-band change, strongest on the near ear.
        self.high_left += 0.09 * (self.low_left - self.high_left);
        self.high_right += 0.09 * (self.low_right - self.high_right);
        let tilt_left = self.elevation * (0.15 + 0.4 * near_left);
        let tilt_right = self.elevation * (0.15 + 0.4 * near_right);
        let mut out_left = self.low_left + (self.low_left - self.high_left) * tilt_left;
        let mut out_right = self.low_right + (self.low_right - self.high_right) * tilt_right;

        if self.reflections > 0.001 {
            let scale = 0.4 + 0.6 * config.reflection_length.clamp(0.0, 1.0);
            let material = config.timbre.min(TIMBRE_DAMPING.len() - 1);
            let damping = TIMBRE_DAMPING[material];
            let absorption = TIMBRE_ABSORPTION[material];
            for index in 0..REFLECTIONS {
                let samples = (REFLECTION_MS[index] * scale * 0.001 * RATE).round() as usize;
                let delay = samples.clamp(1, ROOM_RING - 1);
                let tap = self.room[(self.room_pos + ROOM_RING - delay) % ROOM_RING];
                self.damp[index] += damping * (tap - self.damp[index]);
                let gain = self.reflections * absorption * (0.9 - 0.12 * index as f32) * 0.6;
                if index % 2 == 0 {
                    out_left += self.damp[index] * gain;
                } else {
                    out_right += self.damp[index] * gain;
                }
            }
        }

        out_left += side_term;
        out_right -= side_term;

        [
            if out_left.is_finite() {
                out_left.clamp(-1.0, 1.0)
            } else {
                0.0
            },
            if out_right.is_finite() {
                out_right.clamp(-1.0, 1.0)
            } else {
                0.0
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> PannerConfig {
        PannerConfig {
            enabled: true,
            ..PannerConfig::default()
        }
    }

    fn tone(hz: f32, frames: usize) -> Vec<[f32; 2]> {
        (0..frames)
            .map(|index| {
                let value = (2.0 * std::f32::consts::PI * hz * index as f32 / RATE).sin() * 0.5;
                [value, value]
            })
            .collect()
    }

    fn run(start: PannerConfig, input: &[[f32; 2]]) -> Vec<[f32; 2]> {
        let mut panner = Panner::new(start);
        input.iter().map(|frame| panner.process(*frame, start)).collect()
    }

    fn energy(frames: &[[f32; 2]], channel: usize) -> f64 {
        frames
            .iter()
            .map(|frame| (frame[channel] as f64).powi(2))
            .sum()
    }

    /// A one-pole high band, so the tests measure brightness themselves.
    fn high_energy(frames: &[[f32; 2]], channel: usize) -> f64 {
        let mut low = 0.0f32;
        let mut sum = 0.0f64;
        for frame in frames {
            low += 0.08 * (frame[channel] - low);
            let high = frame[channel] - low;
            sum += (high as f64).powi(2);
        }
        sum
    }

    #[test]
    fn disabled_is_exact_pass_through() {
        let input = tone(440.0, 512);
        let off = PannerConfig {
            enabled: false,
            ..PannerConfig::default()
        };
        let output = run(off, &input);
        for (a, b) in input.iter().zip(output.iter()) {
            assert_eq!(a[0].to_bits(), b[0].to_bits());
            assert_eq!(a[1].to_bits(), b[1].to_bits());
        }
    }

    #[test]
    fn a_centred_source_stays_centred() {
        let start = PannerConfig {
            azimuth: 0.0,
            ..config()
        };
        let output = run(start, &tone(500.0, 2048));
        for frame in &output {
            assert!(
                (frame[0] - frame[1]).abs() < 1e-6,
                "centre drifted: {frame:?}"
            );
        }
    }

    #[test]
    fn azimuth_moves_the_image_and_mirrors() {
        let right = PannerConfig {
            azimuth: 90.0,
            ..config()
        };
        let left = PannerConfig {
            azimuth: -90.0,
            ..config()
        };
        let input = tone(500.0, 2048);
        let to_right = run(right, &input);
        let to_left = run(left, &input);
        let right_ratio = energy(&to_right, 1) / energy(&to_right, 0).max(1e-12);
        let left_ratio = energy(&to_left, 0) / energy(&to_left, 1).max(1e-12);
        assert!(right_ratio > 4.0, "right ratio {right_ratio}");
        assert!(left_ratio > 4.0, "left ratio {left_ratio}");
        assert!((right_ratio - left_ratio).abs() < 0.05 * right_ratio.max(left_ratio));
    }

    #[test]
    fn elevation_changes_brightness() {
        let above = PannerConfig {
            elevation: 60.0,
            ..config()
        };
        let below = PannerConfig {
            elevation: -60.0,
            ..config()
        };
        let input = tone(4_000.0, 2048);
        let tall = run(above, &input);
        let low = run(below, &input);
        let tall_high = high_energy(&tall, 0);
        let low_high = high_energy(&low, 0);
        assert!(
            tall_high > low_high * 1.1,
            "above-elevation energy {tall_high} did not exceed below {low_high}"
        );
    }

    #[test]
    fn width_raises_side_energy() {
        let narrow = PannerConfig {
            width: 0.0,
            ..config()
        };
        let wide = PannerConfig {
            width: 1.0,
            ..config()
        };
        let input: Vec<[f32; 2]> = (0..2048)
            .map(|index| {
                let value = (2.0 * std::f32::consts::PI * 300.0 * index as f32 / RATE).sin() * 0.4;
                [value, -value]
            })
            .collect();
        let side = |frames: &[[f32; 2]]| {
            frames
                .iter()
                .map(|frame| (((frame[0] - frame[1]) * 0.5) as f64).powi(2))
                .sum::<f64>()
        };
        let narrow_side = side(&run(narrow, &input));
        let wide_side = side(&run(wide, &input));
        assert!(
            wide_side > narrow_side * 4.0,
            "wide {wide_side} vs narrow {narrow_side}"
        );
    }

    #[test]
    fn room_adds_early_reflections_after_the_source_stops() {
        let dry = PannerConfig {
            room: false,
            ..config()
        };
        let wet = PannerConfig {
            room: true,
            reflection_level: 0.8,
            reflection_length: 0.8,
            timbre: 3,
            ..config()
        };
        let mut input = tone(800.0, 512);
        input.extend(std::iter::repeat([0.0f32; 2]).take(1_024));
        let dry_output = run(dry, &input);
        let wet_output = run(wet, &input);
        // Late enough that the head-shadow state has died away, so anything left
        // in the dry render is only the tail of the last sample.
        let tail = |frames: &[[f32; 2]]| energy(&frames[880..1_000], 0) + energy(&frames[880..1_000], 1);
        let dry_tail = tail(&dry_output);
        let wet_tail = tail(&wet_output);
        assert!(dry_tail < 1e-4, "the dry render still had a tail: {dry_tail}");
        assert!(
            wet_tail > dry_tail * 5.0,
            "reflection tail {wet_tail} vs dry {dry_tail}"
        );
    }

    #[test]
    fn output_stays_bounded_at_the_extremes() {
        let starts = [
            PannerConfig {
                azimuth: 180.0,
                elevation: 90.0,
                focus: 1.0,
                room: true,
                reflection_level: 1.0,
                reflection_length: 1.0,
                timbre: 4,
                ..config()
            },
            PannerConfig {
                azimuth: -180.0,
                elevation: -90.0,
                focus: 0.0,
                room: true,
                reflection_level: 1.0,
                ..config()
            },
        ];
        for start in starts {
            for frame in run(start, &tone(120.0, 4096)) {
                for channel in frame {
                    assert!(channel.is_finite(), "non-finite sample");
                    assert!((-1.0..=1.0).contains(&channel), "out of range {channel}");
                }
            }
        }
    }

    #[test]
    fn validate_rejects_out_of_range_values() {
        let base = config();
        assert!(validate(&base).is_ok());
        let invalid = [
            PannerConfig {
                azimuth: 181.0,
                ..base
            },
            PannerConfig {
                azimuth: f32::NAN,
                ..base
            },
            PannerConfig {
                elevation: 91.0,
                ..base
            },
            PannerConfig {
                width: 1.5,
                ..base
            },
            PannerConfig {
                focus: -0.1,
                ..base
            },
            PannerConfig {
                reflection_length: 2.0,
                ..base
            },
            PannerConfig {
                reflection_level: f32::INFINITY,
                ..base
            },
            PannerConfig { timbre: 5, ..base },
        ];
        for item in invalid {
            assert!(validate(&item).is_err(), "accepted {item:?}");
        }
        assert_eq!(timbres().len(), TIMBRE_DAMPING.len());
    }



    /// A bypassed stage ignores the material completely — the trap that makes the switch
    /// sound dead while the panner is switched out, whatever the room settings say.
    #[test]
    fn a_bypassed_stage_ignores_the_material() {
        let off = |timbre: usize| PannerConfig {
            enabled: false,
            room: true,
            reflection_level: 1.0,
            timbre,
            ..PannerConfig::default()
        };
        let input = tone(6_000.0, 1_024);
        let stone = run(off(3), &input);
        let glass = run(off(2), &input);
        for (a, b) in stone.iter().zip(&glass) {
            assert_eq!(a[0].to_bits(), b[0].to_bits());
            assert_eq!(a[1].to_bits(), b[1].to_bits());
        }
    }

    /// Every material must be tellable from every other one at the room level the panner
    /// ships with. The taps are the only place a material exists, so the spread has to be
    /// real in the tail: one axis of how much comes back, one of how bright it is.
    #[test]
    fn materials_are_tellable_apart_at_the_shipped_room_level() {
        let tail = |timbre: usize, hz: f32| -> (f64, f64) {
            let mut input = tone(hz, 1_024);
            input.extend(std::iter::repeat([0.0f32; 2]).take(2_048));
            let start = PannerConfig {
                room: true,
                reflection_level: 0.2,
                reflection_length: 0.35,
                timbre,
                ..config()
            };
            let output = run(start, &input);
            (
                energy(&output[1_024..], 0) + energy(&output[1_024..], 1),
                high_energy(&output[1_024..], 0) + high_energy(&output[1_024..], 1),
            )
        };
        let amount: Vec<f64> = (0..5).map(|timbre| 10.0 * tail(timbre, 400.0).0.log10()).collect();
        let tone: Vec<f64> = (0..5).map(|timbre| 10.0 * tail(timbre, 6_000.0).1.log10()).collect();
        for a in 0..timbres().len() {
            for b in a + 1..timbres().len() {
                let apart = (amount[a] - amount[b]).abs().max((tone[a] - tone[b]).abs());
                assert!(
                    apart > 2.0,
                    "{} and {} are only {apart:.2} dB apart: amount {amount:?}, tone {tone:?}",
                    timbres()[a],
                    timbres()[b]
                );
            }
        }
        // Glass returns the most of the tap and keeps the most of its top end.
        let loudest = amount.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
        let brightest = tone.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
        let quietest = amount.iter().enumerate().min_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
        assert_eq!(loudest, 2, "amount by material: {amount:?}");
        assert_eq!(brightest, 2, "tone by material: {tone:?}");
        assert_eq!(quietest, 1, "amount by material: {amount:?}");
    }

    /// The knob is tapered so its bottom half is usable: 20% arrives at about half the tap
    /// gain, and 100% is unchanged, so no setting is louder than it was.
    #[test]
    fn the_room_level_taper_lifts_the_bottom_of_the_knob() {
        assert!((tap_level(0.2) - 0.2_f32.powf(0.45)).abs() < 1e-6);
        assert!(
            (0.45..0.55).contains(&tap_level(0.2)),
            "20% should arrive near half the taps, got {}",
            tap_level(0.2)
        );
        assert_eq!(tap_level(0.0), 0.0);
        assert_eq!(tap_level(1.0), 1.0);
        assert_eq!(tap_level(1.5), 1.0, "the taper must not add gain past full scale");
    }
}
