//! The coverage grid: every family crossed with every oscillator shape and every
//! filter model the engine has.
//!
//! The curated packs are taste — two hundred voice ideas with names. The grid is the
//! other half of the shelf, and it is not a taste at all: it is the product of the
//! three axes the sound is built on, so the catalogue can say what it covers and be
//! checked against the engine rather than against its own description. Twelve
//! families × four oscillator shapes × seven filters is 1,344 patches in 336 packs,
//! and a test reads the generated list back to prove no cell is missing and none is
//! twice.

use std::fmt::Write as _;

use super::Family;
use crate::synth::{FilterKind, Wave};

/// The oscillator shapes a cell can be built on. `Table` is left out on purpose: a
/// table wave's sound is the imported frame, so a grid cell could not promise what it
/// contains.
pub const WAVES: [Wave; 4] = [Wave::Saw, Wave::Square, Wave::Triangle, Wave::Sine];

/// Every filter model the engine runs.
pub const FILTERS: [FilterKind; 7] = [
    FilterKind::Lowpass,
    FilterKind::Bandpass,
    FilterKind::Highpass,
    FilterKind::Ladder,
    FilterKind::Comb,
    FilterKind::Notch,
    FilterKind::Peak,
];

/// The four takes every pack holds, by the name the grid gives them.
pub const VARIANTS: [&str; 4] = ["Core", "Veil", "Edge", "Drift"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub family: Family,
    /// The shape the cell's first oscillator is set to. The second oscillator keeps
    /// the family's own wave, so a cell is a family wearing one new shape and one new
    /// filter rather than a family replaced by a coordinate.
    pub wave: Wave,
    pub filter: FilterKind,
}

pub fn wave_label(wave: Wave) -> &'static str {
    match wave {
        Wave::Saw => "Saw",
        Wave::Square => "Square",
        Wave::Triangle => "Triangle",
        Wave::Sine => "Sine",
        Wave::Table => "Table",
    }
}

pub fn filter_label(kind: FilterKind) -> &'static str {
    match kind {
        FilterKind::Lowpass => "Lowpass",
        FilterKind::Bandpass => "Bandpass",
        FilterKind::Highpass => "Highpass",
        FilterKind::Ladder => "Ladder",
        FilterKind::Comb => "Comb",
        FilterKind::Notch => "Notch",
        FilterKind::Peak => "Peak",
    }
}

/// Every cell of the grid, in a fixed order: family, then the oscillator shape, then
/// the filter. The order is the pack order on the shelf, so a cell's place is a
/// consequence of its coordinates rather than of luck.
pub fn cells() -> Vec<Cell> {
    let mut all = Vec::with_capacity(Family::ALL.len() * WAVES.len() * FILTERS.len());
    for family in Family::ALL {
        for wave in WAVES {
            for filter in FILTERS {
                all.push(Cell { family, wave, filter });
            }
        }
    }
    all
}

/// Where a cell sits in the grid, as compact numbers: the id is these three and which
/// of the four takes, so a patch the page hands back can be rebuilt without a table
/// of names living anywhere.
pub fn id(cell: &Cell, variant: usize) -> String {
    let mut id = String::from("grid:");
    let _ = write!(
        id,
        "{}:{}:{}#{variant}",
        family_index(cell.family),
        wave_index(cell.wave),
        filter_index(cell.filter),
    );
    id
}

pub fn parse(id: &str) -> Option<(Cell, usize)> {
    let rest = id.strip_prefix("grid:")?;
    let (coordinates, variant) = rest.split_once('#')?;
    let variant: usize = variant.parse().ok()?;
    if variant >= VARIANTS.len() {
        return None;
    }
    let numbers: Vec<&str> = coordinates.split(':').collect();
    if numbers.len() != 3 {
        return None;
    }
    let number = |index: usize, high: usize| numbers[index].parse::<usize>().ok().filter(|value| *value < high);
    Some((
        Cell {
            family: *Family::ALL.get(number(0, Family::ALL.len())?)?,
            wave: *WAVES.get(number(1, WAVES.len())?)?,
            filter: *FILTERS.get(number(2, FILTERS.len())?)?,
        },
        variant,
    ))
}

fn family_index(family: Family) -> usize {
    Family::ALL.iter().position(|item| *item == family).unwrap_or(0)
}

