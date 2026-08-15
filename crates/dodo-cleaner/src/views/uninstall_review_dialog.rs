//! The uninstall review dialog (Phase 9).
//!
//! Opened from an Installed Apps row's "Begin uninstall review" action. Follows
//! the `saved_query_form` / `row_editor` pattern from the Database Explorer:
//! the dialog body is its own entity so a later `cx.notify()` — once the
//! background analysis finishes — repaints it without the page knowing
//! anything happened. `open` only ever needs `&mut Window` for the initial
//! `window.open_dialog` call; the analysis itself runs on the background
//! executor and reports back through `Entity::update`, so no continuation
//! here ever needs a `Window` it does not have.
//!
//! Selection follows the ticket's confirmation-flow list for App uninstall:
//! app, related files, confidence, shared/excluded paths, and estimated
//! size are all shown before the single "Move to Trash" action, which is
//! this dialog's own footer button rather than the generic alert dialog —
//! there is no `Dialog::on_ok` footer to misuse here (see
//! `gpui-component-recipes`'s note on why `AlertDialog` is required for a
//! plain confirm/cancel but not for a dialog whose body is a real entity).

use std::collections::HashSet;
use std::path::PathBuf;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::{
    ActiveTheme as _, Sizable as _, StyledExt as _, WindowExt as _, h_flex, v_flex,
};

use crate::core::item::{CleanableItem, CleanableItemId};
use crate::core::risk::{ItemCapability, SelectionPolicy};
use crate::i18n::{cleaner, t};
use crate::macos::applications::confidence::MatchConfidence;
use crate::macos::applications::identity::AppIdentity;
use crate::macos::applications::locations::LocationScope;
use crate::macos::applications::review::{self, UninstallReview, UninstallReviewError};

use super::CleanerView;

const WIDTH: gpui::Pixels = px(640.);
const PADDING: gpui::Pixels = px(32.);

enum ReviewState {
    Loading,
    // Boxed: `UninstallReview` carries a `Vec<UninstallCandidate>` of
    // `CleanableItem`s and is far larger than `Refused`'s bare error enum;
    // boxing keeps `ReviewState` (and every `Option`/local holding one) from
    // being sized to its biggest variant.
    Ready(Box<UninstallReview>),
    Refused(UninstallReviewError),
}

/// Opens the dialog immediately in a loading state, then kicks off the
/// (read-only) leftover analysis on the background executor.
pub fn open(
    page: Entity<CleanerView>,
    app_item: CleanableItem,
    other_apps: Vec<AppIdentity>,
    home: Option<PathBuf>,
    window: &mut Window,
    cx: &mut App,
) {
    let name = app_item.display_name.clone();
    let dialog = cx.new(|cx| UninstallReviewDialog::new(page, app_item, other_apps, home, cx));
    let body = dialog.clone();
    window.open_dialog(cx, move |dialog_builder, _, cx| {
        dialog_builder
            .title(t(
                cleaner::Text::UninstallReviewTitle { name: name.clone() },
                cx,
            ))
            .w(WIDTH)
            .content({
                let body = body.clone();
                move |content, _, _| content.child(div().w(WIDTH - PADDING).child(body.clone()))
            })
    });
}

struct UninstallReviewDialog {
    page: Entity<CleanerView>,
    state: ReviewState,
    selected: HashSet<CleanableItemId>,
    task: Option<gpui::Task<()>>,
}

impl UninstallReviewDialog {
    fn new(
        page: Entity<CleanerView>,
        app_item: CleanableItem,
        other_apps: Vec<AppIdentity>,
        home: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            page,
            state: ReviewState::Loading,
            selected: HashSet::new(),
            task: None,
        };
        this.task = Some(cx.spawn(async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    review::build_uninstall_review(&app_item, &other_apps, home.as_deref())
                })
                .await;
            let _ = weak.update(cx, |this, cx| {
                this.selected = match &result {
                    Ok(review) => default_selection(review),
                    Err(_) => HashSet::new(),
                };
                this.state = match result {
                    Ok(review) => ReviewState::Ready(Box::new(review)),
                    Err(error) => ReviewState::Refused(error),
                };
                this.task = None;
                cx.notify();
            });
        }));
        this
    }

    fn set_selected(&mut self, id: CleanableItemId, checked: bool, cx: &mut Context<Self>) {
        if checked {
            self.selected.insert(id);
        } else {
            self.selected.remove(&id);
        }
        cx.notify();
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ReviewState::Ready(review) = &self.state else {
            return;
        };
        let mut items = vec![review.app.clone()];
        for candidate in &review.candidates {
            if candidate
                .item
                .capabilities
                .contains(&ItemCapability::MoveToTrash)
                && self.selected.contains(&candidate.item.id)
            {
                items.push(candidate.item.clone());
            }
        }
        self.page
            .update(cx, |page, cx| page.start_uninstall_cleanup(items, cx));
        window.close_dialog(cx);
    }
}

fn default_selection(review: &UninstallReview) -> HashSet<CleanableItemId> {
    review
        .candidates
        .iter()
        .filter(|candidate| candidate.item.selection_policy == SelectionPolicy::SelectedByDefault)
        .map(|candidate| candidate.item.id)
        .collect()
}

