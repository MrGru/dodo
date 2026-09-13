// Deliberately not `use super::*`: that pulls in `use gpui_kit::*`, whose `test`
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
    use gpui_kit::component::setting::{
        SettingField, SettingGroup, SettingItem, SettingPage, Settings,
    };
    use gpui_kit::prelude::FluentBuilder as _;
    use gpui_kit::{
        AppContext as _, Axis, Bounds, Context, InteractiveElement as _, IntoElement,
        ParentElement as _, Pixels, Render, SharedString, StyleRefinement, Styled as _,
        TestAppContext, VisualTestContext, Window, WindowBounds, WindowOptions, div, point, px,
        size,
    };

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
                let horizontal = matches!(options.layout(), Axis::Horizontal);
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
        cx.update(gpui_kit::component::init);

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

    #[gpui_kit::test]
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
    #[gpui_kit::test]
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

/// The Settings search box is `List::new(&state).searchable(true)`, hosted in
/// the Settings dialog. It went missing after the gpui-kit 0.6 migration, so
/// [`super::super::view::SettingsView`] now draws the always-visible query input
/// in **normal layout flow** and defers only the expanded result list.
///
/// These render that box exactly as `open()` builds it — a real `Root`, a real
/// `Dialog`, the real window size, `collapsed_height` and the `Settings` panel
/// beside it — in both states. The searchable list draws its query input above
/// the delegate's body, so a body sitting well below the box's top proves the
/// input rendered with real height; and the collapsed box's own height proves it
/// is the in-flow input (~one row) rather than the floated result list.
pub(super) mod search_box {
    use gpui_kit::component::IndexPath;
    use gpui_kit::component::list::{List, ListState};
    use gpui_kit::component::v_flex;
    use gpui_kit::prelude::FluentBuilder as _;
    use gpui_kit::{
        AnyElement, AppContext as _, Bounds, Context, InteractiveElement as _, IntoElement,
        ParentElement as _, Pixels, Render, Styled as _, TestAppContext, VisualTestContext, Window,
        WindowBounds, WindowOptions, div, point, px, size,
    };

    struct Delegate;

    impl gpui_kit::component::list::ListDelegate for Delegate {
        type Item = gpui_kit::component::list::ListItem;

        fn items_count(&self, _: usize, _: &gpui_kit::App) -> usize {
            0
        }

        fn render_item(
            &mut self,
            _: IndexPath,
            _: &mut Window,
            _: &mut Context<ListState<Self>>,
        ) -> Option<Self::Item> {
            None
        }

        fn render_initial(
            &mut self,
            _: &mut Window,
            _: &mut Context<ListState<Self>>,
        ) -> Option<AnyElement> {
            // Tag the body so its top edge is measurable. The searchable input,
            // when it renders, pushes this below itself.
            Some(
                div()
                    .debug_selector(|| "body".into())
                    .h(px(10.))
                    .into_any_element(),
            )
        }

        fn set_selected_index(
            &mut self,
            _: Option<IndexPath>,
            _: &mut Window,
            _: &mut Context<ListState<Self>>,
        ) {
        }
    }

    // ---- The real dialog ----------------------------------------------------
    //
    // `Root::new` only dereferences an `NSView` under `#[cfg(not(test))]` in
    // gpui-kit 0.6, so a real `Dialog` *can* be hosted in a test window now
    // (the `row_layout` note above predates that). This is the faithful
    // reproduction: the search box drawn as `SettingsView::render` builds it,
    // inside a real dialog mounted on a real `Root`.

    use gpui_kit::StyleRefinement;
    use gpui_kit::component::setting::{SelectIndex, SettingGroup, SettingPage, Settings};
    use gpui_kit::component::{Root, WindowExt as _};

    /// Kept in sync with `view::RESULTS_HEIGHT` (private to that module). The
    /// expanded box is far taller than the collapsed one, which is what the
    /// state assertions key off.
    const RESULTS_HEIGHT: f32 = 232.;

    /// The dialog body: a faithful clone of `SettingsView::render` — the
    /// search-box slot (in normal flow when collapsed, a `deferred` overlay when
    /// searching) *and* the `Settings` panel below it, so the box is measured in
    /// the exact company it keeps in production.
    struct DialogBody {
        state: gpui_kit::Entity<ListState<Delegate>>,
        searching: bool,
    }

    fn collapsed(window: &Window) -> Pixels {
        window.rem_size() * 2. + px(3.)
    }

    impl Render for DialogBody {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let collapsed = collapsed(window);
            let searching = self.searching;

            let search_box = v_flex().overflow_hidden().child(
                div()
                    .debug_selector(|| "box".into())
                    .size_full()
                    .child(List::new(&self.state).search_placeholder("Search")),
            );

