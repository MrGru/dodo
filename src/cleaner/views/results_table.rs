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
//!
//! # Which columns exist is not this file's decision
//!
//! [`super::results_layout`] is: it maps the pane width the view measured to
//! an ordered list of columns and their pixel widths, and this file reads
//! that list in [`Self::column`] and dispatches on it in [`Self::render_td`].
//! There is no `match col_ix { 0 => …, 1 => … }` here any more, and no
//! index-shifting for the categories that draw no checkbox — the layout
//! simply does not contain a column those categories have no use for.
//!
//! Two consequences worth naming. **Every column is fixed and non-resizable**
//! (`min_width == width == max_width`): the widths are now derived from the
//! pane so that the actions group is never pushed off the right edge, and a
//! hand-dragged column would both undo itself on the next resize and be able
//! to cause exactly the overflow the derivation exists to prevent.
//! And **`render_last_empty_col` is widened to the vertical scrollbar's own
//! width**, because that scrollbar is absolutely positioned over the table's
//! right edge — the gutter is what keeps it off the last action button.
//!
//! # Every action is a button
//!
//! The actions column used to be 64 px holding a Copy button and an ellipsis
//! that opened a dropdown. It now draws one right-aligned button per
//! capability the item actually carries ([`RowAction`]) and widens to fit
//! them, so nothing an item can do is one click further away than anything
//! else it can do, and a row that cannot be uninstalled shows no uninstall
//! button at all. The buttons are icon-only and every one carries a tooltip.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClipboardItem, Context, Div, Image, ImageFormat, ImageSource,
    InteractiveElement as _, IntoElement, ParentElement as _, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, StyledImage as _, WeakEntity, Window, div, img,
    px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::table::{Column, TableDelegate, TableState};
use gpui_component::{ActiveTheme as _, Icon, Sizable as _, h_flex};

use super::CleanerView;
use super::results_layout::{
    ResultsColumn, ResultsLayout, RowAction, TRAILING_GUTTER, action_slots,
};
use crate::app_icon::AppIcon;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::icon::IconRaster;
use crate::cleaner::core::item::{CleanableItem, CleanableItemId, ItemMetadata};
use crate::cleaner::core::risk::{ItemCapability, RiskLevel};
use crate::i18n::{Str, t};

/// The risk dot's diameter when the badge has been reduced to its colour.
/// `results_layout::RISK_COMPACT_WIDTH` is derived from this.
const RISK_DOT: gpui::Pixels = px(10.);

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

/// The stable element-id prefix for one action's button. Distinct per action
/// so two buttons on the same row never collide.
fn action_id(action: RowAction) -> &'static str {
    match action {
        RowAction::Reveal => "cleaner-row-reveal",
        RowAction::CopyPath => "cleaner-row-copy",
        RowAction::Keep => "cleaner-row-keep",
        RowAction::Uninstall => "cleaner-row-uninstall",
    }
}

/// The tooltip and glyph one action button wears.
fn action_look(action: RowAction) -> (Str, AppIcon) {
    match action {
        RowAction::Reveal => (reveal_label(), AppIcon::FolderOpen),
        RowAction::CopyPath => (Str::CleanerCopyPath, AppIcon::Copy),
        RowAction::Keep => (Str::CleanerKeepItem, AppIcon::CircleCheck),
        RowAction::Uninstall => (Str::CleanerBeginUninstallReview, AppIcon::Trash),
    }
}

