//! Tunings.
//!
//! A tuning is an offset, in cents, from equal temperament for each of the 128
//! keys. The engine adds it before the glide, so a scale is the pitch of the key
//! rather than an effect applied to it. Equal temperament is the empty tuning: a
//! patch that has not been retuned carries no table at all.
//!
//! Scala files are read here because the format is small and exact: a count, then
//! intervals as ratios or cents, repeated at the period the last interval names.

/// A tuning as the engine holds it: cents of offset per key.
#[derive(Clone, Debug, PartialEq)]
pub struct Scale {
    pub name: String,
    pub cents: Vec<f32>,
}

impl Scale {
    pub fn equal() -> Self {
        Self {
            name: "Equal".into(),
            cents: Vec::new(),
        }
    }

    pub fn is_equal(&self) -> bool {
        self.cents.is_empty() || self.cents.iter().all(|offset| offset.abs() < 1e-4)
    }
}

/// A scale's degrees, in cents above its root, repeated to fill the keyboard.
/// `degrees` are the pitches above the root inside one period — eleven of them for
/// a twelve-note scale, because the root and the period are the ends of the ladder.
/// A scale therefore repeats every `degrees.len() + 1` keys, which is where its own
/// count comes from: the root is implicit and the period is stated.
fn from_degrees(name: &str, degrees: &[f32], period: f32) -> Scale {
    if degrees.is_empty() || period <= 0.0 {
        return Scale::equal();
    }
    let count = degrees.len() + 1;
    let mut cents = Vec::with_capacity(128);
    for note in 0..128 {
        let octave = note / count;
        let degree = note % count;
        let within = if degree == 0 { 0.0 } else { degrees[degree - 1] };
        let absolute = octave as f32 * period + within;
        // Equal temperament puts this key at 100 cents a semitone from A4; the
        // offset is what the scale asks for beyond that.
        cents.push(absolute - note as f32 * 100.0);
    }
    Scale {
        name: name.into(),
        cents,
    }
}

/// The ratios of a just major scale, in cents, so the thirds and sixths beat with
/// the harmonics they share rather than against them.
pub fn just_major() -> Scale {
    // Eleven steps, not twelve: the root is where the ladder starts and the octave
    // is where it ends, so neither is a degree of its own.
    let ratios: [f32; 11] = [
        16.0 / 15.0,
        9.0 / 8.0,
        6.0 / 5.0,
        5.0 / 4.0,
        4.0 / 3.0,
        45.0 / 32.0,
        3.0 / 2.0,
        8.0 / 5.0,
        5.0 / 3.0,
        9.0 / 5.0,
        15.0 / 8.0,
    ];
    let degrees: Vec<f32> = ratios.iter().map(|ratio| 1200.0 * ratio.log2()).collect();
    from_degrees("Just major", &degrees, 1200.0)
}

/// The scale the Greeks wrote down: pure fifths stacked, which makes the thirds
/// wide and the fifths exact. The chain runs from six flats to six sharps, each
/// fifth multiplied up and then folded into one octave, and the twelve pitches
/// that come out are sorted: a scale is a ladder of pitches, not the order the
/// fifths were generated in.
pub fn pythagorean() -> Scale {
    let mut pitches: Vec<f32> = (-6..=5)
        .map(|step| {
            let mut cents = step as f32 * 1200.0 * (3.0f32 / 2.0).log2();
            while cents < 0.0 {
                cents += 1200.0;
            }
            while cents >= 1200.0 {
                cents -= 1200.0;
            }
            cents
        })
        .collect();
    pitches.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // The root is not a degree: the ladder starts at it and climbs from there.
    from_degrees("Pythagorean", &pitches[1..], 1200.0)
}

/// Equal divisions of the octave, for the tunings between the historic ones: the
/// step is the period divided by the count, and the key keeps its own step.
pub fn edo(steps: u32) -> Scale {
    let steps = steps.clamp(5, 96);
    let degrees: Vec<f32> = (1..steps)
        .map(|step| 1200.0 * step as f32 / steps as f32)
        .collect();
    from_degrees(&format!("{steps}-EDO"), &degrees, 1200.0)
}

