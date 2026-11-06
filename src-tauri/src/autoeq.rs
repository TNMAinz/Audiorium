//! Published headphone corrections, and the parser that reads their format.
//!
//! Fourteen AutoEq parametric-EQ profiles are vendored under
//! `assets/corrections/`, unmodified, with their origin recorded in
//! `assets/corrections/SOURCES.md`. AutoEq computed each filter set from a
//! measurement by oratory1990; these are therefore AutoEq's calculations from
//! those measurements, not oratory1990's own presets, and the app says so.
//!
//! Two more files there — the Moondrop LAN 2 in its Ref and Pop tunings — are not
//! AutoEq's. AutoEq has no profile for that model, so `tools/fit_correction.py`
//! fitted both from a published squig.link measurement against the Harman IE 2019
//! target. They carry a `Basis` instead of a results directory, the app groups them
//! as `Fits`, and the label names both the measurement and who computed the filters.
//!
//! The parser is the same code an importer will use, so a file that arrives from
//! a user goes through exactly the checks the shipped ones did: every line named
//! if it is wrong, unknown filter types refused rather than skipped, and nothing
//! accepted that the engine could not run.

use crate::eq::{EqFilter, EqKind};
// The response measurement below is a test's instrument, not a shipped path: nothing
// at run time recomputes a correction's curve, so nothing else needs `eq` here.
#[cfg(test)]
use crate::eq::{self, EqConfig};
use crate::presets::{Preset, Provenance};

/// One correction. `text` is the published AutoEq file byte for byte, or the filter
/// set this app fitted itself when the measurement has no AutoEq profile.
pub struct Correction {
    pub slug: &'static str,
    pub model: &'static str,
    /// "over-ear" or "in-ear", as the AutoEq tree files them.
    pub form: &'static str,
    /// The AutoEq results directory a vendored file came from.
    pub directory: Option<&'static str>,
    /// What a fit of our own was made from; `None` for a vendored AutoEq file.
    pub basis: Option<Basis>,
    pub text: &'static str,
}

/// Where a profile this app fitted itself came from. A measurement and the target it
/// was matched to, because a correction without both describes nothing.
pub struct Basis {
    pub measured_by: &'static str,
    pub target: &'static str,
    pub url: &'static str,
}

pub const ROOT: &str = "https://github.com/jaakkopasanen/AutoEq/blob/master/results/oratory1990";