/// The five risk levels' label and colour, shared by the full badge and the
/// compact dot so the two can never disagree about what a colour means.
fn risk_look(risk: RiskLevel, cx: &App) -> (Str, gpui::Hsla) {
    match risk {
        RiskLevel::SafeRecreatable => (Str::CleanerRiskSafe, cx.theme().success),
        RiskLevel::ReviewRecommended => (Str::CleanerRiskReview, cx.theme().warning),
        RiskLevel::UserData => (Str::CleanerRiskUserData, cx.theme().warning),
        RiskLevel::ApplicationMutation => (Str::CleanerRiskAppChange, cx.theme().danger),
        RiskLevel::Protected => (Str::CleanerRiskProtected, cx.theme().danger),
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
    /// The grid's own width in logical pixels, as measured by
    /// `CleanerView::render_results_area`'s `canvas` and handed in through
    /// [`Self::set_grid_width`]. Never read during a render: it is only an
    /// input to [`Self::layout`].
    grid_width: f32,
    /// The widest action set any row currently held carries — what the
    /// actions column has to be wide enough for. Recomputed with the items,
    /// never per frame.
    action_slots: usize,
    /// The columns this grid draws, derived from the three fields above.
    /// [`Self::column`] and [`Self::render_td`] read nothing else about
    /// widths.
    layout: ResultsLayout,
}

impl ResultsTableDelegate {
    pub fn new(view: WeakEntity<CleanerView>) -> Self {
        Self {
            view,
            items: Vec::new(),
            selected_ids: HashSet::new(),
            category: CleanerCategory::SystemJunk,
            icons: HashMap::new(),
            grid_width: super::results_layout::FLOOR_GRID_WIDTH,
            action_slots: 1,
            layout: ResultsLayout::default(),
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
        // One pass over the new items rather than a per-frame `max`: the
        // actions column's width is a property of the whole result — sizing
        // it per visible row would change the column's width as the user
        // scrolled — and the result only changes here.
        self.action_slots = action_slots(items.iter().map(|item| item.capabilities.as_slice()));
        self.items = items;
        self.selected_ids = selected_ids;
        self.recompute_layout();
    }

    /// Replaces which rows are ticked, leaving the rows themselves (and
    /// their icon payloads) exactly where they are. Ticking one checkbox on
    /// a large result must not cost a re-copy of the whole result.
    pub fn set_selection(&mut self, selected_ids: HashSet<CleanableItemId>) {
        self.selected_ids = selected_ids;
    }

    /// Tells the grid how wide it now is, and answers whether that changed
    /// the columns — i.e. whether the caller owes the table a
    /// `TableState::refresh`. A resize that lands inside the same stage and
    /// moves no column boundary answers `false` and costs nothing.
    pub fn set_grid_width(&mut self, grid_width: f32) -> bool {
        if self.grid_width == grid_width {
            return false;
        }
        self.grid_width = grid_width;
        self.recompute_layout()
    }

    /// Returns whether the column list actually changed.
    fn recompute_layout(&mut self) -> bool {
        let layout =
            ResultsLayout::for_grid(self.grid_width, self.shows_selection(), self.action_slots);
        if layout == self.layout {
            return false;
        }
        self.layout = layout;
        true
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
        // The frame ("Explanation:", "Path:") is translated; `item.explanation`
        // is a scanner-produced sentence describing what was found and is not
        // — there is nothing to translate it with, the same posture
        // `dodo-i18n-text` documents for a parser's own error detail.
        //
        // The path line appears only when the grid is too narrow to give the
        // path a column of its own; see `results_layout`'s module doc for why
        // the path is the first thing to go.
        let explanation = SharedString::from(if self.layout.shows_path() {
            format!("{}: {}", t(Str::CleanerExplanation, cx), item.explanation)
        } else {
            format!(
                "{}: {}\n{}: {}",
                t(Str::CleanerPath, cx),
                item.path.display(),
                t(Str::CleanerExplanation, cx),
                item.explanation
            )
        });
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

    /// The labelled badge, or — once the grid is too narrow to spend 112 px
    /// on a word the colour already carries — the colour on its own, with the
    /// word in a tooltip.
    fn render_risk_cell(&self, item: &CleanableItem, compact: bool, cx: &App) -> AnyElement {
        let (label, color) = risk_look(item.risk, cx);
        if compact {
            let text = t(label, cx);
            return h_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .id(("cleaner-row-risk", item.id.0))
                        .w(RISK_DOT)
                        .h(RISK_DOT)
                        .flex_shrink_0()
                        .rounded_full()
                        .bg(color)
                        .tooltip(move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(text.clone()).build(window, cx)
                        }),
                )
                .into_any_element();
        }
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

    /// One button per capability the item carries, right-aligned. The order
    /// is [`RowAction::ORDER`] and never depends on the item, so the same
    /// action sits in the same place from row to row wherever both rows
    /// offer it.
    fn render_actions_cell(&self, item: &CleanableItem, cx: &App) -> AnyElement {
        let mut row = h_flex().size_full().items_center().justify_end().gap_1();
        for action in RowAction::for_capabilities(&item.capabilities) {
            row = row.child(self.render_action_button(action, item, cx));
        }
        row.into_any_element()
    }

    /// One button, with **only** the payload its own handler needs captured.
    ///
    /// Deliberately a `match` around four separate `on_click`s rather than
    /// one closure switching on `action`: the latter would have to capture
    /// everything every action might want, which means cloning the whole
    /// [`CleanableItem`] once per button per visible row per frame. Reveal
    /// and Copy path need a path; only Keep and Uninstall need the item, and
    /// only the rows that offer them pay for it.
    fn render_action_button(
        &self,
        action: RowAction,
        item: &CleanableItem,
        cx: &App,
    ) -> AnyElement {
        let (label, icon) = action_look(action);
        let id = action_id(action);
        let view = self.view.clone();
        let button = Button::new((id, item.id.0))
            .ghost()
            .xsmall()
            .icon(icon)
            .tooltip(t(label, cx));

        match action {
            RowAction::Reveal => {
                let path = item.path.clone();
                button
                    .on_click(move |_, _, cx| {
                        let path = path.clone();
                        let _ = view.update_in(cx, |view, window, cx| {
                            view.reveal_in_finder(path, window, cx)
                        });
                    })
                    .into_any_element()
            }
            RowAction::CopyPath => {
                let path_text = item.path.display().to_string();
                button
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(path_text.clone()));
                    })
                    .into_any_element()
            }
            RowAction::Keep => {
                let item = item.clone();
                button
                    .on_click(move |_, _, cx| {
                        let item = item.clone();
                        let _ = view.update(cx, |view, cx| view.mark_kept(item, cx));
                    })
                    .into_any_element()
            }
            RowAction::Uninstall => {
                let item = item.clone();
                button
                    .on_click(move |_, _, cx| {
                        let item = item.clone();
                        let _ = view.update_in(cx, |view, window, cx| {
                            view.begin_uninstall_review(item, window, cx)
                        });
                    })
                    .into_any_element()
            }
        }
    }

    /// The header label for one column. `None` draws nothing: the checkbox
    /// column has no name, and a compact risk dot has no room for one.
    fn header_label(column: ResultsColumn) -> Option<Str> {
        match column {
            ResultsColumn::Select => None,
            ResultsColumn::Name => Some(Str::CleanerColumnName),
            ResultsColumn::Risk { compact: true } => None,
            ResultsColumn::Risk { compact: false } => Some(Str::CleanerColumnRisk),
            ResultsColumn::Size => Some(Str::CleanerColumnSize),
            ResultsColumn::Path => Some(Str::CleanerPath),
            ResultsColumn::Actions => Some(Str::CleanerColumnActions),
        }
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

impl TableDelegate for ResultsTableDelegate {
    fn columns_count(&self, _cx: &App) -> usize {
        self.layout.len()
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
        let Some(sized) = self.layout.get(col_ix) else {
            return div().into_any_element();
        };
        if sized.column == ResultsColumn::Select {
            return self.render_header_select_cell(cx);
        }
        let Some(label) = Self::header_label(sized.column) else {
            return div().into_any_element();
        };
        // The size and actions cells are right-aligned, so their headers are
        // too — a left-aligned "Actions" over a right-aligned button group
        // reads as a different column.
        let right = matches!(sized.column, ResultsColumn::Size | ResultsColumn::Actions);
        h_flex()
            .size_full()
            .items_center()
            .when(right, |this| this.justify_end())
            .child(t(label, cx))
            .into_any_element()
    }

    fn column(&self, col_ix: usize, cx: &App) -> Column {
        let Some(sized) = self.layout.get(col_ix) else {
            return Column::new("", "");
        };
        let (key, name) = match sized.column {
            ResultsColumn::Select => ("select", SharedString::default()),
            ResultsColumn::Name => ("name", t(Str::CleanerColumnName, cx)),
            ResultsColumn::Risk { .. } => ("risk", t(Str::CleanerColumnRisk, cx)),
            ResultsColumn::Size => ("size", t(Str::CleanerColumnSize, cx)),
            ResultsColumn::Path => ("path", t(Str::CleanerPath, cx)),
            ResultsColumn::Actions => ("actions", t(Str::CleanerColumnActions, cx)),
        };
        // Pinned on all three sides: `results_layout` has already decided
        // this width from the pane, and a column the user could drag would
        // undo itself on the next resize while being free to push the
        // actions group off the right edge in the meantime.
        let width = px(sized.width);
        let column = Column::new(key, name)
            .width(width)
            .min_width(width)
            .max_width(width)
            .resizable(false)
            .movable(false);
        match sized.column {
            ResultsColumn::Name | ResultsColumn::Path => column,
            ResultsColumn::Size | ResultsColumn::Actions => column.text_right().selectable(false),
            _ => column.selectable(false),
        }
    }

    /// The strip after the last column. Widened from the library's 12 px to
    /// the vertical scrollbar's own 16 px, because that scrollbar is
    /// absolutely positioned at the table's right edge: without the gutter it
    /// would sit on top of the rightmost action button.
    fn render_last_empty_col(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        h_flex().w(px(TRAILING_GUTTER)).h_full().flex_shrink_0()
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
        // and a `CleanableItem` carries an application's whole icon payload —
        // cloning it six times a row, sixty times a second, is pure copying.
        let Some(item) = self.items.get(row_ix) else {
            return div().into_any_element();
        };
        let Some(sized) = self.layout.get(col_ix) else {
            return div().into_any_element();
        };
        match sized.column {
            ResultsColumn::Select => self.render_select_cell(item, cx),
            ResultsColumn::Name => self.render_name_cell(item, cx),
            ResultsColumn::Risk { compact } => self.render_risk_cell(item, compact, cx),
            ResultsColumn::Size => self.render_size_cell(item, cx),
            ResultsColumn::Path => self.render_path_cell(item, cx),
            ResultsColumn::Actions => self.render_actions_cell(item, cx),
        }
    }
}
