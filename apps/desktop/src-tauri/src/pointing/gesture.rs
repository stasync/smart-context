//! The hold-to-point gesture: turns global input into lens actions, and
//! decides which events the app underneath never sees (docs/PLAN.md 3.1).

use std::time::{Duration, Instant};

use crate::platform::{Disposition, InputEvent, Key, Point};

/// A hold shorter than this never asks (docs/PLAN.md section 14). Starting value.
pub const MIN_HOLD: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    /// The hotkey went down: show the lens at the cursor.
    Show(Point),
    /// The cursor moved while aiming.
    Move(Point),
    /// Plain scroll while aiming, in points: resize the lens.
    Resize(f64),
    /// Shift+scroll while aiming, in points: step between an element and its
    /// parent or child.
    Step(f64),
    /// Hide the lens without asking.
    Cancel,
    /// Hide the lens and ask the default question about what's under it.
    Ask,
    /// Space while holding: freeze the lens and let the user type a question.
    AskMode,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Idle,
    Aiming {
        since: Instant,
    },
    /// The gesture ended while the hotkey is still held. Waits for its release.
    Finished,
}

#[derive(Debug)]
pub struct Gesture {
    state: State,
    /// Keys whose key-down was swallowed, so their repeats and key-up are too.
    swallowed: Vec<Key>,
}

impl Default for Gesture {
    fn default() -> Self {
        Self {
            state: State::Idle,
            swallowed: Vec::new(),
        }
    }
}

impl Gesture {
    pub fn is_idle(&self) -> bool {
        self.state == State::Idle
    }

    /// Ends any gesture in progress, for example when pointing is switched off.
    pub fn reset(&mut self) -> Option<Action> {
        let was_aiming = matches!(self.state, State::Aiming { .. });
        self.state = State::Idle;
        was_aiming.then_some(Action::Cancel)
    }

    pub fn handle(&mut self, event: InputEvent, now: Instant) -> (Disposition, Option<Action>) {
        use Disposition::{Pass, Swallow};

        if event == InputEvent::HooksStopped {
            // Key-ups for swallowed keys may never come now.
            self.swallowed.clear();
            return (Pass, self.reset());
        }

        match event {
            InputEvent::KeyUp(key) if self.forget_swallowed(key) => return (Swallow, None),
            InputEvent::KeyDown { key, repeat: true } if self.swallowed.contains(&key) => {
                return (Swallow, None);
            }
            _ => {}
        }

        match self.state {
            State::Idle => match event {
                InputEvent::HotkeyDown(p) => {
                    self.state = State::Aiming { since: now };
                    (Pass, Some(Action::Show(p)))
                }
                _ => (Pass, None),
            },

            State::Aiming { since } => match event {
                InputEvent::HotkeyUp => {
                    self.state = State::Idle;
                    let held_long_enough = now.duration_since(since) >= MIN_HOLD;
                    (
                        Pass,
                        Some(if held_long_enough {
                            Action::Ask
                        } else {
                            Action::Cancel
                        }),
                    )
                }
                InputEvent::KeyDown {
                    key: Key::Escape, ..
                } => self.finish_swallowing(Key::Escape, Action::Cancel),
                InputEvent::KeyDown {
                    key: Key::Space, ..
                } => self.finish_swallowing(Key::Space, Action::AskMode),
                // Typing with the hotkey held (Option+key characters and
                // shortcuts) or clicking: the user isn't pointing. Step aside.
                InputEvent::KeyDown { .. }
                | InputEvent::OtherModifier
                | InputEvent::MouseDown(_) => {
                    self.state = State::Finished;
                    (Pass, Some(Action::Cancel))
                }
                InputEvent::MouseMoved(p) => (Pass, Some(Action::Move(p))),
                InputEvent::Scroll {
                    delta,
                    shift: false,
                } => (Swallow, Some(Action::Resize(delta))),
                InputEvent::Scroll { delta, shift: true } => (Swallow, Some(Action::Step(delta))),
                InputEvent::HotkeyDown(_) | InputEvent::KeyUp(_) | InputEvent::HooksStopped => {
                    (Pass, None)
                }
            },

            State::Finished => {
                if event == InputEvent::HotkeyUp {
                    self.state = State::Idle;
                }
                (Pass, None)
            }
        }
    }

    fn finish_swallowing(&mut self, key: Key, action: Action) -> (Disposition, Option<Action>) {
        self.state = State::Finished;
        self.swallowed.push(key);
        (Disposition::Swallow, Some(action))
    }

