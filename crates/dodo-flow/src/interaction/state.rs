//! [`InteractionMachine`] — **one explicit state, not a drawer of booleans**
//! (§25).
//!
//! The requirement is unusually specific about the shape, and it is specific
//! because the failure it prevents is so ordinary: `is_panning`,
//! `is_box_selecting`, `drag_start`, `did_move` accumulate one at a time, each
//! addition is locally reasonable, and the bug is always the same — two of them
//! true at once, in a combination nobody wrote down and nobody can test. An
//! enum makes that state unrepresentable, and it makes the transitions a pure
//! function of (state, event), which is the reason every one of them is
//! asserted below with no window anywhere in sight.
//!
//! # The vocabulary is deliberately not GPUI's
//!
//! [`PointerButton`], [`InputModifiers`] and [`InteractionEvent`] restate what
//! `MouseDownEvent` and friends already carry. That duplication is the price of
//! the crate's central boundary — this file is unit tested with no `App`, no
//! window and no event loop — and it buys a second thing worth having: the
//! machine takes **both** the screen and the world position of the pointer,
//! because it needs screen for panning and world for box selection, and
//! `views/` is the only place that knows the pane's origin.
//!
//! # Which space each state remembers, and why it matters
//!
//! - [`InteractionState::Panning`] remembers the last **screen** position. A
//!   pan is a screen-space displacement; remembering it in world units would
//!   make it change meaning as the pan proceeded.
//! - [`InteractionState::BoxSelecting`] remembers **world** positions. The
//!   rectangle is anchored to the document, so zooming or scrolling mid-drag
//!   leaves it over the same content instead of sliding out from under it.
//!
//! # What this phase deliberately does not do
//!
//! [`InteractionEffect::CommitBoxSelect`] hands back a world rectangle and
//! stops. Turning that rectangle into a set of element ids needs the spatial
//! index's broad phase (§28), which is Phase 4's — and a linear scan over every
//! element would be exactly the thing §40 rule 1 forbids, written in a place it
//! would be easy to forget to remove. The states §25 lists beyond these three
//! (`DraggingElements`, `Connecting`, `Resizing`, …) arrive with the phases
//! that can implement them; the enum is where they will go.
//!
//! **This file names no UI framework.**

use crate::geometry::{Rect, Vec2};

/// The pointer buttons the canvas distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerButton {
    Left,
    Middle,
    Right,
}

/// The keyboard modifiers held when a pointer event happened.
///
/// `command` and `control` are kept apart rather than collapsed into one
/// "platform modifier": §26 asks for configurable bindings, and a binding table
/// that cannot tell them apart cannot express a platform difference.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct InputModifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub command: bool,
}

impl InputModifiers {
    pub const NONE: InputModifiers = InputModifiers {
        shift: false,
        control: false,
        alt: false,
        command: false,
    };

    pub fn shift() -> InputModifiers {
        InputModifiers {
            shift: true,
            ..InputModifiers::NONE
        }
    }

    /// Whether this modifier set means "add to the selection rather than
    /// replace it" — §26's shift-select.
    pub fn is_additive(&self) -> bool {
        self.shift || self.command
    }
}

/// What the canvas is in the middle of.
///
/// Every variant owns the data that only makes sense while it is current, which
/// is the property that makes the illegal combinations unrepresentable rather
/// than merely unlikely.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum InteractionState {
    #[default]
    Idle,
    Panning {
        /// The button that started the pan, so only that button ends it.
        button: PointerButton,
        /// Screen pixels. See the module doc for why not world.
        last_screen: Vec2,
    },
    BoxSelecting {
        /// World units, so the rectangle stays over the same content if the
        /// view zooms or scrolls mid-drag.
        anchor_world: Vec2,
        current_world: Vec2,
        /// Captured at press time rather than read at release: a user who lets
        /// go of shift before the mouse button still meant an additive select.
        additive: bool,
    },
}

/// What happened, in the machine's own vocabulary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InteractionEvent {
    PointerDown {
        screen: Vec2,
        world: Vec2,
        button: PointerButton,
        modifiers: InputModifiers,
        /// Whether the "pan instead" key — space, in dodo's default binding —
        /// was held. Passed in rather than read from `modifiers` because it is
        /// a key rather than a modifier, and because §26 wants it rebindable.
        pan_key_held: bool,
    },
    PointerMove {
        screen: Vec2,
        world: Vec2,
    },
    PointerUp {
        button: PointerButton,
    },
    /// `Esc`, a lost window focus, or anything else that means "stop".
    Cancel,
}

