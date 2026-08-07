//! Plain data: what `session.json` holds, how a saved rectangle becomes a
//! window someone can actually reach, and which tools the sidebar lists.
//!
//! None of the three needs a window to be tested, which is the point —
//! restoring geometry and resolving a stored tool list are the parts of session
//! restoration that fail, and they fail in ways a screenshot would not catch.

pub mod document;
pub mod features;
pub mod geometry;
