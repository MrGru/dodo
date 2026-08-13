//! How wide each column of the results grid is at a given pane width, and
//! which actions one row offers — both as pure functions, so the whole rule
//! is a unit test rather than something found by dragging the window.
//!
//! # Why the grid has to size itself
//!
//! `gpui_component`'s `DataTable` gives every column a definite pixel width
//! (`TableState::render_cell` does `.w(col_group.width)`) and scrolls
//! horizontally when they do not fit. There is no flex mode: a column set
//! that is 80 px too wide for the pane does not squeeze, it pushes the
//! rightmost column — the actions — off the edge behind a scrollbar. So
//! "responsive" here means computing the widths ourselves from the width the
//! grid actually got, which [`super::CleanerView`] measures with a `canvas`
//! and hands in.
//!
//! # The numbers are read off the app, not chosen
//!
//! Every constant below is either something the pinned `gpui-component`
//! checkout fixes (cell padding, an icon button's square, the scrollbar's
//! width) or something dodo's own layout fixes ([`MAIN_MIN_WIDTH`], the
//! Cleaner's category sidebar). The one number that looks arbitrary,
//! [`NAME_FLOOR`], is the *result* of the others at dodo's narrowest allowed
//! pane — `the_name_floor_is_exactly_what_dodos_own_pane_floor_leaves` is the
//! test that keeps it that way.
//!
//! # What gives, and in what order
//!
//! Space is taken back in the order that costs the reader least, because
//! everything given up here is still reachable somewhere else on the row:
//!
//! 1. **The path column goes first.** It is the widest thing on the row and
//!    the most recoverable: every item that has a path also carries
//!    [`ItemCapability::CopyPath`], so the row's own Copy path button still
//!    hands it over, and [`ResultsLayout::shows_path`] is what tells the name
//!    cell to put the path in its tooltip once the column is gone.
//! 2. **The risk badge shrinks to a dot.** The colour *is* the signal; only
//!    the word goes, and it comes back as the dot's tooltip. 86 px for a word
//!    the colour already said.
//! 3. **The size column goes.** Not recoverable on the row, which is why it
//!    is last — but at this point the alternative is a name column with room
//!    for three characters, and a row you cannot identify is worth less than
//!    a row whose size you have to select to see (the toolbar above the grid
//!    reports the selected total).
//!
//! The checkbox, the name and the actions never go — the actions least of
//! all, since they are the only thing on the row that *does* anything.
//!
//! Nothing here is GPUI-aware: widths are plain `f32` logical pixels, the
//! same shape `crate::layout`'s own constants use, so `px(..)` goes on at the
//! few use sites.

use crate::cleaner::core::risk::ItemCapability;
use crate::layout::MAIN_MIN_WIDTH;

/// The horizontal padding `gpui_component` puts inside a table cell at
/// `Size::Medium`, per side (`Size::table_cell_padding` in the pinned
/// checkout). Every content width below is quoted net of both sides.
const CELL_PADDING_X: f32 = 8.;
/// Both sides of it, which is what a column's width has to carry on top of
/// whatever it draws.
const CELL_CHROME: f32 = CELL_PADDING_X * 2.;

/// An `xsmall` icon-only `Button` is `size_5` — 20 px square — and the
/// actions group separates them with `gap_1`.
const ACTION_BUTTON: f32 = 20.;
const ACTION_GAP: f32 = 4.;

/// The Cleaner's own category tree beside the results, and the `gap_4`
/// between them (`CleanerView::render`).
const CLEANER_SIDEBAR_WIDTH: f32 = 240.;
const CLEANER_SIDEBAR_GAP: f32 = 16.;
/// `DataTable::bordered(true)` draws `border_1` on each side, inside the box.
const TABLE_BORDER: f32 = 2.;
/// The strip after the last column. It is also the vertical scrollbar's
/// gutter — the scrollbar is absolutely positioned at the table's right edge
/// and `Scrollbar::width()` is `4 * 2 + 8` = 16 in the pinned checkout — so
/// making the two the same number is what keeps the rightmost action button
/// from sitting underneath it. [`super::results_table`] overrides
/// `render_last_empty_col` to exactly this.
pub const TRAILING_GUTTER: f32 = 16.;

