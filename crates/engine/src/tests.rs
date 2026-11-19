//! The decks and the engine, played offline.

use std::{f64::consts::PI, sync::Arc};

use crate::{
    audio::Audio,
    deck::{DeckCmd, HOT_CUES},
    engine::{Command, Remote, engine},
    grid::Grid,
    mixer::{MixCmd, Side},
};

const RATE: f64 = 48_000.0;

/// A tone, `seconds` long.
fn tone(f: f64, seconds: f64) -> Arc<Audio> {
    let n = (seconds * RATE) as usize;
    let x: Vec<f32> = (0..n)
        .map(|i| (2.0 * PI * f * i as f64 / RATE).sin() as f32 * 0.5)
        .collect();
    Arc::new(Audio::new(x.clone(), x))
}

/// A ramp: each frame's value says where it is, to read positions off the sound.
fn ramp(seconds: f64) -> Arc<Audio> {
    let n = (seconds * RATE) as usize;
    let x: Vec<f32> = (0..n).map(|i| i as f32 / n as f32).collect();
    Arc::new(Audio::new(x.clone(), x))
}

fn load(r: &mut Remote, deck: usize, audio: Arc<Audio>, grid: Option<Grid>) {
    r.deck(
        deck,
        DeckCmd::Load {
            audio,
            id: deck as u64 + 1,
            grid,
            cue: 0.0,
            hot: [None; HOT_CUES],
        },
    );
}

/// The engine run for `seconds`: the master, left side.
fn run(e: &mut crate::engine::Engine, r: &mut Remote, seconds: f64) -> Vec<f32> {
    let n = (seconds * RATE) as usize;
    let (mut ml, mut mr, mut pl, mut pr) = (vec![0.0; n], vec![0.0; n], vec![0.0; n], vec![0.0; n]);
    // In buffers the size a sound card asks for.
    for k in (0..n).step_by(256) {
        let m = 256.min(n - k);
        e.process(
            &mut ml[k..k + m],
            &mut mr[k..k + m],
            &mut pl[k..k + m],
            &mut pr[k..k + m],
        );
        r.poll();
    }
    ml
}

