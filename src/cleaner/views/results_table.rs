//! The virtualized results grid for the active category: one
//! [`TableDelegate`] driving a [`DataTable`](gpui_component::table::DataTable),
//! replacing the round-1 `.children(items.iter().map(...))` list that built
//! every row's element tree on every frame regardless of how many were
//! actually on screen. `DataTable` only calls [`Self::render_td`] for rows
//! inside the current scroll viewport, so a 50,000-item User Cache scan
//! costs the same per frame as a 20-item one.
//!
//! This delegate owns no state that [`super::CleanerView`] does not already
//! own — it is a read-only rendering adapter over whatever
//! [`CleanerView::sync_results_table`] last copied in, never a second source
//! of truth for selection or scan results. Every action a row offers (the
//! checkbox, reveal, copy path, keep, begin uninstall review) calls back
//! into the view through [`WeakEntity`] rather than mutating anything here,
//! for the same reason `dodo-theming-settings`' settings-body pattern uses a
//! weak handle: this delegate lives inside the `Entity<TableState<Self>>`
//! that `CleanerView` owns, so a *strong* back-reference would be a cycle.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui::{
    AnyElement, App, ClipboardItem, Context, Div, Image, ImageFormat, ImageSource,
    InteractiveElement as _, IntoElement, ParentElement as _, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, StyledImage as _, WeakEntity, Window, div, img,
    px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_component::table::{Column, TableDelegate, TableState};
use gpui_component::{ActiveTheme as _, Icon, Sizable as _, h_flex};

use super::CleanerView;
use crate::app_icon::AppIcon;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::icon::IconRaster;
use crate::cleaner::core::item::{CleanableItem, CleanableItemId, ItemMetadata};
use crate::cleaner::core::risk::{ItemCapability, RiskLevel};
use crate::i18n::{Str, t};

const CHECKBOX_COLUMN_WIDTH: gpui::Pixels = px(36.);
const RISK_COLUMN_WIDTH: gpui::Pixels = px(112.);
const SIZE_COLUMN_WIDTH: gpui::Pixels = px(90.);
const ACTIONS_COLUMN_WIDTH: gpui::Pixels = px(64.);

/// The icon drawn beside a row's name, keyed on the category every item in
/// a given render shares (results are always scoped to the active category —
/// see [`crate::cleaner::state::CleanerState::result_for`]).
pub(super) fn category_icon(category: CleanerCategory) -> AppIcon {
    match category {
        CleanerCategory::SystemJunk => AppIcon::Folder,
        CleanerCategory::UserCache => AppIcon::HardDrive,
        CleanerCategory::MailFiles => AppIcon::Inbox,
        CleanerCategory::TrashBins => AppIcon::Trash,
        CleanerCategory::LargeOldFiles => AppIcon::File,
        CleanerCategory::InstalledApps => AppIcon::Building2,
        CleanerCategory::OrphanedFiles => AppIcon::AlertTriangle,
        CleanerCategory::AiApps => AppIcon::Bot,
        CleanerCategory::XcodeJunk => AppIcon::SquareCode,
        CleanerCategory::HomebrewCache => AppIcon::Layers,
        CleanerCategory::NodeToolingCache => AppIcon::SquareTerminal,
        CleanerCategory::DockerCache => AppIcon::Container,
        CleanerCategory::UniversalBinaries => AppIcon::Cpu,
        CleanerCategory::LanguageFiles => AppIcon::Globe,
    }
}

/// The platform-appropriate label for [`ItemCapability::RevealInFinder`] —
/// the same capability powers "Reveal in Finder", "Reveal in Explorer" and
/// "Reveal in file manager", but the word for the thing being revealed into
/// is not the same word on every desktop.
fn reveal_label() -> Str {
    #[cfg(target_os = "macos")]
    {
        Str::CleanerRevealInFinder
    }
    #[cfg(target_os = "windows")]
    {
        Str::CleanerRevealInExplorer
    }
    #[cfg(target_os = "linux")]
    {
        Str::CleanerRevealInFileManager
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Str::CleanerRevealInFinder
    }
}