fn wave_index(wave: Wave) -> usize {
    WAVES.iter().position(|item| *item == wave).unwrap_or(0)
}

fn filter_index(kind: FilterKind) -> usize {
    FILTERS.iter().position(|item| *item == kind).unwrap_or(0)
}

/// The pack's name: its three coordinates, so a shelf of 336 grid packs stays
/// searchable by any of them.
pub fn name(cell: &Cell) -> String {
    format!(
        "{} · {} · {}",
        cell.family.label(),
        wave_label(cell.wave),
        filter_label(cell.filter),
    )
}

/// One line saying exactly what the cell is, rather than how it feels.
pub fn blurb(cell: &Cell) -> String {
    format!(
        "{} through the {} filter, built on the {} voice.",
        wave_label(cell.wave),
        filter_label(cell.filter),
        cell.family.label().to_lowercase(),
    )
}

/// The four takes, named for the cell's family so a loaded patch still says what it
/// is: `Bass Core`, `Bass Veil`, `Bass Edge`, `Bass Drift`.
pub fn patch_names(cell: &Cell) -> [String; 4] {
    VARIANTS.map(|variant| format!("{} {variant}", cell.family.label()))
}

/// Where a cell sits between dark and bright. The filter carries more of the weight
/// than the wave, because on a subtractive voice it is the filter that decides how
/// much of the oscillator reaches the ear.
pub fn character(cell: &Cell) -> f32 {
    let wave = |wave: Wave| match wave {
        Wave::Saw => 0.72,
        Wave::Square => 0.55,
        Wave::Triangle => 0.34,
        Wave::Sine => 0.18,
        Wave::Table => 0.5,
    };
    let filter: f32 = match cell.filter {
        FilterKind::Lowpass => 0.32,
        FilterKind::Bandpass => 0.5,
        FilterKind::Highpass => 0.82,
        FilterKind::Ladder => 0.58,
        FilterKind::Comb => 0.64,
        FilterKind::Notch => 0.44,
        FilterKind::Peak => 0.6,
    };
    (wave(cell.wave) * 0.45 + filter * 0.55).clamp(0.06, 0.94)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_holds_every_family_wave_and_filter_exactly_once() {
        let cells = cells();
        assert_eq!(cells.len(), 12 * 4 * 7);
        let mut seen = std::collections::HashSet::new();
        for cell in &cells {
            assert!(seen.insert(id(cell, 0)), "{} is in the grid twice", name(cell));
        }
        // A missing corner is a missing cell, not a rounding error: every coordinate
        // combination is present.
        for family in Family::ALL {
            for wave in WAVES {
                for filter in FILTERS {
                    let cell = Cell { family, wave, filter };
                    assert!(seen.contains(&id(&cell, 0)), "{family:?} {wave:?} {filter:?} is missing");
                }
            }
        }
    }

    #[test]
    fn every_id_the_grid_hands_out_comes_back_as_its_cell() {
        for cell in cells() {
            for variant in 0..VARIANTS.len() {
                let (parsed, index) = parse(&id(&cell, variant)).expect("a grid id does not parse");
                assert_eq!(parsed, cell);
                assert_eq!(index, variant);
            }
        }
        assert!(parse("grid:0:0:0#4").is_none());
        assert!(parse("grid:12:0:0#0").is_none());
        assert!(parse("grid:0:0:0:0#0").is_none());
        assert!(parse("grid:0:4:0#0").is_none());
        assert!(parse("grid:zero:0:0#0").is_none());
        assert!(parse("grid:0:0#0").is_none());
        assert!(parse("pack:anything#0").is_none());
    }

    #[test]
    fn a_name_carries_every_coordinate_of_its_cell() {
        let cell = Cell {
            family: Family::Pad,
            wave: Wave::Triangle,
            filter: FilterKind::Comb,
        };
        assert_eq!(name(&cell), "Pad · Triangle · Comb");
        assert!(blurb(&cell).contains("Triangle through the Comb filter"));
        assert_eq!(patch_names(&cell), ["Pad Core", "Pad Veil", "Pad Edge", "Pad Drift"]);
        assert!(character(&cell) > 0.0 && character(&cell) < 1.0);
    }
}