/// A tone's frequency, by its upward zero crossings.
fn frequency(x: &[f32]) -> f64 {
    let ups: Vec<usize> = (1..x.len())
        .filter(|&i| x[i - 1] < 0.0 && x[i] >= 0.0)
        .collect();
    let (a, b) = (ups[0], *ups.last().unwrap());
    (ups.len() - 1) as f64 * RATE / (b - a) as f64
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// Limiter and master aside, a deck played flat comes out as it went in, a frame at a time.
#[test]
fn plays_as_recorded() {
    let (mut e, mut r) = engine(RATE);
    load(&mut r, 0, tone(440.0, 4.0), None);
    r.deck(0, DeckCmd::Play);
    let out = run(&mut e, &mut r, 1.0);
    let s = r.last.decks[0];
    assert!(s.playing && s.loaded);
    assert!((s.pos - RATE).abs() < 1.0, "{}", s.pos);
    assert!((frequency(&out[4_800..]) - 440.0).abs() < 0.5);
    // The fade in as it starts aside, at the level it was (the limiter's ceiling is above it).
    assert!((rms(&out[4_800..]) - 0.5 / 2f32.sqrt()).abs() < 0.01);
}

/// The pitch fader without keylock: faster and higher, as a record.
#[test]
fn tempo_moves_pitch_without_keylock() {
    let (mut e, mut r) = engine(RATE);
    load(&mut r, 0, tone(440.0, 6.0), None);
    r.deck(0, DeckCmd::Tempo(1.25));
    r.deck(0, DeckCmd::Play);
    let out = run(&mut e, &mut r, 1.0);
    assert!((r.last.decks[0].pos - 1.25 * RATE).abs() < 2.0);
    assert!((frequency(&out[4_800..]) - 550.0).abs() < 1.0);
}

/// Keylock: faster, the key kept; the key shift moves it on its own.
#[test]
fn keylock_keeps_the_key() {
    let (mut e, mut r) = engine(RATE);
    load(&mut r, 0, tone(440.0, 8.0), None);
    r.deck(0, DeckCmd::Keylock(true));
    r.deck(0, DeckCmd::Tempo(1.25));
    r.deck(0, DeckCmd::Play);
    let out = run(&mut e, &mut r, 2.0);
    let s = r.last.decks[0];
    assert!(s.stretching);
    // What's heard is where the track's got to (the stretch's lead taken back off).
    assert!((s.pos - 2.5 * RATE).abs() < 0.01 * RATE, "{}", s.pos / RATE);
    let f = frequency(&out[24_000..]);
    assert!((f - 440.0).abs() < 2.0, "keylocked at 1.25: {f} Hz");
    assert!(rms(&out[24_000..]) > 0.3);

    r.deck(0, DeckCmd::Key(12.0));
    let out = run(&mut e, &mut r, 1.0);
    let f = frequency(&out[24_000..]);
    assert!((f - 880.0).abs() < 4.0, "an octave up: {f} Hz");
}

/// A beat's loop plays round and round, its length exact; exited, the track plays on from it.
#[test]
fn loops_hold_exactly() {
    for keylock in [false, true] {
        let (mut e, mut r) = engine(RATE);
        load(&mut r, 0, ramp(20.0), Some(Grid::new(120.0, 0.0)));
        r.deck(0, DeckCmd::Keylock(keylock));
        r.deck(0, DeckCmd::Play);
        run(&mut e, &mut r, 1.1);
        r.deck(0, DeckCmd::AutoLoop(1.0));
        run(&mut e, &mut r, 0.01);
        let s = r.last.decks[0];
        assert!(s.loop_on);
        // On the beat the playhead's in: 1.0 s to 1.5 s.
        assert_eq!((s.loop_start, s.loop_end), (RATE, 1.5 * RATE));
        let mut seen = (f64::MAX, f64::MIN);
        for _ in 0..40 {
            run(&mut e, &mut r, 0.1);
            let p = r.last.decks[0].pos;
            seen = (seen.0.min(p), seen.1.max(p));
        }
        assert!(
            seen.0 >= RATE - 1.0 && seen.1 < 1.5 * RATE + 64.0,
            "keylock {keylock}: {:?}",
            (seen.0 / RATE, seen.1 / RATE)
        );
        // 4 s played inside it: 8 times round, back where it went in plus the 0.11 s it had
        // played into it.
        let at = r.last.decks[0].pos;
        let want = RATE + (0.11 + 4.0) * RATE % (0.5 * RATE);
        assert!(
            (at - want).abs() < 70.0,
            "keylock {keylock}: {} vs {}",
            at,
            want
        );
        r.deck(0, DeckCmd::LoopExit);
        run(&mut e, &mut r, 1.0);
        assert!(r.last.decks[0].pos > 1.5 * RATE);
    }
}

/// With quantize, a hot cue jumps in time: as far through its beat as the track was.
#[test]
fn hot_cues_jump_in_time() {
    let (mut e, mut r) = engine(RATE);
    load(&mut r, 0, ramp(30.0), Some(Grid::new(120.0, 0.0)));
    // Set at 2.0 s while stopped.
    r.deck(0, DeckCmd::Seek(2.0 * RATE));
    run(&mut e, &mut r, 0.01);
    r.deck(
        0,
        DeckCmd::HotCue {
            slot: 0,
            down: true,
        },
    );
    r.deck(
        0,
        DeckCmd::HotCue {
            slot: 0,
            down: false,
        },
    );
    run(&mut e, &mut r, 0.01);
    assert_eq!(r.last.decks[0].hot[0], 2.0 * RATE);
    r.deck(0, DeckCmd::Seek(10.0 * RATE));
    r.deck(0, DeckCmd::Play);
    run(&mut e, &mut r, 0.6);
    let before = r.last.decks[0];
    r.deck(
        0,
        DeckCmd::HotCue {
            slot: 0,
            down: true,
        },
    );
    r.deck(
        0,
        DeckCmd::HotCue {
            slot: 0,
            down: false,
        },
    );
    run(&mut e, &mut r, 0.1);
    let after = r.last.decks[0];
    assert!(
        after.pos > 2.0 * RATE && after.pos < 2.6 * RATE,
        "{}",
        after.pos / RATE
    );
    // The beat's phase carries on as if nothing happened.
    let phase = |b: f64| b.rem_euclid(1.0);
    let expect = phase(before.beat + 0.1 * 2.0);
    assert!(
        (phase(after.beat) - expect).abs() < 0.01,
        "{} vs {}",
        phase(after.beat),
        expect
    );
}

/// The CDJ cue: stopped somewhere new, it sets the cue there and plays while held; let go,
/// back to it. Playing, it goes back to the cue and stops.
#[test]
fn cue_button_as_a_cdj() {
    let (mut e, mut r) = engine(RATE);
    load(&mut r, 0, ramp(30.0), None);
    r.deck(0, DeckCmd::Seek(5.0 * RATE));
    run(&mut e, &mut r, 0.01);
    r.deck(0, DeckCmd::Cue(true));
    run(&mut e, &mut r, 0.5);
    let s = r.last.decks[0];
    assert_eq!(s.cue, 5.0 * RATE);
    assert!(s.previewing && s.pos > 5.4 * RATE);
    r.deck(0, DeckCmd::Cue(false));
    run(&mut e, &mut r, 0.05);
    let s = r.last.decks[0];
    assert!(!s.playing && s.pos == 5.0 * RATE);
    // Play, then cue: back and stopped.
    r.deck(0, DeckCmd::Play);
    run(&mut e, &mut r, 1.0);
    r.deck(0, DeckCmd::Cue(true));
    r.deck(0, DeckCmd::Cue(false));
    run(&mut e, &mut r, 0.05);
    let s = r.last.decks[0];
    assert!(!s.playing && s.pos == 5.0 * RATE);
}

/// Sync: a 126 BPM track to a 120 BPM one, at its tempo, its beats on the leader's, and kept
/// there for half a minute; a 64 BPM track goes to double time.
#[test]
fn sync_locks_tempo_and_phase() {
    let (mut e, mut r) = engine(RATE);
    load(&mut r, 0, ramp(60.0), Some(Grid::new(120.0, 0.1)));
    load(&mut r, 1, ramp(60.0), Some(Grid::new(126.0, 0.37)));
    r.deck(0, DeckCmd::Play);
    run(&mut e, &mut r, 1.3);
    r.deck(1, DeckCmd::Keylock(true));
    r.deck(1, DeckCmd::Sync(true));
    r.deck(1, DeckCmd::Play);
    run(&mut e, &mut r, 0.2);
    let s = r.last;
    assert_eq!(s.leader, Some(0));
    assert!((s.decks[1].bpm - 120.0).abs() < 1e-9, "{}", s.decks[1].bpm);
    let phase_err =
        |s: &crate::engine::Status| crate::grid::wrap_half(s.decks[0].beat - s.decks[1].beat).abs();
    assert!(phase_err(&s) < 0.01, "lined up: {}", phase_err(&s));
    for _ in 0..30 {
        run(&mut e, &mut r, 1.0);
        assert!(phase_err(&r.last) < 0.01, "held: {}", phase_err(&r.last));
    }
    // The leader's fader moves: the follower goes with it.
    r.deck(0, DeckCmd::Tempo(1.05));
    run(&mut e, &mut r, 2.0);
    assert!((r.last.decks[1].bpm - 126.0).abs() < 1e-6);
    assert!(phase_err(&r.last) < 0.01);

    load(&mut r, 2, ramp(60.0), Some(Grid::new(64.0, 0.0)));
    r.deck(2, DeckCmd::Sync(true));
    r.deck(2, DeckCmd::Play);
    run(&mut e, &mut r, 0.5);
    assert!(
        (r.last.decks[2].bpm - 63.0).abs() < 1e-6,
        "{}",
        r.last.decks[2].bpm
    );
}

/// A roll loops while it's held; let go, the track is where it would have been.
#[test]
fn rolls_slip_back() {
    let (mut e, mut r) = engine(RATE);
    load(&mut r, 0, ramp(30.0), Some(Grid::new(120.0, 0.0)));
    r.deck(0, DeckCmd::Play);
    run(&mut e, &mut r, 2.05);
    r.deck(
        0,
        DeckCmd::Roll {
            beats: 0.25,
            on: true,
        },
    );
    run(&mut e, &mut r, 1.0);
    let s = r.last.decks[0];
    assert!(s.roll && s.pos >= 2.0 * RATE && s.pos < 2.125 * RATE + 64.0);
    assert!(
        (s.shadow - 3.05 * RATE).abs() < 600.0,
        "{}",
        s.shadow / RATE
    );
    r.deck(
        0,
        DeckCmd::Roll {
            beats: 0.25,
            on: false,
        },
    );
    run(&mut e, &mut r, 0.5);
    let s = r.last.decks[0];
    assert!(!s.loop_on && s.shadow.is_nan());
    assert!((s.pos - 3.55 * RATE).abs() < 600.0, "{}", s.pos / RATE);
}

/// A brake slows to a stop over its time; reverse plays back.
#[test]
fn brake_and_reverse() {
    let (mut e, mut r) = engine(RATE);
    load(&mut r, 0, ramp(30.0), None);
    r.deck(0, DeckCmd::Seek(10.0 * RATE));
    r.deck(0, DeckCmd::Play);
    run(&mut e, &mut r, 0.5);
    r.deck(0, DeckCmd::Reverse(true));
    run(&mut e, &mut r, 1.0);
    let s = r.last.decks[0];
    assert!(
        s.reverse && (s.pos - 9.5 * RATE).abs() < 600.0,
        "{}",
        s.pos / RATE
    );
    r.deck(0, DeckCmd::Reverse(false));
    r.deck(0, DeckCmd::Brake(1.0));
    run(&mut e, &mut r, 0.5);
    assert!(r.last.decks[0].playing);
    run(&mut e, &mut r, 0.6);
    let s = r.last.decks[0];
    assert!(!s.playing);
    // Half a second's worth on average, over a second slowing down.
    assert!((s.pos - 10.0 * RATE).abs() < 0.1 * RATE, "{}", s.pos / RATE);
}

/// Scratching: the platter held, the track holds; turned, it follows the hand.
#[test]
fn scratching_follows_the_hand() {
    let (mut e, mut r) = engine(RATE);
    load(&mut r, 0, ramp(30.0), None);
    r.deck(0, DeckCmd::Seek(5.0 * RATE));
    r.deck(0, DeckCmd::Play);
    run(&mut e, &mut r, 0.1);
    r.deck(0, DeckCmd::Touch(true));
    run(&mut e, &mut r, 0.3);
    let held = r.last.decks[0].pos;
    run(&mut e, &mut r, 0.3);
    assert!((r.last.decks[0].pos - held).abs() < 1.0);
    // A quarter turn back, then forward, in small moves as a jog wheel sends them.
    for _ in 0..25 {
        r.deck(0, DeckCmd::Scratch(-0.01));
        run(&mut e, &mut r, 0.01);
    }
    run(&mut e, &mut r, 0.2);
    let back = r.last.decks[0].pos;
    assert!(
        (held - back - 0.25 * 1.8 * RATE).abs() < 0.01 * RATE,
        "{}",
        (held - back) / RATE
    );
    r.deck(0, DeckCmd::Touch(false));
    run(&mut e, &mut r, 1.0);
    assert!((r.last.decks[0].pos - back - RATE).abs() < 0.02 * RATE);
}

/// The crossfader at A: what's on B is gone, what's on thru stays; cue gets the headphones.
#[test]
fn crossfader_and_cue() {
    let (mut e, mut r) = engine(RATE);
    load(&mut r, 0, tone(440.0, 4.0), None);
    r.deck(0, DeckCmd::Play);
    r.mix(MixCmd::Side {
        ch: 0,
        side: Side::B,
    });
    r.mix(MixCmd::Crossfader(-1.0));
    r.mix(MixCmd::Cue { ch: 0, on: true });
    r.send(Command::Quantize(true));
    let n = (0.5 * RATE) as usize;
    let (mut ml, mut mr, mut pl, mut pr) = (vec![0.0; n], vec![0.0; n], vec![0.0; n], vec![0.0; n]);
    e.process(&mut ml, &mut mr, &mut pl, &mut pr);
    assert!(rms(&ml[n / 2..]) < 1e-4);
    assert!(rms(&pl[n / 2..]) > 0.3);
    r.mix(MixCmd::Side {
        ch: 0,
        side: Side::Thru,
    });
    e.process(&mut ml, &mut mr, &mut pl, &mut pr);
    assert!(rms(&ml[n / 2..]) > 0.3);
}

/// Sync with keylock on both, the leader sped up and looping: the follower's beats stay on the
/// leader's (and on its bars), and sync's hand on its speed rests near nothing.
#[test]
fn sync_holds_a_looping_keylocked_leader() {
    let (mut e, mut r) = engine(RATE);
    load(&mut r, 0, ramp(60.0), Some(Grid::new(123.0, 0.2)));
    load(&mut r, 1, ramp(60.0), Some(Grid::new(127.0, 0.97)));
    for d in 0..2 {
        r.deck(d, DeckCmd::Keylock(true));
    }
    r.deck(0, DeckCmd::Tempo(1.025));
    r.deck(0, DeckCmd::Seek(20.0 * RATE));
    r.deck(0, DeckCmd::Play);
    run(&mut e, &mut r, 1.0);
    r.deck(1, DeckCmd::Seek(15.0 * RATE));
    r.deck(1, DeckCmd::Sync(true));
    r.deck(1, DeckCmd::Play);
    run(&mut e, &mut r, 0.3);
    r.deck(0, DeckCmd::AutoLoop(4.0));
    let mut worst = 0.0f64;
    let mut speed = 0.0;
    for _ in 0..100 {
        run(&mut e, &mut r, 0.1);
        let s = r.last;
        let err = crate::grid::wrap_half(s.decks[0].beat - s.decks[1].beat).abs();
        worst = worst.max(err);
        speed += s.decks[1].speed / s.decks[1].tempo / 100.0;
        // On the same beat of the bar.
        let bar = |b: f64| b.rem_euclid(4.0).floor();
        assert_eq!(bar(s.decks[0].beat + 0.5), bar(s.decks[1].beat + 0.5));
    }
    assert!(worst < 0.01, "phase held: {worst}");
    assert!((speed - 1.0).abs() < 0.001, "sync's hand: {speed}");
}

/// A tone's level in `x` (Goertzel).
fn level_at(x: &[f32], f: f64) -> f64 {
    let w = 2.0 * PI * f / RATE;
    let (mut s1, mut s2) = (0.0, 0.0);
    for &v in x {
        let s0 = v as f64 + 2.0 * w.cos() * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    ((s1 * s1 + s2 * s2 - 2.0 * w.cos() * s1 * s2).sqrt() / x.len() as f64) * 2.0
}

/// Stems: a mix of four tones, each its own stem. Vocals muted, its tone goes and the rest stay;
/// vocals alone (an acapella), only its tone; with keylock as without.
#[test]
fn stems_take_parts_out() {
    use crate::stems::Stems;
    let n = (6.0 * RATE) as usize;
    let sine = |f: f64| -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * PI * f * i as f64 / RATE).sin() as f32 * 0.2)
            .collect()
    };
    let (v, d, b, m) = (sine(880.0), sine(220.0), sine(55.0), sine(1760.0));
    let mix: Vec<f32> = (0..n).map(|i| v[i] + d[i] + b[i] + m[i]).collect();
    for keylock in [false, true] {
        let (mut e, mut r) = engine(RATE);
        load(
            &mut r,
            0,
            Arc::new(Audio::new(mix.clone(), mix.clone())),
            None,
        );
        let stems = Stems::from_parts([
            [v.clone(), v.clone()],
            [d.clone(), d.clone()],
            [b.clone(), b.clone()],
        ]);
        r.deck(
            0,
            DeckCmd::Stems {
                id: 1,
                stems: Arc::new(stems),
            },
        );
        r.deck(0, DeckCmd::Keylock(keylock));
        r.deck(0, DeckCmd::Play);
        let all = run(&mut e, &mut r, 1.0);
        assert!(r.last.decks[0].stems);
        r.deck(0, DeckCmd::StemGain { stem: 0, gain: 0.0 });
        run(&mut e, &mut r, 0.2);
        let no_vocals = run(&mut e, &mut r, 0.5);
        for s in 1..4 {
            r.deck(
                0,
                DeckCmd::StemGain {
                    stem: s,
                    gain: if s == 0 { 1.0 } else { 0.0 },
                },
            );
        }
        r.deck(0, DeckCmd::StemGain { stem: 0, gain: 1.0 });
        run(&mut e, &mut r, 0.2);
        let acapella = run(&mut e, &mut r, 0.5);
        let tail = |x: &[f32]| x[x.len() / 2..].to_vec();
        let (all, no_vocals, acapella) = (tail(&all), tail(&no_vocals), tail(&acapella));
        let k = if keylock { "keylock" } else { "vinyl" };
        assert!(level_at(&all, 880.0) > 0.15, "{k}: vocals in the mix");
        assert!(
            level_at(&no_vocals, 880.0) < 0.01,
            "{k}: vocals out: {}",
            level_at(&no_vocals, 880.0)
        );
        assert!(level_at(&no_vocals, 1760.0) > 0.15, "{k}: melody stays");
        assert!(level_at(&acapella, 880.0) > 0.15, "{k}: acapella");
        for f in [220.0, 55.0, 1760.0] {
            assert!(
                level_at(&acapella, f) < 0.01,
                "{k}: {f} Hz out of the acapella"
            );
        }
    }
}
