# AUDIORIUM audio architecture

Everything in this document is how the code behaves today. Where a number is
given, it is the constant in the source.

## 1. One format, one source

Playback renders at **48 kHz, 32-bit float, stereo**. No stage changes the rate,
so the processing chain never has to be rebuilt for a new file, and the spectrum,
meters, orbit scene and processor state all refer to the same stream.

```
Library file (Symphonia)
        │  decode thread → one-second bounded ring
        ▼
┌─────────────────────────────────────────────────────────────┐
│ player.rs render stream                                     │
│  resample to 48 kHz → level → spatial → panner → reverb     │
│  → equaliser → limiter → WASAPI render                      │
│                        │                                    │
│                        └→ analysis::Meter (every frame)     │
└─────────────────────────────────────────────────────────────┘
                          ▼
                 analysis.rs readings
                 (RMS, peak, 3 bands, 56-band FFT)
                          ▼
              interface polls `analysis_status`
```

There is exactly one render path for a file. Nothing from the Windows mix is
folded into it, so a stage cannot process the same audio twice and the meters
cannot disagree with what the player wrote.

The one other source is the **input capture** (`capture.rs`): the single endpoint
chosen on the Devices page, opened in shared mode at the same 48 kHz float stereo
stream format, meter and all. That endpoint is either a capture endpoint or a
render endpoint opened as Windows' **loopback** of it — what that output is playing
— which is how another application's audio is read without a virtual device. It is not a tap on the mix and it does not feed
the playback path — it measures. `AnalysisState` names its owner, so the panels
show playback or the input and never an average of the two; a capture that is
started takes the panels from playback until it is stopped.

A capture belongs to the endpoint it opened. Choosing another device, another
output to listen on, or turning listening on or off asks for a new capture rather
than waiting for a Stop: the engine marks the running thread stopped and waits for
it to close its streams before the new one opens, so one device is never held by
two streams.

A capture can also be **heard** (`Monitor` in `capture.rs`). The capture thread only
queues the frames it read (bounded at 100 ms, oldest dropped); a thread of the
monitor's own owns the render stream and keeps the device fed, writing silence when
the queue is short and fading the first 10 ms in. A missing packet is therefore a
short gap rather than an underrun, which is what the clicks and bursts of static
were: writing a packet's frames only into whatever space the device happened to
report discarded the rest of the packet and left holes in the stream. The queue is
run through a `Chain` built from the same `Arc<ChainState>` and
`Arc<Mutex<SpatialConfig>>` the player uses, and written to the output chosen on
the Devices page. That is the only second render stream in the app, it exists only
while a listened capture runs, and the panels keep measuring the raw source rather
than the monitor's version of it. A capture of an output played back into **that
same** output is refused: the gain around the ring is one. The refusal is reported
on the route card while the capture keeps metering, because a route that rings is
worse than a route that is quiet.

The player builds `Chain` from the **shared** `Arc<ChainState>` and the same
`Arc<Mutex<SpatialConfig>>`, so a stage switched on in the interface is switched
on in the stream the rack describes.

## 2. Threads and what may not happen on them

| Thread | Owns | Never does |
| --- | --- | --- |
| Playback decoder (`player.rs`) | Symphonia reader/decoder, resampler | touches the render stream directly; it fills a ring buffer and blocks when full |
| Playback render (`player.rs`) | WASAPI render client, `Chain`, ring consumer, `Meter` | allocates per frame, seeks by itself, touches the disk |
| Input capture (`capture.rs`) | WASAPI capture client, its own `Meter` and `Ballistics`, and the monitor's render client | touches playback's ring or its client, or publishes while playback owns the panels |
| Store writer (`store.rs`) | the settings document | runs on the render thread |
| Tauri command handlers | lock, validate, replace config | process audio |

Configuration crosses the thread boundary as a **cloned, validated value**,
never as a shared mutable reference: `ChainState::config()` returns a copy so the
render loop never holds a lock while processing, and `ChainState::update()` takes
a lock, clones, mutates, validates and only then replaces. A rejected value
leaves the previous chain running, and the interface re-reads `chain_config` to
show what is actually live.

`ChainState::effective()` returns an idle configuration when every stage is off,
so a bypassed chain costs one branch per frame and is bit-exact.

## 3. The processing chain, in order

### 3.1 Playback level

A linear level applied before the processors, smoothed per frame (0.002
coefficient) so a slider move cannot step. It is never written to the system
volume.

### 3.2 Spatial renderer (`spatial.rs`)

