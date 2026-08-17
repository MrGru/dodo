//! Canvas interaction: what the user is in the middle of doing, and what the
//! view must do about it.
//!
//! The split is the crate's usual one and it is worth as much here as anywhere:
//! [`state`] is the machine and it names no UI framework, so every transition
//! is asserted with no window; `views/flow.rs` is the glue that turns GPUI's
//! `MouseDownEvent` into an [`InteractionEvent`] and an
//! [`InteractionEffect`] back into a `capture_pointer` or a `cx.notify()`.
//!
//! What is under the pointer is **passed in** rather than looked up: a
//! `PointerDown` carries a [`PointerTarget`](crate::runtime::PointerTarget), so
//! one press means a pan, a box selection, a node drag or a connection without
//! this module knowing that a graph exists. §29's broad phase stays the
//! caller's.
//!
//! Zoom is deliberately **not** in the machine. It is stateless — every wheel
//! notch and every pinch delta is a complete instruction — so modelling it as a
//! state would add a variant that is entered and left within one event, and
//! `Viewport::zoom_by` already owns the arithmetic (§22).

pub mod state;

pub use state::{
    BoxSelection, ConnectionSource, InputModifiers, InteractionEffect, InteractionEvent,
    InteractionMachine, InteractionState, PendingConnection, PointerButton,
};
