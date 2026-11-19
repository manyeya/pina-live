//! The window, drawn to PNGs to look at without a window on screen.

use std::{path::PathBuf, sync::Arc, thread, time::Duration};

use gpui::{AppContext, px};
use pina_live_engine::{deck::DeckCmd, engine::engine, library::Library, mixer::MixCmd};

use crate::live::Live;

/// `PINA_LIVE_SHOT=<dir> PINA_LIVE_SHOT_TRACKS="<file>|<file>" cargo test -p pina-live shots --
/// --ignored`: the window with those tracks on decks A and B (A playing, looping, cued up),
/// drawn to `<dir>/<PINA_LIVE_SHOT_PAGE or "decks">.png`. Its library and waveforms are kept
/// in a folder of its own, never the real one.
#[test]
#[ignore]
fn shots() {
    let out = PathBuf::from(std::env::var("PINA_LIVE_SHOT").expect("PINA_LIVE_SHOT: a folder"));
    std::fs::create_dir_all(&out).unwrap();
    let home = std::env::temp_dir().join(format!("pina-live-shot-{}", std::process::id()));
    // SAFETY: set before any thread that reads it starts (this test runs alone, --ignored).
    unsafe { std::env::set_var("PINA_LIVE_HOME", &home) };
    let page = std::env::var("PINA_LIVE_SHOT_PAGE").unwrap_or_else(|_| "decks".into());
    let w: f32 = std::env::var("PINA_LIVE_SHOT_W")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1440.0);
    let h: f32 = std::env::var("PINA_LIVE_SHOT_H")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(900.0);
    let tracks: Vec<PathBuf> = std::env::var("PINA_LIVE_SHOT_TRACKS")
        .unwrap_or_default()
        .split('|')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect();
    let mut lib = Library::default();
    let ids: Vec<u64> = tracks.iter().map(|t| lib.add_file(t)).collect();
    // PINA_LIVE_SHOT_STEMS: a folder of stems (vocals.wav…) to give deck B's track.
    if let (Some(dir), Some(&id)) = (std::env::var_os("PINA_LIVE_SHOT_STEMS"), ids.get(1)) {
        let to = crate::stems::stems_dir(id);
        std::fs::create_dir_all(&to).unwrap();
        for f in std::fs::read_dir(dir).unwrap().flatten() {
            std::fs::copy(f.path(), to.join(f.file_name())).unwrap();
        }
    }

    let mut cx = gpui::HeadlessAppContext::with_platform(
        Arc::new(gpui_apple::AppleTextSystem::new()),
        Arc::new(()),
        || {
            Ok(Some(Box::new(
                gpui_apple::metal_renderer::MetalHeadlessRenderer::new(),
            )))
        },
    );
    cx.update(|cx| {
        cx.text_system()
            .add_fonts(crate::ui::theme::fonts())
            .unwrap();
        crate::ui::text::bind_keys(cx);
        crate::live::bind_keys(cx);
    });
    let rate = 48_000.0;
    let (e, remote) = engine(rate);
    let window = cx
        .open_window(gpui::size(px(w), px(h)), |window, cx| {
            cx.new(|cx| Live::new(rate, remote, Some(e), lib, window, cx))
        })
        .unwrap();
    let pump = |cx: &mut gpui::HeadlessAppContext, ms: u64| {
        for _ in 0..(ms / 20).max(1) {
            cx.advance_clock(Duration::from_millis(20));
            cx.run_until_parked();
            thread::sleep(Duration::from_millis(20));
        }
    };
    cx.update(|cx| {
        window
            .update(cx, |live, _, cx| {
                live.four = page == "four";
                live.queue_analysis(&ids);
                cx.notify();
            })
            .unwrap()
    });
    // Analysis, then the tracks onto the decks.
    for _ in 0..600 {
        pump(&mut cx, 100);
        let busy = cx.update(|cx| window.read(cx).map(|l| l.analyzer.busy()).unwrap_or(false));
        if !busy {
            break;
        }
    }
    cx.update(|cx| {
        window
            .update(cx, |live, _, cx| {
                for (deck, &id) in ids.iter().take(4).enumerate() {
                    live.load(deck, id, cx);
                }
            })
            .unwrap()
    });
    pump(&mut cx, 3000);
    cx.update(|cx| {
        window
            .update(cx, |live, _, cx| {
                // Deck A: hot cues set on its first beats, playing in a 4-beat loop at +2.5%.
                let r = live.rate;
                let grid = live.decks[0].analysis.as_ref().and_then(|a| a.grid);
                let at = |beat: f64| grid.map_or(beat * 0.5 * r, |g| g.frame_of(beat, r));
                for (k, b) in [32.0, 64.0, 96.0, 160.0].into_iter().enumerate() {
                    live.deck(
                        0,
                        DeckCmd::SetHotCue {
                            slot: k,
                            at: Some(at(b)),
                        },
                    );
                }
                live.deck(0, DeckCmd::Seek(at(127.5)));
                crate::controls::Ctl::Tempo(0).set(live, 0.3125);
                live.deck(0, DeckCmd::Play);
                live.deck(1, DeckCmd::Seek(at(31.0)));
                live.deck(1, DeckCmd::Sync(true));
                live.deck(1, DeckCmd::Play);
                crate::controls::Ctl::Eq(1, 0).set(live, 0.2);
                crate::controls::Ctl::Filter(1).set(live, -0.35);
                crate::controls::Ctl::Xf.set(live, -0.3);
                live.mix.cue[1] = true;
                live.mixer(MixCmd::Cue { ch: 1, on: true });
                cx.notify();
            })
            .unwrap()
    });
    pump(&mut cx, 1200);
    cx.update(|cx| {
        window
            .update(cx, |live, _, cx| {
                live.deck(0, DeckCmd::AutoLoop(4.0));
                live.decks[1].pads = match std::env::var_os("PINA_LIVE_SHOT_STEMS") {
                    Some(_) => crate::live::Pads::Stems,
                    None => crate::live::Pads::Loop,
                };
                live.deck(1, DeckCmd::StemGain { stem: 0, gain: 0.0 });
                live.decks[0].pads = match page.as_str() {
                    "parting" => crate::live::Pads::Stems,
                    _ => crate::live::Pads::HotCue,
                };
                // FX 1: an echo on A; FX 2: a flanger on B.
                use pina_live_engine::fx::{FxCmd, FxKind};
                live.fx[0].settings.on = true;
                live.fx[0].on[0] = true;
                live.remote.fx(FxCmd::Assign {
                    unit: 0,
                    ch: 0,
                    on: true,
                });
                live.send_fx(0);
                live.set_fx_kind(1, FxKind::Flanger);
                live.fx[1].on[1] = true;
                live.remote.fx(FxCmd::Assign {
                    unit: 1,
                    ch: 1,
                    on: true,
                });
                live.show_settings = page == "settings";
                live.fx_menu = (page == "fxmenu").then_some(1);
                live.browser.selected = live.lib.tracks.first().map(|t| t.id);
                cx.notify();
            })
            .unwrap()
    });
    pump(&mut cx, 900);
    cx.capture_screenshot(window.into()).unwrap();
    cx.run_until_parked();
    let image = cx.capture_screenshot(window.into()).unwrap();
    let file = out.join(format!("{page}.png"));
    image.save(&file).unwrap();
    println!("{}", file.display());
    let _ = std::fs::remove_dir_all(&home);
}