            v_flex()
                .w_full()
                .h(px(440.))
                .gap_2()
                .child(
                    div()
                        .relative()
                        .w_full()
                        .flex_none()
                        .h(collapsed)
                        .map(|slot| {
                            if searching {
                                slot.child(
                                    gpui_kit::deferred(
                                        search_box
                                            .absolute()
                                            .top_0()
                                            .left_0()
                                            .right_0()
                                            .h(px(RESULTS_HEIGHT)),
                                    )
                                    .with_priority(1),
                                )
                            } else {
                                slot.child(search_box.size_full())
                            }
                        }),
                )
                .child(
                    div().flex_1().min_h_0().child(
                        Settings::new("dodo-settings-0")
                            .sidebar_width(px(200.))
                            .header_style(&StyleRefinement::default().hidden())
                            .default_selected_index(SelectIndex {
                                page_ix: 0,
                                group_ix: None,
                            })
                            .pages(vec![
                                SettingPage::new("General")
                                    .group(SettingGroup::new().title("General")),
                            ]),
                    ),
                )
        }
    }

    /// A base view that mounts the dialog layer, since `Root::render` does not
    /// mount it itself.
    struct Base;

    impl Render for Base {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let dialog_layer = Root::render_dialog_layer(window, cx);
            div().size_full().children(dialog_layer)
        }
    }

    /// Opens the Settings dialog holding the search-box body in the given state
    /// and returns the bounds of the box and of the delegate's list body.
    fn measure(cx: &mut TestAppContext, searching: bool) -> (Bounds<Pixels>, Bounds<Pixels>) {
        cx.update(gpui_kit::component::init);

        // The real app opens a 900x620 window; the dialog is 760 wide and, with
        // the settings body's fixed height, tall enough that the dialog's own
        // `overflow_y_scrollbar` engages — the condition a huge test viewport
        // would hide.
        let window = cx
            .update(|cx| {
                cx.open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds {
                            origin: point(px(0.), px(0.)),
                            size: size(px(900.), px(620.)),
                        })),
                        ..Default::default()
                    },
                    |window, cx| {
                        let view = cx.new(|_| Base);
                        cx.new(|cx| Root::new(view, window, cx))
                    },
                )
            })
            .unwrap();
        let cx = &mut VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();

        cx.update(|window, cx| {
            let body = cx.new(|cx| DialogBody {
                state: cx.new(|cx| ListState::new(Delegate, window, cx).searchable(true)),
                searching,
            });
            window.open_dialog(cx, move |dialog, _, _| {
                dialog.title("Settings").w(px(760.)).child(body.clone())
            });
        });
        cx.run_until_parked();
        // Advance past the dialog's slide-down animation and redraw so bounds
        // settle.
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(500));
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });

        let box_bounds = cx
            .debug_bounds("box")
            .expect("the search box drew in the dialog");
        let body = cx
            .debug_bounds("body")
            .expect("the list body drew in the dialog");
        (box_bounds, body)
    }

    /// The regression: at rest (empty query) the query input must render, and it
    /// must be the **in-flow** box — one input row tall — not the floated result
    /// list. Before the fix the box lived inside the `deferred` overlay, which
    /// stopped painting it under gpui-kit 0.6.
    #[gpui_kit::test]
    fn the_collapsed_search_input_draws_in_normal_flow(cx: &mut TestAppContext) {
        let (search_box, body) = measure(cx, false);
        assert!(
            search_box.size.width >= px(200.),
            "the collapsed search box lost its width in the dialog: {search_box:?}"
        );
        // A collapsed box is a single input row (`h_8` ≈ 2rem + rule); the
        // floated result list would be RESULTS_HEIGHT tall. Well under half of
        // that proves this is the in-flow input, not the overlay.
        assert!(
            search_box.size.height < px(RESULTS_HEIGHT / 2.),
            "the collapsed box is too tall to be the in-flow input row: {search_box:?}"
        );
        // The input sits above the delegate's body, so the body landing below
        // the box's top proves the input rendered with real height.
        assert!(
            body.top() - search_box.top() >= px(24.),
            "the collapsed search box drew no query input (body {:?} vs box {:?})",
            body.top(),
            search_box.top(),
        );
    }

    /// While searching, the result list floats over the panel: the box is the
    /// tall `deferred` overlay, and the query input still renders at its top.
    #[gpui_kit::test]
    fn the_expanded_result_list_floats_over_the_panel(cx: &mut TestAppContext) {
        let (search_box, body) = measure(cx, true);
        assert!(
            search_box.size.height >= px(RESULTS_HEIGHT - 8.),
            "the expanded search box did not grow into the floating result list: {search_box:?}"
        );
        assert!(
            body.top() - search_box.top() >= px(24.),
            "the expanded search box drew no query input (body {:?} vs box {:?})",
            body.top(),
            search_box.top(),
        );
    }
}
