//! **The tool table — one row per tool, and the only place a tool is declared.**
//!
//! Nothing to do with the repo-root `tools/` directory, which holds
//! `update-manifest`, a standalone crate the release workflow runs.
//!
//! Adding a tool to dodo used to be five edits scattered through a 1,751-line
//! `layout.rs`: a [`View`] variant, a row in `View::ALL` (written out once per
//! platform), an arm each in `View::title`, `View::icon` and `View::code`, a
//! field on `Layout` holding the view entity, and an arm in the main pane's
//! `match` — with a sixth, `View::for_detector`, if the tool accepts a pasted
//! value. Only the first of those was one the compiler would demand. Everything
//! below is generated from the [`tools!`] table at the bottom of this module
//! instead, so a tool is **one row**, and `layout.rs` — down to 1,276 lines —
//! is the shell around whatever the table declares.
//!
//! **This is a table, not a registry.** [`View`] stays an ordinary enum — it is
//! persisted, matched on exhaustively, and handled arm by arm — and the macro
//! only writes out the matches that used to be written by hand. There are no
//! trait objects, no distributed slices, no build script and nothing discovered
//! at runtime: the win is co-location, and every property the enum had is
//! kept.
//!
//! # What a row carries
//!
//! * `code:` — the tool's identity in `session.json`. **A compatibility
//!   surface**: it is the open tool, *and* since the Features settings page it
//!   is also the user's identity for their sidebar order and their per-tool
//!   on/off list, so changing one does not merely reopen a different tool —
//!   it drops that tool out of the stored order and puts it back where
//!   [`Features::resolve`] says a tool the file never named belongs. The table
//!   holds the string **verbatim**; nothing here derives a code from a variant
//!   name, and [`tests::every_declared_tool_has_its_own_stable_code`] pins the
//!   exact set.
//! * `title:` and `icon:` — the sidebar row.
//! * `hosts:` — optional, and the reason [`View::ALL`] is not simply the table.
//!   See "One tool is platform-conditional" below.
//! * `pane:` — the field name and type of the view entity. Every tool's
//!   constructor is `T::new(window, cx)`, which is what lets the table carry
//!   this at all: [`Panes`] is generated from the same rows, so a tool's entity
//!   is declared, built and drawn from one place.
//! * `pastes:` — optional, the [`Detector`]s whose route lands in this tool.
//!   This is a *membership* claim and never an order: `Detector::ORDER` is a
//!   correctness property argued in `quick_nav::models::detect`, the sidebar's
//!   order is a preference, and the two must not be unified. The generated
//!   [`View::for_detector`] is exhaustive over `Detector`, so a detector no row
//!   claims is a compile error and a detector two rows claim is an unreachable
//!   pattern.
//!
//! # One tool is platform-conditional
//!
//! The Input method uses Event Tap on macOS and Keyboard Hook on Windows, so
//! there is no row on platforms with neither implementation.
//! **Its enum variant still exists everywhere**, and that is deliberate: two of
//! the four release targets cannot be compiled from the machine this is
//! developed on, so a shape where a whole tool's title, icon, code and
//! detectors vanish from the type on those targets is a shape whose mistakes
//! only the captain's Windows machine can find. `hosts:` is answered by
//! [`View::available`], a `const fn` over `cfg!(..)` — both answers are
//! typechecked on every target, in the same spirit as
//! `dodo_cleaner::core::category::CleanerCategory::hidden_for` and
//! `dodo_paths`' target-triple classification — and [`View::ALL`] is
//! const-filtered from [`View::DECLARED`] by it. What a Linux build loses is
//! exactly what it lost before: the tool is absent from `View::ALL`, so
//! `View::codes` never offers it, `View::lookup` never returns it and no
//! sidebar row exists for it.
//!
//! Three lines are still `cfg`-gated and cannot be otherwise, because
//! `InputMethodView` itself does not exist off those platforms: the [`Panes`]
//! field, its constructor, and the arm in [`Panes::place`] — whose `cfg(not)`
//! counterpart is generated beside it, so the match stays exhaustive on every
//! target rather than leaning on a catch-all.
//!
//! [`Features::resolve`]: crate::session::models::features::Features::resolve

use gpui::{App, AppContext as _, Div, Entity, ParentElement as _, Window};

use crate::app_icon::AppIcon;
use crate::i18n::{Str, docker, shell};
use crate::quick_nav::models::detect::Detector;
use crate::session::models::features::Features;