pub fn catalogue() -> Vec<Correction> {
    vec![
        Correction {
            slug: "sennheiser-hd-600",
            model: "Sennheiser HD 600",
            form: "over-ear",
            directory: Some("over-ear/Sennheiser HD 600"),
            basis: None,
            text: include_str!("../assets/corrections/sennheiser-hd-600.txt"),
        },
        Correction {
            slug: "sennheiser-hd-650",
            model: "Sennheiser HD 650",
            form: "over-ear",
            directory: Some("over-ear/Sennheiser HD 650"),
            basis: None,
            text: include_str!("../assets/corrections/sennheiser-hd-650.txt"),
        },
        Correction {
            slug: "sennheiser-hd-6xx",
            model: "Sennheiser HD 6XX",
            form: "over-ear",
            directory: Some("over-ear/Sennheiser HD 6XX"),
            basis: None,
            text: include_str!("../assets/corrections/sennheiser-hd-6xx.txt"),
        },
        Correction {
            slug: "sennheiser-hd-560s",
            model: "Sennheiser HD 560S",
            form: "over-ear",
            directory: Some("over-ear/Sennheiser HD 560S"),
            basis: None,
            text: include_str!("../assets/corrections/sennheiser-hd-560s.txt"),
        },
        Correction {
            slug: "sennheiser-hd-800-s",
            model: "Sennheiser HD 800 S",
            form: "over-ear",
            directory: Some("over-ear/Sennheiser HD 800 S"),
            basis: None,
            text: include_str!("../assets/corrections/sennheiser-hd-800-s.txt"),
        },
        Correction {
            slug: "hifiman-sundara-post-2020-earpads",
            model: "HIFIMAN Sundara (post-2020 earpads)",
            form: "over-ear",
            directory: Some("over-ear/HIFIMAN Sundara (post-2020 earpads)"),
            basis: None,
            text: include_str!("../assets/corrections/hifiman-sundara-post-2020-earpads.txt"),
        },
        Correction {
            slug: "akg-k371",
            model: "AKG K371",
            form: "over-ear",
            directory: Some("over-ear/AKG K371"),
            basis: None,
            text: include_str!("../assets/corrections/akg-k371.txt"),
        },
        Correction {
            slug: "beyerdynamic-dt-770-pro",
            model: "Beyerdynamic DT 770 Pro",
            form: "over-ear",
            directory: Some("over-ear/Beyerdynamic DT 770 Pro"),
            basis: None,
            text: include_str!("../assets/corrections/beyerdynamic-dt-770-pro.txt"),
        },
        Correction {
            slug: "beyerdynamic-dt-990-pro",
            model: "Beyerdynamic DT 990 Pro",
            form: "over-ear",
            directory: Some("over-ear/Beyerdynamic DT 990 Pro"),
            basis: None,
            text: include_str!("../assets/corrections/beyerdynamic-dt-990-pro.txt"),
        },
        Correction {
            slug: "audio-technica-ath-m50x",
            model: "Audio-Technica ATH-M50x",
            form: "over-ear",
            directory: Some("over-ear/Audio-Technica ATH-M50x"),
            basis: None,
            text: include_str!("../assets/corrections/audio-technica-ath-m50x.txt"),
        },
        Correction {
            slug: "philips-shp9500",
            model: "Philips SHP9500",
            form: "over-ear",
            directory: Some("over-ear/Philips SHP9500"),
            basis: None,
            text: include_str!("../assets/corrections/philips-shp9500.txt"),
        },
        Correction {
            slug: "meze-99-classics",
            model: "Meze 99 Classics",
            form: "over-ear",
            directory: Some("over-ear/Meze 99 Classics"),
            basis: None,
            text: include_str!("../assets/corrections/meze-99-classics.txt"),
        },
        Correction {
            slug: "moondrop-blessing-2",
            model: "Moondrop Blessing 2",
            form: "in-ear",
            directory: Some("in-ear/Moondrop Blessing 2"),
            basis: None,
            text: include_str!("../assets/corrections/moondrop-blessing-2.txt"),
        },
        Correction {
            slug: "7hz-salnotes-zero",
            model: "7Hz Salnotes Zero",
            form: "in-ear",
            directory: Some("in-ear/7Hz Salnotes Zero"),
            basis: None,
            text: include_str!("../assets/corrections/7hz-salnotes-zero.txt"),
        },
        // AutoEq has no LAN 2 profile: both tunings were fitted in `tools/fit_correction.py`
        // from Super* Review's published squig.link measurement, against the Harman IE 2019
        // target those graphs are drawn to. They are not AutoEq files and do not claim to be.
        Correction {
            slug: "moondrop-lan-2-ref",
            model: "Moondrop LAN 2 (Ref)",
            form: "in-ear",
            directory: None,
            basis: Some(Basis {
                measured_by: "Super* Review",
                target: "Harman IE 2019",
                url: "https://squig.link/data/Moondrop%20Lan%202%20ref%20L.txt",
            }),
            text: include_str!("../assets/corrections/moondrop-lan-2-ref.txt"),
        },
        Correction {
            slug: "moondrop-lan-2-pop",
            model: "Moondrop LAN 2 (Pop)",
            form: "in-ear",
            directory: None,
            basis: Some(Basis {
                measured_by: "Super* Review",
                target: "Harman IE 2019",
                url: "https://squig.link/data/Moondrop%20Lan%202%20pop%20L.txt",
            }),
            text: include_str!("../assets/corrections/moondrop-lan-2-pop.txt"),
        },
    ]
}

/// "…/over-ear/AKG%20K371/AKG%20K371%20ParametricEQ.txt": the file this came from,
/// or the published measurement a profile of our own was fitted from.
pub fn source_url(correction: &Correction) -> String {
    if let Some(basis) = &correction.basis {
        return basis.url.to_string();
    }
    let encoded = |value: &str| value.replace(' ', "%20");
    let directory = correction.directory.unwrap_or_default();
    format!(
        "{ROOT}/{}/{}%20ParametricEQ.txt",
        encoded(directory),
        encoded(correction.model)
    )
}

