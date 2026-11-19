//! The installer's screens: ready to install, installing, removing.

use super::*;

impl Installer {
    /// The right side before anything's done: what installing means here, and the button.
    pub(crate) fn ready(&self, cx: &mut Context<Self>) -> Div {
        let (title, action, note) = match &self.over {
            Over::Nothing => ("Install Pina Live".to_string(), "Install".to_string(), None),
            Over::Older(old) => (
                "Update Pina Live".to_string(),
                format!("Update to {}", self.version),
                Some(format!("Replaces {old}. Your library and settings stay.")),
            ),
            Over::Same => (
                "Pina Live is installed".to_string(),
                "Reinstall".to_string(),
                Some(format!(
                    "{} is here already. Your library and settings stay.",
                    self.version
                )),
            ),
            Over::Newer(newer) => (
                "A newer Pina Live is installed".to_string(),
                format!("Replace with {}", self.version),
                Some(format!("{newer} is here: this one's older.")),
            ),
        };
        let installed = self.over != Over::Nothing;
        let where_to = self.plan.apps.display().to_string().replace(&home(), "~");
        // In turn: the words, each row, the button. The first time, after Pina Live's own entrance.
        let since = self.stage_at.elapsed().as_secs_f32();
        let lead = if self.started == self.stage_at {
            0.45
        } else {
            0.0
        };
        let at = |k: f32| appear(since, lead + k, 0.55);
        let row = |title: &'static str, about: String, on: Option<bool>, sel: &'static str| {
            div()
                .min_h(px(58.))
                .px(px(14.))
                .py(px(10.))
                .flex()
                .items_center()
                .gap(px(12.))
                .rounded(px(10.))
                .bg(rgb(CARD))
                .border_1()
                .border_color(rgb(LINE))
                .debug_selector(move || sel.into())
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .child(
                            div()
                                .text_size(px(13.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(rgb(TEXT_3))
                                .overflow_hidden()
                                .child(about),
                        ),
                )
                .child(match on {
                    Some(on) => switch(on),
                    None => div()
                        .text_size(px(11.5))
                        .text_color(rgb(TEXT_3))
                        .child("Always"),
                })
                .when(on.is_some(), |d| {
                    d.cursor_pointer().hover(|d| d.bg(rgb(0x1f1f25)))
                })
        };

        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(rise(
                        div()
                            .text_size(px(24.))
                            .font_weight(FontWeight::BOLD)
                            .child(title),
                        at(0.0),
                    ))
                    .child(rise(
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(TEXT_3))
                            .child(match note {
                                Some(n) => n,
                                None => format!("Version {} · for {}", self.version, self.archs),
                            }),
                        at(0.07),
                    )),
            )
            .child(div().mt(px(6.)).flex().flex_col().gap(px(8.)).child(rise(
                row("Pina Live", format!("Into {where_to}"), None, "install-app"),
                at(0.16),
            )))
            .child(div().flex_1())
            .child(rise(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .when(installed, |d| {
                        d.child(
                            div()
                                .text_size(px(12.5))
                                .text_color(rgb(TEXT_3))
                                .cursor_pointer()
                                .hover(|d| d.text_color(rgb(TEXT_2)))
                                .debug_selector(|| "install-uninstall".into())
                                .child("Uninstall…")
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|v, _, _, cx| {
                                        v.set_stage(Stage::Removing);
                                        cx.notify();
                                    }),
                                ),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        button(action, true)
                            .debug_selector(|| "install-go".into())
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|v, _, _, cx| v.start(cx)),
                            ),
                    ),
                at(0.40),
            ))
    }

    /// The right side while it works, and once it's done or stuck.
    pub(crate) fn progress(&self, t: f32, cx: &mut Context<Self>) -> Div {
        let (title, sub) = match &self.stage {
            Stage::Waiting => (
                "Quit Pina Live to go on".to_string(),
                "It's open. Quit it, and installing goes on by itself.".to_string(),
            ),
            Stage::Working => (
                "Installing…".to_string(),
                format!("Pina Live {}", self.version),
            ),
            Stage::Done => (
                "Pina Live is ready".to_string(),
                "Everything's in place. Open it and start mixing.".to_string(),
            ),
            Stage::Failed(why) => ("That didn't work".to_string(), why.clone()),
            _ => (String::new(), String::new()),
        };
        let done = self.stage == Stage::Done;
        let working = matches!(self.stage, Stage::Working | Stage::Waiting);
        let since = self.stage_at.elapsed().as_secs_f32();
        let at = |k: f32| appear(since, k, 0.5);
        let steps = self.steps.iter().enumerate().map(|(i, (job, state))| {
            let detail = match state {
                State::Done(d) | State::Skipped(d) => Some(d.clone()),
                _ => None,
            };
            let finished = self
                .step_done
                .get(i)
                .copied()
                .flatten()
                .map(|at| at.elapsed().as_secs_f32());
            // The step running glows a little; its details rise in when it's done.
            let running = *state == State::Running;
            rise(
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .py(px(7.))
                    .px(px(8.))
                    .rounded(px(8.))
                    .when(running, |d| {
                        d.bg(alpha(ACCENT, 0.06 + 0.04 * (t * 3.0).sin().abs()))
                    })
                    .child(div().pt(px(1.)).child(mark(state, t, finished)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .text_color(rgb(match state {
                                        State::Waiting => TEXT_3,
                                        _ => TEXT,
                                    }))
                                    .child(job.title()),
                            )
                            .when_some(detail, |d, text| {
                                let p = appear(finished.unwrap_or(1.0), 0.1, 0.35);
                                d.child(
                                    div()
                                        .opacity(p)
                                        .text_size(px(11.5))
                                        .font_family(MONO)
                                        .text_color(rgb(TEXT_3))
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .child(text),
                                )
                            }),
                    ),
                at(0.12 + i as f32 * 0.07),
            )
        });
        let bar_w = 404.0;
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .when(done, |d| d.child(big_check(since)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(rise(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .text_size(px(24.))
                                    .font_weight(FontWeight::BOLD)
                                    .child(title)
                                    .when(self.stage == Stage::Waiting, |d| {
                                        d.child(
                                            div()
                                                .size(px(8.))
                                                .rounded(px(4.))
                                                .bg(rgb(CURVE))
                                                .opacity(0.35 + 0.65 * (t * 3.0).sin().abs()),
                                        )
                                    }),
                                at(if done { 0.35 } else { 0.0 }),
                            ))
                            .child(rise(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(rgb(if matches!(self.stage, Stage::Failed(_)) {
                                        RECORD
                                    } else {
                                        TEXT_3
                                    }))
                                    .child(sub),
                                at(if done { 0.45 } else { 0.06 }),
                            )),
                    ),
            )
            // The bar: gliding to where it's got to, a shine going along it while it works.
            .child(
                div()
                    .relative()
                    .h(px(4.))
                    .rounded(px(2.))
                    .overflow_hidden()
                    .bg(rgb(RAISED))
                    .child(
                        div()
                            .h(px(4.))
                            .rounded(px(2.))
                            .bg(rgb(if done { MINT } else { ACCENT }))
                            .w(gpui::relative(self.shown.clamp(0.0, 1.0))),
                    )
                    .when(working, |d| {
                        let x = ((t * 0.7) % 1.3 - 0.15) * bar_w;
                        d.child(
                            div()
                                .absolute()
                                .top(px(0.))
                                .left(px(x))
                                .w(px(70.))
                                .h(px(4.))
                                .rounded(px(2.))
                                .bg(alpha(0xffffff, 0.35)),
                        )
                    }),
            )
            .child(div().flex().flex_col().gap(px(2.)).children(steps))
            .child(div().flex_1())
            .child(rise(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(10.))
                    .when(done || matches!(self.stage, Stage::Failed(_)), |d| {
                        d.child(
                            button("Close", false)
                                .debug_selector(|| "install-close".into())
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|_, _, _, cx| cx.quit()),
                                ),
                        )
                    })
                    .when(matches!(self.stage, Stage::Failed(_)), |d| {
                        d.child(button("Try again", true).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|v, _, _, cx| v.start(cx)),
                        ))
                    })
                    .when(done, |d| {
                        d.child(
                            button("Open Pina Live", true)
                                .debug_selector(|| "install-open".into())
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|v, _, _, cx| {
                                        install::launch(&v.plan.app());
                                        install::eject_later();
                                        cx.quit();
                                    }),
                                ),
                        )
                    }),
                if working { 1.0 } else { at(0.6) },
            ))
    }

    /// Taking Pina Live out: what goes and what stays, asked first; then done.
    pub(crate) fn removing(&self, cx: &mut Context<Self>) -> Div {
        let removed = self.stage == Stage::Removed;
        let since = self.stage_at.elapsed().as_secs_f32();
        let at = |k: f32| appear(since, k, 0.5);
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(rise(
                div()
                    .text_size(px(24.))
                    .font_weight(FontWeight::BOLD)
                    .child(if removed {
                        "Pina Live is uninstalled"
                    } else {
                        "Uninstall Pina Live?"
                    }),
                at(0.0),
            ))
            .child(rise(
                div()
                    .text_size(px(13.))
                    .line_height(px(19.))
                    .text_color(rgb(TEXT_3))
                    .child(match removed {
                        true => "It's in the Trash. Your music is where it was.",
                        false => "Pina Live goes to the Trash. Your music stays where it is.",
                    }),
                at(0.07),
            ))
            .when(!removed, |d| {
                d.child(rise(
                    div()
                        .mt(px(4.))
                        .min_h(px(58.))
                        .px(px(14.))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .rounded(px(10.))
                        .bg(rgb(CARD))
                        .border_1()
                        .border_color(rgb(LINE))
                        .cursor_pointer()
                        .debug_selector(|| "uninstall-settings".into())
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(3.))
                                .child(
                                    div()
                                        .text_size(px(13.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Its library and settings too"),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(rgb(TEXT_3))
                                        .child("Crates, tracks it has studied, stems it has parted and its settings: to the Trash as well. Your music isn't touched."),
                                ),
                        )
                        .child(switch(self.and_settings))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|v, _, _, cx| {
                                v.and_settings = !v.and_settings;
                                cx.notify();
                            }),
                        ),
                    at(0.16),
                ))
            })
            .child(div().flex_1())
            .child(rise(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(10.))
                    .when(removed, |d| {
                        d.child(button("Close", true).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|_, _, _, cx| cx.quit()),
                        ))
                    })
                    .when(!removed, |d| {
                        d.child(button("Cancel", false).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|v, _, _, cx| {
                                v.set_stage(Stage::Ready);
                                cx.notify();
                            }),
                        ))
                        .child(
                            button("Move to Trash", true)
                                .bg(rgb(0xc93c3c))
                                .debug_selector(|| "uninstall-go".into())
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|v, _, _, cx| v.remove(cx)),
                                ),
                        )
                    }),
                at(0.3),
            ))
    }
}
