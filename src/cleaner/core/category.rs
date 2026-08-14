use crate::paths::HostOs;

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

    /// Categories the Cleaner window must not list on a host because that
    /// host has no working scanner for them.
    ///
    /// **This function is the whole feature switch, and it takes a [`HostOs`]
    /// rather than using `cfg`.** All three answers are therefore asserted
    /// from every build. [`CleanerCategory::ALL`] still names every category;
    /// hiding changes only whether a row exists to start its scan.
    ///
    /// macOS implements all fourteen categories. Windows and Linux currently
    /// implement only System Junk, User Cache, Trash Bins and Large & Old
    /// Files, so those are the only rows they list. Later rounds must unhide a
    /// row in the same change that registers its scanner.
    ///
    /// Two absences are policy rather than roadmap state. Language Files stays
    /// macOS-only because Windows and Linux have no safe, well-defined unit of
    /// removable localization data. Orphaned Files stays unavailable on
    /// Windows because generic AppData leftovers do not prove ownership;
    /// Linux may add only a conservative, package-manager-aware equivalent.
    ///
    /// Scanner registries remain the owner of what can actually scan. The two
    /// registry/visibility invariant tests below forbid disagreement in either
    /// direction on every target build.
    pub fn hidden_for(host: HostOs) -> &'static [CleanerCategory] {
        match host {
            HostOs::MacOs => &[],
            HostOs::Windows | HostOs::Unix => &[
                CleanerCategory::MailFiles,
                CleanerCategory::InstalledApps,
                CleanerCategory::OrphanedFiles,
                CleanerCategory::AiApps,
                CleanerCategory::XcodeJunk,
                CleanerCategory::HomebrewCache,
                CleanerCategory::NodeToolingCache,
                CleanerCategory::DockerCache,
                CleanerCategory::UniversalBinaries,
                CleanerCategory::LanguageFiles,
            ],
        }
    }

    /// Whether the Cleaner window lists this category at all *in this build*.
    /// A hidden category is unreachable rather than disabled: it has no
    /// sidebar row, so nothing can select it and its `Scan` button never
    /// exists — which is also the answer to "is a hidden category still
    /// scanned?". No. Scans are started only from a category's own pane.
    ///
    /// [`HostOs::current`] is the one place the compiled-for platform enters
    /// the decision; everything above it is pure.
    pub fn is_visible(self) -> bool {
        !Self::hidden_for(HostOs::current()).contains(&self)
    }

    /// Every category the window lists, in `ALL` order. Used for the
    /// default selection, so a hidden category can never be what the panel
    /// opens on.
    pub fn visible() -> impl Iterator<Item = CleanerCategory> {
        CleanerCategory::visible_for(HostOs::current())
    }

    /// [`Self::visible`] for an arbitrary platform — the shape every test
    /// below uses, so both the macOS and the Windows/Linux answer are
    /// asserted on whichever machine runs `cargo test`.
    pub fn visible_for(host: HostOs) -> impl Iterator<Item = CleanerCategory> {
        let hidden = Self::hidden_for(host);
        CleanerCategory::ALL
            .into_iter()
            .filter(move |category| !hidden.contains(category))
    }

    /// The rows one section's tree draws — visible categories only. The
    /// sidebar is this function's only caller, which is why the filter lives
    /// here rather than at the call site: there is no second listing of a
    /// section's categories that could disagree with it.
    pub fn categories_for(section: CleanerSection) -> impl Iterator<Item = CleanerCategory> {
        CleanerCategory::categories_for_host(HostOs::current(), section)
    }

    /// [`Self::categories_for`] for an arbitrary platform.
    pub fn categories_for_host(
        host: HostOs,
        section: CleanerSection,
    ) -> impl Iterator<Item = CleanerCategory> {
        CleanerCategory::visible_for(host).filter(move |category| category.section() == section)
    }
}

#[cfg(test)]
mod tests {
    use super::{CleanerCategory, CleanerSection};
    use crate::paths::HostOs;

    const HIDDEN_OFF_MACOS: [CleanerCategory; 10] = [
        CleanerCategory::MailFiles,
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

    #[test]
    fn hidden_categories_are_exhaustive_for_every_host() {
        assert!(CleanerCategory::hidden_for(HostOs::MacOs).is_empty());
        assert_eq!(CleanerCategory::visible_for(HostOs::MacOs).count(), 14);

        for host in [HostOs::Windows, HostOs::Unix] {
            assert_eq!(CleanerCategory::hidden_for(host), HIDDEN_OFF_MACOS);
            assert_eq!(CleanerCategory::visible_for(host).count(), 4);
            for hidden in HIDDEN_OFF_MACOS {
                assert!(!CleanerCategory::visible_for(host).any(|shown| shown == hidden));
                assert!(
                    !CleanerCategory::categories_for_host(host, hidden.section())
                        .any(|shown| shown == hidden)
                );
            }
        }
    }

    #[test]
    fn hiding_a_category_does_not_remove_it_from_the_enum() {
        assert_eq!(CleanerCategory::ALL.len(), 14);
        for host in [HostOs::MacOs, HostOs::Windows, HostOs::Unix] {
            for hidden in CleanerCategory::hidden_for(host) {
                assert!(CleanerCategory::ALL.contains(hidden));
            }
        }
    }

    #[test]
    fn every_visible_category_shows_in_its_own_section() {
        for host in [HostOs::MacOs, HostOs::Windows, HostOs::Unix] {
            let listed: Vec<CleanerCategory> = CleanerSection::ALL
                .into_iter()
                .flat_map(|section| CleanerCategory::categories_for_host(host, section))
                .collect();
            assert_eq!(listed.len(), CleanerCategory::visible_for(host).count());
            for category in CleanerCategory::visible_for(host) {
                assert!(
                    listed.contains(&category),
                    "{category:?} missing on {host:?}"
                );
            }
        }
    }

    #[test]
    fn a_section_may_have_no_visible_rows() {
        assert_eq!(
            CleanerCategory::categories_for_host(HostOs::MacOs, CleanerSection::Advanced).count(),
            7
        );
        assert_eq!(
            CleanerCategory::categories_for_host(HostOs::Windows, CleanerSection::Advanced).count(),
            0
        );
        assert_eq!(
            CleanerCategory::categories_for_host(HostOs::Unix, CleanerSection::Applications)
                .count(),
            0
        );
    }

    #[test]
    fn the_first_visible_category_exists_and_is_visible() {
        for host in [HostOs::MacOs, HostOs::Windows, HostOs::Unix] {
            assert_eq!(
                CleanerCategory::visible_for(host).next(),
                Some(CleanerCategory::SystemJunk)
            );
        }
        assert!(
            CleanerCategory::visible()
                .next()
                .expect("at least one")
                .is_visible()
        );
    }

    #[test]
    fn a_hidden_category_is_never_one_this_build_scans() {
        for scanner in crate::cleaner::state::default_scanners() {
            let category = scanner.category();
            assert!(
                category.is_visible(),
                "{category:?} has a scanner in this build but no row"
            );
        }
    }

    #[test]
    fn a_listed_category_is_one_this_build_scans() {
        let scanned: Vec<CleanerCategory> = crate::cleaner::state::default_scanners()
            .into_iter()
            .map(|scanner| scanner.category())
            .collect();
        for category in CleanerCategory::visible() {
            assert!(
                scanned.contains(&category),
                "{category:?} has a row in this build but no scanner"
            );
        }
    }
}
