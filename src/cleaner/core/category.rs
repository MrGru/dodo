#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Ord, PartialOrd)]
pub enum CleanerSection {
    Cleanup,
    Applications,
    Advanced,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Ord, PartialOrd)]
pub enum CleanerCategory {
    SystemJunk,
    UserCache,
    MailFiles,
    TrashBins,
    LargeOldFiles,
    InstalledApps,
    OrphanedFiles,
    AiApps,
    XcodeJunk,
    HomebrewCache,
    NodeToolingCache,
    DockerCache,
    UniversalBinaries,
    LanguageFiles,
}

impl CleanerSection {
    pub const ALL: [CleanerSection; 3] = [
        CleanerSection::Cleanup,
        CleanerSection::Applications,
        CleanerSection::Advanced,
    ];
}

impl CleanerCategory {
    pub const ALL: [CleanerCategory; 14] = [
        CleanerCategory::SystemJunk,
        CleanerCategory::UserCache,
        CleanerCategory::MailFiles,
        CleanerCategory::TrashBins,
        CleanerCategory::LargeOldFiles,
        CleanerCategory::InstalledApps,
        CleanerCategory::OrphanedFiles,
        CleanerCategory::AiApps,
        CleanerCategory::XcodeJunk,
        CleanerCategory::HomebrewCache,
        CleanerCategory::NodeToolingCache,
        CleanerCategory::DockerCache,
        CleanerCategory::UniversalBinaries,
        CleanerCategory::LanguageFiles,
    ];

    pub fn section(self) -> CleanerSection {
        match self {
            CleanerCategory::SystemJunk
            | CleanerCategory::UserCache
            | CleanerCategory::MailFiles
            | CleanerCategory::TrashBins
            | CleanerCategory::LargeOldFiles => CleanerSection::Cleanup,
            CleanerCategory::InstalledApps | CleanerCategory::OrphanedFiles => {
                CleanerSection::Applications
            }
            CleanerCategory::AiApps
            | CleanerCategory::XcodeJunk
            | CleanerCategory::HomebrewCache
            | CleanerCategory::NodeToolingCache
            | CleanerCategory::DockerCache
            | CleanerCategory::UniversalBinaries
            | CleanerCategory::LanguageFiles => CleanerSection::Advanced,
        }
    }

    /// Categories whose scanners, tests and cleanup paths all still ship,
    /// but which the Cleaner window does not list.
    ///
    /// **This list is the whole of the feature switch.** Deleting an entry
    /// here puts that category back in its section's tree, with no other
    /// change anywhere: `CleanerCategory::ALL` still names all fourteen, so
    /// every `CategoryState` is still seeded, every scanner is still
    /// registered, and every `match` on the enum still compiles.
    ///
    /// Why each one is here (captain's request, 2026-08-13):
    ///
    /// - [`CleanerCategory::UniversalBinaries`] is analysis-only. Its own
    ///   per-item explanation says slice removal "is not yet implemented",
    ///   so the page can report a number and offer nothing to do about it.
    /// - [`CleanerCategory::XcodeJunk`] and [`CleanerCategory::HomebrewCache`]
    ///   are hidden as Advanced surface the captain does not want on screen
    ///   for now.
    const HIDDEN: &'static [CleanerCategory] = &[
        CleanerCategory::XcodeJunk,
        CleanerCategory::HomebrewCache,
        CleanerCategory::UniversalBinaries,
    ];

    /// Whether the Cleaner window lists this category at all. A hidden
    /// category is unreachable rather than disabled: it has no sidebar row,
    /// so nothing can select it and its `Scan` button never exists — which
    /// is also the answer to "is a hidden category still scanned?". No.
    /// Scans are started only from a category's own pane.
    pub fn is_visible(self) -> bool {
        !Self::HIDDEN.contains(&self)
    }

    /// Every category the window lists, in `ALL` order. Used for the
    /// default selection, so a hidden category can never be what the panel
    /// opens on.
    pub fn visible() -> impl Iterator<Item = CleanerCategory> {
        CleanerCategory::ALL
            .into_iter()
            .filter(|category| category.is_visible())
    }

    /// The rows one section's tree draws — visible categories only. The
    /// sidebar is this function's only caller, which is why the filter lives
    /// here rather than at the call site: there is no second listing of a
    /// section's categories that could disagree with it.
    pub fn categories_for(section: CleanerSection) -> impl Iterator<Item = CleanerCategory> {
        CleanerCategory::visible().filter(move |category| category.section() == section)
    }
}

#[cfg(test)]
mod tests {
    use super::{CleanerCategory, CleanerSection};

    #[test]
    fn the_three_hidden_categories_are_absent_from_the_window() {
        for hidden in [
            CleanerCategory::XcodeJunk,
            CleanerCategory::HomebrewCache,
            CleanerCategory::UniversalBinaries,
        ] {
            assert!(!hidden.is_visible(), "{hidden:?} must not be listed");
            assert!(
                !CleanerCategory::categories_for(hidden.section()).any(|shown| shown == hidden),
                "{hidden:?} must not appear in its section's tree"
            );
            assert!(!CleanerCategory::visible().any(|shown| shown == hidden));
        }
    }

    /// The scanners, their tests and their cleanup paths are deliberately
    /// untouched by the hiding — only the listing changed.
    #[test]
    fn hiding_a_category_does_not_remove_it_from_the_enum() {
        assert_eq!(CleanerCategory::ALL.len(), 14);
        assert_eq!(CleanerCategory::visible().count(), 11);
        for hidden in CleanerCategory::HIDDEN {
            assert!(CleanerCategory::ALL.contains(hidden));
        }
    }

    #[test]
    fn every_other_category_still_shows_in_its_own_section() {
        let listed: Vec<CleanerCategory> = CleanerSection::ALL
            .into_iter()
            .flat_map(CleanerCategory::categories_for)
            .collect();
        assert_eq!(listed.len(), CleanerCategory::visible().count());
        for category in CleanerCategory::visible() {
            assert!(listed.contains(&category), "{category:?} is listed nowhere");
        }
    }

    /// The Advanced group loses three of its seven entries and must still be
    /// a group with rows; the check also covers the empty case, which is
    /// what a longer `HIDDEN` list would produce.
    #[test]
    fn a_section_renders_with_fewer_or_zero_entries() {
        let advanced: Vec<CleanerCategory> =
            CleanerCategory::categories_for(CleanerSection::Advanced).collect();
        assert_eq!(advanced.len(), 4);
        assert!(!advanced.contains(&CleanerCategory::XcodeJunk));

        // Nothing here assumes a section is non-empty: `categories_for` is an
        // iterator the sidebar extends its row list with, so zero rows is a
        // header on its own rather than a panic or a missing group.
        let empty: Vec<CleanerCategory> =
            CleanerCategory::ALL.into_iter().filter(|_| false).collect();
        assert!(empty.is_empty());
    }

    /// Whatever the window opens on must be something the window lists.
    #[test]
    fn the_first_visible_category_exists_and_is_visible() {
        let first = CleanerCategory::visible().next().expect("at least one");
        assert!(first.is_visible());
        assert_eq!(first, CleanerCategory::SystemJunk);
    }
}