fn confidence_label(confidence: MatchConfidence, cx: &App) -> gpui::SharedString {
    let str = match confidence {
        MatchConfidence::Confirmed => cleaner::Text::ConfidenceConfirmed,
        MatchConfidence::High => cleaner::Text::ConfidenceHigh,
        MatchConfidence::Medium => cleaner::Text::ConfidenceMedium,
        MatchConfidence::Low => cleaner::Text::ConfidenceLow,
        MatchConfidence::SharedOrUnsafe => cleaner::Text::ConfidenceSharedOrUnsafe,
    };
    t(str, cx)
}

impl Render for UninstallReviewDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.state {
            ReviewState::Loading => v_flex()
                .gap_2()
                .p_2()
                .child(t(cleaner::Text::UninstallLoading, cx)),
            ReviewState::Refused(UninstallReviewError::ProtectedApplication) => v_flex()
                .gap_2()
                .p_2()
                .child(t(cleaner::Text::UninstallRefusedProtected, cx))
                .child(close_button(cx)),
            ReviewState::Refused(UninstallReviewError::NotAnApplication) => v_flex()
                .gap_2()
                .p_2()
                .child(t(cleaner::Text::UninstallRefusedNotApplication, cx))
                .child(close_button(cx)),
            ReviewState::Ready(review) => {
                let app = review.app.clone();
                let candidates = review.candidates.clone();
                let selected_count = candidates
                    .iter()
                    .filter(|candidate| self.selected.contains(&candidate.item.id))
                    .count();
                let total_size = app.logical_size
                    + candidates
                        .iter()
                        .filter(|candidate| self.selected.contains(&candidate.item.id))
                        .map(|candidate| candidate.item.logical_size)
                        .sum::<u64>();

                v_flex()
                    .gap_3()
                    .p_2()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().font_bold().child(app.display_name.clone()))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(app.path.display().to_string()),
                            ),
                    )
                    .child(
                        div()
                            .font_bold()
                            .text_sm()
                            .child(t(cleaner::Text::UninstallRelatedFilesHeader, cx)),
                    )
                    .when(candidates.is_empty(), |list| {
                        list.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(t(cleaner::Text::UninstallNoRelatedFiles, cx)),
                        )
                    })
                    .child({
                        // A plain `for` loop, not `.iter().map(..)`: each
                        // `candidate_row` call reborrows `cx` (to build its
                        // own `cx.listener` checkbox handler) and returns an
                        // owned `AnyElement`, which an `FnMut` closure passed
                        // to `.children(..)` cannot do — the borrow would
                        // have to escape the closure body.
                        let mut rows = Vec::with_capacity(candidates.len());
                        for candidate in &candidates {
                            let selected = self.selected.contains(&candidate.item.id);
                            rows.push(candidate_row(candidate, selected, cx).into_any_element());
                        }
                        div()
                            .id("cleaner-uninstall-candidates")
                            .max_h(px(320.))
                            .overflow_scroll()
                            .child(v_flex().gap_2().children(rows))
                    })
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t(cleaner::Text::UninstallDestinationNote, cx)),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(div().text_sm().child(format!(
                                "{} · {}",
                                CleanerView::format_bytes(total_size),
                                selected_count + 1
                            )))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        Button::new("cleaner-uninstall-cancel")
                                            .ghost()
                                            .small()
                                            .label(t(cleaner::Text::CancelScan, cx))
                                            .on_click(|_, window, cx| window.close_dialog(cx)),
                                    )
                                    .child(
                                        Button::new("cleaner-uninstall-confirm")
                                            .danger()
                                            .small()
                                            .label(t(cleaner::Text::UninstallMoveToTrash, cx))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.confirm(window, cx)
                                            })),
                                    ),
                            ),
                    )
            }
        }
    }
}

fn close_button(cx: &App) -> impl IntoElement {
    Button::new("cleaner-uninstall-close")
        .ghost()
        .small()
        .label(t(cleaner::Text::UninstallClose, cx))
        .on_click(|_, window, cx| window.close_dialog(cx))
}

fn candidate_row(
    candidate: &review::UninstallCandidate,
    selected: bool,
    cx: &mut Context<UninstallReviewDialog>,
) -> impl IntoElement {
    let item_id = candidate.item.id;
    let can_select = candidate
        .item
        .capabilities
        .contains(&ItemCapability::MoveToTrash);
    let is_system = candidate.scope == LocationScope::System;

    v_flex()
        .gap_1()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .p_2()
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .gap_2()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .when(can_select, |row| {
                            row.child(
                                Checkbox::new(("cleaner-uninstall-candidate", item_id.0))
                                    .checked(selected)
                                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                        this.set_selected(item_id, *checked, cx)
                                    })),
                            )
                        })
                        .child(div().text_sm().child(candidate.item.display_name.clone())),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .when(is_system, |row| {
                            row.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t(cleaner::Text::UninstallScanOnlyBadge, cx)),
                            )
                        })
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(confidence_label(candidate.confidence, cx)),
                        )
                        .child(
                            div()
                                .text_sm()
                                .child(CleanerView::format_bytes(candidate.item.logical_size)),
                        ),
                ),
        )
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(candidate.item.path.display().to_string()),
        )
        .when(!candidate.item.warnings.is_empty(), |card| {
            card.children(candidate.item.warnings.iter().map(|warning| {
                div()
                    .text_xs()
                    .text_color(cx.theme().warning)
                    .child(warning.message.clone())
            }))
        })
}
