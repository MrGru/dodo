use std::fs;
use std::path::{Path, PathBuf};

use crate::core::category::CleanerCategory;
use crate::core::errors::SafetyError;
use crate::paths::HostOs;

/// Deny-by-default deletion policy. An empty policy authorizes nothing, and
/// an allowed root never authorizes deleting the root itself.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct DeletionPolicy {
    pub allowed_roots: Vec<AllowedRoot>,
    pub protected_paths: Vec<PathBuf>,
    pub user_home: Option<PathBuf>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AllowedRoot {
    pub path: PathBuf,
    pub allowed_categories: Vec<CleanerCategory>,
}

/// Whether `path` is lexically absolute for `host`, including Windows drive,
/// UNC and extended-length paths even when tested from a Unix host.
pub fn is_absolute_path(host: HostOs, path: &Path) -> bool {
    normalize_path(host, path).is_some()
}

/// Host-aware lexical containment. Windows paths compare case-insensitively
/// and understand drive, UNC and extended-length prefixes; Unix and macOS
/// remain case-sensitive.
pub fn contains_path(host: HostOs, parent: &Path, child: &Path) -> bool {
    let (Some(parent), Some(child)) = (normalize_path(host, parent), normalize_path(host, child))
    else {
        return false;
    };
    parent.contains(&child)
}

/// Host-aware direct-child check, used when a scanner promises a bounded
/// one-level search rather than arbitrary descendant discovery.
pub fn is_direct_child(host: HostOs, parent: &Path, child: &Path) -> bool {
    let (Some(parent), Some(child)) = (normalize_path(host, parent), normalize_path(host, child))
    else {
        return false;
    };
    parent.contains(&child) && child.components.len() == parent.components.len() + 1
}

pub fn dedupe_nested_paths(host: HostOs, mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort_by_key(|path| {
        normalize_path(host, path)
            .map(|path| path.components.len())
            .unwrap_or(usize::MAX)
    });

    let mut deduped = Vec::with_capacity(paths.len());
    for path in paths {
        if deduped
            .iter()
            .any(|existing: &PathBuf| contains_path(host, existing, &path))
        {
            continue;
        }
        deduped.push(path);
    }
    deduped
}

/// Validates a deletion target immediately before it is moved to Trash.
///
/// Lexical checks reject traversal and root deletion. Canonical paths are then
/// compared for both the declared root and target, so a symlink, Windows
/// junction or other reparse redirect cannot escape the scanner-declared root.
/// Any path or filesystem fact that cannot be normalized or resolved is
/// refused.
pub fn validate_path(
    host: HostOs,
    path: &Path,
    category: CleanerCategory,
    policy: &DeletionPolicy,
) -> Result<(), SafetyError> {
    let candidate_roots = validate_lexical_path(host, path, category, policy)?;

    let metadata =
        fs::symlink_metadata(path).map_err(|_| SafetyError::EntryChanged(path.to_path_buf()))?;
    if metadata.file_type().is_symlink() {
        return Err(SafetyError::SymlinkRejected(path.to_path_buf()));
    }

    let resolved_path =
        fs::canonicalize(path).map_err(|_| SafetyError::EntryChanged(path.to_path_buf()))?;
    let resolved_roots: Vec<(usize, PathBuf)> = candidate_roots
        .into_iter()
        .filter_map(|index| {
            fs::canonicalize(&policy.allowed_roots[index].path)
                .ok()
                .map(|path| (index, path))
        })
        .collect();
    if resolved_roots.is_empty() {
        return Err(SafetyError::OutsideAllowedRoot(path.to_path_buf()));
    }

    let resolved_protected_paths: Vec<PathBuf> = policy
        .protected_paths
        .iter()
        .filter_map(|path| fs::canonicalize(path).ok())
        .collect();
    let resolved_home = policy
        .user_home
        .as_ref()
        .map(|home| {
            fs::canonicalize(home).map_err(|_| SafetyError::EntryChanged(home.to_path_buf()))
        })
        .transpose()?;

    validate_resolved_path(
        host,
        path,
        &resolved_path,
        policy,
        &resolved_roots,
        &resolved_protected_paths,
        resolved_home.as_deref(),
    )
}