/// The line under a correction: who measured it, and who computed the filters.
pub fn provenance(correction: &Correction) -> (String, String, &'static str) {
    match &correction.basis {
        Some(basis) => (
            format!(
                "AUDIORIUM fit · measurement by {} · {}",
                basis.measured_by, correction.form
            ),
            format!("{} · target {}", basis.url, basis.target),
            "The measurement belongs to its publisher; the filter set is this app's own fit to a published target.",
        ),
        None => (
            format!(
                "AutoEq · measurement by oratory1990 · {}",
                correction.form
            ),
            source_url(correction),
            "AutoEq is MIT. Its repository states no separate licence for its result files.",
        ),
    }
}

fn decibels(value: f32) -> String {
    let sign = if value < 0.0 { "\u{2212}" } else { "+" };
    format!("{sign}{:.1} dB", value.abs())
}

/// A number as the files write it, with the unit word allowed after it.
fn number(token: &str) -> Option<f32> {
    token.parse::<f32>().ok().filter(|value| value.is_finite())
}

/// One `Filter N: ON PK Fc 8445 Hz Gain 3.3 dB Q 1.61` line.
fn parse_filter(line: &str, at_line: usize) -> Result<EqFilter, String> {
    let fail = |why: &str| format!("Line {at_line}: {why}");
    let body = line
        .split_once(':')
        .map(|(_, rest)| rest)
        .ok_or_else(|| fail("a filter line needs a number and a colon"))?;
    let tokens: Vec<&str> = body.split_whitespace().collect();
    if tokens.len() < 10 {
        return Err(fail("a filter line needs a state, a type, Fc, Gain and Q"));
    }
    let enabled = match tokens[0].to_ascii_uppercase().as_str() {
        "ON" => true,
        "OFF" => false,
        other => return Err(fail(&format!("unknown filter state {other}"))),
    };
    let kind = match tokens[1].to_ascii_uppercase().as_str() {
        "PK" => EqKind::Peaking,
        "LSC" | "LS" => EqKind::Lowshelf,
        "HSC" | "HS" => EqKind::Highshelf,
        "LPQ" | "LP" => EqKind::Lowpass,
        "HPQ" | "HP" => EqKind::Highpass,
        "NO" | "NOTCH" => EqKind::Notch,
        other => return Err(fail(&format!("unknown filter type {other}"))),
    };
    // The labels are checked as well as the numbers: a format that reorders its
    // fields would otherwise be read as if it had not changed.
    for (at, label) in [(2, "Fc"), (4, "Hz"), (5, "Gain"), (7, "dB"), (8, "Q")] {
        if !tokens[at].eq_ignore_ascii_case(label) {
            return Err(fail(&format!("expected {label}, found {}", tokens[at])));
        }
    }
    let frequency = number(tokens[3]).ok_or_else(|| fail("Fc is not a number"))?;
    let gain_db = number(tokens[6]).ok_or_else(|| fail("Gain is not a number"))?;
    let q = number(tokens[9]).ok_or_else(|| fail("Q is not a number"))?;
    Ok(EqFilter {
        kind,
        frequency,
        gain_db,
        q,
        enabled,
    })
}

/// The whole file: the preamp and the bands, in the order the file lists them.
pub fn parse(text: &str) -> Result<(f32, Vec<EqFilter>), String> {
    let mut preamp: Option<f32> = None;
    let mut filters: Vec<EqFilter> = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let number = index + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("Preamp:") {
            let value = rest
                .trim()
                .trim_end_matches("dB")
                .trim()
                .trim_end_matches(',');
            preamp = Some(
                number_of(value).ok_or_else(|| format!("Line {number}: the preamp is not a number"))?,
            );
            continue;
        }
        if !line.to_ascii_lowercase().starts_with("filter ") {
            return Err(format!("Line {number}: expected a Preamp or Filter line"));
        }
        filters.push(parse_filter(line, number)?);
    }
    let preamp = preamp.ok_or("The profile has no Preamp line")?;
    if filters.is_empty() {
        return Err("The profile has no filters".into());
    }
    Ok((preamp, filters))
}

fn number_of(token: &str) -> Option<f32> {
    token.parse::<f32>().ok().filter(|value| value.is_finite())
}

