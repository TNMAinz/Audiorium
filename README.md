# Audiorium

A music player and a software instrument for Windows, both of which show you what
they are doing to the sound.

Audiorium decodes the files on your disk and plays them through a spatial renderer
and a rack of six processors — a binaural panner, a reverb, a 24-band parametric
equaliser, a compressor, a saturator and a look-ahead limiter — in an order you set
and can change while the music plays. Its meters and its spectrum are computed from
real audio — the frames it hands to the sound card, or a source you point it at — so
a change to the chain shows up in what you see as well as in what you hear.

The same rack is the effects section of a sixteen-voice wavetable instrument on the
Synth page, and it runs over a whole file offline on the Master page: there it
measures the loudness before and after, checks the result against a delivery target,
and writes a WAV or FLAC.

It runs on one machine and reads one folder. There is no account, no telemetry and
no network code anywhere in the app. The only files it writes are its own settings,
a log, the takes and masters you ask it to render, and the patches you save.

![The observatory, with a track playing](screenshots/observatory.png)

## Playing your files

The Library page walks a folder you choose, reads tags from the files, lists what
it finds and plays the one you pick. MP3, FLAC, WAV, ALAC, AAC/M4A and Ogg Vorbis are decoded in Rust
with Symphonia; AIFF and Opus are refused with the decoder's own message rather
than guessed at. Each track is decoded on its own thread, resampled to the
player's own 48 kHz stereo float stream and written to a WASAPI output in shared
mode, with the chain processing every frame on the way.

The transport does what it says: play, pause, resume, stop, seek, previous and
next, with the position taken from the audio that actually reached the device,
plus a shuffle and a repeat mode (off, all, one) that only act when a track ends
or you press next. Closing the window asks whether to quit or to keep playing
from the tray, and the tray icon brings it back.
Playback can be moved to another output while a track is running; the stream is
reopened at the position already heard. The chosen output is remembered, and if
it is missing at launch the Windows default is used until it returns.

## The chain

Every frame meets the spatial renderer, then walks a rack of six processors. The
order is a list the engine runs, not a drawing of one: each strip on the Processors
page has arrows that move it earlier or later, and moving a stage changes what the
audio does. The rack can also run on left and right or on mid and side, where the
processors work on the sum and the difference, so an equaliser can shape the centre
of a mix without moving its sides. Any stage can be taken out of the path, and a
bypassed stage is bit-exact: the samples come back out unchanged.

The spatial renderer sits outside the rack because it is the speaker layout rather
than a processor. A master renders the rack alone.

1. **Spatial renderer.** Six modes. Stereo passes through untouched; 5.1 and 7.1
   place a stereo mix on virtual speakers; binaural applies head shadowing,
   interaural delay and crossfeed; Manual places two speakers by hand with an
   optional height pair; Atmos derives a 7.1.4 bed and hands it to Windows
   Spatial Sound, which renders it with whatever format is selected in Windows
   sound settings.
2. **Binaural panner.** A spherical-head model with azimuth, elevation, width,
   focus and four timed early reflections in five material colours. It is not a
   measured HRTF and it has no tail of its own.
3. **Reverb.** Eight prime-length delay lines that feed each other, joined in one
   of three routings, with separate dry and wet levels and a decay that reads as
   reverb time from a fifth of a second to sixteen. New room rolls a network and
   keeps it, so the same room comes back after a restart.
4. **Parametric equaliser.** Up to 24 RBJ biquads — peaking, low shelf, high
   shelf, low pass, high pass and notch — behind a preamp, with the response
   curve drawn from the coefficients the audio path is actually running. A band
   change is crossfaded over 10 ms so it does not click, and the page reports the
   headroom the curve leaves. A band can also be made dynamic: while that band's
   own level is above its threshold it is pulled back towards a straight wire, so
   a quiet verse keeps a boost that a loud chorus does not get doubled.
5. **Compressor.** Stereo-linked and feed-forward: the louder channel decides, so
   one loud side ducks the whole image. An over-easy knee at the threshold, a
   ratio, attack and release (each calibrated to reach 90 % of its change in the
   stated time) and make-up gain after the reduction. It is not a limiter and
   holds no ceiling — nothing is delayed, so a fast attack shaves a transient
   instead of catching it in advance. That is why the limiter comes last.
6. **Saturator.** A tanh curve run at four times the sample rate and split back
   down through a windowed-sinc filter, so the harmonics it adds land below the
   band instead of folding back as aliasing. Drive is input gain into the curve,
   normalised so a full-scale sample leaves at full scale; Tone tilts the result
   dark or bright from 2.5 kHz; Mix blends it against the dry signal, delay-matched
   so a half mix does not comb. It is not an amp sim and not tape: no wow, no bias.