fn validate_lexical_path(
    host: HostOs,
    path: &Path,
    category: CleanerCategory,
    policy: &DeletionPolicy,
) -> Result<Vec<usize>, SafetyError> {
    let Some(path_normalized) = normalize_path(host, path) else {
        return Err(SafetyError::OutsideAllowedRoot(path.to_path_buf()));
    };

    if path_normalized.is_root()
        || policy.allowed_roots.iter().any(|root| {
            normalize_path(host, &root.path).is_some_and(|root| root == path_normalized)
        })
    {
        return Err(SafetyError::RootDeletionRejected(path.to_path_buf()));
    }

    if is_protected(host, &path_normalized, &policy.protected_paths)
        || policy.user_home.as_ref().is_some_and(|home| {
            normalize_path(host, home)
                .is_some_and(|home| path_normalized == home || path_normalized.contains(&home))
        })
    {
        return Err(SafetyError::ProtectedPath(path.to_path_buf()));
    }

    let candidates: Vec<usize> = policy
        .allowed_roots
        .iter()
        .enumerate()
        .filter(|(_, root)| root.allowed_categories.contains(&category))
        .filter_map(|(index, root)| {
            normalize_path(host, &root.path)
                .filter(|root| root.contains(&path_normalized))
                .map(|_| index)
        })
        .collect();
    if candidates.is_empty() {
        return Err(SafetyError::OutsideAllowedRoot(path.to_path_buf()));
    }

    Ok(candidates)
}

fn validate_resolved_path(
    host: HostOs,
    path: &Path,
    resolved_path: &Path,
    policy: &DeletionPolicy,
    resolved_roots: &[(usize, PathBuf)],
    resolved_protected_paths: &[PathBuf],
    resolved_home: Option<&Path>,
) -> Result<(), SafetyError> {
    let Some(path_normalized) = normalize_path(host, path) else {
        return Err(SafetyError::OutsideAllowedRoot(path.to_path_buf()));
    };
    let Some(resolved_normalized) = normalize_path(host, resolved_path) else {
        return Err(SafetyError::OutsideAllowedRoot(path.to_path_buf()));
    };

    if resolved_normalized.is_root() {
        return Err(SafetyError::RootDeletionRejected(path.to_path_buf()));
    }
    if is_protected(host, &resolved_normalized, resolved_protected_paths)
        || resolved_home.is_some_and(|home| {
            normalize_path(host, home).is_some_and(|home| {
                resolved_normalized == home || resolved_normalized.contains(&home)
            })
        })
    {
        return Err(SafetyError::ProtectedPath(path.to_path_buf()));
    }

    for (index, resolved_root) in resolved_roots {
        let Some(root) = normalize_path(host, &policy.allowed_roots[*index].path) else {
            continue;
        };
        let Some(suffix) = path_normalized.relative_to(&root) else {
            continue;
        };
        let Some(resolved_root) = normalize_path(host, resolved_root) else {
            continue;
        };

        if resolved_normalized == resolved_root {
            return Err(SafetyError::RootDeletionRejected(path.to_path_buf()));
        }

        // The same suffix on both sides rules out redirects in every
        // component below the declared root, including junctions that happen
        // to land back inside it.
        if resolved_normalized.relative_to(&resolved_root) == Some(suffix) {
            return Ok(());
        }
    }

    Err(SafetyError::OutsideAllowedRoot(path.to_path_buf()))
}