/// The corrections as patches: same shape as the built-in ones, plus where the
/// numbers came from.
pub fn presets() -> Vec<Preset> {
    catalogue()
        .into_iter()
        .filter_map(|correction| {
            let (preamp_db, filters) = match parse(correction.text) {
                Ok(parsed) => parsed,
                Err(error) => {
                    // The tests below prove this cannot happen for a shipped file.
                    eprintln!("AUDIORIUM: {} is not a usable profile: {error}", correction.slug);
                    return None;
                }
            };
            let (label, url, terms) = provenance(&correction);
            let note = match &correction.basis {
                Some(basis) => format!(
                    "{} bands, preamp {}. This app's own fit to a {} measurement, matched to the {} target; your pads, seal and unit will differ.",
                    filters.len(),
                    decibels(preamp_db),
                    basis.measured_by,
                    basis.target
                ),
                None => format!(
                    "{} bands, preamp {}. AutoEq's calculation from an oratory1990 measurement; your pads, seal and unit will differ.",
                    filters.len(),
                    decibels(preamp_db)
                ),
            };
            let id = match &correction.basis {
                Some(_) => format!("fit:{}", correction.slug),
                None => format!("autoeq:{}", correction.slug),
            };
            Some(Preset {
                id,
                name: correction.model.to_string(),
                builtin: true,
                note,
                preamp_db,
                filters,
                source: Some(Provenance {
                    label,
                    url,
                    terms: terms.into(),
                }),
            })
        })
        .collect()
}