pub struct ResultsTableDelegate {
    view: WeakEntity<CleanerView>,
    items: Vec<CleanableItem>,
    selected_ids: HashSet<CleanableItemId>,
    category: CleanerCategory,
    /// One ready-to-draw GPUI image per row that carries an icon, built when
    /// the items are replaced and never in [`Self::render_td`].
    /// `Image::from_bytes` takes an owned `Vec` and hashes it to derive the
    /// id GPUI's asset cache is keyed on, so building one per visible cell
    /// per frame would re-copy and re-hash every visible icon sixty times a
    /// second for a value that cannot change until the items do.
    icons: HashMap<CleanableItemId, Arc<Image>>,
}

impl ResultsTableDelegate {
    pub fn new(view: WeakEntity<CleanerView>) -> Self {
        Self {
            view,
            items: Vec::new(),
            selected_ids: HashSet::new(),
            category: CleanerCategory::SystemJunk,
            icons: HashMap::new(),
        }
    }

    /// Replaces what the grid shows. The caller (`CleanerView::render`, via
    /// `sync_results_table`) follows this with `TableState::refresh`, and
    /// only calls it when the *items* actually changed — see
    /// `super::results_sync`.
    pub fn set(
        &mut self,
        category: CleanerCategory,
        items: Vec<CleanableItem>,
        selected_ids: HashSet<CleanableItemId>,
    ) {
        self.category = category;
        self.icons = items
            .iter()
            .filter_map(|item| {
                let raster = item_icon(item)?;
                Some((
                    item.id,
                    Arc::new(Image::from_bytes(
                        ImageFormat::Png,
                        raster.as_bytes().to_vec(),
                    )),
                ))
            })
            .collect();
        self.items = items;
        self.selected_ids = selected_ids;
    }

    /// Replaces which rows are ticked, leaving the rows themselves (and
    /// their icon payloads) exactly where they are. Ticking one checkbox on
    /// a large result must not cost a re-copy of the whole result.
    pub fn set_selection(&mut self, selected_ids: HashSet<CleanableItemId>) {
        self.selected_ids = selected_ids;
    }

    fn shows_selection(&self) -> bool {
        !matches!(
            self.category,
            CleanerCategory::InstalledApps
                | CleanerCategory::AiApps
                | CleanerCategory::XcodeJunk
                | CleanerCategory::HomebrewCache
                | CleanerCategory::UniversalBinaries
                | CleanerCategory::LanguageFiles
        )
    }

    /// Every row the header checkbox may bulk-select — the same
    /// [`ItemCapability::MoveToTrash`] gate the per-row checkbox already
    /// uses, so the header never offers to select a read-only row.
    fn selectable_ids(&self) -> Vec<CleanableItemId> {
        self.items
            .iter()
            .filter(|item| item.capabilities.contains(&ItemCapability::MoveToTrash))
            .map(|item| item.id)
            .collect()
    }

