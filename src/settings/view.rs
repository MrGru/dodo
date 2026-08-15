use std::{cell::Cell, rc::Rc};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::list::{List, ListState};
use gpui_component::setting::{SelectIndex, Settings};
use gpui_component::{ActiveTheme as _, v_flex};

use super::general::StartupStatus;
use super::pages::pages;
use super::search::{SearchDelegate, Setting};
use super::{DismissSettingsResults, SEARCH_CONTEXT, SIDEBAR_WIDTH};
use crate::i18n::{shell, t};
use crate::layout::Layout;

/// Height of the search box once the user has typed something. It is fixed so
/// that the list's own `size_full` layout has a definite box to fill; an empty
/// query collapses the box back to [`collapsed_height`].
///
/// The box is drawn as an overlay, so growing to this height covers the
/// settings panel instead of pushing it down.
const RESULTS_HEIGHT: f32 = 232.;

/// Height of the search box with no results under it.
///
/// The library draws the query input at `h_8` — 2rem, so it tracks the font
/// size setting — with a 1px rule under it, and the box adds its own 1px border
/// top and bottom.
fn collapsed_height(window: &Window) -> Pixels {
    window.rem_size() * 2. + px(3.)
}

/// The dialog body: the search box and result list above the library's own
/// settings panel.
pub(super) struct SettingsView {
    search: Entity<ListState<SearchDelegate>>,
    /// The pane the Features page edits. See [`open`] for why it is weak.
    layout: WeakEntity<Layout>,
    /// Index into the vec [`pages`] returns — the open sidebar entry.
    page_ix: usize,
    /// Bumped on every jump. `Settings` keeps its selected page in window state
    /// keyed by the element id and reads `default_selected_index` only when
    /// that state is first created, so handing it a fresh id is the only way to
    /// drive the selection from outside.
    nonce: usize,
    /// Where the last jump landed. Its control is drawn with the accent colour
    /// until the next jump, so the setting is obvious on arrival.
    highlight: Option<Setting>,
    /// This field-local snapshot keeps the OS reader out of render.
    start_with_os: Rc<Cell<StartupStatus>>,
}

impl SettingsView {
    pub(super) fn new(
        layout: WeakEntity<Layout>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let view = cx.entity().downgrade();
        let search = cx.new(|cx| {
            let delegate = SearchDelegate {
                view,
                query: SharedString::default(),
                matches: Vec::new(),
                selected: None,
            };

            ListState::new(delegate, window, cx).searchable(true)
        });

        let start_with_os = Rc::new(Cell::new(StartupStatus::Loading));

        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let start_with_os = start_with_os.clone();
            cx.spawn(async move |_, cx| {
                // SMAppService's headers do not mark these selectors as
                // main-thread-only, so this (and Windows' registry read) can
                // run off the UI thread. The first Settings frame therefore
                // paints before the potentially slow OS query completes.
                let status = cx
                    .background_executor()
                    .spawn(async move { StartupStatus::read_once(crate::tray::startup::enabled) })
                    .await;
                cx.update(|cx| {
                    start_with_os.set(status);
                    cx.refresh_windows();
                });
            })
            .detach();
        }

        Self {
            search,
            layout,
            page_ix: 0,
            nonce: 0,
            highlight: None,
            start_with_os,
        }
    }

    /// Switches to the setting's section, highlights it, and clears the query so
    /// the result list gets out of the way.
    ///
    /// The clearing is deferred: this runs from inside the list's own update, so
    /// touching the list again here would panic.
    pub(super) fn navigate_to(
        &mut self,
        setting: Setting,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.page_ix = setting.page_ix();
        self.highlight = Some(setting);
        self.nonce += 1;

        cx.defer_in(window, Self::clear_search);
        cx.notify();
    }

    /// Empties the search box and the results under it.
    ///
    /// `ListState::set_query` alone is not enough despite what its doc comment
    /// says: it goes through `InputState::set_value`, which suppresses the
    /// `Change` event, so the delegate's own search never runs. Clearing the
    /// delegate by hand is the other half.
    fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |list, cx| {
            list.set_query("", window, cx);

            let delegate = list.delegate_mut();
            delegate.query = SharedString::default();
            delegate.matches.clear();
            delegate.selected = None;

            cx.notify();
        });
        cx.notify();
    }

    /// Escape with results showing dismisses them; Escape with an empty query
    /// propagates, letting the input, the list and finally the dialog handle it
    /// as before.
    fn dismiss_results(
        &mut self,
        _: &DismissSettingsResults,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.search.read(cx).delegate().query.is_empty() {
            cx.propagate();
            return;
        }

        self.clear_search(window, cx);
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let searching = !self.search.read(cx).delegate().query.is_empty();
        let collapsed = collapsed_height(window);

        v_flex()
            .key_context(SEARCH_CONTEXT)
            .on_action(cx.listener(Self::dismiss_results))
            .w_full()
            .h(px(440.))
            .gap_2()
            .child(
                // The slot the search box occupies in the layout. It never grows:
                // the box itself is drawn by the overlay below, so results float
                // over the settings panel rather than pushing it down.
                div().relative().w_full().flex_none().h(collapsed).child(
                    // `deferred` paints after the rest of the dialog, which is
                    // what puts the results on top of the panel; `left_0` +
                    // `right_0` size the box from the slot's own edges, so the
                    // input inside it gets a real width to lay text out in.
                    deferred(
                        v_flex()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .h(if searching {
                                px(RESULTS_HEIGHT)
                            } else {
                                collapsed
                            })
                            .overflow_hidden()
                            .bg(cx.theme().background)
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded(cx.theme().radius)
                            .when(searching, |this| this.shadow_md())
                            .child(
                                List::new(&self.search).search_placeholder(t(
                                    shell::Text::SearchSettingsPlaceholder,
                                    cx,
                                )),
                            ),
                    )
                    .with_priority(1),
                ),
            )
            .child(
                div().flex_1().min_h_0().child(
                    Settings::new(SharedString::from(format!("dodo-settings-{}", self.nonce)))
                        .sidebar_width(SIDEBAR_WIDTH)
                        .header_style(&StyleRefinement::default().hidden())
                        .default_selected_index(SelectIndex {
                            page_ix: self.page_ix,
                            group_ix: None,
                        })
                        .pages(pages(
                            &self.layout,
                            self.highlight,
                            self.start_with_os.clone(),
                            cx,
                        )),
                ),
            )
    }
}
