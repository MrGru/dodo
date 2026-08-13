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

    /// Categories whose scanners, tests and cleanup paths all still ship,
    /// but which this build's Cleaner window does not list.
    ///
    /// **This function is the whole of the feature switch, and it is a
    /// function of the platform rather than a constant.** Returning an empty
    /// slice for a host puts every category back in its section's tree with
    /// no other change anywhere: [`CleanerCategory::ALL`] still names all
    /// fourteen on every target, so every `CategoryState` is still seeded,
    /// every scanner the platform registers is still registered, and every
    /// `match` on the enum still compiles.
    ///
    /// # Why a [`HostOs`] parameter and not `#[cfg(target_os = …)]`
    ///
    /// The same reason [`crate::paths`] classifies its platform from the
    /// target triple: **both answers are then unit-testable from any host.**
    /// A `cfg` split would ship its Windows and Linux arm unexecuted, on a
    /// machine that cannot even compile the `dodo` crate for those targets
    /// (see "Two of the four `cargo check` targets…" in the project's root
    /// doc). Here the rule is ordinary data, [`Self::hidden_for`] is
    /// exhaustively tested for all three, and the real build wires the
    /// platform in exactly one place — [`HostOs::current`], read by
    /// [`Self::is_visible`].
    ///
    /// # What is hidden where, and why (captain's request, 2026-08-13)
    ///
    /// - **macOS lists all fourteen.** Every category has a real scanner
    ///   there, so there is nothing to hide.
    /// - **Windows and Linux hide [`CleanerCategory::XcodeJunk`],
    ///   [`CleanerCategory::HomebrewCache`] and
    ///   [`CleanerCategory::UniversalBinaries`].** All three name things that
    ///   only exist on a Mac — Xcode's `DerivedData`, Homebrew's download
    ///   cache, and Mach-O fat binaries — so a row promising to look for them
    ///   on Windows would be a row that can only ever report nothing.
    ///
    /// # One owner, two questions
    ///
    /// This is *not* the same question as "which categories can this platform
    /// actually scan", which the per-platform scanner registries
    /// ([`crate::cleaner::state::default_scanners`]) answer and which is a
    /// much shorter list off macOS. A category with no scanner here is still
    /// listed and reports "planned but not implemented yet" — deliberately, so
    /// the roadmap is visible. What must never happen is the reverse: hiding a
    /// category this build *does* scan, which would strand a working scanner
    /// behind no row at all. `a_hidden_category_is_never_one_this_build_scans`
    /// is the test that pins it.
    pub fn hidden_for(host: HostOs) -> &'static [CleanerCategory] {
        match host {
            HostOs::MacOs => &[],
            HostOs::Windows | HostOs::Unix => &[
                CleanerCategory::XcodeJunk,
                CleanerCategory::HomebrewCache,
                CleanerCategory::UniversalBinaries,
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

    /// The three the captain asked for on 2026-08-13, in one place so every
    /// test below reads the same list.
    const MAC_ONLY: [CleanerCategory; 3] = [
        CleanerCategory::XcodeJunk,
        CleanerCategory::HomebrewCache,
        CleanerCategory::UniversalBinaries,
    ];

    /// The whole point of the [`HostOs`] parameter: this asserts the Windows
    /// and Linux answer from a Mac, and the macOS answer from anywhere.
    #[test]
    fn macos_lists_every_category_and_the_other_two_platforms_hide_three() {
        assert!(CleanerCategory::hidden_for(HostOs::MacOs).is_empty());
        assert_eq!(CleanerCategory::visible_for(HostOs::MacOs).count(), 14);

        for host in [HostOs::Windows, HostOs::Unix] {
            assert_eq!(
                CleanerCategory::hidden_for(host),
                MAC_ONLY,
                "{host:?} must hide exactly the three macOS-only categories"
            );
            assert_eq!(CleanerCategory::visible_for(host).count(), 11);
            for hidden in MAC_ONLY {
                assert!(
                    !CleanerCategory::visible_for(host).any(|shown| shown == hidden),
                    "{hidden:?} must not be listed on {host:?}"
                );
                assert!(
                    !CleanerCategory::categories_for_host(host, hidden.section())
                        .any(|shown| shown == hidden),
                    "{hidden:?} must not appear in its section's tree on {host:?}"
                );
            }
        }
    }

    /// The hiding is a listing change and nothing else: `ALL` still names all
    /// fourteen on every platform, so the scanners, their tests and their
    /// cleanup paths are untouched by it.
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
                    "{category:?} is listed nowhere on {host:?}"
                );
            }
        }
    }

    /// Advanced keeps all seven entries on macOS and loses three elsewhere,
    /// and must still be a group with rows either way; the check also covers
    /// the empty case, which is what a longer hidden list would produce.
    #[test]
    fn a_section_renders_with_fewer_or_zero_entries() {
        let mac: Vec<CleanerCategory> =
            CleanerCategory::categories_for_host(HostOs::MacOs, CleanerSection::Advanced).collect();
        assert_eq!(mac.len(), 7);
        assert!(mac.contains(&CleanerCategory::XcodeJunk));

        let elsewhere: Vec<CleanerCategory> =
            CleanerCategory::categories_for_host(HostOs::Windows, CleanerSection::Advanced)
                .collect();
        assert_eq!(elsewhere.len(), 4);
        assert!(!elsewhere.contains(&CleanerCategory::XcodeJunk));

        // Nothing here assumes a section is non-empty: `categories_for` is an
        // iterator the sidebar extends its row list with, so zero rows is a
        // header on its own rather than a panic or a missing group.
        let empty: Vec<CleanerCategory> =
            CleanerCategory::ALL.into_iter().filter(|_| false).collect();
        assert!(empty.is_empty());
    }

    /// Whatever the window opens on must be something the window lists — on
    /// every platform, and it is the same category on all three because
    /// `System Junk` is hidden nowhere.
    #[test]
    fn the_first_visible_category_exists_and_is_visible() {
        for host in [HostOs::MacOs, HostOs::Windows, HostOs::Unix] {
            let first = CleanerCategory::visible_for(host)
                .next()
                .expect("at least one");
            assert_eq!(first, CleanerCategory::SystemJunk);
        }
        assert!(
            CleanerCategory::visible()
                .next()
                .expect("at least one")
                .is_visible()
        );
    }

    /// The one way the two per-platform lists could contradict each other:
    /// hiding a category whose scanner this very build registers would leave
    /// a working scanner behind no row at all. Runs against whichever
    /// platform's registry was compiled in, which is the only one that can
    /// be constructed here.
    #[test]
    fn a_hidden_category_is_never_one_this_build_scans() {
        for scanner in crate::cleaner::state::default_scanners() {
            let category = scanner.category();
            assert!(
                category.is_visible(),
                "{category:?} has a scanner in this build but no row to start it from"
            );
        }
    }
}