/// The checkbox column: a 14 px `Checkbox` centred, with room to click
/// either side of it. Unchanged from the grid's first version.
const SELECT_WIDTH: f32 = 36.;
/// The risk badge at full size — a `px_2` pill around the longest of the five
/// translated risk labels.
const RISK_WIDTH: f32 = 112.;
/// The risk badge reduced to its colour: a 10 px dot plus cell padding.
const RISK_COMPACT_WIDTH: f32 = 10. + CELL_CHROME;
/// A right-aligned size, at most `"1023.9 MB"`-shaped.
const SIZE_WIDTH: f32 = 90.;
/// The actions column never goes below what the widest action set real
/// scanners produce needs — three buttons, so 3 × 20 + 2 × 4 + 16 = 84 —
/// even on a result whose rows carry fewer.
///
/// Two reasons, and the second is the one that would have been missed.
/// **The column's own header has to fit**: "Actions" is 7 characters and
/// Vietnamese's "Hành động" is 9, neither of which can be *measured* without
/// a window, but 68 px of text after the cell's padding clears both at
/// `text_sm` — where the 64 px this column used to be would not have. And
/// **a column that changed width with the action set would make the grid
/// jump** every time the user switched category, since Installed Apps'
/// removable bundles carry three buttons and its system ones carry two.
const ACTIONS_MIN_WIDTH: f32 = 84.;

/// The width below which the name column stops being worth widening and the
/// path column is not worth drawing at all — 140 px is 100 px of text after
/// the icon, the gap and the padding.
const NAME_MIN: f32 = 140.;
/// Ditto for the path: below 160 px a path shows a fragment of a directory
/// name and nothing that identifies it, so the column is dropped rather than
/// squeezed further.
const PATH_MIN: f32 = 160.;

/// The width the grid has at dodo's narrowest allowed main pane.
///
/// `layout::MAIN_MIN_WIDTH` is the floor `tool_box` holds the tool at — below
/// it the pane scrolls rather than squeezing further — and the Cleaner spends
/// a fixed 240 + 16 of that on its category tree before the grid sees any of
/// it. This is what is left after the table's own border and trailing gutter:
/// 520 - 240 - 16 - 2 - 16 = 246.
pub const FLOOR_GRID_WIDTH: f32 =
    MAIN_MIN_WIDTH - CLEANER_SIDEBAR_WIDTH - CLEANER_SIDEBAR_GAP - TABLE_BORDER - TRAILING_GUTTER;

/// The narrowest the name column is ever asked to be.
///
/// **Derived, not chosen**: it is exactly what [`FLOOR_GRID_WIDTH`] has left
/// after the checkbox, a compact risk dot and the widest action set real
/// scanners produce (three buttons) — 246 - 36 - 26 - 84. So dodo's own
/// window floor is the last stage's floor too, and the grid reaches it
/// without ever needing a horizontal scrollbar.
const NAME_FLOOR: f32 = 100.;

/// One column the grid can draw.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResultsColumn {
    Select,
    Name,
    /// `compact` swaps the labelled badge for a coloured dot with the label
    /// in its tooltip.
    Risk {
        compact: bool,
    },
    Size,
    Path,
    Actions,
}

/// A column and the width this layout gives it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SizedColumn {
    pub column: ResultsColumn,
    pub width: f32,
}

/// The grid's columns, left to right, for one pane width.
#[derive(Clone, PartialEq, Debug)]
pub struct ResultsLayout {
    columns: Vec<SizedColumn>,
}

impl Default for ResultsLayout {
    /// What the grid draws before it has ever been measured — the floor, so
    /// the first frame of a wide window is briefly cramped rather than
    /// briefly overflowing.
    fn default() -> Self {
        Self::for_grid(FLOOR_GRID_WIDTH, true, 1)
    }
}

