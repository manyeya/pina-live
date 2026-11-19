//! The laptop's keys, for DJing without a controller: the left hand on deck A, the right on B.
//! Q and P play, A and L cue (held, as the button), Z and , sync, W and O a four-beat loop,
//! 1 to 4 and 7 to 0 hot cues (held). Only while nothing that takes typing has the keys.

use gpui::{Context, KeyDownEvent, KeyUpEvent, Window};
use pina_live_engine::deck::DeckCmd;

use crate::{controls::Hold, live::Live};

/// What a key does: a deck and a press, or a hold.
enum Key {
    Press(usize, fn(&Live, usize) -> DeckCmd),
    Hold(usize, Hold),
}

fn key(k: &str) -> Option<Key> {
    let hot = |d, n| Some(Key::Hold(d, Hold::HotCue(n)));
    match k {
        "q" => Some(Key::Press(0, |_, _| DeckCmd::Play)),
        "p" => Some(Key::Press(1, |_, _| DeckCmd::Play)),
        "a" => Some(Key::Hold(0, Hold::Cue)),
        "l" => Some(Key::Hold(1, Hold::Cue)),
        "z" => Some(Key::Press(0, |l, d| {
            DeckCmd::Sync(!l.status().decks[d].sync)
        })),
        "," => Some(Key::Press(1, |l, d| {
            DeckCmd::Sync(!l.status().decks[d].sync)
        })),
        "w" => Some(Key::Press(0, |_, _| DeckCmd::AutoLoop(4.0))),
        "o" => Some(Key::Press(1, |_, _| DeckCmd::AutoLoop(4.0))),
        "1" => hot(0, 0),
        "2" => hot(0, 1),
        "3" => hot(0, 2),
        "4" => hot(0, 3),
        "7" => hot(1, 0),
        "8" => hot(1, 1),
        "9" => hot(1, 2),
        "0" => hot(1, 3),
        _ => None,
    }
}

impl Live {
    pub fn key_down(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let m = e.keystroke.modifiers;
        if e.is_held
            || m.platform
            || m.control
            || m.alt
            || !self.focus_handle_ref().is_focused(window)
        {
            return;
        }
        match key(&e.keystroke.key) {
            Some(Key::Press(d, cmd)) => {
                let c = cmd(self, d);
                self.act(d, c);
                cx.notify();
            }
            Some(Key::Hold(d, h)) => self.hold(d, h, cx),
            None => {}
        }
    }

    pub fn key_up(&mut self, e: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(Key::Hold(d, h)) = key(&e.keystroke.key)
            && self.holding(d, h)
            && let Some(drag) = self.drag.take()
        {
            self.drag_end(drag, cx);
        }
    }
}