    /// The header checkbox: unchecked when nothing selectable is selected
    /// (click selects every selectable row), checked when every selectable
    /// row is (click clears), and a small hand-drawn indeterminate square —
    /// `Checkbox` has no tri-state visual at this revision — in between
    /// (click also selects the rest, the common "finish the selection"
    /// convention). Empty when there is nothing selectable to bulk-act on.
    fn render_header_select_cell(&self, cx: &App) -> AnyElement {
        let selectable = self.selectable_ids();
        if selectable.is_empty() {
            return div().into_any_element();
        }
        let selected_count = selectable
            .iter()
            .filter(|id| self.selected_ids.contains(id))
            .count();
        let view = self.view.clone();

        if selected_count == 0 {
            h_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(
                    Checkbox::new("cleaner-select-all")
                        .checked(false)
                        .tooltip(t(Str::CleanerSelectAll, cx))
                        .on_click(move |_, _, cx| {
                            let _ = view.update(cx, |view, cx| view.select_all_visible(cx));
                        }),
                )
                .into_any_element()
        } else if selected_count == selectable.len() {
            h_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(
                    Checkbox::new("cleaner-select-all")
                        .checked(true)
                        .tooltip(t(Str::CleanerDeselectAll, cx))
                        .on_click(move |_, _, cx| {
                            let _ = view.update(cx, |view, cx| view.deselect_all(cx));
                        }),
                )
                .into_any_element()
        } else {
            h_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .id("cleaner-select-all-indeterminate")
                        .w(px(14.))
                        .h(px(14.))
                        .rounded_sm()
                        .bg(cx.theme().primary)
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .tooltip(move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(t(Str::CleanerSelectAll, cx))
                                .build(window, cx)
                        })
                        .child(
                            div()
                                .w(px(8.))
                                .h(px(2.))
                                .rounded_sm()
                                .bg(cx.theme().primary_foreground),
                        )
                        .on_click(move |_, _, cx| {
                            let _ = view.update(cx, |view, cx| view.select_all_visible(cx));
                        }),
                )
                .into_any_element()
        }
    }

    fn render_select_cell(&self, item: &CleanableItem, cx: &App) -> AnyElement {
        if !item.capabilities.contains(&ItemCapability::MoveToTrash) {
            return div().into_any_element();
        }
        let selected = self.selected_ids.contains(&item.id);
        let item_id = item.id;
        let view = self.view.clone();
        h_flex()
            .size_full()
            .items_center()
            .justify_center()
            .child(
                Checkbox::new(("cleaner-row-select", item_id.0))
                    .checked(selected)
                    .tooltip(if selected {
                        t(Str::CleanerDeselectItem, cx)
                    } else {
                        t(Str::CleanerSelectItem, cx)
                    })
                    .on_click(move |_, _, cx| {
                        let _ = view.update(cx, |view, cx| view.toggle_selected(item_id, cx));
                    }),
            )
            .into_any_element()
    }

    fn render_name_cell(&self, item: &CleanableItem, cx: &App) -> AnyElement {
        // The frame ("Explanation:") is translated; `item.explanation` is a
        // scanner-produced sentence describing what was found and is not —
        // there is nothing to translate it with, the same posture
        // `dodo-i18n-text` documents for a parser's own error detail.
        let explanation = SharedString::from(format!(
            "{}: {}",
            t(Str::CleanerExplanation, cx),
            item.explanation
        ));
        div()
            .id(("cleaner-row-name", item.id.0))
            .size_full()
            .min_w_0()
            .tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(explanation.clone()).build(window, cx)
            })
            .child(
                h_flex()
                    .size_full()
                    .min_w_0()
                    .items_center()
                    .gap_2()
                    .child(self.render_icon(item, cx))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(SharedString::from(item.display_name.clone())),
                    ),
            )
            .into_any_element()
    }

    /// The glyph beside a row's name: the application's own icon when the
    /// scanner captured one, and this category's generic glyph otherwise —
    /// which is also what a failed decode falls back to, so a row is never
    /// blank.
    fn render_icon(&self, item: &CleanableItem, cx: &App) -> AnyElement {
        let fallback = category_icon(item.category);
        self.icons.get(&item.id).map_or_else(
            || {
                Icon::new(fallback)
                    .size_4()
                    .flex_shrink_0()
                    .text_color(cx.theme().muted_foreground)
                    .into_any_element()
            },
            |image| {
                img(ImageSource::Image(image.clone()))
                    .size_4()
                    .flex_shrink_0()
                    .with_fallback(move || Icon::new(fallback).size_4().into_any_element())
                    .into_any_element()
            },
        )
    }

    fn render_risk_cell(&self, item: &CleanableItem, cx: &App) -> AnyElement {
        let (label, color) = match item.risk {
            RiskLevel::SafeRecreatable => (Str::CleanerRiskSafe, cx.theme().success),
            RiskLevel::ReviewRecommended => (Str::CleanerRiskReview, cx.theme().warning),
            RiskLevel::UserData => (Str::CleanerRiskUserData, cx.theme().warning),
            RiskLevel::ApplicationMutation => (Str::CleanerRiskAppChange, cx.theme().danger),
            RiskLevel::Protected => (Str::CleanerRiskProtected, cx.theme().danger),
        };
        h_flex()
            .size_full()
            .items_center()
            .child(
                div()
                    .px_2()
                    .rounded(cx.theme().radius)
                    .bg(color.opacity(0.12))
                    .text_color(color)
                    .text_xs()
                    .child(t(label, cx)),
            )
            .into_any_element()
    }

    fn render_size_cell(&self, item: &CleanableItem, _cx: &App) -> AnyElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_end()
            .text_sm()
            .child(CleanerView::format_bytes(item.logical_size))
            .into_any_element()
    }

    fn render_path_cell(&self, item: &CleanableItem, _cx: &App) -> AnyElement {
        let path_text = SharedString::from(item.path.display().to_string());
        div()
            .id(("cleaner-row-path", item.id.0))
            .size_full()
            .min_w_0()
            .flex()
            .items_center()
            .text_sm()
            .tooltip({
                let path_text = path_text.clone();
                move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(path_text.clone()).build(window, cx)
                }
            })
            .child(div().min_w_0().truncate().child(path_text))
            .into_any_element()
    }

    fn render_actions_cell(&self, item: &CleanableItem, cx: &App) -> AnyElement {
        let can_reveal = item.capabilities.contains(&ItemCapability::RevealInFinder);
        let can_keep = item.capabilities.contains(&ItemCapability::MarkAsKept);
        let can_uninstall = item
            .capabilities
            .contains(&ItemCapability::UninstallApplication);
        if !can_reveal && !can_keep && !can_uninstall {
            return h_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(copy_path_button(item, cx))
                .into_any_element();
        }

        let view = self.view.clone();
        let reveal_path = item.path.clone();
        let keep_item = item.clone();
        let uninstall_item = item.clone();

        h_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_1()
            .child(copy_path_button(item, cx))
            .child(
                Button::new(("cleaner-row-actions", item.id.0))
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::Ellipsis)
                    .tooltip(t(Str::CleanerMoreActions, cx))
                    .dropdown_menu(move |menu, _, cx| {
                        let mut menu = menu;
                        if can_reveal {
                            let view = view.clone();
                            let reveal_path = reveal_path.clone();
                            menu = menu.item(
                                PopupMenuItem::new(t(reveal_label(), cx))
                                    .icon(AppIcon::FolderOpen)
                                    .on_click(move |_, _, cx| {
                                        let path = reveal_path.clone();
                                        let _ = view.update_in(cx, |view, window, cx| {
                                            view.reveal_in_finder(path, window, cx)
                                        });
                                    }),
                            );
                        }
                        if can_keep {
                            let view = view.clone();
                            let keep_item = keep_item.clone();
                            menu = menu.item(
                                PopupMenuItem::new(t(Str::CleanerKeepItem, cx))
                                    .icon(AppIcon::CircleCheck)
                                    .on_click(move |_, _, cx| {
                                        let item = keep_item.clone();
                                        let _ =
                                            view.update(cx, |view, cx| view.mark_kept(item, cx));
                                    }),
                            );
                        }
                        if can_uninstall {
                            let view = view.clone();
                            let uninstall_item = uninstall_item.clone();
                            menu = menu.item(
                                PopupMenuItem::new(t(Str::CleanerBeginUninstallReview, cx))
                                    .icon(AppIcon::Trash)
                                    .on_click(move |_, _, cx| {
                                        let item = uninstall_item.clone();
                                        let _ = view.update_in(cx, |view, window, cx| {
                                            view.begin_uninstall_review(item, window, cx)
                                        });
                                    }),
                            );
                        }
                        menu
                    }),
            )
            .into_any_element()
    }
}