/// A completed box selection, handed to whatever can resolve it into elements.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxSelection {
    /// World units, always normalised — dragging up-left gives the same
    /// rectangle as dragging down-right.
    pub rect: Rect,
    pub additive: bool,
}

/// What the caller must do about a transition.
///
/// One effect per event, which is enough because no transition here needs two.
/// The view's whole job is a `match` over this, so a new variant is a compile
/// error at the one place that has to handle it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InteractionEffect {
    /// Nothing to do, and — importantly — nothing to repaint.
    None,
    /// A pan started. The view captures the pointer so the drag survives
    /// leaving the pane.
    BeginPan,
    /// Move the viewport by this screen-space delta.
    PanBy(Vec2),
    EndPan,
    /// A box selection started, with its (degenerate) initial rectangle.
    BeginBoxSelect(Rect),
    /// The selection rectangle changed; repaint it.
    UpdateBoxSelect(Rect),
    /// The drag finished. Phase 4 resolves this into element ids.
    CommitBoxSelect(BoxSelection),
    /// The drag was abandoned; stop drawing the rectangle.
    CancelBoxSelect,
}

impl InteractionEffect {
    /// Whether the view should call `Window::capture_pointer`, so the drag
    /// keeps receiving moves once the pointer leaves the canvas. Capture
    /// auto-releases on mouse up, so there is no matching "release" question.
    pub fn starts_a_drag(&self) -> bool {
        matches!(
            self,
            InteractionEffect::BeginPan | InteractionEffect::BeginBoxSelect(_)
        )
    }

    /// Whether the canvas has to be repainted. dodo repaints on change and
    /// never on a timer (§35, §40 rule 15), so this is the whole condition for
    /// a `cx.notify()`.
    pub fn needs_repaint(&self) -> bool {
        !matches!(self, InteractionEffect::None)
    }
}

/// The canvas's interaction state, and the transitions between them.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct InteractionMachine {
    state: InteractionState,
}

impl InteractionMachine {
    pub fn new() -> InteractionMachine {
        InteractionMachine::default()
    }

