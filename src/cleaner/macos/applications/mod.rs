//! Application identity, leftover matching and uninstall review (Phase 9).
//!
//! Kept as a sibling of [`super::scanners`] rather than inside it: this code
//! is shared by two features — the `InstalledApps` scanner (via
//! [`bundle::parse_bundle`]) and the uninstall review workflow the ticket
//! calls "Application uninstall and related files" — and none of it is a
//! `CleanerScanner` implementation itself.
//!
//! Module split, narrowest first:
//!
//! - [`bundle`]: `Info.plist` parsing. No knowledge of matching or scoring.
//! - [`identity`]: pure normalization (`AppIdentity` and its derivation
//!   functions). No filesystem access.
//! - [`confidence`]: the ticket's point table as pure, unit-tested scoring.
//!   No filesystem access.
//! - [`locations`]: the fixed leftover-location list and the impure
//!   directory scan that turns a location plus an `AppIdentity` into
//!   [`locations::LeftoverMatch`] candidates.
//! - [`review`]: orchestrates the above into an [`review::UninstallReview`]
//!   the view can render, refusing protected apps per the ticket.
//! - [`orphans`] (Phase 10): the inverse question — walks the same fixed
//!   location list and flags entries *no* installed app's identity explains,
//!   using the whole installed-app index rather than one `AppIdentity` at a
//!   time.

pub mod bundle;
pub mod confidence;
pub mod identity;
pub mod locations;
pub mod orphans;
pub mod review;