Builds a virtual layout for the selected mode (2.0, 5.1, 7.1, Atmos, binaural,
manual) from the stereo mix by matrix, then renders that layout to two channels
using per-speaker level, interaural time delay and a head-shadow filter. Stereo
mode is exactly transparent. Mode changes ramp instead of jumping.

Atmos is the exception: the spatial stage is a pass-through, and after the panner,
reverb, equaliser and limiter the render loop derives twelve mono feeds from the
processed stereo frame (`Bed`) and writes them to a Windows Spatial Sound stream
(`spatial_out.rs`) as a 7.1.4 static bed. Windows renders that bed with the spatial
format the user selected for the endpoint — Dolby Atmos for Headphones or Home
Theater when that is the chosen format, Windows Sonic otherwise. The app ships no
Dolby decoder and decodes no Atmos bitstream; the feeds are still derived from two
channels. When spatial sound is off for the output, or the stream cannot be opened,
the track uses the app's own derived 7.1.4 renderer (`DerivedHeights`) and reports
`spatial: "derived"` instead of pretending to be in Atmos.

This is not an HRTF: the app-rendered modes convey rear and height speakers by level
and spectral tilt, which is what a two-channel headphone render can honestly do.
The Atmos path's height and rear cues come from Windows' renderer, not from this
code.

### 3.3 Binaural panner (`panner.rs`)

Places one stereo image anywhere around the listener. Cues: ITD up to 0.7 ms, a
one-pole shadow on the far ear, an elevation tilt, and four early reflections
(7–23 ms, scaled by `reflectionLength`) with five materials. A material is two
numbers, not a recolour: a one-pole damping coefficient (how much of the tap's top
end survives, 0.14 Stone to 0.85 Glass) and an absorption gain (how much of the tap
comes back, 0.55 Wood to 1.85 Glass). Both are needed because the taps sit about
20 dB under the direct sound — a filter alone moved the mix by half a decibel at
the shipped reflection level, which is why choosing a material sounded like nothing.
`reflectionLevel` is tapered (`level^0.45`) for the same reason: 20% now arrives at
about half the tap gain while 100% is unchanged. `focus` trades externalisation
against tonal fidelity. No reverb tail is added here.

### 3.4 Reverb (`reverb.rs`)

A networked feedback-delay reverb in the Protoverb tradition, not a bank of
parallel combs: eight prime-length delay lines (181–4099 samples, 3.8–85 ms) in
one feedback loop, four diffusing allpasses on each input side, and a unitary
mixing matrix that hands every line's output to every other. The matrix is what
makes the tail dense instead of a row of separate echoes — the reason a comb bank
sounds metallic and this does not. The network string (`woven:4211`) is part of
the sound; the same string rebuilds the same reverb. The rack exposes it as three
routing diagrams plus one **New room** button rather than as a text field: the
string is the engine's identity for a room, not something the user has to type.

`decay` is a reverb time, not a feedback amount: each line's gain is solved from
its own length and the control's RT60, swept geometrically from 0.2 s to 16 s, and
the measured tail follows the knob within the spread the lines carry. Each loop
also carries air absorption (a one-pole low pass near 8 kHz, so the top dies
before the bottom) and a DC blocker near 5 Hz (so no standing offset outlives the
room). The tap is weighted by `sqrt(1 − g²)`, so a sixteen-second room is not a
quieter one than a short one. Dry and wet are separate gains, not a crossfade.

The three routings are different matrices in the same loop, not three mixes.
`parallel` is the Householder matrix (`x − 2/N · Σx`), orthogonal and uncoloured;
`woven` applies a perfect shuffle before the same matrix, spreading each line's
energy two places away; `series` is the cyclic permutation, where each line hands
its output to the next and the last back to the first. All three are unitary, so
the matrix never adds energy and the decay stays the one the gains ask for; all
three are level-matched to within a few dB at the same `decay` and `wet`.

### 3.5 Equaliser (`eq.rs`)

Six RBJ cookbook biquads — peaking, low shelf, high shelf, low pass, high pass,
notch — cascaded per channel, up to 24 bands, plus a preamp. Direct form II
transposed, so a gain change cannot make the filter numerically unstable.

* **Why here:** a headphone correction belongs next to the transducer, after the
  spatial renderer has produced the two channels that reach the ear, and before
  the limiter so the limiter can guard the headroom a boost takes.
* **Response:** `eq_response` evaluates the same coefficients the audio path runs
  at 160 log-spaced frequencies from 20 Hz to 20 kHz and also reports the largest
  boost and the resulting headroom (`preamp + max boost`). The plot has no other
  source.
* **Transitions:** a changed band list swaps in a new cascade and crossfades the
  old one out over 480 samples (10 ms), so dragging a band does not click.
  Switching the stage off is immediate, bit-exact pass-through, and clears the
  filter state so re-enabling cannot play a tail from before.