/// The per-application icon a scanner captured, if this item has one — the
/// one place that knows both metadata variants carry the same kind of
/// payload.
fn item_icon(item: &CleanableItem) -> Option<&IconRaster> {
    match &item.metadata {
        ItemMetadata::Application(metadata) => metadata.icon.as_ref(),
        ItemMetadata::UniversalBinary(metadata) => metadata.icon.as_ref(),
        _ => None,
    }
}

fn copy_path_button(item: &CleanableItem, cx: &App) -> impl IntoElement {
    let path_text = item.path.display().to_string();
    Button::new(("cleaner-row-copy", item.id.0))
        .ghost()
        .xsmall()
        .icon(AppIcon::Copy)
        .tooltip(t(Str::CleanerCopyPath, cx))
        .on_click(move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(path_text.clone()));
        })
}

impl TableDelegate for ResultsTableDelegate {
    fn columns_count(&self, _cx: &App) -> usize {
        5 + self.shows_selection() as usize
    }

    fn rows_count(&self, _cx: &App) -> usize {
        self.items.len()
    }

    fn render_th(
        &mut self,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        if self.shows_selection() && col_ix == 0 {
            return self.render_header_select_cell(cx);
        }
        div()
            .size_full()
            .child(self.column(col_ix, cx).name.clone())
            .into_any_element()
    }

