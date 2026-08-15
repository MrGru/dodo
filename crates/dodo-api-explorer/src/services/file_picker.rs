//! Asking the platform for a file, without stalling the frame that asked.
//!
//! Three surfaces need the same three steps — open the native picker, learn how
//! big the chosen file is, hand both back to the entity that asked — and all
//! three would otherwise write their own `spawn`/`background_executor` dance.
//! The one thing worth getting right is that **neither the prompt nor the
//! `stat` runs on the UI thread**: the prompt is a `oneshot` the platform
//! resolves, and the size lookup goes to the background executor.
//!
//! Cancelling is not an outcome anyone has to handle: `apply` simply never
//! runs.

use std::path::PathBuf;

use gpui::{App, Context, Entity, PathPromptOptions};

/// A file the user picked, with whatever the filesystem could say about it.
///
/// `size` is `None` when the `stat` failed — a file picked off a volume that
/// vanished a moment later. The pick still stands; only the size line is
/// missing, which is better than refusing a file the picker just returned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChosenFile {
    pub path: PathBuf,
    pub size: Option<u64>,
}

impl ChosenFile {
    /// The file's own name, for the label a picker button shows once a file has
    /// been chosen.
    pub fn display_name(&self) -> String {
        self.path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string()
    }
}

/// Opens the native single-file picker and calls `apply` on `entity` with the
/// result, back on the UI thread.
pub fn choose_file<T: 'static>(
    entity: Entity<T>,
    cx: &mut App,
    apply: impl FnOnce(&mut T, ChosenFile, &mut Context<T>) + 'static,
) {
    let receiver = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: None,
    });

    cx.spawn(async move |cx| {
        let Ok(Ok(Some(paths))) = receiver.await else {
            return;
        };
        let Some(path) = paths.into_iter().next() else {
            return;
        };

        let chosen = {
            let stat = path.clone();
            let size = cx
                .background_executor()
                .spawn(async move { std::fs::metadata(&stat).ok().map(|data| data.len()) })
                .await;
            ChosenFile { path, size }
        };

        entity.update(cx, |state, cx| apply(state, chosen, cx));
    })
    .detach();
}

/// Reads a file's size on the background executor and hands it back.
///
/// Used when a path arrives from somewhere other than the picker — a saved
/// request being reopened, or a pasted cURL command — so the Body tab can show
/// the same "name · size" line either way.
pub fn refresh_size<T: 'static>(
    entity: Entity<T>,
    path: PathBuf,
    cx: &mut App,
    apply: impl FnOnce(&mut T, Option<u64>, &mut Context<T>) + 'static,
) {
    cx.spawn(async move |cx| {
        let size = cx
            .background_executor()
            .spawn(async move { std::fs::metadata(&path).ok().map(|data| data.len()) })
            .await;
        entity.update(cx, |state, cx| apply(state, size, cx));
    })
    .detach();
}

/// A byte count in the units a person reads.
///
/// Binary units, one decimal place past a kilobyte, because that is what every
/// file manager shows and the number is only ever a sanity check ("did I pick
/// the 4 GB one?").
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024. && unit + 1 < UNITS.len() {
        value /= 1024.;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{ChosenFile, format_size};

    #[test]
    fn sizes_read_in_the_units_a_person_uses() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(999), "999 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_size(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn the_label_is_the_file_name_alone() {
        let chosen = ChosenFile {
            path: PathBuf::from("/Users/ada/reports/q3.pdf"),
            size: Some(10),
        };
        assert_eq!(chosen.display_name(), "q3.pdf");
    }
}