* **Limits enforced in Rust and mirrored in the UI:** frequency 20 Hz–20 kHz,
  gain ±24 dB, Q 0.1–18, preamp -36–0 dB, at most 24 bands. NaN and infinity are
  rejected.

### 3.6 Limiter (`limiter.rs`)

72-sample (1.5 ms) look-ahead: the peak of the detector signal over the window
decides the gain applied to the same audio after its matching delay. Output -0.3
dBFS by default, three release speeds, both/left/right detector, optional soft clip
and auto make-up. The gain always holds peaks at the threshold and Output is one
constant trim after it, so pulling the output down keeps the waveform's shape
instead of forcing the gain to chase a second, lower ceiling. The detector is the
loudest channel, not the mid sum: a mid detector reads a panned signal several dB
low and the gain it asks for leaves the louder channel to be clipped flat at the
ceiling, which is heard as grit. Sample peak only; nothing is oversampled, so
inter-sample peaks are not measured and "true peak" is never claimed.

### 3.7 What is stored (`store.rs`, `presets.rs`)

* The whole chain configuration and the saved patch library are one JSON document
  at `%APPDATA%\AUDIORIUM\state.json` (or `$AUDIORIUM_CONFIG_DIR/state.json`).
  It carries a `version` and defaults for every field, so a partial or older file
  still loads; a file whose chain no longer validates is replaced by the neutral
  chain rather than half-applied.
* Writing happens on a background thread that keeps only the newest document: a
  knob drag sends a value per pointer move, and one write replaces one temporary
  file by rename, so a crash cannot leave a truncated file in place.
* A patch is a preamp and a band list. The ten shipped shapes, the fourteen
  published corrections and the two fitted here are constants in `presets.rs` and `autoeq.rs` and are
  never written to the file, so nothing on disk can damage them. The first edit
  that changes a shipped patch's values saves a copy with a new id (`custom:n`)
  and a name like `Bass shelf (edited)`; later edits follow that copy. The id is
  the identity, so renaming never breaks the link.
* A restored session keeps its link to a patch only if the running bands are still
  that patch's bands. A shipped shape that was rebuilt between versions, or a copy
  edited in the file, therefore comes back as unsaved bands with the bands
  untouched, rather than as a picker that names a curve it is not playing.
* Nothing in the audio callback touches the disk, the store or the patch list.

## 4. Metering and telemetry

* The render loop hands **every frame it wrote to the device** to
  `analysis::Meter`, which keeps the window's RMS, sample peak, three energy
  bands, each channel's energy and their cross term, and a 2048-sample ring for
  the FFT. Every 16 ms it publishes a `Readings`
  value into `Arc<AnalysisState>`; stopping playback publishes the idle value, so
  no reading outlives the sound that produced it.
* The interface polls `analysis_status` every 20 ms and `chain_meters` every
  200 ms. No audio sample ever reaches React; no React render happens per sample.
  The engine publishes faster than the interface reads, so a poll nearly always
  carries a fresh measurement instead of the same frame twice: at 120 ms the
  curve moved eight times a second, which is what made it look like stepping.
  The trace on the Analysis page keeps 600 polls (12 s), and its span label is
  computed from the poll interval rather than hard-coded.
* The scene samples the same readings through a ref on its own animation frame,
  and is memoised, so a poll cannot re-render the WebGL tree for a picture that
  did not change. It smooths every value it uses (`1 - exp(-dt * rate)`) before
  applying it, so a poll's step never becomes a jump in the picture.
* The observatory shows that one measurement as one reading, and `sceneModes.ts`
  owns its shape as plain data: the geodesic shell (icosahedron subdivided twice,
  deduplicated edges). `OrbitScene` turns that array into buffers; it computes no
  geometry of its own, which is why the shape can be tested without a WebGL
  context. There is no mode switch, no strip or caption over the field, and no way
  to hide the rail: the object and the instrument stack are always on screen
  together, and the player bar reports what is playing.
* The engine's spectrum is the skin of one membrane orb, sitting between the core
  and the geodesic shell. Its 56 bands are written as one 56-byte row and sampled
  round the orb's longitude in the vertex shader, so the silhouette is the mix:
  each band pushes and pulls its own meridian, a neighbour sample tilts the normal
  so the waves are lit, and two slow waves keep the surface travelling. A quiet
  mix is compressed with `pow(band, 0.65)` so it still shows; a band of zero stays
  zero, and a stopped engine writes nothing and takes the displacement to zero, so
  the orb is a plain sphere rather than a frozen shape. Mid turns the shell and
  treble lifts the nodes; nothing else on the object changes size with the music,
  because a core that swells reads as decoration rather than a measurement. The
  phase plot is not drawn in the scene at all: it belongs to the Analysis page,
  where it has axes and numbers.