impl ResultsLayout {
    /// The columns for a grid `available` logical pixels wide.
    ///
    /// `shows_selection` is the category's own answer (a category whose rows
    /// can never be bulk-selected draws no checkbox column at all), and
    /// `action_slots` is the widest action set any row currently on screen
    /// carries — see [`RowAction::count_for`]. The stages are described in
    /// the module doc; each one is entered only when the one above it cannot
    /// hold its columns at their minimum.
    pub fn for_grid(available: f32, shows_selection: bool, action_slots: usize) -> Self {
        let select = shows_selection.then_some(SELECT_WIDTH);
        let actions = actions_width(action_slots);
        let fixed = select.unwrap_or(0.) + actions;

        let mut columns = Vec::with_capacity(6);
        let mut push = |column, width| columns.push(SizedColumn { column, width });

        let full = fixed + RISK_WIDTH + SIZE_WIDTH;
        let compact = fixed + RISK_COMPACT_WIDTH + SIZE_WIDTH;

        if let Some(width) = select {
            push(ResultsColumn::Select, width);
        }

        if available - full >= NAME_MIN + PATH_MIN {
            // Everything fits: name and path keep their floors and split
            // whatever is left over equally, so the path stays the wider of
            // the two by exactly the 20 px their floors differ by.
            let surplus = (available - full - NAME_MIN - PATH_MIN) / 2.;
            push(ResultsColumn::Name, NAME_MIN + surplus);
            push(ResultsColumn::Risk { compact: false }, RISK_WIDTH);
            push(ResultsColumn::Size, SIZE_WIDTH);
            push(ResultsColumn::Path, PATH_MIN + surplus);
        } else if available - full >= NAME_MIN {
            push(ResultsColumn::Name, available - full);
            push(ResultsColumn::Risk { compact: false }, RISK_WIDTH);
            push(ResultsColumn::Size, SIZE_WIDTH);
        } else if available - compact >= NAME_MIN {
            push(ResultsColumn::Name, available - compact);
            push(ResultsColumn::Risk { compact: true }, RISK_COMPACT_WIDTH);
            push(ResultsColumn::Size, SIZE_WIDTH);
        } else {
            let rest = fixed + RISK_COMPACT_WIDTH;
            push(ResultsColumn::Name, (available - rest).max(NAME_FLOOR));
            push(ResultsColumn::Risk { compact: true }, RISK_COMPACT_WIDTH);
        }

        push(ResultsColumn::Actions, actions);
        Self { columns }
    }

    /// The width a grid gets inside a pane box `pane` pixels wide: the box
    /// minus the table's border and the trailing scrollbar gutter.
    pub fn grid_width_for_pane(pane: f32) -> f32 {
        (pane - TABLE_BORDER - TRAILING_GUTTER).max(0.)
    }

    pub fn len(&self) -> usize {
        self.columns.len()
    }

    pub fn get(&self, ix: usize) -> Option<SizedColumn> {
        self.columns.get(ix).copied()
    }

    /// Whether the path has a column of its own — which is also the question
    /// "does the name cell owe the reader a path in its tooltip?".
    pub fn shows_path(&self) -> bool {
        self.columns
            .iter()
            .any(|sized| sized.column == ResultsColumn::Path)
    }

    /// Every column's width plus the trailing gutter: what the grid's content
    /// actually measures, and therefore whether it overflows its pane.
    ///
    /// Nothing at runtime needs this — the widths are handed to
    /// `gpui_component` one column at a time — but the "no overflow at any
    /// width" property is the whole point of this module, and it cannot be
    /// asserted without summing them.
    #[cfg(test)]
    pub fn total_width(&self) -> f32 {
        self.columns.iter().map(|sized| sized.width).sum::<f32>() + TRAILING_GUTTER
    }
}

/// What the actions column costs for `slots` buttons: the buttons, the gaps
/// between them, the cell's own padding — and never less than
/// [`ACTIONS_MIN_WIDTH`].
fn actions_width(slots: usize) -> f32 {
    let buttons = match slots {
        0 => 0.,
        n => ACTION_BUTTON * n as f32 + ACTION_GAP * (n - 1) as f32,
    };
    (buttons + CELL_CHROME).max(ACTIONS_MIN_WIDTH)
}