fn is_protected(host: HostOs, path: &NormalizedPath, protected_paths: &[PathBuf]) -> bool {
    protected_paths.iter().any(|protected| {
        normalize_path(host, protected)
            .is_some_and(|protected| *path == protected || path.contains(&protected))
    })
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum PathAnchor {
    UnixRoot,
    WindowsDrive(char),
    WindowsUnc { server: String, share: String },
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct NormalizedPath {
    anchor: PathAnchor,
    components: Vec<String>,
}

impl NormalizedPath {
    fn contains(&self, child: &NormalizedPath) -> bool {
        self.anchor == child.anchor
            && self.components.len() <= child.components.len()
            && child.components.starts_with(&self.components)
    }

    fn is_root(&self) -> bool {
        self.components.is_empty()
    }

    fn relative_to<'a>(&'a self, parent: &NormalizedPath) -> Option<&'a [String]> {
        parent
            .contains(self)
            .then(|| &self.components[parent.components.len()..])
    }
}

fn normalize_path(host: HostOs, path: &Path) -> Option<NormalizedPath> {
    let path = path.to_str()?;
    match host {
        HostOs::Windows => normalize_windows_path(path),
        HostOs::MacOs | HostOs::Unix => normalize_unix_path(path),
    }
}

fn normalize_unix_path(path: &str) -> Option<NormalizedPath> {
    if !path.starts_with('/') {
        return None;
    }
    Some(NormalizedPath {
        anchor: PathAnchor::UnixRoot,
        components: normalize_components(path.split('/'), false)?,
    })
}

fn normalize_windows_path(path: &str) -> Option<NormalizedPath> {
    let mut path = path.replace('/', "\\");
    if let Some(extended) = path.strip_prefix("\\\\?\\") {
        path = if extended
            .get(..4)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("UNC\\"))
        {
            format!("\\\\{}", &extended[4..])
        } else {
            extended.to_owned()
        };
    }

    if let Some(unc) = path.strip_prefix("\\\\") {
        let mut parts = unc.split('\\').filter(|part| !part.is_empty());
        let server = parts.next()?;
        let share = parts.next()?;
        if matches!(server, "." | "..") || matches!(share, "." | "..") {
            return None;
        }
        return Some(NormalizedPath {
            anchor: PathAnchor::WindowsUnc {
                server: server.to_lowercase(),
                share: share.to_lowercase(),
            },
            components: normalize_components(parts, true)?,
        });
    }

    let bytes = path.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' || bytes[2] != b'\\' {
        return None;
    }
    Some(NormalizedPath {
        anchor: PathAnchor::WindowsDrive((bytes[0] as char).to_ascii_lowercase()),
        components: normalize_components(path[3..].split('\\'), true)?,
    })
}

