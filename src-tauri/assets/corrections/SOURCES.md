# Where these correction files came from

Fourteen parametric-EQ profiles, fetched on **2026-10-30** from the AutoEq repository's
`results/oratory1990` tree and stored here **unmodified, byte for byte**, so the numbers in the
app are the numbers that were published.

AutoEq is a generator, not a measurement lab: it computes a parametric filter set from a
measurement and a target. These files are therefore **AutoEq's calculation from an oratory1990
measurement**, not oratory1990's own published preset. The distinction matters for provenance and
is stated in the app.

Two further files here are not AutoEq's: the Moondrop LAN 2 tunings, which AutoEq has no profile
for and which this app fitted itself from a published squig.link measurement. They are described
separately at the end, and the app groups them `Fits` rather than `Corrections` for that reason.

## Files

| Local file | AutoEq directory | Measurement |
| --- | --- | --- |
| `sennheiser-hd-600.txt` | `over-ear/Sennheiser HD 600` | oratory1990 |
| `sennheiser-hd-650.txt` | `over-ear/Sennheiser HD 650` | oratory1990 |
| `sennheiser-hd-6xx.txt` | `over-ear/Sennheiser HD 6XX` | oratory1990 |
| `sennheiser-hd-560s.txt` | `over-ear/Sennheiser HD 560S` | oratory1990 |
| `sennheiser-hd-800-s.txt` | `over-ear/Sennheiser HD 800 S` | oratory1990 |
| `hifiman-sundara-post-2020-earpads.txt` | `over-ear/HIFIMAN Sundara (post-2020 earpads)` | oratory1990 |
| `akg-k371.txt` | `over-ear/AKG K371` | oratory1990 |
| `beyerdynamic-dt-770-pro.txt` | `over-ear/Beyerdynamic DT 770 Pro` | oratory1990 |
| `beyerdynamic-dt-990-pro.txt` | `over-ear/Beyerdynamic DT 990 Pro` | oratory1990 |
| `audio-technica-ath-m50x.txt` | `over-ear/Audio-Technica ATH-M50x` | oratory1990 |
| `philips-shp9500.txt` | `over-ear/Philips SHP9500` | oratory1990 |
| `meze-99-classics.txt` | `over-ear/Meze 99 Classics` | oratory1990 |
| `moondrop-blessing-2.txt` | `in-ear/Moondrop Blessing 2` | oratory1990 |
| `7hz-salnotes-zero.txt` | `in-ear/7Hz Salnotes Zero` | oratory1990 |

Each file was fetched from
`https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results/oratory1990/<directory>/<model> ParametricEQ.txt`
and is named here after the model it corrects.

## Format

```
Preamp: -6.3 dB
Filter 1: ON LSC Fc 105 Hz Gain 6.5 dB Q 0.70
Filter 2: ON PK Fc 125 Hz Gain -2.7 dB Q 0.55
```

`LSC` is a low shelf, `HSC` a high shelf, `PK` a peaking band. All sixteen files use only those
three, which are three of the six biquads this app's engine implements. `autoeq.rs` in
`src-tauri/src/` parses this format; nothing is transcribed by hand.

## Terms

* **AutoEq code:** MIT License, Copyright (c) 2018-2022 Jaakko Pasanen. The licence text is at
  `https://github.com/jaakkopasanen/AutoEq/blob/master/LICENSE`.
* **The result files:** the AutoEq repository states no separate licence for
  `results/**/ParametricEQ.txt`. Its root MIT licence covers the software; whether every result
  file is covered by it is not stated, and the measurements behind them come from third parties
  with their own terms (the AutoEq wiki notes it uses oratory1990's *measurements*, not his
  equaliser profiles).
* **oratory1990's own presets** are a different artifact and carry a restriction on unlicensed
  commercial use. None of the files here is one of those presets.

This repository ships these files verbatim with attribution for its own local use. Before
distributing AUDIORIUM commercially, the terms for the result files and the underlying
measurements need to be confirmed, or the set replaced with data whose licence is explicit. That
is recorded again in `docs/DEPENDENCIES_AND_LICENSES.md`.

## The two Moondrop LAN 2 sets

`moondrop-lan-2-ref.txt` and `moondrop-lan-2-pop.txt` are **not** in the table above and are not
AutoEq files. AutoEq has no LAN 2 profile, so these two are fitted here, by
`tools/fit_correction.py`, from the published squig.link measurement of both tunings:

| Local file | Measurement | Target |
| --- | --- | --- |
| `moondrop-lan-2-ref.txt` | Super\* Review, `Moondrop Lan 2 ref` L/R on squig.link | Harman IE 2019 |
| `moondrop-lan-2-pop.txt` | Super\* Review, `Moondrop Lan 2 pop` L/R on squig.link | Harman IE 2019 |

The measurement is a REW log-sweep export; the fit averages the two channels, smooths to 1/12
of an octave, takes target minus measured, and centres that difference at 1 kHz so no band carries
the overall level. It is a low shelf, a high shelf and eight peaking bands — the same biquads this
app runs — refined one band at a time, with gains quantised to 0.5 dB as the engine's files are.
Both files are the output of `tools/fit_correction.py` run on the two exports named above:

```
python tools/fit_correction.py <model>-ref-L.txt <model>-ref-R.txt <harman-ie-2019.txt> \
  "Moondrop Lan 2 (Ref)" src-tauri/assets/corrections/moondrop-lan-2-ref.txt
```

The left and right exports are the two files at the URLs `autoeq.rs` records for each entry. The
target is the Harman IE 2019 curve in the same (frequency, dB) form; it is not vendored here,
because it is a published target rather than this app's data.

The preamp is the fit's own computed peak, not a borrowed figure: −3.0 dB for Ref, −2.5 dB for Pop.
The app states the two halves of it in the evidence row — `AUDIORIUM fit · measurement by Super*
Review · in-ear`, followed by the measurement URL and `target Harman IE 2019` — and in `autoeq.rs`
as a `Basis` on the entry, so a fit is never read as a vendored AutoEq calculation.

What the fit cannot claim:

* It is a fit to a *published* measurement of one pair on one rig, not a measurement of your pair.
  Insertion depth, tips and unit spread move an IEM more than they move an over-ear.
* Above 9 kHz the bands stop chasing the curve: that region is dominated by the rig and the
  insertion, so only the high shelf's overall tilt is fitted there.
* The measurement belongs to its publisher (Super\* Review / squig.link), whose database terms
  apply; the filter set fitted from it is this app's own work, not AutoEq's and not oratory1990's.

## What a correction cannot promise

A correction is computed from one measurement of one unit, usually on one rig, with one set of
pads at one wear level. Manufacturing spread, pad wear, seal, insertion depth (for an IEM) and
your own ears all change the result. It is a better starting point than a guess, not a guarantee,
and it is not a claim about your particular pair.
