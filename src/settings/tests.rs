// Deliberately not `use super::*`: that pulls in `use gpui::*`, whose `test`
// re-export shadows the standard attribute. See the dodo-build-validate skill.
use std::cell::Cell;

use super::general::StartupStatus;
use super::search::rank;

fn labels() -> Vec<String> {
    [
        "Language General",
        "Font size Appearance",
        "Border radius Appearance",
        "Theme Appearance",
    ]
    .map(str::to_owned)
    .to_vec()
}

fn best(query: &str) -> Option<usize> {
    rank(query, &labels()).first().map(|(ix, _)| *ix)
}

#[test]
fn abbreviations_find_their_setting() {
    assert_eq!(best("brdr"), Some(2));
    assert_eq!(best("fnt"), Some(1));
    assert_eq!(best("lang"), Some(0));
    assert_eq!(best("thm"), Some(3));
}

#[test]
fn several_matches_come_back_best_first() {
    // "ea" is a subsequence of every label, so this exercises the ordering
    // rather than the filtering.
    let ranked = rank("ea", &labels());
    assert_eq!(ranked.len(), labels().len());
    assert!(
        ranked.windows(2).all(|pair| pair[0].1 >= pair[1].1),
        "ranked = {ranked:?}"
    );
    // The three Appearance labels contain "ea" in "Appearance"; the General
    // one only scatters it, so it has to come last.
    assert_eq!(ranked.last().map(|(ix, _)| *ix), Some(0));
}

#[test]
fn a_section_name_lists_that_section() {
    let ranked = rank("appearance", &labels());
    let mut found: Vec<usize> = ranked.into_iter().map(|(ix, _)| ix).collect();
    found.sort();
    assert_eq!(found, vec![1, 2, 3]);
}

#[test]
fn an_empty_query_matches_nothing() {
    assert!(rank("", &labels()).is_empty());
    assert!(rank("   ", &labels()).is_empty());
}

#[test]
fn a_query_matching_nothing_returns_nothing() {
    assert!(rank("zzqx", &labels()).is_empty());
}

#[test]
fn matching_is_case_insensitive() {
    assert_eq!(best("FONT"), Some(1));
    assert_eq!(best("BoRdEr"), Some(2));
}

#[test]
fn vietnamese_labels_match_accented_and_plain_typing() {
    let vietnamese = ["Ngôn ngữ Chung".to_owned(), "Cỡ chữ Giao diện".to_owned()];
    let best = |query: &str| rank(query, &vietnamese).first().map(|(ix, _)| *ix);

    assert_eq!(best("cỡ chữ"), Some(1));
    assert_eq!(best("co chu"), Some(1));
    assert_eq!(best("ngon ngu"), Some(0));
}

#[test]
fn folding_strips_accents_without_losing_letters() {
    assert_eq!(super::search::fold("Cỡ chữ"), "Co chu");
    assert_eq!(super::search::fold("Giao diện"), "Giao dien");
    assert_eq!(super::search::fold("Định dạng"), "Dinh dang");
    assert_eq!(super::search::fold("Border radius"), "Border radius");
}

#[test]
fn repeated_start_with_os_renders_do_not_read_the_os_status() {
    let reads = Cell::new(0);
    let status = StartupStatus::Loading;

    for _ in 0..3 {
        assert_eq!(status, StartupStatus::Loading);
    }
    assert_eq!(reads.get(), 0);

    let status = StartupStatus::read_once(|| {
        reads.set(reads.get() + 1);
        true
    });
    for _ in 0..3 {
        assert_eq!(status, StartupStatus::Known(true));
    }
    assert_eq!(reads.get(), 1);
}

#[test]
fn start_with_os_write_transitions_keep_only_trustworthy_values() {
    let mut status = StartupStatus::Loading;
    assert_eq!(status, StartupStatus::Loading);

    status = StartupStatus::read_once(|| false);
    assert_eq!(status, StartupStatus::Known(false));

    status = StartupStatus::after_successful_set(true);
    assert_eq!(status, StartupStatus::Known(true));

    status = StartupStatus::after_failed_set();
    assert_eq!(status, StartupStatus::Unknown);
}

/// Measures a setting row against the box that has to contain it.
///
/// These are the only tests here that need a frame. They do not drive the
/// dialog — `Root::new` dereferences a real `NSView`, so a dialog cannot be
/// hosted in a GPUI test window on macOS — but the dialog contributes nothing
/// to a row's width except the box it hands the panel, so the panel is rendered
/// directly into a div of exactly that width ([`DIALOG_WIDTH`] less
/// `CARD_CHROME`) and the row is measured inside it.
///
/// The field is a stand-in rather than the real [`pattern_field`]: nothing can
/// tag a library-internal element for `debug_bounds`, so the probe reproduces
/// what `setting::fields::string::StringField::render` builds — `w_64` in a
/// horizontal row, `w_full` in a stacked one. Should upstream drop that fixed
/// width, [`a_side_by_side_row_would_not_fit`] fails and this whole workaround
/// can go.
pub(super) mod row_layout {
    use gpui::prelude::FluentBuilder as _;
    use gpui::{
        AppContext as _, Axis, Bounds, Context, InteractiveElement as _, IntoElement,
        ParentElement as _, Pixels, Render, SharedString, StyleRefinement, Styled as _,
        TestAppContext, VisualTestContext, Window, WindowBounds, WindowOptions, div, point, px,
        size,
    };
    use gpui_component::setting::{SettingField, SettingGroup, SettingItem, SettingPage, Settings};

