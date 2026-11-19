# Pina Live

DJ software in Rust and GPUI, built on [Pina Studio](https://github.com/manyeya/pina-studio)'s
engine: its decoding, song study, effects, limiter and stem separation.

## What it does

**Decks** (two or four): keylock that follows the pitch fader every block (real-time stretch),
key shift ±12, CDJ cue (set and preview while held), 8 hot cues, quantized jumps that keep the
beat, auto loops from 1/4 to 32 beats, manual loop in/out, halve/double/move, rolls, beat jumps,
slip, reverse and censor, brake and spinback, and a platter or waveform you scratch with the
mouse. Sync follows a leader (picked, or whichever has played longest) at half or double time
when that's nearer, lines up on the bar, then holds the beat.

**Mixer**: per channel trim, auto gain (every track to −10 LUFS), a 3-band isolator EQ with full
kills, a one-knob filter with resonance, channel faders, crossfader assign and curve (smooth or
scratch), headphone cue with cue mix (on its own outputs, or split on one jack), and a master
through Pina Studio's true-peak limiter.

**FX**: two beat-synced units on any channels, after the fader, so echoes and reverbs ring out
when the fader comes down or the effect's switched off. Echo, Tape Echo, Dub Echo, Ping Pong,
Reverb, Shimmer, Flanger, Phaser, Filter Sweep, Trans, Pitch, Pitch Echo, Crush, Tape Stop,
Noise (most of them Pina Studio's own effects).

**Stems**: parts a track into vocals, drums, bass and melody with a separation model you've added
to Pina Studio (Settings › Stem models; Pina Live uses the same folder). Mute any part, or go
acapella, instrumental or drums only, with keylock on or off. It runs in the background, kept
once done. On the CPU it takes about 3× the track's length with BS-RoFormer.

**Library**: folders watched, every track analysed in the background (tempo and beat grid, key
and its Camelot number, phrases such as intro, build, drop and outro, loudness, a three-band
waveform), cues and hot cues kept per track, crates, history, and search that takes words with
`bpm:120-128`, `bpm:124`, `key:8A` or `key:~8A` (anything that mixes with it), and `rating:4`.
Keys that mix with the leading deck are lit.

**Also**: recording the mix to WAV (~/Music/Pina Live/Recordings), MIDI controllers by MIDI learn
(jogs and pads go straight to the audio thread), and laptop keys.

## Keys

| Deck A | Deck B | |
|---|---|---|
| Q | P | play / pause |
| A | L | cue (held) |
| Z | , | sync |
| W | O | 4-beat loop |
| 1–4 | 7–0 | hot cues 1–4 (held) |

⇧← / ⇧→ load the selected track onto A / B; ↑ ↓ move through the library; ⌘F search;
⌘4 two or four decks; ⌘= ⌘− zoom the waveforms.

## MIDI

Click **MIDI** in the top bar, click a control on screen, then move it on your controller. For
a jog, touch it and then turn it. Settings › Controllers lists every mapping; flip one that runs
backwards, or remove it.

## Build

```sh
cargo run --release -p pina-live        # run
scripts/bundle.sh                       # build and install Pina Live.app in /Applications
cargo test --workspace                  # engine and app tests
PINA_LIVE_SHOT=out PINA_LIVE_SHOT_TRACKS="a.mp3|b.mp3" cargo test -p pina-live shots -- --ignored
```

`cargo run --release -p pina-live-engine --example bench` times the engine's heaviest load (four
keylocked decks with stems and both FX units on every channel: about 6% of one core on an M-series
Mac).

## Builds and installers

Every push to `main` builds all three (Actions; a `vX.Y.Z` tag puts them on its release):

- **macOS**: `scripts/release-mac.sh` makes `dist/PinaLive-<version>.dmg`, Apple silicon and
  Intel, with *Install Pina Live* on it (`crates/installer`, the animated installer: decks mixing
  while it installs). Signed ad hoc unless `PINA_SIGN_ID` names a Developer ID.
- **Windows**: `PinaLive-<version>-windows-x64-setup.exe` (Inno Setup, `scripts/windows`) and a zip
  of the same.
- **Web**: Pina Live in a browser, at <https://manyeya.github.io/pina-live/>. Drop your tracks onto
  the page (they stay in it, never uploaded) or try the demo songs. It's the same app, built to
  WebAssembly: the keylock is `wsola.rs` (Signalsmith is C++), tracks are decoded and analysed
  in Web Workers (`web.rs`, `web/worker.js`) so the page never stops for them, and stems,
  plugins and recording are the desktop's. To build it:

```sh
cargo build --release --target wasm32-unknown-unknown -p pina-live
wasm-bindgen --target web --no-typescript --out-dir web/pkg \
  target/wasm32-unknown-unknown/release/pina-live.wasm
cd web && python3 -m http.server   # then open http://localhost:8000
```
