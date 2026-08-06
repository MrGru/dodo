use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::SafetyError;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DeletionPolicy {
    pub allowed_roots: Vec<AllowedRoot>,
    pub protected_paths: Vec<PathBuf>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AllowedRoot {
    pub path: PathBuf,
    pub allow_root_itself: bool,
    pub allowed_categories: Vec<CleanerCategory>,
}

pub fn contains_path(parent: &Path, child: &Path) -> bool {
    let parent = normalized_components(parent);
    let child = normalized_components(child);
    parent.len() <= child.len() && child.starts_with(&parent)
}

pub fn dedupe_nested_paths(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort();
    let mut deduped = Vec::with_capacity(paths.len());
    for path in paths {
        if deduped
            .iter()
            .any(|existing: &PathBuf| contains_path(existing.as_path(), path.as_path()))
        {
            continue;
        }
        deduped.push(path);
    }
    deduped
}

pub fn validate_path(
    path: &Path,
    category: CleanerCategory,
    policy: &DeletionPolicy,
) -> Result<(), SafetyError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| SafetyError::EntryChanged(path.to_path_buf()))?;
    if metadata.file_type().is_symlink() {
        return Err(SafetyError::SymlinkRejected(path.to_path_buf()));
    }

    for protected in &policy.protected_paths {
        if path == protected || contains_path(path, protected.as_path()) {
            return Err(SafetyError::ProtectedPath(path.to_path_buf()));
        }
    }

    let Some(root) = policy.allowed_roots.iter().find(|root| {
        root.allowed_categories.contains(&category) && contains_path(root.path.as_path(), path)
    }) else {
        return Err(SafetyError::OutsideAllowedRoot(path.to_path_buf()));
    };

    if !root.allow_root_itself && path == root.path {
        return Err(SafetyError::RootDeletionRejected(path.to_path_buf()));
    }

    Ok(())
}

fn normalized_components(path: &Path) -> Vec<Component<'_>> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(components.last(), Some(Component::Normal(_))) {
                    components.pop();
                } else {
                    components.push(component);
                }
            }
            _ => components.push(component),
        }
    }
    components
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use crate::cleaner::core::category::CleanerCategory;
    use crate::cleaner::core::errors::SafetyError;
    use crate::cleaner::core::safety::{
        AllowedRoot, DeletionPolicy, contains_path, dedupe_nested_paths, validate_path,
    };

    #[test]
    fn containment_uses_path_components_not_prefixes() {
        assert!(contains_path(
            std::path::Path::new("/tmp/foo"),
            std::path::Path::new("/tmp/foo/bar")
        ));
        assert!(!contains_path(
            std::path::Path::new("/tmp/foo"),
            std::path::Path::new("/tmp/foobar")
        ));
    }

    #[test]
    fn nested_children_are_removed_when_parent_is_selected() {
        let deduped = dedupe_nested_paths(vec![
            PathBuf::from("/tmp/cache"),
            PathBuf::from("/tmp/cache/nested"),
            PathBuf::from("/tmp/other"),
        ]);
        assert_eq!(
            deduped,
            vec![PathBuf::from("/tmp/cache"), PathBuf::from("/tmp/other")]
        );
    }

    #[test]
    fn protected_roots_and_root_deletions_are_rejected() {
        let temp = std::env::temp_dir().join(format!("dodo-cleaner-safety-{}", std::process::id()));
        let root = temp.join("Library").join("Caches");
        let target = root.join("app-cache");
        fs::create_dir_all(&target).expect("creates cache tree");

        let policy = DeletionPolicy {
            allowed_roots: vec![AllowedRoot {
                path: root.clone(),
                allow_root_itself: false,
                allowed_categories: vec![CleanerCategory::UserCache],
            }],
            protected_paths: vec![std::path::PathBuf::from("/Applications")],
        };

        assert!(matches!(
            validate_path(root.as_path(), CleanerCategory::UserCache, &policy),
            Err(SafetyError::RootDeletionRejected(_))
        ));
        assert!(matches!(
            validate_path(
                temp.as_path(),
                CleanerCategory::UserCache,
                &DeletionPolicy {
                    allowed_roots: vec![AllowedRoot {
                        path: root.clone(),
                        allow_root_itself: false,
                        allowed_categories: vec![CleanerCategory::UserCache],
                    }],
                    protected_paths: vec![temp.clone()],
                }
            ),
            Err(SafetyError::ProtectedPath(_))
        ));
        assert!(validate_path(target.as_path(), CleanerCategory::UserCache, &policy).is_ok());

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn symlinks_are_rejected() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-safety-link-{}", std::process::id()));
        let root = temp.join("Library").join("Caches");
        let target = root.join("link");
        fs::create_dir_all(&root).expect("creates root");
        #[cfg(unix)]
        std::os::unix::fs::symlink("/tmp", &target).expect("creates symlink");

        let policy = DeletionPolicy {
            allowed_roots: vec![AllowedRoot {
                path: root,
                allow_root_itself: false,
                allowed_categories: vec![CleanerCategory::UserCache],
            }],
            protected_paths: vec![],
        };

        #[cfg(unix)]
        assert!(matches!(
            validate_path(target.as_path(), CleanerCategory::UserCache, &policy),
            Err(SafetyError::SymlinkRejected(_))
        ));

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }
}
