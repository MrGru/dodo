//! Plain data for the Database Explorer.
//!
//! The rule this layer exists to enforce, and the reason most of the module's
//! risk is testable: **no GPUI, no driver crate, no other dodo module**. Every
//! type here is data, every function is pure, and `cargo test` covers them with
//! no server, no daemon and no `Window` — the same trick that makes the API
//! Explorer's send *ordering* unit-testable.
//!
//! What that buys, concretely: the memory bound on a result
//! ([`page`]), the way an oversized cell is reported ([`value`]), the rule for
//! where one statement ends and the next begins ([`split`]), what a saved
//! connection is and when it is incomplete ([`connection`]), and how the object
//! tree names things without any backend owning the vocabulary ([`catalog`]),
//! the password-free persisted query schema ([`library`]), the proof that a
//! row has a catalog-backed unique identity ([`identity`]),
//! and the sole generated-mutation SQL owner ([`statement`]).
//!
//! The two exceptions to "nothing but data", both deliberate:
//!
//! - [`error::DbError`] and a few others name [`Str`](crate::i18n::Str),
//!   because an error that cannot say itself in two languages is not finished.
//!   `Str` is itself plain data.
//! - [`sql_format`] names `sqlformat`. It is a pure text-in/text-out crate, not
//!   a driver, and that module is the only place allowed to name it.

pub mod catalog;
pub mod connection;
pub mod detail;
pub mod engine;
pub mod error;
pub mod identity;
pub mod library;
pub mod page;
pub mod query;
pub mod split;
pub mod sql_format;
pub mod statement;
pub mod value;
