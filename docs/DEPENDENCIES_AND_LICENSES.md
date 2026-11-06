# Dependencies, licenses and integration decisions

Two rules were applied throughout: nothing is added that the repository does not
already need, and nothing is claimed to be integrated that is not running in the
audio path.

## 1. Rust

| Crate | Version | License | What it does here |
| --- | --- | --- | --- |
| `tauri` / `tauri-build` | 2 | MIT OR Apache-2.0 | Application shell, IPC, window |
| `serde` | 1.0.229 | MIT OR Apache-2.0 | Configuration and status serialisation |
| `symphonia` | 0.5.4 | MPL-2.0 | Decoding, with the `mp3`, `flac`, `wav`, `pcm`, `ogg`, `vorbis`, `isomp4`, `aac`, `alac` features |
| `wasapi` | 0.26.0 (Windows only) | MIT | WASAPI render client and device enumeration |
| `windows` | 0.62 (Windows only) | MIT OR Apache-2.0 | `ISpatialAudioClient`, static spatial objects and the `PROPVARIANT` activation blob for the Atmos output |

`serde`, `symphonia`, `wasapi` and `windows` are the whole dependency set: the DSP is
written in this repository and uses no external filter, FFT or metering crate. The
Atmos path uses Microsoft's platform renderer through `windows`; it does not bundle
Dolby code or decode an Atmos bitstream.

MPL-2.0 is file-level copyleft. Symphonia is used unmodified as a linked library;
the files of this repository are not derived from it, so no source disclosure is
triggered. The licence text ships with the crate.

## 2. Frontend

| Package | Version | License | What it does here |
| --- | --- | --- | --- |
| `react`, `react-dom` | 19.3 | MIT | Interface |
| `three` | 0.186 | MIT | Observatory geometry and shaders |
| `@react-three/fiber` | 9.8 | MIT | React renderer for three |
| `@react-three/drei` | 10.7 | MIT | Scene helpers |
| `@tauri-apps/api` | 2.12 | MIT OR Apache-2.0 | `invoke`, window controls |
| `lucide-react` | 1.62 | ISC | Icons |
| `vite`, `vitest`, `typescript`, `@vitejs/plugin-react` | current | MIT | Build and test only, not shipped |

Fonts are self-hosted and both are under the SIL Open Font License 1.1, with the
licence files kept beside them: Inter (`src/assets/fonts/OFL-Inter.txt`) and IBM
Plex Mono (`src/assets/fonts/OFL-IBM-Plex-Mono.txt`).

## 3. The named projects, evaluated

### AutoEq (headphone correction)

**Decision: fourteen published profiles are bundled, unmodified, with their terms
stated in the app, plus two profiles this app fitted from a published measurement
(squig.link's, for the Moondrop LAN 2 tunings, which AutoEq has no profile for). A
user-facing importer is not built yet.**
AutoEq is a corpus of measurement-derived corrections plus tooling; it is not a
DSP backend. What it needed from AUDIORIUM was a parametric equaliser that can run
a 10-band peaking/shelf cascade at 48 kHz — that exists (`eq.rs`, 24 bands,
peaking/shelf/pass/notch, preamp, response curve).

The files live in `src-tauri/assets/corrections/`, one per model, fetched from
`results/oratory1990` and stored byte for byte; `src-tauri/assets/corrections/SOURCES.md`
lists every file, its AutoEq directory and the date it was fetched, and `autoeq.rs` parses
the published format rather than transcribing numbers by hand. Each patch carries a
`Provenance` record — label, file URL and terms — which the equaliser's picker shows above
the plot, so the interface can never present measured data as something written here.

Terms: AutoEq's code is MIT (Copyright (c) 2018-2022 Jaakko Pasanen). The repository states
no separate licence for the `results/**/ParametricEQ.txt` files, and the measurements behind
them come from third parties with their own terms; oratory1990's own published presets are a
different artifact and carry a restriction on unlicensed commercial use, which is one reason
none of them is bundled. **Before distributing AUDIORIUM commercially, the terms for the
result files and the underlying measurements must be confirmed or the set replaced** with data
whose licence is explicit. That is also recorded in `SOURCES.md` beside the files.

The remaining work is the importer: a file the user brings goes through the same parser, the
same validation and the same headroom check as the bundled ones.

### CamillaDSP

**Decision: not adopted. The chains are native Rust.**
CamillaDSP is a complete, separate audio engine built around its own configuration
and its own device handling. Embedding it as a library is not a small integration,
and running it as a second process would duplicate what this repository already
does (own capture, own render, own device selection) and put two engines in front
of one output. The requirement it would have met — a configurable filter pipeline
— is now met by `chain.rs` + `eq.rs`, with the same honesty about what each stage
is. If a future requirement needs convolution or VST hosting, this decision should
be revisited rather than assumed.

### libebur128

**Decision: not integrated.** There is no loudness measurement in the app, and
none is displayed. Every meter is labelled dBFS, and the reference panel says
"dBFS · not LUFS" rather than offering a number that would be wrong. Integrating
BS.1770 loudness is a separate piece of work that needs oversampled true peak to
be honest about dBTP, and it is not faked in the meantime.

### FFmpeg

**Decision: not added.** Symphonia already decodes the formats this app opens
(MP3, FLAC, WAV, Ogg/Vorbis, MP4/AAC, ALAC). Adding FFmpeg would add a large
binary, a codec-licensing surface and a security update obligation for formats the
library can already read. No external process is spawned anywhere in the
application, so there is no shell-injection surface either.

### REW (Room EQ Wizard)

**Decision: not integrated.** No measurement import, no measurement file format,
no calibration data. The Spatial field shows the virtual layout the renderer is
using and says so; it does not present a room measurement it does not have.

## 4. Distribution notes

* The app bundles an NSIS installer for the current user alongside the portable
  executable. The build is unsigned until a code-signing certificate is
  configured, so a public download warns until it is signed.
* No GPL component is linked, so the binary is not forced under a copyleft
  licence by a dependency.
* WASAPI is used through the `wasapi` crate's safe bindings; the Windows SDK is
  required to build, not redistributed.
* If a profile dataset (AutoEq or a measurement set) is added later, its licence
  and attribution must be recorded here and shown in the interface beside the
  profile it produced.
