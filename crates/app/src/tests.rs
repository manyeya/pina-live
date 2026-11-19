//! The window, tested as a person uses it: tracks dragged onto decks, edges dragged to size the
//! panels, the library shut and full screen, every deck state at every window size checked for
//! anything spilling out of its panel (with macOS's own type, headless), and drawn to PNGs to
//! look at.

use std::{path::PathBuf, sync::Arc, thread, time::Duration};

use gpui::{AppContext, px};
use pina_live_engine::{deck::DeckCmd, engine::engine, library::Library, mixer::MixCmd};

use crate::live::Live;

/// A WAV of a tone, 16-bit stereo, for the tests' own library.
fn write_wav(path: &std::path::Path, seconds: f32) {
    let rate = 48_000u32;
    let n = (seconds * rate as f32) as u32;
    let mut b = Vec::new();
    let data = n * 4;
    b.extend(b"RIFF");
    b.extend((36 + data).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(rate.to_le_bytes());
    b.extend((rate * 4).to_le_bytes());
    b.extend(4u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(data.to_le_bytes());
    for i in 0..n {
        let v = ((i as f32 * 0.05).sin() * 8000.0) as i16;
        b.extend(v.to_le_bytes());
        b.extend(v.to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}

/// The tests' own data folder and separation models, never the person's: one for the run. Its
/// one model is a stand-in (an empty ONNX file) named as the longest real one, for the layout.
fn test_home() -> PathBuf {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let d = std::env::temp_dir().join(format!("pina-live-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let model = d.join("models").join("roformer");
        std::fs::create_dir_all(&model).unwrap();
        std::fs::write(
            model.join("model.json"),
            r#"{"name": "BS-RoFormer SW", "stems": ["bass", "drums", "other", "vocals"], "input": "wave"}"#,
        )
        .unwrap();
        std::fs::write(model.join("model.onnx"), b"").unwrap();
        // SAFETY: set once, before any test thread reads them.
        unsafe {
            std::env::set_var("PINA_LIVE_HOME", &d);
            std::env::set_var("PINA_MODELS", d.join("models"));
        }
        d
    })
    .clone()
}

/// A track dragged out of the library and let go over deck B's card loads on B (and the card
/// says so while it's over it); let go over nothing, nothing loads.
#[gpui::test]
fn dragging_a_track_onto_a_deck_loads_it(cx: &mut gpui::TestAppContext) {
    let home = test_home();
    let (a, b) = (home.join("First.wav"), home.join("Second.wav"));
    write_wav(&a, 3.0);
    write_wav(&b, 3.0);
    let mut lib = Library::default();
    let first = lib.add_file(&a);
    lib.add_file(&b);
    cx.update(|cx| {
        crate::ui::text::bind_keys(cx);
        crate::live::bind_keys(cx);
    });
    let rate = 48_000.0;
    let (e, remote) = engine(rate);
    let (live, cx) =
        cx.add_window_view(|window, cx| Live::new(rate, remote, Some(e), lib, window, cx));
    cx.run_until_parked();
    let (row, deck_b) = live.read_with(cx, |live, _| {
        let heads = live.browser.heads_at.get();
        // The rows are in the library's newest-first order: First is the second one down.
        let k = live
            .browser
            .rows
            .iter()
            .position(|&i| live.lib.tracks[i].id == first)
            .unwrap();
        let row = gpui::point(
            heads.origin.x + px(320.),
            heads.origin.y + heads.size.height + px(crate::browser::ROW_H * (k as f32 + 0.5)),
        );
        (row, live.decks[1].overview_at.get().center())
    });
    cx.simulate_mouse_down(row, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_move(
        row + gpui::point(px(30.), px(-20.)),
        gpui::MouseButton::Left,
        gpui::Modifiers::none(),
    );
    cx.simulate_mouse_move(deck_b, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.run_until_parked();
    let over = live.read_with(cx, |live, _| live.drop_on.clone());
    assert_eq!(over.map(|o| o.0), Some(1), "deck B says it'll take it");
    cx.simulate_mouse_up(deck_b, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.run_until_parked();
    live.read_with(cx, |live, _| {
        assert_eq!(live.decks[1].track, Some(first));
        assert_eq!(live.decks[0].track, None);
        assert!(live.drop_on.is_none());
    });
}

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
                live.four = page == "four" || page == "four-narrow" || page == "stemsbox";
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
                match page.as_str() {
                    "closed" => live.settings.layout.library = crate::layout::LibView::Closed,
                    "full" => live.settings.layout.library = crate::layout::LibView::Full,
                    "stemsbox" => live
                        .decks
                        .iter_mut()
                        .for_each(|d| d.pads = crate::live::Pads::Stems),
                    _ => {}
                }
                if page == "drop" {
                    live.drop_on = Some((1, "Climax (LIVE cover by Leroy)".into()));
                    live.keep_drop = true;
                }
                if page == "crates" {
                    let ids: Vec<u64> = live.lib.tracks.iter().map(|t| t.id).collect();
                    live.lib.crates.push(pina_live_engine::library::Crate {
                        name: "Sunday warm-up".into(),
                        tracks: ids[..2].to_vec(),
                    });
                    live.lib.crates.push(pina_live_engine::library::Crate {
                        name: "Afro house peak".into(),
                        tracks: ids.clone(),
                    });
                    live.browser.refresh(&live.lib);
                    live.grid_edit = Some(0);
                    live.menu = Some((
                        crate::crates::Menu::Track(ids[2]),
                        gpui::point(px(620.), px(760.)),
                    ));
                }
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

/// FX 1 starts on deck A and FX 2 on B; switching one on while it's on no deck puts it on the
/// leading deck (A with none leading), so On always does something.
#[gpui::test]
fn fx_are_on_a_deck_when_switched_on(cx: &mut gpui::TestAppContext) {
    test_home();
    let rate = 48_000.0;
    let (e, remote) = engine(rate);
    let (live, cx) = cx.add_window_view(|window, cx| {
        Live::new(rate, remote, Some(e), Library::default(), window, cx)
    });
    cx.run_until_parked();
    live.update(cx, |live, cx| {
        assert!(live.fx[0].on[0] && live.fx[1].on[1]);
        live.toggle_send(1, 1, cx);
        assert!(!live.fx[1].on.iter().any(|x| *x));
        live.toggle_fx(1, cx);
        assert!(live.fx[1].settings.on && live.fx[1].on[0]);
    });
}

/// Every state a deck's in, at every size the window comes in, two decks and four, the library
/// open, shut and full screen: drawn with macOS's own type (headless, as the real window draws
/// it) and every panel's contents checked against it. A pad, a box, a button or a line of text
/// past the edge of what holds it fails, saying which and by how much.
#[cfg(target_os = "macos")]
#[test]
fn layouts_fit() {
    use crate::{layout::LibView, live::Pads};
    use pina_live_engine::stems::Stems;
    let home = test_home();
    let files: Vec<PathBuf> = [
        "Euphoria",
        "That Night (Extended Afro Mix) feat. Somebody Long",
        "In Time",
        "Yours",
    ]
    .iter()
    .map(|n| {
        let p = home.join(format!("{n}.wav"));
        write_wav(&p, 6.0);
        p
    })
    .collect();
    let sizes = [
        (1440.0, 900.0, false),
        (1440.0, 900.0, true),
        (1100.0, 700.0, false),
        (1100.0, 700.0, true),
        (1366.0, 768.0, false),
        (1280.0, 800.0, false),
        (1600.0, 1000.0, true),
        (1920.0, 1080.0, false),
    ];
    let mut failures = Vec::new();
    for (w, h, four) in sizes {
        let mut lib = Library::default();
        let ids: Vec<u64> = files.iter().map(|f| lib.add_file(f)).collect();
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
        let shown = if four { 4 } else { 2 };
        cx.update(|cx| {
            window
                .update(cx, |live, _, cx| {
                    live.four = four;
                    // Deck D left empty with four decks: an empty deck's card is a state too.
                    for d in 0..shown.min(3) {
                        live.load(d, ids[d], cx);
                    }
                })
                .unwrap()
        });
        for _ in 0..100 {
            pump(&mut cx, 40);
            let done = cx.update(|cx| {
                window
                    .read(cx)
                    .map(|l| l.decks.iter().all(|d| d.loading.is_none()))
                    .unwrap()
            });
            if done {
                break;
            }
        }
        // B's track parted already; A's being parted.
        cx.update(|cx| {
            window
                .update(cx, |live, _, cx| {
                    let n = (6.0 * rate) as usize;
                    let part = || [vec![0.0f32; n], vec![0.0f32; n]];
                    let stems = Stems::from_parts([part(), part(), part()]);
                    live.deck(
                        1,
                        DeckCmd::Stems {
                            id: ids[1],
                            stems: Arc::new(stems),
                        },
                    );
                    live.deck(
                        0,
                        DeckCmd::SetHotCue {
                            slot: 0,
                            at: Some(rate),
                        },
                    );
                    live.deck(0, DeckCmd::Play);
                    cx.notify();
                })
                .unwrap()
        });
        pump(&mut cx, 200);
        type Set = Box<dyn Fn(&mut Live)>;
        let modes = |m: Pads| -> Set {
            Box::new(move |l: &mut Live| l.decks.iter_mut().for_each(|d| d.pads = m))
        };
        let scenarios: Vec<(&str, Set)> = vec![
            ("hot cues", modes(Pads::HotCue)),
            ("loops", modes(Pads::Loop)),
            ("rolls", modes(Pads::Roll)),
            ("jumps", modes(Pads::Jump)),
            (
                "stems, A parting",
                Box::new(move |l: &mut Live| {
                    l.decks.iter_mut().for_each(|d| d.pads = Pads::Stems);
                    l.separating = Some(crate::stems::Job::stuck(ids[0], 0.4));
                }),
            ),
            (
                "stems, none parting",
                Box::new(|l: &mut Live| {
                    l.decks.iter_mut().for_each(|d| d.pads = Pads::Stems);
                    l.separating = None;
                }),
            ),
            (
                "grid tools, learning",
                Box::new(|l: &mut Live| {
                    l.decks.iter_mut().for_each(|d| d.pads = Pads::HotCue);
                    l.grid_edit = Some(0);
                    l.learning = true;
                }),
            ),
            (
                "library shut",
                Box::new(|l: &mut Live| {
                    l.grid_edit = None;
                    l.learning = false;
                    l.settings.layout.library = LibView::Closed;
                }),
            ),
            (
                "library full screen",
                Box::new(|l: &mut Live| l.settings.layout.library = LibView::Full),
            ),
            (
                "the other layer",
                Box::new(|l: &mut Live| {
                    l.settings.layout.library = LibView::Open;
                    l.layer = [2, 3];
                }),
            ),
        ];
        for (what, set) in &scenarios {
            cx.update(|cx| {
                window
                    .update(cx, |live, _, cx| {
                        set(live);
                        live.probes.borrow_mut().clear();
                        cx.notify();
                    })
                    .unwrap()
            });
            pump(&mut cx, 40);
            cx.capture_screenshot(window.into()).unwrap();
            let probes =
                cx.update(|cx| window.read(cx).map(|l| l.probes.borrow().clone()).unwrap());
            let at = format!("{w}x{h} {} decks, {what}", if four { 4 } else { 2 });
            for spill in crate::probe::spills(&probes) {
                failures.push(format!("{at}: {spill}"));
            }
            // And nothing past the window's own edges.
            for (name, b) in &probes {
                let (r, bt) = (
                    f32::from(b.origin.x + b.size.width),
                    f32::from(b.origin.y + b.size.height),
                );
                if r > w + 0.5 || bt > h + 0.5 {
                    failures.push(format!(
                        "{at}: {name} goes past the window ({r:.0}, {bt:.0})"
                    ));
                }
            }
            // What should be there, is.
            let full = what.contains("full screen");
            for d in 0..2 {
                if !full
                    && !probes
                        .keys()
                        .any(|k| k == &format!("deck{d}") || k == &format!("deck{}", d + 2))
                {
                    failures.push(format!("{at}: no card for deck {d}'s side"));
                }
            }
            if full && probes.keys().any(|k| k.starts_with("deck")) {
                failures.push(format!("{at}: decks drawn under the full-screen library"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} spills:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The edges between panels, dragged, size them; double-clicked, they're back as they were;
/// the library shuts, opens and goes full screen by its keys.
#[gpui::test]
fn panels_resize_and_the_library_shuts(cx: &mut gpui::TestAppContext) {
    use crate::layout::{Layout, LibView};
    use gpui::{Modifiers, MouseButton};
    test_home();
    cx.update(|cx| {
        crate::ui::text::bind_keys(cx);
        crate::live::bind_keys(cx);
    });
    let rate = 48_000.0;
    let (e, remote) = engine(rate);
    let (live, cx) = cx.add_window_view(|window, cx| {
        Live::new(rate, remote, Some(e), Library::default(), window, cx)
    });
    cx.run_until_parked();
    let grip = |live: &gpui::Entity<Live>, cx: &mut gpui::VisualTestContext, name: &str| {
        live.read_with(cx, |l, _| l.probes.borrow()[name].center())
    };
    let drag = |cx: &mut gpui::VisualTestContext,
                from: gpui::Point<gpui::Pixels>,
                by: gpui::Point<gpui::Pixels>| {
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(from + by, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(from + by, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
    };
    // The waveforms: 30 px down is 15 px a waveform (two of them).
    let before = live.read_with(cx, |l, _| l.wave_h);
    let at = grip(&live, cx, "grip Waves");
    drag(cx, at, gpui::point(px(0.), px(-30.)));
    let after = live.read_with(cx, |l, _| l.settings.layout.waves);
    assert!((after - (before - 15.0)).abs() < 0.5, "{before} → {after}");
    // The library's sidebar.
    let side = live.read_with(cx, |l, _| l.settings.layout.side);
    let at = grip(&live, cx, "grip Side");
    drag(cx, at, gpui::point(px(60.), px(0.)));
    assert_eq!(
        live.read_with(cx, |l, _| l.settings.layout.side),
        side + 60.0
    );
    // Double-clicked, back as it was.
    let at = grip(&live, cx, "grip Side");
    cx.simulate_event(gpui::MouseDownEvent {
        button: MouseButton::Left,
        position: at,
        modifiers: Modifiers::none(),
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        live.read_with(cx, |l, _| l.settings.layout.side),
        Layout::default().side
    );
    // The library by its keys.
    let lib = |live: &gpui::Entity<Live>, cx: &mut gpui::VisualTestContext| {
        live.read_with(cx, |l, _| l.settings.layout.library)
    };
    cx.simulate_keystrokes("cmd-l");
    assert_eq!(lib(&live, cx), LibView::Closed);
    cx.simulate_keystrokes("cmd-l");
    assert_eq!(lib(&live, cx), LibView::Open);
    cx.simulate_keystrokes("cmd-shift-l");
    assert_eq!(lib(&live, cx), LibView::Full);
    cx.simulate_keystrokes("escape");
    assert_eq!(lib(&live, cx), LibView::Open);
}