/// One button the actions column can draw, and the capability that earns it.
///
/// The list is deliberately the *reachable* actions rather than every
/// [`ItemCapability`]: `MoveToTrash` is the row's checkbox, `EmptyTrash`
/// belongs to the Trash Bins pane's own button (that category never reaches
/// this grid), and `RunExternalCleanup`, `RemoveArchitecture` and
/// `RemoveLocalization` have no per-row control wired to them yet. Adding one
/// later is a variant here plus an arm in
/// [`super::results_table::ResultsTableDelegate`], and the column widens to
/// fit it on its own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowAction {
    Reveal,
    CopyPath,
    Keep,
    Uninstall,
}

impl RowAction {
    /// Left to right, destructive last.
    pub const ORDER: [RowAction; 4] = [
        RowAction::Reveal,
        RowAction::CopyPath,
        RowAction::Keep,
        RowAction::Uninstall,
    ];

    pub fn capability(self) -> ItemCapability {
        match self {
            RowAction::Reveal => ItemCapability::RevealInFinder,
            RowAction::CopyPath => ItemCapability::CopyPath,
            RowAction::Keep => ItemCapability::MarkAsKept,
            RowAction::Uninstall => ItemCapability::UninstallApplication,
        }
    }

    /// The buttons one row draws, in [`Self::ORDER`]. An item that cannot be
    /// uninstalled gets no uninstall button — the grid never draws a control
    /// that would do nothing.
    pub fn for_capabilities(capabilities: &[ItemCapability]) -> Vec<RowAction> {
        Self::ORDER
            .into_iter()
            .filter(|action| capabilities.contains(&action.capability()))
            .collect()
    }

    /// How many buttons that row draws, without building the list.
    pub fn count_for(capabilities: &[ItemCapability]) -> usize {
        Self::ORDER
            .into_iter()
            .filter(|action| capabilities.contains(&action.capability()))
            .count()
    }
}

/// How many action buttons the column has to be wide enough for, over a whole
/// result.
///
/// Never below one: an empty result still draws the column's header, and a
/// column narrower than the word "Actions" would clip it. Takes capability
/// slices rather than items so the rule is testable without building a
/// `CleanableItem`.
pub fn action_slots<'a>(rows: impl Iterator<Item = &'a [ItemCapability]>) -> usize {
    rows.map(RowAction::count_for).max().unwrap_or(0).max(1)
}

#[cfg(test)]
mod tests {
    use super::{
        ACTIONS_MIN_WIDTH, FLOOR_GRID_WIDTH, NAME_FLOOR, NAME_MIN, PATH_MIN, RISK_COMPACT_WIDTH,
        RISK_WIDTH, ResultsColumn, ResultsLayout, RowAction, SELECT_WIDTH, SIZE_WIDTH,
        TRAILING_GUTTER, action_slots, actions_width,
    };
    use crate::cleaner::core::risk::ItemCapability;

    /// The widest action set any scanner in the tree actually produces:
    /// Installed Apps' removable bundles (reveal + copy + uninstall) and
    /// Orphaned Files' candidates (reveal + copy + keep).
    const REAL_MAX_ACTIONS: usize = 3;

    fn kinds(layout: &ResultsLayout) -> Vec<ResultsColumn> {
        (0..layout.len())
            .map(|ix| layout.get(ix).expect("in range").column)
            .collect()
    }

    fn width_of(layout: &ResultsLayout, column: ResultsColumn) -> f32 {
        (0..layout.len())
            .filter_map(|ix| layout.get(ix))
            .find(|sized| sized.column == column)
            .unwrap_or_else(|| panic!("{column:?} is not in this layout"))
            .width
    }

    #[test]
    fn the_floor_grid_width_is_what_dodos_own_pane_floor_leaves() {
        // 520 (layout::MAIN_MIN_WIDTH) - 240 - 16 (the Cleaner's category
        // tree and its gap) - 2 (the table border) - 16 (the gutter).
        assert_eq!(FLOOR_GRID_WIDTH, 246.);
    }

    #[test]
    fn the_name_floor_is_exactly_what_dodos_own_pane_floor_leaves() {
        assert_eq!(
            NAME_FLOOR,
            FLOOR_GRID_WIDTH - SELECT_WIDTH - RISK_COMPACT_WIDTH - actions_width(REAL_MAX_ACTIONS)
        );
    }