    fn column(&self, col_ix: usize, cx: &App) -> Column {
        let col_ix = col_ix + (!self.shows_selection()) as usize;
        match col_ix {
            0 => Column::new("select", "")
                .width(CHECKBOX_COLUMN_WIDTH)
                .min_width(CHECKBOX_COLUMN_WIDTH)
                .max_width(CHECKBOX_COLUMN_WIDTH)
                .resizable(false)
                .movable(false)
                .selectable(false),
            1 => Column::new("name", t(Str::CleanerColumnName, cx)).min_width(px(140.)),
            2 => Column::new("risk", t(Str::CleanerColumnRisk, cx))
                .width(RISK_COLUMN_WIDTH)
                .min_width(RISK_COLUMN_WIDTH)
                .selectable(false),
            3 => Column::new("size", t(Str::CleanerColumnSize, cx))
                .width(SIZE_COLUMN_WIDTH)
                .min_width(SIZE_COLUMN_WIDTH)
                .text_right()
                .selectable(false),
            4 => Column::new("path", t(Str::CleanerPath, cx)).min_width(px(160.)),
            5 => Column::new("actions", t(Str::CleanerColumnActions, cx))
                .width(ACTIONS_COLUMN_WIDTH)
                .min_width(ACTIONS_COLUMN_WIDTH)
                .max_width(px(96.))
                .movable(false)
                .selectable(false),
            _ => Column::new("", ""),
        }
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        h_flex()
            .size_full()
            .justify_center()
            .items_center()
            .text_color(cx.theme().muted_foreground)
            .child(t(Str::CleanerNoResultsYet, cx))
    }

    fn render_tr(
        &mut self,
        row_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        div().id(("cleaner-result-row", row_ix))
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        // Borrowed, never cloned: this runs once per visible cell per frame,
        // and a `CleanableItem` carries an application's whole TIFF icon —
        // cloning it six times a row, sixty times a second, is pure copying.
        let Some(item) = self.items.get(row_ix) else {
            return div().into_any_element();
        };
        let col_ix = col_ix + (!self.shows_selection()) as usize;
        match col_ix {
            0 => self.render_select_cell(item, cx),
            1 => self.render_name_cell(item, cx),
            2 => self.render_risk_cell(item, cx),
            3 => self.render_size_cell(item, cx),
            4 => self.render_path_cell(item, cx),
            5 => self.render_actions_cell(item, cx),
            _ => div().into_any_element(),
        }
    }
}