/// Writes out everything that used to be one hand-maintained `match` per
/// question: the [`View`] enum, the platform filter, the sidebar metadata, the
/// quick-navigation mapping, and the [`Panes`] struct holding one entity per
/// tool.
///
/// Row attributes (doc comments included) land on the enum variant. Do not put
/// a `#[cfg]` there — `hosts:` is how a tool says where it exists, and it is
/// the only spelling the generated code understands.
macro_rules! tools {
    ($(
        $(#[$attr:meta])*
        $name:ident {
            code: $code:literal,
            title: $title:expr,
            icon: $icon:expr,
            $(hosts: $hosts:meta,)?
            pane: $field:ident : $pane:ty,
            $(pastes: [$($paste:ident),* $(,)?],)?
        }
    )*) => {
        /// Which tool is currently shown in the main pane. Selecting a sidebar
        /// item swaps the active view.
        ///
        /// Every tool is one flat row, Docker included: its four pages moved
        /// onto the tab rail inside `DockerView` because a nested sidebar group
        /// renders no children at all once the sidebar collapses to icons,
        /// which made those pages unreachable.
        ///
        /// **Adding a tool is one row in the [`tools!`] table below, plus the
        /// tool's own crate or module.** That row is the variant, the sidebar
        /// order, the title, the icon, the `session.json` code, the entity
        /// field and the main-pane arm; there is nothing else to remember and
        /// no fifth place to forget. If it also accepts a pasted value, its
        /// `pastes:` list is the whole of the mapping onto it, and quick
        /// navigation's own two halves — a `Detector` and a `Route` variant in
        /// `quick_nav::models::detect` — plus one arm in `Layout::apply_route`
        /// carrying the payload are what remain.
        ///
        /// The costs the table did **not** remove, because neither is a
        /// property of a tool: `Layout::apply_route`'s arm, which unpacks a
        /// route's payload into a method call that differs per tool
        /// (`accept_text`, `accept_decode`, `accept_curl`, `accept_uri`,
        /// none of them a common shape); and Docker's polling lifecycle in
        /// `Layout::activate` and `Layout::new`, which is Docker's alone.
        ///
        /// [`View::code`] is what `session.json` stores, so it is the one piece
        /// of a row that is a **compatibility surface** — see this module's doc
        /// for what changing one costs. A code this build does not know opens a
        /// tool it does have rather than failing to start; see [`View::shown`].
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum View {
            $(
                $(#[$attr])*
                $name,
            )*
        }

        impl View {
            /// Every tool the table declares, whether or not this build has it.
            ///
            /// **Not the sidebar's list** — that is [`View::ALL`], this filtered
            /// by [`View::available`]. This one exists so the platform-
            /// conditional tool's title, icon, code and detectors are
            /// typechecked and asserted on every target rather than only where
            /// it ships.
            pub(crate) const DECLARED: &'static [View] = &[$(View::$name),*];

            /// Whether this build has the tool at all.
            ///
            /// `cfg!` rather than `#[cfg]`, so both answers compile everywhere
            /// and the platform rule is one readable expression instead of a
            /// list written out twice.
            const fn available(self) -> bool {
                match self {
                    $( View::$name => true $(&& cfg!($hosts))? , )*
                }
            }

            /// The tool's own name — what the sidebar row reads. The main
            /// pane's title goes through `layout::pane_title` instead, because
            /// Docker titles itself after the rail's selected page.
            pub fn title(self) -> Str {
                match self {
                    $( View::$name => $title.into(), )*
                }
            }

            /// The glyph on the sidebar row — which, collapsed to the rail, is
            /// the only thing the row is.
            ///
            /// **No two tools share one.** The Input method used to draw
            /// `AppIcon::Globe` on its settings page, which is the API
            /// Explorer's row; as a tool beside it that would have been two
            /// unrelated things under one mark.
            /// [`tests::no_two_tools_wear_the_same_icon`] is what keeps the next
            /// borrowing from happening.
            pub fn icon(self) -> AppIcon {
                match self {
                    $( View::$name => $icon, )*
                }
            }

            /// The tool's stable identifier in `session.json`.
            ///
            /// Never a localized title and never the variant name: a title
            /// changes with the language and a variant name changes with a
            /// refactor, and this has to survive both. The table holds the
            /// literal, so no refactor here can quietly rewrite one.
            pub fn code(self) -> &'static str {
                match self {
                    $( View::$name => $code, )*
                }
            }

            /// The tool a detected paste belongs to.
            ///
            /// **The one mapping from `quick_nav`'s list onto this one**, read
            /// twice and for two different reasons: `Layout::apply_route` uses
            /// it to decide where a route goes, and `Layout::allowed_detectors`
            /// uses it *before* detection to decide which detectors a
            /// switched-off tool has taken out of play. Two copies could
            /// disagree, and the disagreement would be silent.
            ///
            /// Not injective: the Encoder/Decoder answers for both JWT and
            /// Base64. Exhaustive over [`Detector`] by the compiler, which is
            /// what a `pastes:` list on the wrong row runs into.
            pub(crate) fn for_detector(detector: Detector) -> View {
                match detector {
                    $( $( $( Detector::$paste => View::$name, )* )? )*
                }
            }

            /// Every tool **this build has**, in default sidebar order.
            ///
            /// No longer the order the sidebar draws: that is the user's, held
            /// by `Layout::features`. This is what a stored order is resolved
            /// against — the list of what exists, and where a tool the stored
            /// order never mentions belongs.
            ///
            /// Const-filtered from [`View::DECLARED`] instead of being written
            /// out once per platform. An attribute on an array element is not a
            /// thing stable Rust has, and a `Vec` would cost every caller the
            /// fixed length [`View::codes`] returns; a `const` filter costs
            /// neither and keeps the platform rule in [`View::available`],
            /// where both answers compile.
            pub(crate) const ALL: [View; AVAILABLE] = {
                let mut all = [View::DECLARED[0]; AVAILABLE];
                let mut found = 0;
                let mut ix = 0;
                while ix < View::DECLARED.len() {
                    if View::DECLARED[ix].available() {
                        all[found] = View::DECLARED[ix];
                        found += 1;
                    }
                    ix += 1;
                }
                all
            };

            /// Every tool's code, in default sidebar order — what a stored
            /// order is placed against by `Features::resolve`.
            pub(crate) fn codes() -> [&'static str; View::ALL.len()] {
                View::ALL.map(View::code)
            }

            /// The tool this code names, if this build has one.
            ///
            /// The strict half of `View::shown`, and the way back from a
            /// `Features` entry — whose codes came out of [`View::codes`], so
            /// there it is total.
            pub fn lookup(code: &str) -> Option<View> {
                View::ALL.into_iter().find(|view| view.code() == code)
            }
        }

        /// How many of [`View::DECLARED`] this build actually has — the length
        /// of [`View::ALL`], computed rather than written out per platform.
        const AVAILABLE: usize = {
            let mut found = 0;
            let mut ix = 0;
            while ix < View::DECLARED.len() {
                if View::DECLARED[ix].available() {
                    found += 1;
                }
                ix += 1;
            }
            found
        };

        /// The live view entity behind every tool this build has, built once by
        /// `Layout::new` and never rebuilt.
        ///
        /// One field per row of the table, so a tool cannot be declared without
        /// a pane or given a pane nothing draws. The fields are
        /// `pub(crate)` because `Layout::apply_route` hands a decoded payload to
        /// a *particular* tool by name — the one thing about a tool that has no
        /// common shape.
        pub(crate) struct Panes {
            $(
                $(#[cfg($hosts)])?
                pub(crate) $field: Entity<$pane>,
            )*
        }

        impl Panes {
            /// Builds every tool's view. Order is the table's, which is
            /// [`View::ALL`]'s.
            pub(crate) fn new(window: &mut Window, cx: &mut App) -> Self {
                Self {
                    $(
                        $(#[cfg($hosts)])?
                        $field: cx.new(|cx| <$pane>::new(window, cx)),
                    )*
                }
            }

            /// Puts the active tool into the box the main pane scrolls.
            ///
            /// The `cfg(not(..))` arm is generated beside its counterpart
            /// rather than replaced by a catch-all, so this match stays
            /// exhaustive over [`View`] on every target: a row added without a
            /// pane is still a compile error, and a tool this platform lacks is
            /// still unreachable — `Layout::active` only ever holds what
            /// [`View::lookup`] returned, and that searches [`View::ALL`].
            pub(crate) fn place(&self, view: View, container: Div) -> Div {
                match view {
                    $(
                        $(#[cfg($hosts)])?
                        View::$name => container.child(self.$field.clone()),
                        $( #[cfg(not($hosts))]
                        View::$name => container, )?
                    )*
                }
            }
        }
    };
}

tools! {
    /// dodo's default tool, and what an unrecognised saved code falls back to.
    JsonFormatter {
        code: "json-formatter",
        title: shell::Text::JsonFormatterTitle,
        icon: AppIcon::Json,
        pane: json_formatter: crate::json_formatter::JsonFormatter,
        pastes: [Json],
    }

    /// Answers for **two** detectors, which is why `for_detector` is not
    /// injective and why switching this off silences both.
    EncoderDecoder {
        code: "encoder-decoder",
        title: shell::Text::EncoderDecoderTitle,
        icon: AppIcon::Binary,
        pane: encoder_decoder: crate::encoder_decoder::EncoderDecoder,
        pastes: [Jwt, Base64],
    }

    ApiExplorer {
        code: "api-explorer",
        title: shell::Text::ApiExplorerTitle,
        icon: AppIcon::Globe,
        pane: api_explorer: crate::api_explorer::ApiExplorer,
        pastes: [Curl],
    }

    Cleaner {
        code: "cleaner",
        title: shell::Text::CleanerTitle,
        icon: AppIcon::Cleaner,
        pane: cleaner: crate::cleaner::CleanerView,
    }

    /// One flat row like every other tool: its four pages are the rail inside
    /// `DockerView`, not sidebar children.
    Docker {
        code: "docker",
        title: docker::Text::Docker,
        icon: AppIcon::Container,
        pane: docker: crate::docker::DockerView,
    }

    Database {
        code: "database",
        title: shell::Text::DatabaseTitle,
        icon: AppIcon::Database,
        pane: database: crate::database::DatabaseView,
        pastes: [DatabaseUri],
    }

    /// Last, which is also where its settings page sat. Linux stays hidden
    /// until it has an implementation.
    InputMethod {
        code: "input-method",
        title: shell::Text::InputMethod,
        icon: AppIcon::Keyboard,
        hosts: any(target_os = "macos", target_os = "windows"),
        pane: input_method: crate::input_method::views::InputMethodView,
    }
}

impl View {
    /// The tool dodo opens on when nothing has been saved, and the one an
    /// unrecognised saved code falls back to.
    ///
    /// Written out rather than taken as the table's first row: which tool is
    /// the default is its own decision, and it should survive a reorder of the
    /// sidebar's default order.
    const DEFAULT: View = View::JsonFormatter;

    /// The tool to show, given the one that was asked for.
    ///
    /// **The single answer to three questions**: a `session.json` naming a tool
    /// this build does not have, one naming a tool the user has since switched
    /// off, and the tool that is open right now being switched off.
    /// `Features::active` decides all three — the asked-for tool if the sidebar
    /// still lists it, and otherwise the first tool it does list — and this only
    /// maps the answer back onto a variant.
    ///
    /// **Anything unrecognised therefore opens the app rather than refusing
    /// to**, which is what the `from_code` this replaced existed for.
    /// [`View::DEFAULT`] is the last resort for a build with no tools at all,
    /// and is unreachable while [`View::ALL`] is non-empty.
    pub(crate) fn shown(features: &Features, wanted: Option<&str>) -> View {
        features
            .active(wanted)
            .and_then(View::lookup)
            .unwrap_or(View::DEFAULT)
    }
}

#[cfg(test)]
mod tests {
    use gpui::SharedString;
    use gpui_component::IconNamed as _;

    use super::{AVAILABLE, View};
    use crate::i18n::{Str, docker};
    use crate::quick_nav::models::detect::Detector;
    use crate::session::models::features::Features;
    use crate::session::services::session_store::parse_document;

    /// **The macro's platform machinery, compiled and asserted both ways on
    /// every target.**
    ///
    /// This is the point of `hosts:` being a `cfg!` predicate rather than a
    /// `#[cfg]` attribute on the row. Two of dodo's four release targets cannot
    /// be built from the machine this is developed on, and the real table has
    /// exactly one conditional tool — so on a Mac the "this platform does not
    /// have it" half of the generated code is never compiled at all, and the
    /// `reveal_label` fix is the reminder of what that costs.
    ///
    /// The probe table below closes that with two rows whose answer is fixed by
    /// construction: `cfg(all())` is true everywhere and `cfg(any())` is false
    /// everywhere. So every target compiles the present field *and* the absent
    /// one, the `cfg` arm of `Panes::place` *and* its `cfg(not(..))` twin, and
    /// const-filters a genuinely shorter [`View::ALL`] out of
    /// [`View::DECLARED`] — the one path the real table cannot exercise here,
    /// because on macOS nothing is filtered out.
    ///
    /// It builds no entity: `Panes` needs a window, and what is being checked
    /// is that the generated code typechecks and that the filter answers.
    #[allow(
        dead_code,
        reason = "The probe exists so rustc compiles both halves of the \
                  generated `cfg`s; nothing calls the generated pane code."
    )]
    mod platform_probe {
        use gpui::{App, AppContext as _, Div, Entity, ParentElement as _, Window};
        use gpui_component::IconNamed as _;

        use crate::app_icon::AppIcon;
        use crate::i18n::{Str, shell};
        use crate::quick_nav::models::detect::Detector;

        tools! {
            /// An ordinary row: no `hosts:`, so nothing about it is conditional.
            Everywhere {
                code: "probe-everywhere",
                title: shell::Text::JsonFormatterTitle,
                icon: AppIcon::Json,
                pane: everywhere: crate::json_formatter::JsonFormatter,
                pastes: [Json, Jwt, Base64],
            }

            /// `cfg(all())` is true on every target, so this compiles the
            /// *present* half of every generated `cfg` — and its `cfg(not(..))`
            /// twin is compiled away, exactly as the Input method's is on macOS.
            Always {
                code: "probe-always",
                title: shell::Text::EncoderDecoderTitle,
                icon: AppIcon::Binary,
                hosts: all(),
                pane: always: crate::json_formatter::JsonFormatter,
                pastes: [Curl],
            }

            /// `cfg(any())` is false on every target, so this compiles the
            /// *absent* half: no field, no constructor line, and the
            /// `cfg(not(..))` arm of `Panes::place` — which is the arm no Mac
            /// ever compiles for the real table.
            Never {
                code: "probe-never",
                title: shell::Text::DatabaseTitle,
                icon: AppIcon::Database,
                hosts: any(),
                pane: never: crate::json_formatter::JsonFormatter,
                pastes: [DatabaseUri],
            }
        }

        /// The filter drops the unavailable row, on every target — which is the
        /// assertion the real `View::ALL` cannot make from a Mac.
        #[test]
        fn a_row_no_platform_has_is_declared_but_never_listed() {
            assert_eq!(
                View::DECLARED,
                [View::Everywhere, View::Always, View::Never]
            );
            assert_eq!(View::ALL, [View::Everywhere, View::Always]);
            assert_eq!(AVAILABLE, View::ALL.len());

            assert!(View::Everywhere.available());
            assert!(View::Always.available());
            assert!(!View::Never.available());

            // Declared, so it still has a code, a title and an icon here…
            assert_eq!(View::Never.code(), "probe-never");
            assert_eq!(View::Never.icon().path(), "icons/database.svg");
            // …and is still not something a stored session can reopen.
            assert_eq!(View::lookup("probe-never"), None);
            assert_eq!(View::codes(), ["probe-everywhere", "probe-always"]);

            // A `pastes:` list on an unavailable row still maps, and is still
            // exhaustive over `Detector` — the compiler proved the second.
            assert_eq!(View::for_detector(Detector::DatabaseUri), View::Never);
            assert_eq!(View::for_detector(Detector::Curl), View::Always);
        }

        /// `Panes` has a field for each row the platform has and none for the
        /// row it does not — asserted through the type, since a missing field
        /// is a compile error and a surplus one an unused-field warning.
        #[test]
        fn panes_holds_one_entity_per_available_row() {
            fn takes_fields(
                panes: &Panes,
            ) -> (
                &Entity<crate::json_formatter::JsonFormatter>,
                &Entity<crate::json_formatter::JsonFormatter>,
            ) {
                (&panes.everywhere, &panes.always)
            }
            let _ = takes_fields;
            let _: fn(&Panes, View, Div) -> Div = Panes::place;
            let _: fn(&mut Window, &mut App) -> Panes = Panes::new;
        }
    }

    /// The tool list of someone who has never opened the Features page: every
    /// tool, in `View::ALL` order, all of them visible.
    fn everything() -> Features {
        Features::resolve(None, &View::codes())
    }

    /// Whether this build ships an Input method implementation.
    const INPUT_METHOD_HOST: bool = cfg!(any(target_os = "macos", target_os = "windows"));

    /// **The compatibility surface, pinned.**
    ///
    /// Every code in the table, in table order, exactly as it is written — and
    /// asserted for *declared* tools rather than available ones, so the Input
    /// method's spelling is checked on Linux too. Changing one of these strings
    /// does not merely reopen a different tool: it drops that tool out of every
    /// user's stored sidebar order and resets it to a default position.
    #[test]
    fn every_declared_tool_has_its_own_stable_code() {
        let codes: Vec<&str> = View::DECLARED.iter().map(|view| view.code()).collect();

        assert_eq!(
            codes,
            [
                "json-formatter",
                "encoder-decoder",
                "api-explorer",
                "cleaner",
                "docker",
                "database",
                "input-method",
            ]
        );

        let mut unique = codes.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), codes.len(), "two tools share a code");
    }

    /// …and the codes this build actually offers are that list minus what the
    /// platform has no host for. `View::codes` is what `Features::resolve`
    /// places a stored order against, so this is the list a `session.json` is
    /// read against.
    #[test]
    fn every_tool_this_build_has_keeps_its_code() {
        let codes: Vec<&str> = View::codes().to_vec();

        let mut expected = vec![
            "json-formatter",
            "encoder-decoder",
            "api-explorer",
            "cleaner",
            "docker",
            "database",
        ];
        if INPUT_METHOD_HOST {
            expected.push("input-method");
        }
        assert_eq!(codes, expected);
    }

    /// The default order, in full, and the platform filter that produces it.
    ///
    /// `View::DECLARED` is the table and is the same everywhere; `View::ALL` is
    /// what this build has. Asserting the whole of both rather than a prefix,
    /// so a tool cannot be dropped from the middle of either unnoticed.
    #[test]
    fn the_sidebar_lists_every_tool_once_with_docker_flat_and_last() {
        assert_eq!(
            View::DECLARED,
            [
                View::JsonFormatter,
                View::EncoderDecoder,
                View::ApiExplorer,
                View::Cleaner,
                View::Docker,
                View::Database,
                View::InputMethod,
            ]
        );

        #[cfg(any(target_os = "macos", target_os = "windows"))]
        assert_eq!(
            View::ALL,
            [
                View::JsonFormatter,
                View::EncoderDecoder,
                View::ApiExplorer,
                View::Cleaner,
                View::Docker,
                View::Database,
                View::InputMethod,
            ]
        );
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        assert_eq!(
            View::ALL,
            [
                View::JsonFormatter,
                View::EncoderDecoder,
                View::ApiExplorer,
                View::Cleaner,
                View::Docker,
                View::Database,
            ]
        );

        // One row per tool: Docker and Database are each a single entry, not a
        // group of children — an icon-collapsed sidebar renders no children at
        // all, which is what made Docker's four pages unreachable.
        assert_eq!(View::DECLARED.len(), 7);
        assert_eq!(View::ALL.len(), if INPUT_METHOD_HOST { 7 } else { 6 });
        assert_eq!(AVAILABLE, View::ALL.len());
    }

    /// The platform rule itself, asserted on every target because
    /// `View::available` is `cfg!` rather than `#[cfg]`: on Linux this build
    /// still knows the Input method's variant, code, title and icon, and still
    /// refuses to list it.
    #[test]
    fn the_input_method_is_available_where_dodo_has_an_implementation() {
        assert!(View::JsonFormatter.available());
        assert_eq!(View::InputMethod.available(), INPUT_METHOD_HOST);

        assert_eq!(View::InputMethod.code(), "input-method");
        assert_eq!(View::ALL.contains(&View::InputMethod), INPUT_METHOD_HOST,);
        assert_eq!(View::codes().contains(&"input-method"), INPUT_METHOD_HOST,);
        assert_eq!(View::lookup("input-method").is_some(), INPUT_METHOD_HOST,);
    }

    /// Registered exactly once. `View::ALL` is what `Features::resolve` places a
    /// stored order against, and a tool listed twice there would be two sidebar
    /// rows opening one pane — and two entries fighting over one code in
    /// `session.json`.
    #[test]
    fn no_tool_is_registered_twice() {
        for view in View::ALL {
            assert_eq!(
                View::ALL.iter().filter(|other| **other == view).count(),
                1,
                "{view:?} appears in View::ALL more than once",
            );
        }
        assert_eq!(
            everything().all().len(),
            View::ALL.len(),
            "the resolved tool list has one entry per tool",
        );
    }

    /// Every tool's sidebar row is a *different* glyph, which collapsed to the
    /// icon rail is the only thing distinguishing one row from another.
    ///
    /// Over `View::DECLARED`, so a new row cannot borrow the Input method's
    /// keyboard on a platform that does not draw it and be found only later.
    #[test]
    fn no_two_tools_wear_the_same_icon() {
        // `AppIcon` is not `PartialEq`; its asset path is the identity that
        // matters anyway, since that is what gpui rasterizes.
        let mut paths: Vec<SharedString> = View::DECLARED
            .iter()
            .map(|view| view.icon().path())
            .collect();
        let total = paths.len();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), total, "two sidebar rows draw the same glyph");

        assert_eq!(View::ApiExplorer.icon().path(), "icons/globe.svg");
        assert_eq!(View::InputMethod.icon().path(), "icons/keyboard.svg");
    }

    /// The Docker row reads "Docker" — the pane heading is the one that follows
    /// the rail's selected page, and that is `layout::pane_title`'s job.
    #[test]
    fn a_tools_title_is_its_own_name() {
        assert_eq!(View::Docker.title(), Str::from(docker::Text::Docker));
        for view in View::DECLARED {
            assert_eq!(view.title(), view.title(), "{view:?}");
        }
    }

    /// With nothing chosen, the sidebar is exactly what it was before the
    /// Features page existed: every tool, in `View::ALL` order.
    #[test]
    fn an_untouched_feature_list_is_the_sidebar_as_it_always_was() {
        let visible: Vec<View> = everything().visible().filter_map(View::lookup).collect();
        assert_eq!(visible, View::ALL);
    }

    /// The user's order and their on/off choices are what the sidebar draws —
    /// `View::codes` is only what those choices are resolved against.
    #[test]
    fn the_sidebar_draws_the_users_order_and_skips_what_they_hid() {
        let mut features = everything();
        features.move_to(View::Docker.code(), 0);
        features
            .set_enabled(View::Cleaner.code(), false)
            .expect("the others remain");

        let visible: Vec<View> = features.visible().filter_map(View::lookup).collect();
        // A prefix rather than the whole list, because the tail is
        // platform-dependent: the moved tool leads, the hidden one is gone, and
        // everything else keeps `View::ALL`'s order.
        assert_eq!(
            &visible[..5],
            &[
                View::Docker,
                View::JsonFormatter,
                View::EncoderDecoder,
                View::ApiExplorer,
                View::Database,
            ]
        );
        assert!(!visible.contains(&View::Cleaner));
        assert_eq!(visible.len(), View::ALL.len() - 1);
    }

    #[test]
    fn a_saved_code_comes_back_as_the_tool_that_wrote_it() {
        for view in View::ALL {
            assert_eq!(View::lookup(view.code()), Some(view));
            assert_eq!(View::shown(&everything(), Some(view.code())), view);
        }
    }

    /// The requirement that keeps a renamed or removed tool from being a failure
    /// to start: anything unrecognised opens the first tool the sidebar lists.
    #[test]
    fn an_unknown_or_absent_code_falls_back_to_the_first_visible_tool() {
        for code in [
            None,
            Some(""),
            Some("graphql-explorer"),
            Some("JsonFormatter"),
            Some("json_formatter"),
            Some("JSON Formatter"),
        ] {
            assert_eq!(
                View::shown(&everything(), code),
                View::DEFAULT,
                "{code:?} must not stop dodo opening",
            );
            assert!(code.and_then(View::lookup).is_none());
        }
        assert_eq!(View::DEFAULT, View::JsonFormatter);
    }

    /// Trap 2, at the seam `session::models::features` cannot reach: a
    /// remembered tool the user has since switched off opens the first tool
    /// they *did* leave visible, not a pane with no sidebar row above it.
    #[test]
    fn a_remembered_tool_that_is_now_hidden_opens_the_first_visible_one() {
        let mut features = everything();
        features
            .set_enabled(View::JsonFormatter.code(), false)
            .expect("five others remain");

        assert_eq!(
            View::shown(&features, Some(View::JsonFormatter.code())),
            View::EncoderDecoder,
        );
    }

    /// …and it follows the user's own order, not `View::ALL`'s.
    #[test]
    fn the_fallback_follows_the_users_own_sidebar_order() {
        let mut features = everything();
        features.move_to(View::Database.code(), 0);
        features
            .set_enabled(View::Docker.code(), false)
            .expect("five others remain");

        assert_eq!(View::shown(&features, None), View::Database);
        assert_eq!(
            View::shown(&features, Some(View::Docker.code())),
            View::Database,
        );
    }

    /// **Every tool is an ordinary tool here**, with no exception for the one
    /// still being built. The Cleaner is a legitimate thing to switch off —
    /// arguably the most likely one — and a special case for it would be a rule
    /// `session::models::features` does not have and should not gain.
    #[test]
    fn every_tool_can_be_switched_off_including_the_unfinished_one() {
        for view in View::ALL {
            let mut features = everything();
            assert!(features.can_toggle(view.code()), "{view:?}");
            features
                .set_enabled(view.code(), false)
                .unwrap_or_else(|_| panic!("{view:?} is not the last of six"));

            assert!(!features.is_enabled(view.code()));
            assert_eq!(
                features.all().len(),
                View::ALL.len(),
                "it is hidden, not gone"
            );
            assert_ne!(View::shown(&features, Some(view.code())), view);
        }
        assert!(View::ALL.contains(&View::Cleaner));
    }

    /// Trap 7's first half: the Features page can only ever hide a **tool**, and
    /// the Settings and Check-for-updates buttons are not tools — they are
    /// footer buttons drawn beside the menu, not rows in it. If either ever
    /// became a row in the table, this fails and someone has to think about how
    /// a user gets their tools back. `layout`'s own half checks the button is
    /// still in the footer.
    #[test]
    fn settings_is_not_a_tool_and_so_cannot_be_switched_off() {
        for reserved in ["settings", "check-for-updates"] {
            assert!(
                !View::DECLARED.iter().any(|view| view.code() == reserved),
                "`{reserved}` is a sidebar footer button; making it a tool would let \
                 the Features page hide the only way back to itself",
            );
        }
    }

    // ---- quick navigation meets the tool table -----------------------------

    /// The mapping both `apply_route` and `allowed_detectors` read, now the
    /// table's `pastes:` lists inverted. If a detector ever answered for a
    /// different tool than the route it produces lands in, switching that tool
    /// off would silence the wrong detector.
    #[test]
    fn every_detector_names_the_tool_its_route_lands_in() {
        for (detector, view) in [
            (Detector::Curl, View::ApiExplorer),
            (Detector::DatabaseUri, View::Database),
            (Detector::Jwt, View::EncoderDecoder),
            (Detector::Json, View::JsonFormatter),
            (Detector::Base64, View::EncoderDecoder),
        ] {
            assert_eq!(View::for_detector(detector), view);
        }
    }

    /// Every detector belongs to a tool this build could list, and the mapping
    /// is total over `Detector::ORDER` — the compiler already proves totality,
    /// this proves no detector was parked on a tool that does not exist.
    #[test]
    fn no_detector_belongs_to_a_tool_the_table_does_not_declare() {
        for detector in Detector::ORDER {
            let view = View::for_detector(detector);
            assert!(
                View::DECLARED.contains(&view),
                "{detector:?} routes to {view:?}, which is not in the table",
            );
            assert!(
                view.available(),
                "{detector:?} routes to {view:?}, which this build does not have",
            );
        }
    }

    // ---- a stored session, restored ---------------------------------------

    /// A realistic `session.json` from someone who has used the Features page:
    /// Docker dragged to the top, the API Explorer and the Cleaner switched
    /// off, and the Input method never mentioned because this file predates it.
    const STORED_SESSION: &[u8] = br#"{
        "version": 3,
        "appearance": { "theme": "Ayu Dark", "font_size": 15 },
        "window": { "x": 120, "y": 80, "width": 1280, "height": 820, "mode": "windowed" },
        "workspace": {
            "active_tool": "docker",
            "sidebar_collapsed": false,
            "tools": [
                { "code": "docker", "enabled": true },
                { "code": "database", "enabled": true },
                { "code": "json-formatter", "enabled": true },
                { "code": "api-explorer", "enabled": false },
                { "code": "encoder-decoder", "enabled": true },
                { "code": "cleaner", "enabled": false }
            ]
        }
    }"#;

    /// **The whole restore path, from bytes on disk to the tool on screen.**
    ///
    /// This is what a code string change would break, and it is asserted end to
    /// end rather than at any one function: the order the user dragged, the two
    /// tools they switched off, and the tool that was open all come back
    /// exactly. The Input method — a tool this file has never heard of — comes
    /// back *beside its default neighbour* (after Database, which it follows in
    /// `View::ALL`) and enabled, which is `Features::resolve`'s rule and not
    /// something this file may quietly change.
    #[test]
    fn a_realistic_stored_session_restores_exactly() {
        let document = parse_document(STORED_SESSION).expect("a version-3 session parses");
        let workspace = &document.workspace;

        assert_eq!(workspace.active_tool.as_deref(), Some("docker"));
        assert_eq!(workspace.sidebar_collapsed, Some(false));

        let features = Features::resolve(workspace.tools.as_deref(), &View::codes());

        let mut expected: Vec<(&str, bool)> = vec![("docker", true), ("database", true)];
        if INPUT_METHOD_HOST {
            // Beside its default neighbour, not at an absolute index: the list
            // it is joining is the user's order, and index 6 of that means
            // nothing.
            expected.push(("input-method", true));
        }
        expected.extend([
            ("json-formatter", true),
            ("api-explorer", false),
            ("encoder-decoder", true),
            ("cleaner", false),
        ]);

        let restored: Vec<(&str, bool)> = features
            .all()
            .iter()
            .map(|entry| (entry.code, entry.enabled))
            .collect();
        assert_eq!(restored, expected);

        // The tool that was open is the tool that opens.
        assert_eq!(
            View::shown(&features, workspace.active_tool.as_deref()),
            View::Docker,
        );

        // …and the sidebar draws the four they left on, in their order.
        let visible: Vec<View> = features.visible().filter_map(View::lookup).collect();
        let mut wanted = vec![View::Docker, View::Database];
        if INPUT_METHOD_HOST {
            wanted.push(View::InputMethod);
        }
        wanted.extend([View::JsonFormatter, View::EncoderDecoder]);
        assert_eq!(visible, wanted);
    }

    /// A second real shape, and the commonest one: **every tool present, none
    /// hidden, purely reordered** — the file a user who has only dragged rows
    /// around leaves behind. Nothing is dropped and nothing is inserted, so the
    /// resolved order is the stored order verbatim, and the tool that was open
    /// is the tool that opens.
    ///
    /// Written with the Input method in the middle deliberately: on a build
    /// without an Input method implementation that entry is *dropped* rather than moved, and the
    /// six around it keep their order and their positions relative to each
    /// other.
    #[test]
    fn a_reordered_session_with_every_tool_comes_back_in_the_users_order() {
        const REORDERED: &[u8] = br#"{
            "version": 3,
            "appearance": { "language": "en", "theme": "Catppuccin Frappe" },
            "workspace": {
                "active_tool": "cleaner",
                "sidebar_collapsed": true,
                "tools": [
                    { "code": "api-explorer", "enabled": true },
                    { "code": "database", "enabled": true },
                    { "code": "input-method", "enabled": true },
                    { "code": "json-formatter", "enabled": true },
                    { "code": "docker", "enabled": true },
                    { "code": "encoder-decoder", "enabled": true },
                    { "code": "cleaner", "enabled": true }
                ]
            }
        }"#;

        let document = parse_document(REORDERED).expect("a version-3 session parses");
        let features = Features::resolve(document.workspace.tools.as_deref(), &View::codes());

        let mut expected = vec!["api-explorer", "database"];
        if INPUT_METHOD_HOST {
            expected.push("input-method");
        }
        expected.extend(["json-formatter", "docker", "encoder-decoder", "cleaner"]);

        let restored: Vec<&str> = features.all().iter().map(|entry| entry.code).collect();
        assert_eq!(restored, expected);
        assert!(
            features.all().iter().all(|entry| entry.enabled),
            "nothing was hidden, so nothing comes back hidden",
        );
        assert_eq!(features.all().len(), View::ALL.len());

        assert_eq!(
            View::shown(&features, document.workspace.active_tool.as_deref()),
            View::Cleaner,
        );
        assert_eq!(document.workspace.sidebar_collapsed, Some(true));
    }

    /// The same file, written back out: an unchanged list round-trips through
    /// `Features::record` with the codes it arrived with, which is what makes
    /// the next launch identical to this one.
    #[test]
    fn a_restored_session_writes_back_the_codes_it_read() {
        let document = parse_document(STORED_SESSION).expect("a version-3 session parses");
        let features = Features::resolve(document.workspace.tools.as_deref(), &View::codes());

        let written = features.record();
        let round_tripped = Features::resolve(Some(&written), &View::codes());

        assert_eq!(round_tripped, features);
        for record in &written {
            assert!(
                View::lookup(&record.code).is_some(),
                "`{}` is not a tool this build has, so it must not be written",
                record.code,
            );
        }
    }
}