    /// The acceptance criterion, stated as one property over the whole range
    /// of widths dodo can produce: from its own floor up to a very wide
    /// display, the columns plus the gutter never exceed the grid.
    #[test]
    fn nothing_overflows_at_any_width_the_app_allows() {
        for slots in 0..=REAL_MAX_ACTIONS {
            for shows_selection in [true, false] {
                let mut width = FLOOR_GRID_WIDTH;
                while width <= 4000. {
                    let layout = ResultsLayout::for_grid(width, shows_selection, slots);
                    assert!(
                        layout.total_width() <= width + TRAILING_GUTTER + 0.001,
                        "{width} px with {slots} actions overflowed: {:?}",
                        layout.total_width()
                    );
                    width += 1.;
                }
            }
        }
    }

    /// The three columns that may never be taken away, at every width.
    #[test]
    fn the_checkbox_the_name_and_the_actions_are_never_dropped() {
        for width in [0., 100., FLOOR_GRID_WIDTH, 400., 700., 2000.] {
            let layout = ResultsLayout::for_grid(width, true, REAL_MAX_ACTIONS);
            let columns = kinds(&layout);
            assert!(columns.contains(&ResultsColumn::Select), "{width}");
            assert!(columns.contains(&ResultsColumn::Name), "{width}");
            assert!(columns.contains(&ResultsColumn::Actions), "{width}");
            assert!(
                width_of(&layout, ResultsColumn::Name) >= NAME_FLOOR,
                "{width}"
            );
        }
    }

    /// A category that never bulk-selects draws no checkbox column, and the
    /// 36 px goes to the name rather than to empty space.
    #[test]
    fn a_category_without_checkboxes_gives_the_name_that_column() {
        let with = ResultsLayout::for_grid(500., true, 3);
        let without = ResultsLayout::for_grid(500., false, 3);
        assert!(!kinds(&without).contains(&ResultsColumn::Select));
        assert_eq!(
            width_of(&without, ResultsColumn::Name),
            width_of(&with, ResultsColumn::Name) + SELECT_WIDTH
        );
    }

    /// Stage 1: a wide grid draws all six, and the two flexible columns
    /// share the surplus so the path stays exactly the 20 px wider its floor
    /// makes it.
    #[test]
    fn a_wide_grid_draws_every_column_and_splits_the_surplus() {
        let layout = ResultsLayout::for_grid(1200., true, 3);
        assert_eq!(
            kinds(&layout),
            vec![
                ResultsColumn::Select,
                ResultsColumn::Name,
                ResultsColumn::Risk { compact: false },
                ResultsColumn::Size,
                ResultsColumn::Path,
                ResultsColumn::Actions,
            ]
        );
        assert_eq!(
            width_of(&layout, ResultsColumn::Path) - width_of(&layout, ResultsColumn::Name),
            PATH_MIN - NAME_MIN
        );
        assert_eq!(layout.total_width(), 1200. + TRAILING_GUTTER);
        assert!(layout.shows_path());
    }

    /// Stage 2: one pixel below the width both flexible columns need, the
    /// path goes and the name takes the space.
    #[test]
    fn the_path_is_the_first_column_to_go() {
        let fixed = SELECT_WIDTH + RISK_WIDTH + SIZE_WIDTH + actions_width(3);
        let boundary = fixed + NAME_MIN + PATH_MIN;

        let just_wide_enough = ResultsLayout::for_grid(boundary, true, 3);
        assert!(just_wide_enough.shows_path());
        assert_eq!(width_of(&just_wide_enough, ResultsColumn::Name), NAME_MIN);
        assert_eq!(width_of(&just_wide_enough, ResultsColumn::Path), PATH_MIN);

        let one_less = ResultsLayout::for_grid(boundary - 1., true, 3);
        assert!(!one_less.shows_path());
        assert!(kinds(&one_less).contains(&ResultsColumn::Size));
        assert_eq!(
            kinds(&one_less)
                .iter()
                .filter(|column| matches!(column, ResultsColumn::Risk { compact: false }))
                .count(),
            1,
            "the risk badge keeps its label while there is room for it"
        );
    }

