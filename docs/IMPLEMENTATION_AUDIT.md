# AUDIORIUM implementation audit

Written against the repository as it stands at the start of this pass, then
updated with what the pass actually changed. Every statement below was checked in
the source; nothing here is aspirational.

## 1. What the repository is

| Layer | What is present | Evidence |
| --- | --- | --- |
| Desktop shell | Tauri 2, one undecorated window, 1440x900, minimum 760x600 | `src-tauri/tauri.conf.json` |
| Backend | Rust 2021, single binary crate, no workspace | `src-tauri/Cargo.toml` |
| Frontend | React 19, TypeScript, Vite 8, vitest | `package.json` |
| 3D | three 0.186 + @react-three/fiber + drei, hand-written shader/geometry for the observatory | `src/components/OrbitScene.tsx` |
| Styling | One stylesheet, custom properties, no UI framework | `src/styles.css` |
| Icons | lucide-react | `package.json` |
| Persistence | One settings file (chain + saved equaliser patches) in `%APPDATA%\AUDIORIUM\state.json`. No database yet | `src-tauri/src/store.rs` |

There is no git history in this checkout, so no release or change history could
be reconstructed.

## 2. What is already real

**Playback.** `src-tauri/src/player.rs` decodes with Symphonia (MP3, FLAC, WAV,
PCM, Ogg/Vorbis, MP4/AAC, ALAC) on its own thread into a bounded ring buffer and
renders to a WASAPI shared polling stream at 48 kHz stereo f32. Sources at other
rates are converted with cubic Hermite interpolation and a second-order
Butterworth pre-filter when reducing the rate. Transport covers play, pause,
resume, stop, seek, previous/next, volume, queue position and error reporting
(`playback_status`).

**Analysis.** `src-tauri/src/analysis.rs` measures the stream the player writes:
every frame handed to the device also goes to a `Meter`, which accumulates RMS,
sample peak, three one-pole energy bands, each channel's energy with their cross
term, and a 2048-sample ring. Every 16 ms it publishes a `Readings` value into
`AnalysisState` — a 56-band spectrum from a
2048-point Hann FFT, 30 Hz to 16 kHz, -84 dBFS floor, plus the inter-channel
correlation, the L/R balance and the window's own mid/side pairs for the goniometer —
and stopping playback
publishes the idle value, so no reading outlives the sound that produced it.
There is no capture of the Windows mix and no second monitor output; the
interface polls `analysis_status` and nothing else measures audio. The one other
publisher is `capture.rs`, which reads the single input endpoint chosen on the
Devices page in shared mode and meters it with the same `Meter`; `AnalysisState`
named an owner, so only one of the two sources is on the panels at a time.
`src-tauri/src/audio.rs` keeps only the wiring: output enumeration, the spatial
configuration and the chain, all shared with the player.

**DSP.** Four stages, all native Rust, all in the playback path through
`ChainState`:

| Stage | File | What it really does |
| --- | --- | --- |
| Spatial renderer | `spatial.rs`, `spatial_out.rs` | Builds a virtual speaker layout from the stereo mix by matrix; renders it back to two channels with per-speaker level, interaural delay and head shadow. The Atmos position instead derives twelve mono feeds from that layout and hands them to Windows Spatial Sound as a 7.1.4 static bed (`spatial_out.rs`), which renders it with the format the user selected — Dolby Atmos when that is the format. Not an HRTF, not an Atmos bitstream decoder; with spatial sound off, the track uses the app's derived heights. |
| Binaural panner | `panner.rs` | Spherical-head placement: azimuth, elevation, width, a one-pole head shadow, up to 0.7 ms ITD, four early reflections with per-material damping. |
| Reverb | `reverb.rs` | Eight delay lines, two diffusing allpasses, seeded prime lengths, three routings (`parallel`, `series`, `woven`). A resonant network, not a convolution reverb. |
| Limiter | `limiter.rs` | 1.5 ms look-ahead peak limiter, -0.3 dBFS ceiling, three release speeds, mid/left/right detector, optional soft clip. Sample peak, not true peak. |

**Interface.** Six workspaces (Observatory, Spatial field, Analysis, Signal
chain, Library, Devices), a contextual reference stack, a processor rack dialog,
a persistent player bar, three themes, a reduced-motion setting stored in
`localStorage`, and a fit regime verified by browser gates at eight widths.

**Library.** `library.rs` walks a folder, reads ID3v2/ID3v1 (MP3) and Vorbis
comments (FLAC), falls back to the file name, bounds the scan and reports what it
skipped. Nothing is copied, uploaded or modified.

## 3. What was missing, and what this pass did about it

| Gap | Status after this pass |
| --- | --- |
| No equaliser of any kind: no biquad, no EQ curve, therefore no headphone correction | **Implemented.** `src-tauri/src/eq.rs`: six RBJ biquad kinds, 24 bands, preamp, response curve from the running coefficients, headroom report. In the real path, before the limiter. |
| No filter-response view | **Implemented.** `eq_response` returns the cascade's own magnitude response; the plot draws exactly that array. |
| No DSP page that could hold a graph and a band list | **Implemented.** New `Equaliser` workspace with a log-frequency plot, draggable band handles, band table and editor. |
| No loudness measurement (LUFS, LRA), no true peak | **Not implemented.** Metering is RMS and sample peak. |
| No offline file analysis, no spectrogram | **Not implemented.** The Analysis page measures playback as it happens. |
| No blind listening lab, no audio test bench | **Not implemented.** |
| No room or speaker measurement import | **Not implemented.** |
| No headphone profile store, no AutoEq import | **Partially implemented.** Fourteen AutoEq corrections from oratory1990 measurements are vendored and parsed with their provenance, plus two Moondrop LAN 2 profiles fitted here from a published squig.link measurement; saved patches persist, and a correction forks into a saved copy on edit. A file importer and a per-device mapping do not exist yet. |
| No persistence of settings or profiles | **Implemented for the processing chain.** `store.rs` keeps the panner, reverb, equaliser, limiter and the patch library, and the app reads them at startup. The chosen output device, the playback level, the spatial field and the playback queue are still in memory only. |
| No output-mode switching, no exclusive mode, no buffer control | **Not implemented.** The stream is `PollingShared`. |
| Library has no database, playlists, favourites or duplicate detection | **Not implemented.** |

## 4. Architecture boundaries that were respected

The equaliser follows the rule the other stages set: configuration lives in Rust,
the realtime path reads it, the interface only ever displays what the engine
accepted. `ChainState::update` validates before replacing, the rejected value is
never shown as if it were running, and the band list is sent as one atomic
replacement so the UI cannot describe a cascade the audio path is not running.

## 5. Risks carried forward

1. **Persistence is partial.** The processing chain and its patches survive a
   restart; the output device, the playback level, the spatial field and the
   playback queue do not. There is no backup or restore path beyond copying the file.
2. **No loudness metering.** The app deliberately says "dBFS · not LUFS" rather
   than offering a number that would be wrong.
3. **Headphone correction needs data.** The EQ can express a correction and the
   store can keep it, but no profile can be imported yet; nothing may be invented
   in the meantime.
4. **Shared-mode only.** Bit-perfect output is not claimed and cannot be
   verified from inside a shared WASAPI stream.
5. **One rate.** Everything renders at 48 kHz; higher-rate sources are converted
   rather than allowed to switch the stream.