/// Headroom a shipped correction asks for, from the engine's own coefficients.
#[cfg(test)]
fn headroom(preset: &Preset) -> f32 {
    eq::response(
        &EqConfig {
            enabled: true,
            preamp_db: preset.preamp_db,
            filters: preset.filters.clone(),
        },
        512,
    )
    .headroom_db
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_vendored_file_parses_into_something_the_engine_can_run() {
        let catalogue = catalogue();
        assert_eq!(catalogue.len(), 16, "the shipped set changed size");
        for correction in &catalogue {
            let (preamp, filters) = parse(correction.text)
                .unwrap_or_else(|error| panic!("{} does not parse: {error}", correction.slug));
            if correction.directory.is_some() {
                assert_eq!(filters.len(), 10, "{} is not a ten-band correction", correction.slug);
            }
            assert!(
                (8..=10).contains(&filters.len()),
                "{} has {} bands",
                correction.slug,
                filters.len()
            );
            assert!(filters.iter().all(|filter| filter.enabled), "{} has a disabled band", correction.slug);
            for filter in &filters {
                assert!(
                    matches!(
                        filter.kind,
                        EqKind::Peaking | EqKind::Lowshelf | EqKind::Highshelf
                    ),
                    "{} uses a filter kind the shipped engine does not expect",
                    correction.slug
                );
            }
            eq::validate(&EqConfig {
                enabled: true,
                preamp_db: preamp,
                filters: filters.clone(),
            })
            .unwrap_or_else(|error| panic!("{} is not a runnable band set: {error}", correction.slug));
            let url = source_url(correction);
            match &correction.basis {
                Some(basis) => {
                    // A profile of our own names the published measurement it was fitted from.
                    assert!(url.starts_with(basis.url), "{} has no measurement source", correction.slug);
                    assert!(url.contains("squig.link"), "{} does not name a database", correction.slug);
                }
                None => assert!(
                    url.starts_with(&format!("{ROOT}/")),
                    "{} has no canonical source",
                    correction.slug
                ),
            }
            assert!(!url.contains(' '), "{} has an unencoded source url", correction.slug);
        }
    }

    #[test]
    fn the_parser_reads_the_published_values_field_by_field() {
        // The first three lines of the vendored Sennheiser HD 600 profile.
        let correction = catalogue()
            .into_iter()
            .find(|correction| correction.slug == "sennheiser-hd-600")
            .expect("the HD 600 profile is gone");
        let (preamp, filters) = parse(correction.text).expect("the profile parses");
        assert!((preamp + 6.3).abs() < 1e-4, "preamp read as {preamp}");
        let first = filters[0];
        assert_eq!(first.kind, EqKind::Lowshelf);
        assert!((first.frequency - 105.0).abs() < 1e-4, "{}", first.frequency);
        assert!((first.gain_db - 6.5).abs() < 1e-4, "{}", first.gain_db);
        assert!((first.q - 0.70).abs() < 1e-4, "{}", first.q);
        // Filter 3 is the one whose frequency lands in the mid treble.
        let third = filters[2];
        assert_eq!(third.kind, EqKind::Peaking);
        assert!((third.frequency - 8_445.0).abs() < 1e-3, "{}", third.frequency);
        assert!((third.gain_db - 3.3).abs() < 1e-4, "{}", third.gain_db);
        assert!((third.q - 1.61).abs() < 1e-4, "{}", third.q);
    }

    #[test]
    fn autoeqs_preamp_matches_what_the_engine_measures_of_the_same_curve() {
        // Two independent implementations of the same response: AutoEq chose each
        // preamp to cover its own predicted peak, this engine recomputes that peak
        // from the parsed coefficients. They have to agree.
        for preset in presets() {
            let headroom = headroom(&preset);
            assert!(
                headroom.abs() < 1.0,
                "{}: AutoEq's preamp and this engine's curve disagree by {:.2} dB",
                preset.name,
                headroom
            );
        }
    }

    #[test]
    fn every_correction_is_a_patch_the_picker_can_show() {
        let presets = presets();
        assert_eq!(presets.len(), 16, "a correction was dropped");
        for preset in &presets {
            assert!(preset.builtin, "{} is not a shipped patch", preset.name);
            let ours = preset.id.starts_with("fit:");
            assert!(
                ours || preset.id.starts_with("autoeq:"),
                "{} has no correction id",
                preset.id
            );
            let source = preset.source.as_ref().expect("a correction without provenance");
            // A correction names who measured it and who computed the filters.
            assert!(
                source.label.contains(if ours { "Super* Review" } else { "oratory1990" }),
                "{} credits nobody",
                preset.name
            );
            assert!(
                !ours || source.label.contains("AUDIORIUM fit"),
                "{} does not say who fitted it",
                preset.name
            );
            assert!(
                source.url.contains(if ours { "squig.link" } else { "AutoEq" }),
                "{} has no source url",
                preset.name
            );
            assert!(!source.terms.is_empty(), "{} states no terms", preset.name);
            assert!(preset.note.contains("10 bands"), "{} does not say what it holds", preset.name);
        }
        let ids: std::collections::BTreeSet<&str> = presets.iter().map(|preset| preset.id.as_str()).collect();
        assert_eq!(ids.len(), presets.len(), "two corrections share an id");
    }

    #[test]
    fn a_broken_profile_is_rejected_with_the_line_that_breaks_it() {
        let preamp = "Preamp: -6.0 dB\n";
        // No preamp at all.
        let error = parse("Filter 1: ON PK Fc 100 Hz Gain 3.0 dB Q 1.00").unwrap_err();
        assert!(error.contains("Preamp"), "{error}");
        // An unknown filter type is refused, not skipped.
        let error = parse(&format!("{preamp}Filter 1: ON XYZ Fc 100 Hz Gain 3.0 dB Q 1.00")).unwrap_err();
        assert!(error.contains("Line 2") && error.contains("XYZ"), "{error}");
        // A missing Q.
        let error = parse(&format!("{preamp}Filter 1: ON PK Fc 100 Hz Gain 3.0 dB")).unwrap_err();
        assert!(error.contains("Line 2"), "{error}");
        // Fields out of order: the labels are checked, so this cannot be misread.
        let error = parse(&format!("{preamp}Filter 1: ON PK Fc 100 Hz Q 1.00 Gain 3.0 dB")).unwrap_err();
        assert!(error.contains("Gain"), "{error}");
        // A number that is not finite.
        let error = parse(&format!("{preamp}Filter 1: ON PK Fc NaN Hz Gain 3.0 dB Q 1.00")).unwrap_err();
        assert!(error.contains("Fc"), "{error}");
        // A file with a preamp but no bands.
        let error = parse(preamp).unwrap_err();
        assert!(error.contains("no filters"), "{error}");
        // A good file, including an OFF band and a comment, still reads.
        let (value, filters) = parse(&format!(
            "{preamp}# a comment\nFilter 1: ON LSC Fc 105 Hz Gain 6.5 dB Q 0.70\nFilter 2: OFF PK Fc 1000 Hz Gain -2.0 dB Q 1.00"
        ))
        .expect("the good file parses");
        assert!((value + 6.0).abs() < 1e-4);
        assert_eq!(filters.len(), 2);
        assert!(!filters[1].enabled, "an OFF band was read as on");
    }
}