    /// Stage 3: the badge becomes a dot before the size column is touched,
    /// and the 86 px it gives up go straight to the name.
    #[test]
    fn the_risk_badge_becomes_a_dot_before_the_size_goes() {
        let fixed = SELECT_WIDTH + SIZE_WIDTH + actions_width(3);
        let width = fixed + RISK_WIDTH + NAME_MIN - 1.;
        let layout = ResultsLayout::for_grid(width, true, 3);
        assert!(kinds(&layout).contains(&ResultsColumn::Risk { compact: true }));
        assert!(kinds(&layout).contains(&ResultsColumn::Size));
        assert_eq!(
            width_of(&layout, ResultsColumn::Name),
            width - fixed - RISK_COMPACT_WIDTH
        );
    }

    /// Stage 4, which is also what dodo's own window floor produces: the
    /// size goes, the name lands on its floor, and the columns add up to the
    /// grid exactly.
    #[test]
    fn the_app_floor_lands_on_the_last_stage_with_nothing_to_spare() {
        let layout = ResultsLayout::for_grid(FLOOR_GRID_WIDTH, true, REAL_MAX_ACTIONS);
        assert_eq!(
            kinds(&layout),
            vec![
                ResultsColumn::Select,
                ResultsColumn::Name,
                ResultsColumn::Risk { compact: true },
                ResultsColumn::Actions,
            ]
        );
        assert_eq!(width_of(&layout, ResultsColumn::Name), NAME_FLOOR);
        assert_eq!(layout.total_width(), FLOOR_GRID_WIDTH + TRAILING_GUTTER);
        assert!(!layout.shows_path());
    }

    /// Widening the grid never takes a column away, and within one stage —
    /// the same columns in the same forms — it only ever widens the name.
    /// Across a stage boundary it does not: see
    /// [`a_returning_column_takes_its_width_back_from_the_name`].
    #[test]
    fn widening_never_removes_a_column() {
        let mut previous = ResultsLayout::for_grid(FLOOR_GRID_WIDTH, true, 3);
        let mut width = FLOOR_GRID_WIDTH + 1.;
        while width <= 2000. {
            let layout = ResultsLayout::for_grid(width, true, 3);
            assert!(
                layout.len() >= previous.len(),
                "{width} px dropped a column the narrower grid had"
            );
            if kinds(&layout) == kinds(&previous) {
                assert!(
                    width_of(&layout, ResultsColumn::Name)
                        >= width_of(&previous, ResultsColumn::Name),
                    "{width} px narrowed the name column without changing the column set"
                );
            }
            previous = layout;
            width += 1.;
        }
    }

    /// A stage boundary reallocates rather than merely grows: at the width
    /// where the size column comes back, it comes back out of the name's
    /// share, so the name is briefly narrower in the wider window.
    ///
    /// That is the price of letting the name absorb everything a dropped
    /// column freed, and it is the deliberate half of the trade — the
    /// alternative is a band of dead space at the right of every narrow
    /// grid, which is worse at the width where space is scarcest.
    #[test]
    fn a_returning_column_takes_its_width_back_from_the_name() {
        let boundary = SELECT_WIDTH + RISK_COMPACT_WIDTH + SIZE_WIDTH + actions_width(3) + NAME_MIN;

        let narrower = ResultsLayout::for_grid(boundary - 1., true, 3);
        assert!(!kinds(&narrower).contains(&ResultsColumn::Size));

        let wider = ResultsLayout::for_grid(boundary, true, 3);
        assert!(kinds(&wider).contains(&ResultsColumn::Size));
        assert_eq!(width_of(&wider, ResultsColumn::Name), NAME_MIN);
        assert!(width_of(&narrower, ResultsColumn::Name) > width_of(&wider, ResultsColumn::Name));
    }

