//! The Flow Canvas's rendering. **These are the only files in the crate that
//! may name `gpui` or `gpui-component`**, and that restriction is the crate's
//! main design constraint — `lib.rs` says why.
//!
//! [`flow::FlowView`] is the entity both the standalone launcher and the app's
//! `tools!` row mount. It owns the
//! [`Viewport`](crate::geometry::Viewport), the graph, the spatial index and
//! §24's snapshot, and it is where the two halves of the hybrid renderer meet.
//!
//! `canvas()` is sufficient and no custom `Element` is needed, but its `id()`
//! returns `None` in the pinned gpui, so the focus handle, key context and
//! cursor style live on a wrapping `div` and the canvas sits inside it.
//!
//! # The two halves, and which is which
//!
//! ```text
//! div  ── canvas()      the painted half: grid, edges, bodies, dots, labels
//!     ├─ absolute layer the rich half: node elements, handles, selection ring and grips
//!     ├─ absolute layer §45's tool palette
//!     └─ absolute layer the contextual property panel
//! ```
//!
//! [`palette`] and [`properties`] are a third thing again: chrome rather than
//! content. They belong to the canvas rather than to the launcher because Phase
//! 8's sidebar row mounts the same [`flow::FlowView`], and a control wired into
//! the launcher would be one that vanished the day the tool shipped.
//!
//! [`properties`] is the larger of the two and is **contextual**: what it draws
//! is decided by [`crate::properties`], which is pure and tested with no window,
//! and this directory draws whatever that answers.
//!
//! [`images`] is a fourth thing, and the odd one: §10's pictures are canvas
//! content that has to be an *element*, because a sprite's opacity is only
//! reachable from the element tree. They are built and laid out during the
//! canvas's prepaint and painted by the canvas's own painter, at the point in
//! the paint order the image run occupies — so they keep their place among the
//! bodies rather than floating above everything the way the rich half does.
//! That file's doc has the whole argument.
//!
//! [`nodes`] is the rich half. It draws **tens** of elements — the ones
//! [`RenderSnapshot`](crate::render::RenderSnapshot) marked rich, plus controls
//! for the one node being worked on — and it can never draw more, because the
//! snapshot never offers more. Everything else on screen is paint.
//!
//! Which half a node lands in is §15's and §16's answer and it is made in
//! `render::snapshot`, not here. This directory turns decisions into elements;
//! it does not take them.

pub mod flow;
pub mod images;
pub mod keymap;
pub mod nodes;
pub mod palette;
pub mod properties;

pub use flow::FlowView;
pub use keymap::{Redo, SelectTool, Undo, init};

#[cfg(test)]
mod chrome_budget_tests {
    use crate::budgets::{CHROME_PATH_VERTEX_RESERVE, for_host};
    use dodo_paths::HostOs;

    #[test]
    fn fixed_chrome_and_canvas_fit_every_safe_ceiling() {
        // A panel cannot repeat a row or an action as selection size grows.
        use crate::properties::{
            ElementAction, Labelled, SelectionItem, SelectionKind, sections_for,
        };
        for &kind in SelectionKind::ALL {
            for labelled in [Labelled::No, Labelled::Yes] {
                let item = SelectionItem::new(kind, labelled);
                for count in [1, 2, 100] {
                    let sections = sections_for(&vec![item; count]);
                    for (index, section) in sections.iter().enumerate() {
                        assert!(!sections[..index].contains(section));
                    }
                }
                for (loose, grouped) in [(false, false), (true, false), (false, true)] {
                    let actions = ElementAction::for_selection(kind, loose, grouped);
                    for (index, action) in actions.iter().enumerate() {
                        assert!(!actions[..index].contains(action));
                    }
                }
            }
        }
        // All glyph variants together overcount mutually exclusive rows and actions.
        let chrome =
            super::palette::chrome_vertex_bound() + super::properties::chrome_vertex_bound();
        for host in [HostOs::MacOs, HostOs::Windows, HostOs::Unix] {
            let safe = for_host(host).safe_path_vertex_ceiling;
            assert!(
                chrome * 2 <= CHROME_PATH_VERTEX_RESERVE as usize,
                "{host:?}: {chrome}"
            );
            assert!(safe >= CHROME_PATH_VERTEX_RESERVE);
            let canvas = safe.saturating_sub(CHROME_PATH_VERTEX_RESERVE);
            assert!(canvas + CHROME_PATH_VERTEX_RESERVE <= safe);
        }
    }
}
