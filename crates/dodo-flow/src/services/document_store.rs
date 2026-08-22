//! The active diagram, stored as `flow.json` beneath dodo's `data_dir()`.
//!
//! The store is blocking by contract. The view calls it only on GPUI's
//! background executor. Writes use a sibling temporary file and rename so a
//! crash cannot replace the last good document with a partial one.

use std::path::PathBuf;

use crate::{models::FlowDocument, paths::data_dir};

pub(crate) const DOCUMENT_FILE: &str = "flow.json";

#[derive(Debug)]
pub(crate) struct StoreError(String);

impl StoreError {
    fn at(path: &std::path::Path, error: impl std::fmt::Display) -> StoreError {
        StoreError(format!("{}: {error}", path.display()))
    }
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

pub(crate) trait DocumentStore: Send + Sync + 'static {
    fn load(&self) -> Result<FlowDocument, StoreError>;
    fn persist(&self, document: &FlowDocument) -> Result<(), StoreError>;
}

pub(crate) struct DiskDocumentStore {
    path: PathBuf,
}

impl Default for DiskDocumentStore {
    fn default() -> Self {
        Self {
            path: data_dir().join(DOCUMENT_FILE),
        }
    }
}

impl DiskDocumentStore {
    pub(crate) fn new() -> DiskDocumentStore {
        Self::default()
    }

    #[cfg(test)]
    fn at(path: PathBuf) -> DiskDocumentStore {
        DiskDocumentStore { path }
    }
}

impl DocumentStore for DiskDocumentStore {
    fn load(&self) -> Result<FlowDocument, StoreError> {
        match std::fs::read_to_string(&self.path) {
            Ok(json) => {
                FlowDocument::from_json(&json).map_err(|error| StoreError::at(&self.path, error))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(FlowDocument::new()),
            Err(error) => Err(StoreError::at(&self.path, error)),
        }
    }

    fn persist(&self, document: &FlowDocument) -> Result<(), StoreError> {
        let directory = self
            .path
            .parent()
            .expect("a file beneath data_dir has a parent");
        std::fs::create_dir_all(directory).map_err(|error| StoreError::at(directory, error))?;
        let json = document
            .to_json()
            .map_err(|error| StoreError::at(&self.path, error))?;
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, json).map_err(|error| StoreError::at(&temporary, error))?;
        std::fs::rename(&temporary, &self.path).map_err(|error| StoreError::at(&self.path, error))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{DOCUMENT_FILE, DiskDocumentStore, DocumentStore};
    use crate::{
        geometry::Vec2,
        models::{ElementKind, FlowDocument},
    };

    fn path() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "dodo-flow-store-{}-{sequence}/{DOCUMENT_FILE}",
            std::process::id()
        ))
    }

    use std::path::PathBuf;

    #[test]
    fn a_missing_file_is_an_empty_diagram() {
        let store = DiskDocumentStore::at(path());
        assert_eq!(store.load().expect("first run"), FlowDocument::new());
    }

    #[test]
    fn a_diagram_survives_a_restart_at_the_contract_file_name() {
        let path = path();
        let mut document = FlowDocument::new();
        document.add_node(
            ElementKind::default(),
            Vec2::new(10.0, 20.0),
            Vec2::new(120.0, 60.0),
        );

        DiskDocumentStore::at(path.clone())
            .persist(&document)
            .expect("persists");

        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some(DOCUMENT_FILE)
        );
        assert_eq!(
            DiskDocumentStore::at(path.clone()).load().unwrap(),
            document
        );
        assert!(!path.with_extension("json.tmp").exists());

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// **§13's render style is the author's choice, so it survives the file.**
    ///
    /// The whole chain a user walks — switch it on the palette, let the canvas
    /// write, reopen — rather than serde alone: `to_document` and
    /// `from_document` are where a settings field gets quietly dropped, and
    /// neither of them is exercised by round-tripping a `FlowDocument`.
    #[test]
    fn the_render_style_survives_a_save_and_a_reload() {
        use crate::{commands::FlowEditor, models::RenderStyle};

        let path = path();
        let mut editor = FlowEditor::new();
        assert!(editor.set_render_style(RenderStyle::Sketch));

        DiskDocumentStore::at(path.clone())
            .persist(&editor.to_document())
            .expect("persists");

        let reopened = DiskDocumentStore::at(path.clone()).load().expect("loads");
        let (editor, _) = FlowEditor::from_document(&reopened);
        assert_eq!(editor.settings().render_style, RenderStyle::Sketch);

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_corrupt_file_is_refused_and_left_untouched() {
        let path = path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not a diagram").unwrap();

        assert!(DiskDocumentStore::at(path.clone()).load().is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"not a diagram");

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