    use super::super::{DIALOG_WIDTH, SIDEBAR_WIDTH};

    /// What the dialog card keeps for itself before the panel sees any width:
    /// a 1px border and `Dialog`'s default `Edges::all(16)` padding, per side.
    const CARD_CHROME: Pixels = px(34.);

    /// The settings panel, sized and configured exactly as the dialog does it,
    /// holding one item that stands in for a quick-navigation pattern row.
    ///
    /// `stacked` picks how that item is built: through [`super::super::quick_nav::input_item`],
    /// which is the production path, or through the bare [`SettingItem::new`]
    /// it replaced.
    struct Probe {
        width: Pixels,
        stacked: bool,
    }

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let title = SharedString::from("Database URI pattern");
            let field = SettingField::render(|options, _, _| {
                let horizontal = matches!(options.layout, Axis::Horizontal);
                div()
                    .debug_selector(|| "field".into())
                    .h(px(32.))
                    .map(|this| {
                        if horizontal {
                            this.w_64()
                        } else {
                            this.w_full()
                        }
                    })
            });

            let item = if self.stacked {
                super::super::quick_nav::input_item(title, field)
            } else {
                SettingItem::new(title, field)
            };

            let page = SettingPage::new("Quick navigation").resettable(false).group(
                SettingGroup::new().title("Quick navigation").item(
                    item
                    // The longest of the three descriptions these rows carry:
                    // the label column's width is what the control has to fit
                    // beside, so a short one would understate the row.
                    .description(
                        "Optional. dodo already has a real parser for this format and uses it; a \
                         pattern here only narrows what is offered to it. Leave it empty to try \
                         the parser on everything.",
                    ),
                ),
            );

            div()
                .w(self.width)
                .h(px(440.))
                .debug_selector(|| "panel".into())
                .child(
                    Settings::new("row-layout-probe")
                        .sidebar_width(SIDEBAR_WIDTH)
                        .header_style(&StyleRefinement::default().hidden())
                        .pages(vec![page]),
                )
        }
    }

    /// Right edge of the row's control, and of the box that must contain it.
    fn edges(cx: &mut TestAppContext, width: Pixels, stacked: bool) -> (Pixels, Pixels) {
        cx.update(gpui_component::init);

        let window = cx
            .update(|cx| {
                cx.open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds {
                            origin: point(px(0.), px(0.)),
                            size: size(px(1200.), px(800.)),
                        })),
                        ..Default::default()
                    },
                    |_, cx| cx.new(|_| Probe { width, stacked }),
                )
            })
            .unwrap();
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();

        let field = cx.debug_bounds("field").expect("the probe row was drawn");
        let panel = cx.debug_bounds("panel").expect("the probe panel was drawn");
        (field.right(), panel.right())
    }

    /// The panel width the dialog actually hands the settings panel, plus what
    /// it would have at the narrowest the window itself can be dragged. The
    /// dialog does not resize with the window, so the second is hypothetical
    /// today — but measuring both says the row is bounded by its own box rather
    /// than by luck about how much room happens to be there.
    fn widths() -> [Pixels; 2] {
        [
            DIALOG_WIDTH - CARD_CHROME,
            crate::layout::window_min_size().width - CARD_CHROME,
        ]
    }

    #[gpui::test]
    fn a_pattern_row_stays_inside_the_card(cx: &mut TestAppContext) {
        for width in widths() {
            let (field, panel) = edges(cx, width, true);
            assert!(
                field <= panel,
                "at a {width:?} panel the stacked control reaches {field:?}, past {panel:?}"
            );
        }
    }

    /// Why [`super::super::quick_nav::input_item`] exists. Not a wish — if this ever stops
    /// overflowing, the stacked layout is no longer load-bearing and the row can
    /// go back to sitting beside its label.
    #[gpui::test]
    fn a_side_by_side_row_would_not_fit(cx: &mut TestAppContext) {
        let width = DIALOG_WIDTH - CARD_CHROME;
        let (field, panel) = edges(cx, width, false);
        assert!(
            field > panel,
            "a horizontal input row now fits ({field:?} within {panel:?}); \
             super::input_item's stacked layout may no longer be needed"
        );
    }
}