    pub fn state(&self) -> &InteractionState {
        &self.state
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.state, InteractionState::Idle)
    }

    pub fn is_panning(&self) -> bool {
        matches!(self.state, InteractionState::Panning { .. })
    }

    /// The selection rectangle to draw, in **world** units, or `None` when no
    /// box selection is in progress. The painter's only question.
    pub fn selection_rect(&self) -> Option<Rect> {
        match self.state {
            InteractionState::BoxSelecting {
                anchor_world,
                current_world,
                ..
            } => Some(Rect::from_corners(anchor_world, current_world)),
            _ => None,
        }
    }

    /// **The transition function.** Pure, total, and the only way the state
    /// changes.
    pub fn handle(&mut self, event: InteractionEvent) -> InteractionEffect {
        match (self.state, event) {
            // ---- starting something, from rest ----
            (
                InteractionState::Idle,
                InteractionEvent::PointerDown {
                    screen,
                    world,
                    button,
                    modifiers,
                    pan_key_held,
                },
            ) => match button {
                // Middle-drag always pans, and space-drag turns a left press
                // into a pan — the two bindings every infinite canvas shares.
                PointerButton::Middle => {
                    self.state = InteractionState::Panning {
                        button,
                        last_screen: screen,
                    };
                    InteractionEffect::BeginPan
                }
                PointerButton::Left if pan_key_held => {
                    self.state = InteractionState::Panning {
                        button,
                        last_screen: screen,
                    };
                    InteractionEffect::BeginPan
                }
                PointerButton::Left => {
                    self.state = InteractionState::BoxSelecting {
                        anchor_world: world,
                        current_world: world,
                        additive: modifiers.is_additive(),
                    };
                    InteractionEffect::BeginBoxSelect(Rect::from_corners(world, world))
                }
                // The context menu is a later phase's, and swallowing the press
                // here would make it impossible to add without changing this
                // match — which is the point of it being explicit.
                PointerButton::Right => InteractionEffect::None,
            },

            // ---- a press while already busy ----
            //
            // Ignored rather than treated as a restart. A second button going
            // down mid-drag is almost always accidental, and the alternative —
            // silently switching modes under the user's hand — is the exact
            // class of bug the enum exists to prevent.
            (_, InteractionEvent::PointerDown { .. }) => InteractionEffect::None,

            // ---- dragging ----
            (
                InteractionState::Panning {
                    button,
                    last_screen,
                },
                InteractionEvent::PointerMove { screen, .. },
            ) => {
                self.state = InteractionState::Panning {
                    button,
                    last_screen: screen,
                };
                InteractionEffect::PanBy(screen - last_screen)
            }

            (
                InteractionState::BoxSelecting {
                    anchor_world,
                    additive,
                    ..
                },
                InteractionEvent::PointerMove { world, .. },
            ) => {
                self.state = InteractionState::BoxSelecting {
                    anchor_world,
                    current_world: world,
                    additive,
                };
                InteractionEffect::UpdateBoxSelect(Rect::from_corners(anchor_world, world))
            }

            (InteractionState::Idle, InteractionEvent::PointerMove { .. }) => {
                InteractionEffect::None
            }

            // ---- finishing ----
            (
                InteractionState::Panning {
                    button: started_with,
                    ..
                },
                InteractionEvent::PointerUp { button },
            ) => {
                if button == started_with {
                    self.state = InteractionState::Idle;
                    InteractionEffect::EndPan
                } else {
                    InteractionEffect::None
                }
            }

            (
                InteractionState::BoxSelecting {
                    anchor_world,
                    current_world,
                    additive,
                },
                InteractionEvent::PointerUp { button },
            ) => {
                if button == PointerButton::Left {
                    self.state = InteractionState::Idle;
                    InteractionEffect::CommitBoxSelect(BoxSelection {
                        rect: Rect::from_corners(anchor_world, current_world),
                        additive,
                    })
                } else {
                    InteractionEffect::None
                }
            }

            (InteractionState::Idle, InteractionEvent::PointerUp { .. }) => InteractionEffect::None,

            // ---- giving up ----
            (InteractionState::Idle, InteractionEvent::Cancel) => InteractionEffect::None,
            (InteractionState::Panning { .. }, InteractionEvent::Cancel) => {
                self.state = InteractionState::Idle;
                InteractionEffect::EndPan
            }
            (InteractionState::BoxSelecting { .. }, InteractionEvent::Cancel) => {
                self.state = InteractionState::Idle;
                InteractionEffect::CancelBoxSelect
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn down(button: PointerButton) -> InteractionEvent {
        InteractionEvent::PointerDown {
            screen: Vec2::new(100.0, 100.0),
            world: Vec2::new(10.0, 10.0),
            button,
            modifiers: InputModifiers::NONE,
            pan_key_held: false,
        }
    }

    fn move_to(screen: Vec2, world: Vec2) -> InteractionEvent {
        InteractionEvent::PointerMove { screen, world }
    }

    fn up(button: PointerButton) -> InteractionEvent {
        InteractionEvent::PointerUp { button }
    }

    #[test]
    fn a_fresh_machine_is_idle_and_draws_no_rectangle() {
        let machine = InteractionMachine::new();
        assert!(machine.is_idle());
        assert!(!machine.is_panning());
        assert_eq!(machine.selection_rect(), None);
    }

    #[test]
    fn a_middle_press_begins_a_pan_and_a_release_ends_it() {
        let mut machine = InteractionMachine::new();

        assert_eq!(
            machine.handle(down(PointerButton::Middle)),
            InteractionEffect::BeginPan
        );
        assert!(machine.is_panning());

        assert_eq!(
            machine.handle(move_to(Vec2::new(140.0, 130.0), Vec2::ZERO)),
            InteractionEffect::PanBy(Vec2::new(40.0, 30.0))
        );

        assert_eq!(
            machine.handle(up(PointerButton::Middle)),
            InteractionEffect::EndPan
        );
        assert!(machine.is_idle());
    }

    /// Pan deltas are relative to the last move, not to the press. If they were
    /// absolute the view would accelerate away from the pointer.
    #[test]
    fn pan_deltas_are_incremental() {
        let mut machine = InteractionMachine::new();
        machine.handle(down(PointerButton::Middle));

        assert_eq!(
            machine.handle(move_to(Vec2::new(110.0, 100.0), Vec2::ZERO)),
            InteractionEffect::PanBy(Vec2::new(10.0, 0.0))
        );
        assert_eq!(
            machine.handle(move_to(Vec2::new(115.0, 100.0), Vec2::ZERO)),
            InteractionEffect::PanBy(Vec2::new(5.0, 0.0)),
        );
    }

    #[test]
    fn space_turns_a_left_press_into_a_pan() {
        let mut machine = InteractionMachine::new();
        let effect = machine.handle(InteractionEvent::PointerDown {
            screen: Vec2::ZERO,
            world: Vec2::ZERO,
            button: PointerButton::Left,
            modifiers: InputModifiers::NONE,
            pan_key_held: true,
        });

        assert_eq!(effect, InteractionEffect::BeginPan);
        assert!(machine.is_panning());
        assert_eq!(machine.selection_rect(), None);
    }

    #[test]
    fn a_plain_left_press_begins_a_box_selection() {
        let mut machine = InteractionMachine::new();
        let effect = machine.handle(down(PointerButton::Left));

        assert_eq!(
            effect,
            InteractionEffect::BeginBoxSelect(Rect::new(Vec2::new(10.0, 10.0), Vec2::ZERO))
        );
        assert_eq!(
            machine.selection_rect(),
            Some(Rect::new(Vec2::new(10.0, 10.0), Vec2::ZERO))
        );
    }

    /// Dragging up and to the left has to give the same rectangle as dragging
    /// down and to the right — the reason the corners are normalised rather
    /// than subtracted.
    #[test]
    fn a_backwards_drag_normalises() {
        let mut machine = InteractionMachine::new();
        machine.handle(down(PointerButton::Left));
        machine.handle(move_to(Vec2::ZERO, Vec2::new(-30.0, -50.0)));

        let rect = machine
            .selection_rect()
            .expect("a rectangle is in progress");
        assert_eq!(rect.origin, Vec2::new(-30.0, -50.0));
        assert_eq!(rect.size, Vec2::new(40.0, 60.0));
    }

    #[test]
    fn releasing_commits_the_selection_and_returns_to_idle() {
        let mut machine = InteractionMachine::new();
        machine.handle(down(PointerButton::Left));
        machine.handle(move_to(Vec2::ZERO, Vec2::new(60.0, 40.0)));

        let effect = machine.handle(up(PointerButton::Left));

        assert_eq!(
            effect,
            InteractionEffect::CommitBoxSelect(BoxSelection {
                rect: Rect::new(Vec2::new(10.0, 10.0), Vec2::new(50.0, 30.0)),
                additive: false,
            })
        );
        assert!(machine.is_idle());
        assert_eq!(machine.selection_rect(), None);
    }

    /// Shift is read at press time. Letting go of it before the mouse button
    /// must not silently turn an additive select into a replacing one.
    #[test]
    fn additive_is_captured_when_the_drag_starts_not_when_it_ends() {
        let mut machine = InteractionMachine::new();
        machine.handle(InteractionEvent::PointerDown {
            screen: Vec2::ZERO,
            world: Vec2::ZERO,
            button: PointerButton::Left,
            modifiers: InputModifiers::shift(),
            pan_key_held: false,
        });
        machine.handle(move_to(Vec2::ZERO, Vec2::splat(10.0)));

        match machine.handle(up(PointerButton::Left)) {
            InteractionEffect::CommitBoxSelect(selection) => assert!(selection.additive),
            other => panic!("expected a commit, got {other:?}"),
        }
    }

    #[test]
    fn command_is_additive_too() {
        assert!(InputModifiers::shift().is_additive());
        assert!(
            InputModifiers {
                command: true,
                ..InputModifiers::NONE
            }
            .is_additive()
        );
        assert!(!InputModifiers::NONE.is_additive());
        assert!(
            !InputModifiers {
                alt: true,
                ..InputModifiers::NONE
            }
            .is_additive()
        );
    }

    #[test]
    fn escape_abandons_a_box_selection_without_committing_it() {
        let mut machine = InteractionMachine::new();
        machine.handle(down(PointerButton::Left));
        machine.handle(move_to(Vec2::ZERO, Vec2::splat(40.0)));

        assert_eq!(
            machine.handle(InteractionEvent::Cancel),
            InteractionEffect::CancelBoxSelect
        );
        assert!(machine.is_idle());
        assert_eq!(machine.selection_rect(), None);
    }

    #[test]
    fn escape_ends_a_pan() {
        let mut machine = InteractionMachine::new();
        machine.handle(down(PointerButton::Middle));

        assert_eq!(
            machine.handle(InteractionEvent::Cancel),
            InteractionEffect::EndPan
        );
        assert!(machine.is_idle());
    }

    /// Only the button that started the drag ends it. Right-clicking during a
    /// left-drag is not a release.
    #[test]
    fn a_different_button_does_not_end_the_drag() {
        let mut machine = InteractionMachine::new();
        machine.handle(down(PointerButton::Middle));

        assert_eq!(
            machine.handle(up(PointerButton::Left)),
            InteractionEffect::None
        );
        assert!(machine.is_panning());

        let mut machine = InteractionMachine::new();
        machine.handle(down(PointerButton::Left));
        assert_eq!(
            machine.handle(up(PointerButton::Right)),
            InteractionEffect::None
        );
        assert!(machine.selection_rect().is_some());
    }

    /// A second press mid-drag must not switch modes under the user's hand.
    #[test]
    fn a_press_while_busy_changes_nothing() {
        let mut machine = InteractionMachine::new();
        machine.handle(down(PointerButton::Middle));
        let before = *machine.state();

        assert_eq!(
            machine.handle(down(PointerButton::Left)),
            InteractionEffect::None
        );
        assert_eq!(*machine.state(), before);
    }

    #[test]
    fn a_right_press_starts_nothing_yet() {
        let mut machine = InteractionMachine::new();
        assert_eq!(
            machine.handle(down(PointerButton::Right)),
            InteractionEffect::None
        );
        assert!(machine.is_idle());
    }

    #[test]
    fn moving_while_idle_costs_nothing_and_repaints_nothing() {
        let mut machine = InteractionMachine::new();
        let effect = machine.handle(move_to(Vec2::splat(400.0), Vec2::splat(40.0)));

        assert_eq!(effect, InteractionEffect::None);
        assert!(!effect.needs_repaint());
        assert!(machine.is_idle());
    }

    /// Which effects the view has to act on. Stated as a test because the two
    /// predicates are the whole of the view's `match`, and getting
    /// `needs_repaint` wrong is either a stuck frame or an idle repaint loop.
    #[test]
    fn the_effects_that_capture_the_pointer_and_the_ones_that_repaint() {
        let rect = Rect::ZERO;
        let selection = BoxSelection {
            rect,
            additive: false,
        };

        for effect in [
            InteractionEffect::BeginPan,
            InteractionEffect::BeginBoxSelect(rect),
        ] {
            assert!(effect.starts_a_drag(), "{effect:?}");
        }
        for effect in [
            InteractionEffect::None,
            InteractionEffect::PanBy(Vec2::ZERO),
            InteractionEffect::EndPan,
            InteractionEffect::UpdateBoxSelect(rect),
            InteractionEffect::CommitBoxSelect(selection),
            InteractionEffect::CancelBoxSelect,
        ] {
            assert!(!effect.starts_a_drag(), "{effect:?}");
        }

        assert!(!InteractionEffect::None.needs_repaint());
        for effect in [
            InteractionEffect::BeginPan,
            InteractionEffect::PanBy(Vec2::ZERO),
            InteractionEffect::EndPan,
            InteractionEffect::BeginBoxSelect(rect),
            InteractionEffect::UpdateBoxSelect(rect),
            InteractionEffect::CommitBoxSelect(selection),
            InteractionEffect::CancelBoxSelect,
        ] {
            assert!(effect.needs_repaint(), "{effect:?}");
        }
    }

    /// The machine has to survive being driven by a stream it did not expect —
    /// a lost mouse-up, a move with no press, a cancel out of nowhere. It ends
    /// idle every time, and never panics.
    #[test]
    fn any_sequence_of_events_leaves_it_in_a_state_it_can_recover_from() {
        let events = [
            down(PointerButton::Left),
            down(PointerButton::Middle),
            move_to(Vec2::splat(1.0), Vec2::splat(1.0)),
            up(PointerButton::Right),
            InteractionEvent::Cancel,
            up(PointerButton::Left),
            move_to(Vec2::splat(9.0), Vec2::splat(9.0)),
            down(PointerButton::Right),
            up(PointerButton::Middle),
        ];

        // Every rotation of the sequence, so no ordering is privileged.
        for start in 0..events.len() {
            let mut machine = InteractionMachine::new();
            for offset in 0..events.len() {
                machine.handle(events[(start + offset) % events.len()]);
            }
            machine.handle(InteractionEvent::Cancel);
            assert!(machine.is_idle(), "stuck after rotation {start}");
        }
    }
}