* Reduced motion is the scene's low-effects mode: the frameloop drops to `demand`,
  rotation stops, and a hidden document does the same.
* The analyser is a 2048-point real FFT with a Hann window, binned into 56
  log-spaced bands between 30 Hz and 16 kHz with a -84 dBFS floor. FFT magnitude
  is not calibrated SPL and is never labelled as such.
* The measurement is **after** the chain and the volume, so it describes the
  signal leaving the app. Turning up a band moves the curve; engaging the limiter
  shows up in the peak.
* Phase is measured, not assumed. The same window keeps the correlation of the two
  channels (the cross term over the product of their energies: `+1` the same
  signal, `0` unrelated, `-1` one inverted) and their balance (right minus left of
  the channel RMS, normalised). Both lean with `0.12 s` attack and `0.32 s`
  release, so a cymbal cannot swing the needle for a frame. Silence has neither
  reading, and neither prints a NaN.
* The same window also keeps its own samples as mid/side pairs — every fourth
  frame, at most 256 points — so the Analysis page can draw a real goniometer
  instead of an aggregate: mid runs up the vertical axis, side runs across it with
  a left-heavy pair to the left, and hard L / R are the ±45° diagonals. The cloud
  is handed over with the reading and the next window starts empty, so nothing is
  replayed. The ring is the window's own loudest sample rather than a fixed grid,
  and the note says so: a quiet passage draws small instead of being stretched to
  fill the circle. Nothing moves raw audio through the interface — a 16 ms window
  is 192 points.
* The goniometer is a phosphor scope, not a picture per poll. A canvas fades what
  it drew last time (`destination-out`) and adds the newest window's Lissajous path
  on top (`lighter`), so the field accumulates a trace instead of replacing a few
  hundred dots fifty times a second; the scale under it is one static SVG that the
  poll never touches. Drawing imperatively also keeps those points out of the
  React tree — a poll re-renders the readouts, not the cloud — and the needle and
  balance bar carry no CSS transition, because a transition restarted every 20 ms
  reads as jitter on top of the meter's own ballistics.
* The landscape is drawn as a surface, not a single curve. `useAnalysis` keeps the
  last published spectra and the panel draws the newest 24 of them (half a second
  at the poll rate), each one
  as a ridge lifted by its age, with its own glow anchored to its own baseline:
  warm along the ridge, violet under it, gone below. Only the frame in front fills
  to the floor and carries a lit crest. A frame behind it fills from its own
  baseline down and traces a crest line only where its own energy passes a gate,
  because a line on every frame draws a parallel ridge for each one wherever the
  spectrum is quiet and hatches the empty half of the plot. History ridges are
  drawn at a quarter of the band resolution — they are faint, and thirty of them
  at full resolution cost the frame. Every ridge is clamped to the frame's own
  baseline and the plot clips, so the surface cannot paint over the panel
  heading. An idle engine keeps no surface at all: the panel is one flat line.
* While nothing is playing the meters read `—` and the spectrum draws a flat
  floor: an idle instrument, not a fabricated waveform.
* A pause is a hold, not an idle. The engine stops measuring, keeps the last
  window and publishes it flagged `paused`, so the surface, the scope and every
  readout freeze where the music left them and resuming continues from that
  window rather than from silence; a stop clears the reading, and only that lets
  both panels fade out over a second.

## 5. Gain and headroom

* The playback level is a linear multiplier with a real dB readout, reported as
  `40 % · −8.0 dB` rather than a bare percentage.
* The equaliser reports the predicted peak of its own curve against the preamp.
  Above 0 dB the interface says the boost runs past full scale and offers to set
  the preamp to match. That is a prediction from the coefficients, not a
  measurement of the programme material.
* Actual clipping risk downstream of the DAC or amplifier cannot be seen from
  inside a shared WASAPI stream and is not claimed.

## 6. Buffering and failure behaviour

* Playback: a one-second ring between decoder and render; the decoder waits (3 ms
  at a time) when the ring is full so a seek or stop is never blocked by a slow
  device. The render loop never blocks on the file.
* A seek reopens the file at the requested position rather than dropping frames,
  and position is counted from frames actually heard.
* The output device is resolved by id; an empty id means the Windows default
  and a missing id is an error, not a silent fallback to a different device.
* A device that disappears mid-track reports the render error and closes the
  stream; the interface shows it in the inspector and the Library page.
