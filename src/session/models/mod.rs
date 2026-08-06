//! Plain data: what `session.json` holds, and how a saved rectangle becomes a
//! window someone can actually reach.
//!
//! Neither half needs a window to be tested, which is the point — restoring
//! geometry is the part of this feature that fails, and it fails in ways a
//! screenshot would not catch.

pub mod document;
pub mod geometry;