fn normalize_components<'a>(
    components: impl IntoIterator<Item = &'a str>,
    lowercase: bool,
) -> Option<Vec<String>> {
    let mut normalized = Vec::new();
    for component in components {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            normalized.pop()?;
            continue;
        }
        normalized.push(if lowercase {
            component.to_lowercase()
        } else {
            component.to_owned()
        });
    }
    Some(normalized)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{
        AllowedRoot, DeletionPolicy, contains_path, dedupe_nested_paths, is_direct_child,
        normalize_path, validate_lexical_path, validate_path, validate_resolved_path,
    };
    use crate::core::category::CleanerCategory;
    use crate::core::errors::SafetyError;
    use crate::paths::HostOs;

    fn policy(root: impl Into<PathBuf>) -> DeletionPolicy {
        DeletionPolicy {
            allowed_roots: vec![AllowedRoot {
                path: root.into(),
                allowed_categories: vec![CleanerCategory::UserCache],
            }],
            protected_paths: Vec::new(),
            user_home: None,
        }
    }

    #[test]
    fn normalization_is_host_aware() {
        assert_eq!(
            normalize_path(
                HostOs::Windows,
                Path::new(r"\\?\C:\Users\Alice\Cache\.\npm")
            ),
            normalize_path(HostOs::Windows, Path::new(r"c:/users/ALICE/cache/npm"))
        );
        assert_eq!(
            normalize_path(
                HostOs::Windows,
                Path::new(r"\\?\UNC\Server\Share\Cache\npm")
            ),
            normalize_path(HostOs::Windows, Path::new(r"\\server\share\cache/npm"))
        );
        assert_ne!(
            normalize_path(HostOs::Unix, Path::new("/home/Alice/cache")),
            normalize_path(HostOs::Unix, Path::new("/home/alice/cache"))
        );
        assert_eq!(
            normalize_path(
                HostOs::MacOs,
                Path::new("/Users/me/Library/./Caches/a/../b")
            ),
            normalize_path(HostOs::MacOs, Path::new("/Users/me/Library/Caches/b"))
        );
    }

    #[test]
    fn containment_uses_host_rules_and_path_components() {
        assert!(contains_path(
            HostOs::MacOs,
            Path::new("/tmp/foo"),
            Path::new("/tmp/foo/bar")
        ));
        assert!(!contains_path(
            HostOs::MacOs,
            Path::new("/tmp/foo"),
            Path::new("/tmp/foobar")
        ));
        assert!(contains_path(
            HostOs::Windows,
            Path::new(r"C:\Users\Me\Cache"),
            Path::new(r"\\?\c:\users\me\cache\npm")
        ));
        assert!(!contains_path(
            HostOs::Unix,
            Path::new("/home/me/Cache"),
            Path::new("/home/me/cache/npm")
        ));
    }

    #[test]
    fn direct_children_use_host_path_rules_without_accepting_descendants() {
        let parent = Path::new(r"C:\Users\Ada\AppData\Local\Programs");
        assert!(is_direct_child(
            HostOs::Windows,
            parent,
            Path::new(r"c:\users\ada\appdata\local\programs\Dodo")
        ));
        assert!(!is_direct_child(
            HostOs::Windows,
            parent,
            Path::new(r"C:\Users\Ada\AppData\Local\Programs\Tools\Dodo")
        ));
        assert!(!is_direct_child(HostOs::Windows, parent, parent));
    }

    #[test]
    fn nested_children_are_removed_when_parent_is_selected() {
        let deduped = dedupe_nested_paths(
            HostOs::Windows,
            vec![
                PathBuf::from(r"C:\Users\Me\CACHE\nested"),
                PathBuf::from(r"c:\users\me\cache"),
                PathBuf::from(r"C:\other"),
            ],
        );
        assert_eq!(deduped.len(), 2);
        assert!(deduped.contains(&PathBuf::from(r"c:\users\me\cache")));
        assert!(deduped.contains(&PathBuf::from(r"C:\other")));
    }

    #[test]
    fn traversal_and_a_junction_shaped_escape_are_rejected() {
        let policy = policy(r"C:\Users\me\AppData\Local\cache");
        let traversed = Path::new(r"C:\Users\me\AppData\Local\cache\..\secrets");
        assert!(matches!(
            validate_lexical_path(
                HostOs::Windows,
                traversed,
                CleanerCategory::UserCache,
                &policy
            ),
            Err(SafetyError::OutsideAllowedRoot(_))
        ));

        let target = Path::new(r"C:\Users\me\AppData\Local\cache\junction\victim");
        let candidate_roots =
            validate_lexical_path(HostOs::Windows, target, CleanerCategory::UserCache, &policy)
                .expect("lexically inside the root");
        let resolved_roots = candidate_roots
            .into_iter()
            .map(|index| (index, PathBuf::from(r"\\?\C:\Users\me\AppData\Local\cache")))
            .collect::<Vec<_>>();
        assert!(matches!(
            validate_resolved_path(
                HostOs::Windows,
                target,
                Path::new(r"\\?\D:\outside\victim"),
                &policy,
                &resolved_roots,
                &[],
                Some(Path::new(r"C:\Users\me")),
            ),
            Err(SafetyError::OutsideAllowedRoot(_))
        ));
    }

    #[test]
    fn roots_declared_roots_and_user_homes_are_never_deletion_targets() {
        assert!(matches!(
            validate_lexical_path(
                HostOs::Windows,
                Path::new(r"C:\"),
                CleanerCategory::UserCache,
                &DeletionPolicy::default()
            ),
            Err(SafetyError::RootDeletionRejected(_))
        ));
        assert!(matches!(
            validate_lexical_path(
                HostOs::Unix,
                Path::new("/"),
                CleanerCategory::UserCache,
                &DeletionPolicy::default()
            ),
            Err(SafetyError::RootDeletionRejected(_))
        ));

        let mut policy = policy(r"C:\Users\me\Cache");
        assert!(matches!(
            validate_lexical_path(
                HostOs::Windows,
                Path::new(r"c:\users\ME\cache"),
                CleanerCategory::UserCache,
                &policy
            ),
            Err(SafetyError::RootDeletionRejected(_))
        ));

        policy.user_home = Some(PathBuf::from(r"C:\Users\me"));
        assert!(matches!(
            validate_lexical_path(
                HostOs::Windows,
                Path::new(r"c:\users\ME"),
                CleanerCategory::UserCache,
                &policy
            ),
            Err(SafetyError::ProtectedPath(_))
        ));
    }

    #[test]
    fn an_empty_policy_authorizes_nothing() {
        let target = std::env::temp_dir().join(format!(
            "dodo-cleaner-safety-default-{}",
            std::process::id()
        ));
        fs::write(&target, b"test").expect("creates target");
        assert!(matches!(
            validate_path(
                HostOs::MacOs,
                &target,
                CleanerCategory::UserCache,
                &DeletionPolicy::default()
            ),
            Err(SafetyError::OutsideAllowedRoot(_))
        ));
        fs::remove_file(target).expect("removes target");
    }

    #[test]
    fn declared_roots_are_refused_and_valid_children_are_allowed() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-safety-policy-{}", std::process::id()));
        let root = temp.join("Library").join("Caches");
        let target = root.join("app-cache");
        fs::create_dir_all(&target).expect("creates cache tree");

        let mut policy = policy(root.clone());
        policy.protected_paths.push(temp.clone());
        assert!(matches!(
            validate_path(HostOs::MacOs, &root, CleanerCategory::UserCache, &policy),
            Err(SafetyError::RootDeletionRejected(_))
        ));
        assert!(validate_path(HostOs::MacOs, &target, CleanerCategory::UserCache, &policy).is_ok());

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    #[cfg(unix)]
    fn symlinks_and_symlinked_ancestor_escapes_are_rejected() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-safety-links-{}", std::process::id()));
        let root = temp.join("root");
        let outside = temp.join("outside");
        fs::create_dir_all(&root).expect("creates root");
        fs::create_dir_all(&outside).expect("creates outside");
        fs::write(outside.join("victim"), b"test").expect("creates victim");
        std::os::unix::fs::symlink(&outside, root.join("junction")).expect("creates ancestor link");
        std::os::unix::fs::symlink(&outside, root.join("target-link"))
            .expect("creates target link");
        let policy = policy(root.clone());

        assert!(matches!(
            validate_path(
                HostOs::MacOs,
                &root.join("target-link"),
                CleanerCategory::UserCache,
                &policy
            ),
            Err(SafetyError::SymlinkRejected(_))
        ));
        assert!(matches!(
            validate_path(
                HostOs::MacOs,
                &root.join("junction").join("victim"),
                CleanerCategory::UserCache,
                &policy
            ),
            Err(SafetyError::OutsideAllowedRoot(_))
        ));

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    #[cfg(unix)]
    fn a_directory_replaced_by_a_symlink_after_scanning_is_rejected_at_cleanup_time() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-safety-toctou-{}", std::process::id()));
        let root = temp.join("root");
        let target = root.join("app-cache");
        fs::create_dir_all(&target).expect("creates scanned directory");
        let policy = policy(root.clone());

        assert!(validate_path(HostOs::MacOs, &target, CleanerCategory::UserCache, &policy).is_ok());
        fs::remove_dir_all(&target).expect("removes original directory");
        std::os::unix::fs::symlink("/tmp", &target).expect("replaces it with a symlink");

        assert!(matches!(
            validate_path(HostOs::MacOs, &target, CleanerCategory::UserCache, &policy),
            Err(SafetyError::SymlinkRejected(_))
        ));

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }
}
