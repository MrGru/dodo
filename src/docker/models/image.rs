//! One row of the Images table: an engine image reduced to the seven columns
//! the page renders and the reference the search box filters on.
//!
//! The service builds these from `bollard`'s `ImageSummary`; nothing above the
//! service sees a wire type. The compose of a repository and a tag comes from
//! the summary's `RepoTags`, which is where the `<none>` handling lives — an
//! untagged image carries no repository or tag, so both are `None` and the view
//! renders the localized `<none>` token. "Containers using" is *not* a field: it
//! is derived from the container set at render time
//! ([`ContainerUsage`](crate::docker::models::usage::ContainerUsage)).

/// The engine's token for an absent repository or tag. Parsed into `None` so the
/// view is free to render the localized placeholder rather than this literal.
const NONE: &str = "<none>";

/// An image as the table renders it.
#[derive(Clone, PartialEq, Debug)]
pub struct Image {
    /// The full content-addressable id (`sha256:…`). The stable row key and the
    /// value a container's resolved image id is matched against.
    pub id: String,
    /// The repository, or `None` for an untagged image.
    pub repository: Option<String>,
    /// The tag, or `None` for an untagged image.
    pub tag: Option<String>,
    /// Total image size in bytes.
    pub size: i64,
    /// Creation time as Unix seconds (the summary reports it directly).
    pub created: i64,
}

impl Image {
    /// The short, 12-character id the way `docker images` prints it, with the
    /// `sha256:` algorithm prefix stripped.
    pub fn short_id(&self) -> String {
        let hex = self
            .id
            .split_once(':')
            .map(|(_, hex)| hex)
            .unwrap_or(&self.id);
        hex.chars().take(12).collect()
    }

    /// Whether the image has neither a repository nor a tag — an intermediate or
    /// orphaned layer that can still be referenced only by its id.
    pub fn is_untagged(&self) -> bool {
        self.repository.is_none() && self.tag.is_none()
    }

    /// The label the delete confirmation names the image by: its `repo:tag`
    /// reference when tagged, else the short id. Shared by the row's Delete button
    /// and the right-click menu so both name the same thing.
    pub fn confirm_label(&self) -> String {
        match (&self.repository, &self.tag) {
            (Some(repo), Some(tag)) => format!("{repo}:{tag}"),
            _ => self.short_id(),
        }
    }

    /// Whether this row matches a search query, case-insensitively over the
    /// repository and the tag. An empty query matches everything; an untagged
    /// image has no text to match, so a non-empty query never selects it.
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return true;
        }
        [self.repository.as_deref(), self.tag.as_deref()]
            .into_iter()
            .flatten()
            .any(|field| field.to_lowercase().contains(&query))
    }
}

/// Splits an image's `RepoTags` into the repository and tag the row shows.
///
/// The first tag wins (an image can carry several). A missing list, or the
/// engine's `<none>:<none>` placeholder, yields `(None, None)`; either half
/// being `<none>` on its own yields `None` for that half. The tag is the part
/// after the final `:` **only** when that part has no `/`, so a registry port
/// like `localhost:5000/app` is not mistaken for a tag.
pub fn split_repo_tag(repo_tags: &[String]) -> (Option<String>, Option<String>) {
    let Some(first) = repo_tags.iter().find(|tag| !tag.is_empty()) else {
        return (None, None);
    };
    let (repo, tag) = match first.rfind(':') {
        Some(index) if !first[index + 1..].contains('/') => {
            (&first[..index], &first[index + 1..])
        }
        _ => (first.as_str(), NONE),
    };
    (none_if_placeholder(repo), none_if_placeholder(tag))
}

/// Maps the engine's `<none>` placeholder (and the empty string) to `None`.
fn none_if_placeholder(value: &str) -> Option<String> {
    if value.is_empty() || value == NONE {
        None
    } else {
        Some(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{Image, split_repo_tag};

    fn image(repo: Option<&str>, tag: Option<&str>) -> Image {
        Image {
            id: "sha256:abcdef0123456789".into(),
            repository: repo.map(Into::into),
            tag: tag.map(Into::into),
            size: 0,
            created: 0,
        }
    }

    #[test]
    fn short_id_strips_the_algorithm_and_truncates() {
        let image = image(Some("nginx"), Some("latest"));
        assert_eq!(image.short_id(), "abcdef012345");
        // An id without an algorithm prefix is truncated as-is.
        let bare = Image {
            id: "0123456789abcdef".into(),
            ..image
        };
        assert_eq!(bare.short_id(), "0123456789ab");
    }

    #[test]
    fn a_plain_repo_and_tag_split_cleanly() {
        assert_eq!(
            split_repo_tag(&["nginx:latest".into()]),
            (Some("nginx".into()), Some("latest".into()))
        );
        assert_eq!(
            split_repo_tag(&["docker.io/library/redis:7".into()]),
            (Some("docker.io/library/redis".into()), Some("7".into()))
        );
    }

    #[test]
    fn a_registry_port_is_not_read_as_a_tag() {
        assert_eq!(
            split_repo_tag(&["localhost:5000/app".into()]),
            (Some("localhost:5000/app".into()), None)
        );
    }

    #[test]
    fn untagged_images_map_to_none() {
        assert_eq!(split_repo_tag(&[]), (None, None));
        assert_eq!(split_repo_tag(&["<none>:<none>".into()]), (None, None));
        assert!(image(None, None).is_untagged());
    }

    #[test]
    fn the_first_tag_wins() {
        assert_eq!(
            split_repo_tag(&["app:1".into(), "app:latest".into()]),
            (Some("app".into()), Some("1".into()))
        );
    }

    #[test]
    fn search_is_case_insensitive_over_repository_and_tag() {
        let tagged = image(Some("Nginx"), Some("Latest"));
        assert!(tagged.matches("nginx"));
        assert!(tagged.matches("LATEST"));
        assert!(tagged.matches(""));
        assert!(!tagged.matches("redis"));
        // An untagged image never matches a non-empty query.
        assert!(!image(None, None).matches("nginx"));
        assert!(image(None, None).matches(""));
    }
}