    fn forget_swallowed(&mut self, key: Key) -> bool {
        let before = self.swallowed.len();
        self.swallowed.retain(|k| *k != key);
        self.swallowed.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Disposition::{Pass, Swallow};

    const CURSOR: Point = Point { x: 100.0, y: 200.0 };

    /// Feeds events at fixed times (ms after the start) and collects the results.
    fn run(events: &[(u64, InputEvent)]) -> Vec<(Disposition, Option<Action>)> {
        let start = Instant::now();
        let mut gesture = Gesture::default();
        events
            .iter()
            .map(|(ms, e)| gesture.handle(*e, start + Duration::from_millis(*ms)))
            .collect()
    }

    fn key_down(key: Key) -> InputEvent {
        InputEvent::KeyDown { key, repeat: false }
    }

    #[test]
    fn holding_then_releasing_asks() {
        let out = run(&[
            (0, InputEvent::HotkeyDown(CURSOR)),
            (500, InputEvent::HotkeyUp),
        ]);
        assert_eq!(out[0], (Pass, Some(Action::Show(CURSOR))));
        assert_eq!(out[1], (Pass, Some(Action::Ask)));
    }

    #[test]
    fn a_short_tap_never_asks() {
        let out = run(&[
            (0, InputEvent::HotkeyDown(CURSOR)),
            (MIN_HOLD.as_millis() as u64 - 1, InputEvent::HotkeyUp),
        ]);
        assert_eq!(out[1], (Pass, Some(Action::Cancel)));
    }

    #[test]
    fn typing_while_holding_cancels_and_lets_keys_through() {
        let a = Key::Other(0);
        let out = run(&[
            (0, InputEvent::HotkeyDown(CURSOR)),
            (300, key_down(a)),
            (310, InputEvent::KeyUp(a)),
            (320, key_down(Key::Space)),
            (400, InputEvent::HotkeyUp),
        ]);
        assert_eq!(out[1], (Pass, Some(Action::Cancel)));
        assert_eq!(out[2], (Pass, None));
        // Option+Space types a character once the gesture is over.
        assert_eq!(out[3], (Pass, None));
        assert_eq!(out[4], (Pass, None), "releasing after a cancel doesn't ask");
    }

    #[test]
    fn other_modifiers_and_clicks_cancel() {
        for event in [InputEvent::OtherModifier, InputEvent::MouseDown(CURSOR)] {
            let out = run(&[(0, InputEvent::HotkeyDown(CURSOR)), (300, event)]);
            assert_eq!(out[1], (Pass, Some(Action::Cancel)), "{event:?}");
        }
    }

    #[test]
    fn escape_cancels_and_is_swallowed_through_its_release() {
        let out = run(&[
            (0, InputEvent::HotkeyDown(CURSOR)),
            (300, key_down(Key::Escape)),
            (
                350,
                InputEvent::KeyDown {
                    key: Key::Escape,
                    repeat: true,
                },
            ),
            (400, InputEvent::HotkeyUp),
            (450, InputEvent::KeyUp(Key::Escape)),
            (500, key_down(Key::Escape)),
        ]);
        assert_eq!(out[1], (Swallow, Some(Action::Cancel)));
        assert_eq!(out[2], (Swallow, None));
        assert_eq!(out[3], (Pass, None));
        assert_eq!(out[4], (Swallow, None), "key-up after the hotkey's release");
        assert_eq!(out[5], (Pass, None), "a later Esc reaches the app");
    }

    #[test]
    fn stopped_hooks_end_the_gesture() {
        let out = run(&[
            (0, InputEvent::HotkeyDown(CURSOR)),
            (300, key_down(Key::Escape)),
            (350, InputEvent::HooksStopped),
            (400, InputEvent::KeyUp(Key::Escape)),
            (500, InputEvent::HotkeyDown(CURSOR)),
            (600, InputEvent::HooksStopped),
        ]);
        assert_eq!(out[2], (Pass, None), "already finished by Esc");
        assert_eq!(out[3], (Pass, None), "nothing is swallowed afterwards");
        assert_eq!(out[4], (Pass, Some(Action::Show(CURSOR))));
        assert_eq!(out[5], (Pass, Some(Action::Cancel)), "hides the lens");
    }

    #[test]
    fn space_switches_to_ask_mode() {
        let out = run(&[
            (0, InputEvent::HotkeyDown(CURSOR)),
            (300, key_down(Key::Space)),
            (400, InputEvent::KeyUp(Key::Space)),
            (500, InputEvent::HotkeyUp),
        ]);
        assert_eq!(out[1], (Swallow, Some(Action::AskMode)));
        assert_eq!(out[2], (Swallow, None));
        assert_eq!(
            out[3],
            (Pass, None),
            "releasing after ask mode doesn't ask again"
        );
    }

    #[test]
    fn scrolling_while_aiming_is_swallowed() {
        let out = run(&[
            (
                0,
                InputEvent::Scroll {
                    delta: 5.0,
                    shift: false,
                },
            ),
            (10, InputEvent::HotkeyDown(CURSOR)),
            (
                20,
                InputEvent::Scroll {
                    delta: 5.0,
                    shift: false,
                },
            ),
            (
                30,
                InputEvent::Scroll {
                    delta: -3.0,
                    shift: true,
                },
            ),
        ]);
        assert_eq!(
            out[0],
            (Pass, None),
            "scrolling without the hotkey is untouched"
        );
        assert_eq!(out[2], (Swallow, Some(Action::Resize(5.0))));
        assert_eq!(out[3], (Swallow, Some(Action::Step(-3.0))));
    }

    #[test]
    fn mouse_moves_are_reported_only_while_aiming() {
        let moved = Point { x: 150.0, y: 250.0 };
        let out = run(&[
            (0, InputEvent::MouseMoved(moved)),
            (10, InputEvent::HotkeyDown(CURSOR)),
            (20, InputEvent::MouseMoved(moved)),
        ]);
        assert_eq!(out[0], (Pass, None));
        assert_eq!(out[2], (Pass, Some(Action::Move(moved))));
    }

    #[test]
    fn a_stray_release_is_ignored() {
        assert_eq!(run(&[(0, InputEvent::HotkeyUp)])[0], (Pass, None));
    }

    #[test]
    fn reset_cancels_only_while_aiming() {
        let mut gesture = Gesture::default();
        assert_eq!(gesture.reset(), None);
        gesture.handle(InputEvent::HotkeyDown(CURSOR), Instant::now());
        assert_eq!(gesture.reset(), Some(Action::Cancel));
        assert_eq!(gesture.state, State::Idle);
    }
}