/// Quarter-comma meantone: the fifths are flattened a quarter of a comma so the
/// major third lands on a pure 5/4.
pub fn meantone() -> Scale {
    let fifth = 1200.0 * (5.0f32 / 4.0).log2() / 4.0 + 1200.0 * (3.0f32 / 2.0).log2() / 4.0 * 3.0;
    let _ = fifth;
    // A fifth flattened by a quarter of a syntonic comma: 696.578 cents.
    let meantone_fifth = 696.578;
    let degrees: Vec<f32> = (0..12)
        .map(|step| {
            let mut cents = (step as f32 * meantone_fifth) % 1200.0;
            if cents < 0.0 {
                cents += 1200.0;
            }
            cents
        })
        .collect();
    from_degrees("Meantone", &degrees, 1200.0)
}

/// Every tuning the picker offers, equal temperament first because it is what a
/// patch starts on.
pub fn builtin() -> Vec<Scale> {
    vec![
        Scale::equal(),
        just_major(),
        pythagorean(),
        meantone(),
        edo(19),
        edo(22),
        edo(31),
        edo(53),
    ]
}

/// A Scala file: a description, a count, then that many intervals. A ratio is
/// `a/b`, cents is a number with a point, and a bare integer is a ratio over one.
pub fn parse_scl(text: &str) -> Result<Scale, String> {
    let mut lines = text
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('!'))
        .map(|line| line.split('!').next().unwrap_or("").trim().to_string())
        .filter(|line| !line.is_empty());

    let name = lines
        .next()
        .ok_or_else(|| "The scale has no description line.".to_string())?;
    let count: usize = lines
        .next()
        .ok_or_else(|| "The scale does not say how many notes it has.".to_string())?
        .parse()
        .map_err(|_| "The scale's note count is not a number.".to_string())?;
    if count == 0 || count > 128 {
        return Err("A scale is between 1 and 128 notes.".into());
    }

    let mut degrees = Vec::with_capacity(count);
    let mut period = 0.0f32;
    for index in 0..count {
        let line = lines
            .next()
            .ok_or_else(|| format!("The scale promises {count} notes and stops at {}.", index + 1))?;
        let cents = if line.contains('.') {
            line.parse::<f32>()
                .map_err(|_| format!("`{line}` is not a number of cents."))?
        } else if let Some((numerator, denominator)) = line.split_once('/') {
            let a: f32 = numerator
                .trim()
                .parse()
                .map_err(|_| format!("`{line}` is not a ratio."))?;
            let b: f32 = denominator
                .trim()
                .parse()
                .map_err(|_| format!("`{line}` is not a ratio."))?;
            if a <= 0.0 || b <= 0.0 {
                return Err(format!("`{line}` is not a ratio above zero."));
            }
            1200.0 * (a / b).log2()
        } else {
            let ratio: f32 = line
                .parse()
                .map_err(|_| format!("`{line}` is neither cents nor a ratio."))?;
            if ratio <= 0.0 {
                return Err(format!("`{line}` is not a ratio above zero."));
            }
            1200.0 * ratio.log2()
        };
        // A Scala line is the pitch of that note above the root, not the interval
        // from the line before it: 5/4 is a pure third, and 386.3 cents is the same
        // pitch written another way.
        if index + 1 < count {
            degrees.push(cents);
        } else {
            period = cents;
        }
    }
    if period <= 0.0 {
        return Err("The scale's period is not above zero.".into());
    }
    if degrees.is_empty() {
        return Err("The scale has no degrees above its root.".into());
    }
    Ok(from_degrees(&name, &degrees, period))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_temperament_is_no_offset_at_all() {
        let scale = Scale::equal();
        assert!(scale.is_equal());
        assert!(just_major().cents.iter().any(|offset| offset.abs() > 1.0));
    }

    #[test]
    fn a_just_major_third_lands_on_its_ratio() {
        // E in just intonation is 5/4 above C, so the offset from equal is −13.7
        // cents: the third is flatter than a piano's.
        let scale = just_major();
        let e = scale.cents[4 + 60];
        assert!((e - -13.686).abs() < 0.1, "E came out at {e} cents");
        // The fifth is pure against the root, which lands it 2 cents above the
        // key's equal-tempered pitch: 701.955 against 700.
        assert!((scale.cents[7 + 60] - 1.955).abs() < 0.1);
    }

    #[test]
    fn a_scala_file_of_cents_is_read() {
        let text = "! mean tone\nQuarter comma\n12\n100.0\n200.0\n300.0\n400.0\n500.0\n600.0\n700.0\n800.0\n900.0\n1000.0\n1100.0\n2/1\n";
        let scale = parse_scl(text).expect("a readable scale");
        assert_eq!(scale.name, "Quarter comma");
        assert_eq!(scale.cents.len(), 128);
        assert!(scale.cents[60].abs() < 0.01);
    }

    #[test]
    fn a_scala_file_of_ratios_is_read_and_a_bad_one_is_refused() {
        let text = "Just\n12\n16/15\n9/8\n6/5\n5/4\n4/3\n45/32\n3/2\n8/5\n5/3\n9/5\n15/8\n2/1\n";
        let scale = parse_scl(text).expect("a readable scale");
        assert!((scale.cents[4 + 60] - -13.686).abs() < 0.1);

        assert!(parse_scl("").is_err());
        assert!(parse_scl("Name\n12\n1/1\n").is_err(), "a short scale must fail");
        assert!(parse_scl("Name\n12\n0/1\n").is_err(), "a ratio of zero must fail");
    }

    #[test]
    fn a_nineteen_note_scale_fills_the_keyboard_by_repetition() {
        let mut text = String::from("19 EDO\n19\n");
        for step in 1..=19 {
            text.push_str(&format!("{:.4}\n", step as f32 * 1200.0 / 19.0));
        }
        let scale = parse_scl(&text).expect("19 steps");
        assert_eq!(scale.cents.len(), 128);
        // Nineteen keys now fill an octave, so key 72 — three of the scale's octaves
        // and fifteen of its steps up — sits far below the equal-tempered key.
        let expected = 3600.0 + 15.0 * 1200.0 / 19.0 - 7200.0;
        assert!((scale.cents[60 + 12] - expected).abs() < 0.1, "{}", scale.cents[72]);
    }

    #[test]
    fn the_fifths_of_pythagorean_are_pure_and_the_scale_is_sorted() {
        let scale = pythagorean();
        assert_eq!(scale.cents.len(), 128);
        // Every degree of one octave is within half a semitone of its key: the
        // ladder is the pitches, and the offsets are how far they sit from equal.
        for offset in &scale.cents[60..72] {
            assert!(offset.abs() < 60.0, "{offset} is not a degree of this octave");
        }
        // G is 3/2 above C: 2 cents above the key's equal-tempered pitch.
        assert!((scale.cents[60 + 7] - 1.955).abs() < 0.1);
        // The Pythagorean third is 81/64, which is 7.8 cents sharp of 5/4.
        assert!((scale.cents[60 + 4] - 7.82).abs() < 0.2);
    }

    #[test]
    fn an_equal_division_puts_its_own_step_on_every_key() {
        let scale = edo(19);
        assert_eq!(scale.cents.len(), 128);
        // Nineteen keys fill an octave, so key 72 is fifteen 19-EDO steps above the
        // third octave — far below the equal-tempered key of the same number.
        let expected = 3600.0 + 15.0 * 1200.0 / 19.0 - 7200.0;
        assert!((scale.cents[72] - expected).abs() < 0.1, "{}", scale.cents[72]);
        assert!(builtin().first().map(Scale::is_equal).unwrap_or(false));
    }

    #[test]
    fn a_scale_whose_count_is_not_a_number_is_refused() {
        let reason = parse_scl("Broken\nmany\n100.0\n").expect_err("a count must be a number");
        assert!(reason.contains("count"), "{reason}");
    }

    #[test]
    fn every_offered_tuning_is_named_once_and_fills_the_keyboard() {
        let offered = builtin();
        let names: std::collections::HashSet<&str> =
            offered.iter().map(|scale| scale.name.as_str()).collect();
        assert_eq!(names.len(), offered.len(), "two tunings share a name");
        // Equal temperament is the empty tuning by design; every other scale is a
        // full table of 128 offsets.
        assert!(offered[0].is_equal());
        for scale in offered.iter().skip(1) {
            assert_eq!(
                scale.cents.len(),
                128,
                "{} does not fill the keyboard",
                scale.name
            );
            assert!(
                scale.cents.iter().all(|cents| cents.is_finite()),
                "{} carries a bad offset",
                scale.name
            );
        }
    }
}