7. **Limiter.** A look-ahead peak limiter: 72 samples of look-ahead, an output
   level of −0.3 dBFS by default, fast/medium/slow release, both/left/right
   detection, optional soft clip and automatic make-up that normalises the
   limited peak to the output level. The gain works from the threshold and the
   output is one trim after it, so turning the output down is quieter rather than
   more limited. It is not a true-peak limiter and not a loudness normaliser.

Five of the six are edited on the Processors page, which names what each stage is
holding, orders them and meters the compressor and the limiter from the engine. The
equaliser keeps a page of its own, and the Signal chain page draws the whole path
in the order the audio meets it.

![The Processors page, with all six stages in the path](screenshots/processors.png)

## Equaliser patches

The equaliser workspace ships three kinds of patch and keeps them apart, because
they rest on different evidence.

- **Shapes** are ten starting points written for this app: Flat, Bass shelf,
  Treble trim, Presence, Rumble filter, Vocal, Warm tilt, Bright tilt,
  Butterworth 30 Hz and Quiet level. Each is nine peaking bands on the octave
  centres at one-octave Q, so a shape can be bent band by band.
- **Corrections** are fourteen headphone profiles computed by
  [AutoEq](https://github.com/jaakkopasanen/AutoEq) from oratory1990's
  measurements, vendored unmodified with their model, source directory and fetch
  date recorded in `src-tauri/assets/corrections/SOURCES.md`.
- **Fits** are the two Moondrop LAN 2 tunings, which AutoEq has no profile for.
  They were fitted here from a published squig.link measurement against the
  Harman in-ear target, and the app says so where the picker names them.

Editing a shipped patch does not overwrite it: the engine saves a copy, makes
that copy active and follows it from then on. All of it survives a restart.

![The equaliser workspace](screenshots/equaliser.png)

## The instrument

The Synth page is a second source with a clock of its own. It does not read a file
and does not share the player's stream: a file is read at whatever rate the device
asks for, while an instrument has to be paced by the device and given a note before
the next buffer is due. Its voices go through the same rack, so the processors are
its effects section, and a stage switched out of the path is out of the instrument
too.

- **Sixteen voices, two oscillators each.** PolyBLEP saw and square, triangle and
  sine, a wavetable oscillator, a sub an octave down and noise, with up to eight
  detuned copies per oscillator spread across the image. The stack is divided by
  its own count and each copy keeps its own phase, so eight copies are wider
  rather than eight times as loud.
- **Wavetables, including your own.** Four ship — Basic, Formant, Bell and Vocal.
  **Import table** decodes a file with the player's own decoder, cuts it into
  2 048-sample cycles, analyses each cycle's first 128 harmonics and keeps the
  result as harmonic levels, so a table plays band-limited at every pitch instead
  of aliasing near the top of the keyboard.
- **Filters with drive.** Low, band and high pass, notch and peak from one
  state-variable topology, a ladder that saturates inside its own feedback at
  twice the rate, and a comb whose cutoff is heard as a note. Each model keeps its
  own state, so switching models mid-note cannot inherit another one's tail.
- **Envelopes, LFOs and a matrix.** Two ADSRs, two LFOs, envelope and key tracking
  on the filter, glide, and a modulation matrix of eleven sources — both
  envelopes, both LFOs, velocity, key, the wheel, per-note pressure and four
  macros — into eight destinations, with a curve on every route that keeps its
  sign. Routes to one destination sum.
- **Tunings.** Equal, just major, Pythagorean, meantone, 19-, 22-, 31- and 53-EDO,
  or a Scala `.scl` file. The offset is applied to the key itself, before the
  glide, so a scale is the key's pitch rather than an effect after it.
- **Three ways to play it.** The page's keyboard — three octaves, where a drag
  across the keys plays every key it crosses and two fingers can hold two notes —
  the QWERTY row with Z and X moving it an octave, or MIDI: note on and off, pitch
  bend, CC 1, all-notes-off, polyphonic aftertouch and channel pressure. An MPE
  controller's member channels bend and press their own note rather than the whole
  keyboard.
- **A take, and a bounce.** Notes are timestamped on the audio thread and rebased
  to the first one; a bounce renders the same engine offline over the take, runs
  the rack over it and writes 48 kHz 32-bit float WAV into `Synth` in the library,
  where the Library and Master pages can pick it up.
- **The display is the output.** The scope is what the stream thread handed the
  device, peak-held into 96 columns of a 48 ms window. A stopped instrument draws
  the patch instead, from a port of the same oscillator and filter maths, and says
  so.
- **Its own output stream.** 48 kHz, 32-bit float, stereo, on the endpoint the
  Devices page names. Choosing another one while it runs reopens the stream there,
  and the notes being held end with the old stream because nothing is carrying
  them any more.
- **Patches.** Eight factory patches and your own, saved with the patch book in
  `%APPDATA%\AUDIORIUM\synth.json`, which is a separate file from the chain's
  settings so neither part of the app rewrites the other's.

![The Synth page with a chord held](screenshots/synth.png)

## Mastering

The Master page renders the rack over a whole file, offline and on its own thread
with its own copy of the chain: playback keeps playing while a render runs, and
neither can move the other's state. The spatial layout is left out, so a master is
the rack's work rather than the mode the speakers happened to be in.

Output is WAV or FLAC at 44.1, 48 or 96 kHz, resampled when the file's own rate
differs. WAV carries 16- or 24-bit PCM, dithered and noise-shaped at 16 bits, or
32-bit float with no dither; FLAC has no float sample, so choosing it leaves 16 and
24 bit and packs the same sample codes losslessly, with the sample digest a FLAC
stream states about itself. A master keeps the source's tags and its cover. Prints
go to a `Mastered` folder at the library root with `(mastered)` in the name, or
beside a file that came from outside the library; the source is never overwritten,
and the library scan skips that folder so a print never comes back as a new track.

The before/after figures come from the same pass as the audio, one meter before the
rack and one after, and they are BS.1770-4 rather than an approximation: gated
integrated loudness, loudness range, 4× oversampled true peak, sample peak, DC
offset and channel correlation. A delivery target — streaming at −14 LUFS, podcast
at −16, broadcast at −23, or none — is judged within half a unit and under the
ceiling. It is a check and never a normaliser: the gain that would close a gap is
stated and not applied. After a render the page can play the source and the master
back to back at matched loudness, both without the rack, and measure a reference
track to compare a master against. Two further cards read the whole file — its
envelope, loudness and spectrum — and run a bypass test that renders with every
stage out and says whether the result is sample-identical to the source.

![The Master page after a render](screenshots/master.png)

## Measurement

The Analysis page draws a 2048-sample Hann FFT folded into 56 logarithmic bands
from 30 Hz to 16 kHz, the RMS and sample peak in dBFS, low/mid/high energy, a
goniometer with the window's correlation, and a rolling level trace. The
spectrum is drawn as a surface: every frame from the last half second sits behind
the live curve, so you can see where the music has just been. Pausing freezes the
readings where they are; stopping fades them out.

Everything is labelled for what it is. The levels on this page are dBFS, and the
app says so where it shows them; programme loudness in LUFS is measured over
blocks, on the Master page. An idle engine reports a dash instead of inventing a
number, and the observatory settles back to a plain sphere.

![The analysis workspace while a tone plays](screenshots/analysis.png)

## Listening to something else

The Devices page can capture a single source — any input endpoint, or the
loopback of any output, which is how an application playing on its own device
still reaches these meters — and send it to the monitors through the same chain,
on an output you choose. Capturing the device you are monitoring into is refused
with the reason shown. Nothing is recorded to disk and nothing from the Windows
mix is captured wholesale. The instrument plays to the output chosen here as
well, so one choice on this page is one output for the whole app.

The optional virtual audio device that can give Audiorium its own endpoints —
so any application could pick **Speakers (Audiorium)** as its output — is a
kernel driver and is not part of this repository. The app works without it, and
the Devices page still reports whether a build of the device is installed and
running on the machine.

## Building it

No binaries are published here; the app is built from this source. You need, on
Windows 10 or 11:

- Node.js 24 and npm
- a stable Rust toolchain with the MSVC target
- Visual Studio 2022 C++ build tools and the Windows SDK
- the WebView2 runtime, which ships with Windows 10 and 11

```sh
npm ci
npm run tauri dev
```

`npm run dev` alone serves the frontend on `127.0.0.1:1420` in a browser, which
is useful for layout work; the native features report themselves as unavailable
there. `npm run build` type-checks strictly and produces the frontend bundle.
`npm run tauri build` produces `src-tauri/target/release/audiorium.exe` and an
NSIS installer under `src-tauri/target/release/bundle/nsis/` that installs for
the current user and needs no administrator.

## Tests

```sh
npm test                                        # frontend, Vitest
cargo test --manifest-path src-tauri/Cargo.toml # Rust, including all the DSP
```

The Rust suite covers the parts where being wrong is not visible on screen: the
limiter holding its ceiling, the compressor's knee and release, the saturator's
oversampling, the panner's symmetry, the reverb's seeded reproducibility, the
equaliser's measured response, bit-exact bypass for every stage, and a master that
comes back sample-identical when the rack is out. On the instrument it counts what
playing a note produces — A4 at 440 Hz, an octave doubling, a bend landing where it
should, forty note-ons never exceeding sixteen voices, the same take rendering the
same frames twice, every tuning naming 128 finite cents — and that each filter
model stays finite at full resonance. The frontend suite covers the scene's
geometry, the readings it derives from the engine, the delivery-target check, the
synthesizer's own voice maths and keyboard, and the workspace components.

## Where your data goes

| Path | What is there |
| --- | --- |
| `%APPDATA%\AUDIORIUM\state.json` | the chain settings, the rack order and every saved equaliser patch |
| `%APPDATA%\AUDIORIUM\synth.json` | the instrument's patch, its active preset and the patches you saved |
| `%APPDATA%\AUDIORIUM\tables.json` | imported wavetables, as harmonic levels |
| `%APPDATA%\AUDIORIUM\window.json` | where the window was left |
| `%APPDATA%\AUDIORIUM\audiorium.log` | the launch line and any panic, capped at 256 KB |

The library folder, the chosen devices, the playback level, the spatial settings,
the theme and the reduced-motion preference live in the WebView's own local
storage. Deleting
`%APPDATA%\AUDIORIUM` clears the chain and every saved patch, the instrument's
patches and imported tables. `AUDIORIUM_CONFIG_DIR`
and `AUDIORIUM_LOG_PATH` move the folder and the log, which is how the tests keep
their own.

A master is written next to the music rather than in that folder: `Mastered` at the
library root, or beside a file that was never in the library. A bounced take goes
to `Synth` in the library, where the scan picks it up as a track.

## Layout

```
src/                          React frontend: workspaces, instruments, scene
src-tauri/src/                Rust: decoder, spatial renderer, six processors,
                              synth engine and MIDI, analysis, programme
                              loudness, offline render and export, capture,
                              devices, library, settings
src-tauri/assets/corrections/ published headphone profiles and their sources
tools/                        the scripts that fitted the two LAN 2 profiles
docs/                         audio architecture, dependency and license record,
                              feature status, implementation audit
```

## What it does not do

- It is Windows-only. The audio path, the device dialogs and the spatial output
  are Windows APIs, and no other platform has been attempted.
- Playback is one shared-mode stream at 48 kHz, so nothing is bit-perfect, nothing
  uses exclusive mode, and high-resolution files are resampled. A master is
  written at the rate you pick, but it is rendered offline rather than played back
  bit-perfect.
- The queue is the scanned folder order held in memory: no playlists, no gapless
  playback, no resume point and no play counts.
- There is no convolution, no VST hosting, no multiband compression and no
  spectrogram of a live stream. The compressor is single-band, the saturator is
  not an amplifier model, and nothing here is a true-peak limiter: the limiter
  holds sample peaks, so a master's true peak can sit above what it held, which is
  exactly what the Master page measures.
- The instrument has no sequencer, no arpeggiator, no plugin hosting and no
  multisampling, and there is no editor for a wavetable's frames or a spectral
  warp. Its MIDI path is covered by message tests and has not been played on a
  controller here, so MPE has not been tried against hardware.
- A delivery target is a check, not a normaliser. Nothing applies loudness
  normalisation or a limiter for you on the way out.
- The Atmos mode needs spatial sound enabled for the selected output in Windows.
  A stereo file carries no objects, so the bed is derived, and no Atmos bitstream
  is decoded anywhere.
- The installer is unsigned. Until a certificate is configured, public downloads
  warn about an unknown publisher, and SmartScreen reputation builds separately
  from the signature.
- Window dragging, keyboard navigation, long runs and slow machines have not been
  tested as thoroughly as the DSP has. `docs/FEATURE_STATUS.md` records the state
  of each feature and what was actually observed.

## Third-party material

The code is MIT. Bundled material is not all covered by that, and the difference
matters:

- The fourteen AutoEq profiles were fetched from AutoEq's `results/oratory1990`
  directory. AutoEq's code is MIT, but no separate license is stated for those
  result files, and the measurements behind them belong to third parties. **Their
  terms for redistribution are unconfirmed**, which `src-tauri/assets/corrections/SOURCES.md`
  and `docs/DEPENDENCIES_AND_LICENSES.md` both record. Treat this as a question
  to settle before any commercial use.
- Inter and IBM Plex Mono are self-hosted under the SIL Open Font License 1.1,
  with both license files beside the fonts in `src/assets/fonts/`.
- The FLAC encoder a master is written with is `flacenc`, Apache-2.0, linked
  unmodified. Every other Rust and JavaScript dependency, its version, license and
  role in the app is listed in `docs/DEPENDENCIES_AND_LICENSES.md`.

## License

[MIT](LICENSE). Do what you like with the code, keep the notice.

## Contributing

Issues and pull requests are welcome. Two things make a patch easy to take: the
tests above still pass, and the app still tells the truth about what it is doing
— every reading it shows should come from the engine rather than from the
interface's imagination.
