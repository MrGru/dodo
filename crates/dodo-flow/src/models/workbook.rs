//! The persisted collection of diagram boards.
//!
//! A workbook owns the inactive boards as documents and cameras. The canvas
//! builds one [`GraphWorld`](crate::runtime::GraphWorld) for its active board;
//! it never creates a runtime per tab.

use serde::{Deserialize, Serialize};

use crate::{geometry::Viewport, models::FlowDocument};

/// A stable board identity, separate from the element ids inside its document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BoardId(u64);

impl BoardId {
    pub const fn new(value: u64) -> BoardId {
        BoardId(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// One independent diagram and the camera it was last viewed through.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FlowBoard {
    pub id: BoardId,
    pub name: String,
    pub document: FlowDocument,
    pub viewport: Viewport,
}

impl Default for FlowBoard {
    fn default() -> FlowBoard {
        FlowBoard {
            id: BoardId::new(1),
            name: "Board 1".into(),
            document: FlowDocument::new(),
            viewport: Viewport::default(),
        }
    }
}

/// The serialized diagram unit: ordered boards and the selected board.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FlowWorkbook {
    pub boards: Vec<FlowBoard>,
    pub active_board: BoardId,
    next_board_id: u64,
}

impl Default for FlowWorkbook {
    fn default() -> FlowWorkbook {
        FlowWorkbook {
            boards: vec![FlowBoard::default()],
            active_board: BoardId::new(1),
            next_board_id: 2,
        }
    }
}

impl FlowWorkbook {
    pub fn new() -> FlowWorkbook {
        FlowWorkbook::default()
    }

    /// Wraps a pre-workbook document without changing its content.
    pub fn with_document(document: FlowDocument) -> FlowWorkbook {
        FlowWorkbook {
            boards: vec![FlowBoard {
                document,
                ..FlowBoard::default()
            }],
            ..FlowWorkbook::default()
        }
    }

    pub fn active_board(&self) -> &FlowBoard {
        self.board(self.active_board)
            .expect("a workbook always has its active board")
    }

    pub fn active_board_mut(&mut self) -> &mut FlowBoard {
        self.board_mut(self.active_board)
            .expect("a workbook always has its active board")
    }

    pub fn board(&self, id: BoardId) -> Option<&FlowBoard> {
        self.boards.iter().find(|board| board.id == id)
    }

    pub fn board_mut(&mut self, id: BoardId) -> Option<&mut FlowBoard> {
        self.boards.iter_mut().find(|board| board.id == id)
    }

    pub fn create_board(&mut self) -> BoardId {
        let id = BoardId::new(self.next_board_id);
        self.next_board_id += 1;
        self.boards.push(FlowBoard {
            id,
            name: format!("Board {}", id.0),
            document: FlowDocument::new(),
            viewport: Viewport::default(),
        });
        self.active_board = id;
        id
    }

    pub fn select_board(&mut self, id: BoardId) -> bool {
        if self.board(id).is_none() || self.active_board == id {
            return false;
        }
        self.active_board = id;
        true
    }

    pub fn rename_board(&mut self, id: BoardId, name: String) -> bool {
        let Some(board) = self.board_mut(id) else {
            return false;
        };
        if board.name == name {
            return false;
        }
        board.name = name;
        true
    }

    /// Removes a board and selects its preceding neighbour when needed.
    ///
    /// The final board cannot be removed: every workbook always remains a
    /// usable, empty-capable board collection.
    pub fn delete_board(&mut self, id: BoardId) -> bool {
        if self.boards.len() == 1 {
            return false;
        }
        let Some(index) = self.boards.iter().position(|board| board.id == id) else {
            return false;
        };
        self.boards.remove(index);
        if self.active_board == id {
            self.active_board = self.boards[index.saturating_sub(1)].id;
        }
        true
    }

    /// Saves the sole live world's content and camera into the active board.
    pub fn save_active_board(&mut self, document: FlowDocument, viewport: Viewport) {
        let board = self.active_board_mut();
        board.document = document;
        board.viewport = viewport;
    }

    pub(crate) fn normalize(&mut self) {
        if self.boards.is_empty() {
            *self = FlowWorkbook::default();
            return;
        }
        if self.board(self.active_board).is_none() {
            self.active_board = self.boards[0].id;
        }
        let next = self
            .boards
            .iter()
            .map(|board| board.id.0)
            .max()
            .unwrap_or_default()
            .saturating_add(1);
        self.next_board_id = self.next_board_id.max(next).max(1);
    }
}

#[cfg(test)]
mod tests {
    use super::FlowWorkbook;
    use crate::{
        geometry::{Vec2, Viewport},
        models::ElementKind,
    };

    #[test]
    fn boards_are_created_selected_renamed_and_deleted_without_losing_the_last_one() {
        let mut workbook = FlowWorkbook::new();
        let first = workbook.active_board;
        workbook.active_board_mut().document.add_node(
            ElementKind::default(),
            Vec2::ZERO,
            Vec2::ONE,
        );
        workbook.active_board_mut().viewport = Viewport::new(Vec2::new(10.0, 20.0), 2.0, Vec2::ONE);
        let second = workbook.create_board();
        workbook.active_board_mut().document.add_node(
            ElementKind::default(),
            Vec2::new(30.0, 40.0),
            Vec2::ONE,
        );

        assert_eq!(workbook.active_board, second);
        assert!(workbook.select_board(first));
        assert_eq!(workbook.active_board().document.nodes.len(), 1);
        assert_eq!(
            workbook.active_board().viewport.pan(),
            Vec2::new(10.0, 20.0)
        );
        assert!(workbook.rename_board(first, "Sketches".into()));
        assert_eq!(workbook.board(first).unwrap().name, "Sketches");
        assert!(workbook.delete_board(first));
        assert_eq!(workbook.active_board, second);
        assert!(!workbook.delete_board(second));
        assert_eq!(workbook.boards.len(), 1);
    }
}