    /// The actions column is the one that must never be clipped, so it is
    /// sized from the buttons rather than fixed — but it never drops below
    /// [`ACTIONS_MIN_WIDTH`], which is itself what three buttons need.
    #[test]
    fn the_actions_column_fits_its_buttons_and_its_header() {
        // 3 * 20 + 2 * 4 + 16 — the floor and the widest real set are the
        // same number, which is why a result with fewer buttons per row
        // still lays the rest of the grid out identically.
        assert_eq!(ACTIONS_MIN_WIDTH, 3. * 20. + 2. * 4. + 16.);
        assert_eq!(actions_width(0), ACTIONS_MIN_WIDTH);
        assert_eq!(actions_width(1), ACTIONS_MIN_WIDTH);
        assert_eq!(actions_width(2), ACTIONS_MIN_WIDTH);
        assert_eq!(actions_width(3), ACTIONS_MIN_WIDTH);
        // 4 * 20 + 3 * 4 + 16
        assert_eq!(actions_width(4), 108.);
        for slots in 0..=4 {
            let layout = ResultsLayout::for_grid(900., true, slots);
            assert_eq!(
                width_of(&layout, ResultsColumn::Actions),
                actions_width(slots)
            );
        }
    }

    /// The pane box the view measures is not the column budget: the table's
    /// own border and the scrollbar gutter come off first.
    #[test]
    fn the_pane_box_loses_its_border_and_gutter_before_the_columns_see_it() {
        assert_eq!(ResultsLayout::grid_width_for_pane(264.), FLOOR_GRID_WIDTH);
        assert_eq!(ResultsLayout::grid_width_for_pane(0.), 0.);
    }

    #[test]
    fn a_row_draws_one_button_per_capability_it_carries_and_no_others() {
        let orphan = [
            ItemCapability::RevealInFinder,
            ItemCapability::CopyPath,
            ItemCapability::MarkAsKept,
            ItemCapability::MoveToTrash,
        ];
        assert_eq!(
            RowAction::for_capabilities(&orphan),
            vec![RowAction::Reveal, RowAction::CopyPath, RowAction::Keep]
        );

        let application = [
            ItemCapability::RevealInFinder,
            ItemCapability::CopyPath,
            ItemCapability::UninstallApplication,
        ];
        assert_eq!(
            RowAction::for_capabilities(&application),
            vec![RowAction::Reveal, RowAction::CopyPath, RowAction::Uninstall]
        );

        let system_application = [ItemCapability::RevealInFinder, ItemCapability::CopyPath];
        assert_eq!(
            RowAction::for_capabilities(&system_application),
            vec![RowAction::Reveal, RowAction::CopyPath],
            "a bundle that cannot be uninstalled must not offer the button"
        );

        // Docker's rows: the external-cleanup capability has no per-row
        // control, so only the copy button is drawn.
        let docker = [ItemCapability::RunExternalCleanup, ItemCapability::CopyPath];
        assert_eq!(
            RowAction::for_capabilities(&docker),
            vec![RowAction::CopyPath]
        );

        assert!(RowAction::for_capabilities(&[]).is_empty());
    }

    #[test]
    fn the_button_count_agrees_with_the_button_list() {
        let all: Vec<ItemCapability> = RowAction::ORDER
            .into_iter()
            .map(RowAction::capability)
            .collect();
        for take in 0..=all.len() {
            let capabilities = &all[..take];
            assert_eq!(
                RowAction::count_for(capabilities),
                RowAction::for_capabilities(capabilities).len()
            );
        }
    }

    /// The column is sized for the widest row in the result, not for the
    /// row being drawn — otherwise the column would change width as the user
    /// scrolled.
    #[test]
    fn the_actions_column_is_sized_for_the_widest_row_in_the_result() {
        let plain: &[ItemCapability] = &[ItemCapability::CopyPath];
        let application: &[ItemCapability] = &[
            ItemCapability::RevealInFinder,
            ItemCapability::CopyPath,
            ItemCapability::UninstallApplication,
        ];
        assert_eq!(action_slots([plain, application].into_iter()), 3);
        assert_eq!(action_slots([plain, plain].into_iter()), 1);

        // An empty result and a result whose rows offer nothing both still
        // owe the column its header.
        assert_eq!(action_slots(std::iter::empty()), 1);
        assert_eq!(action_slots([&[] as &[ItemCapability]].into_iter()), 1);
    }

    /// Every action maps to a distinct capability — a duplicate would draw
    /// two buttons doing the same thing.
    #[test]
    fn no_two_actions_share_a_capability() {
        for (ix, action) in RowAction::ORDER.iter().enumerate() {
            for other in &RowAction::ORDER[ix + 1..] {
                assert_ne!(action.capability(), other.capability());
            }
        }
    }
}
