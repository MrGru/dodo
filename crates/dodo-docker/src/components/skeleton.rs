//! The non-blocking loading placeholder: a few muted bars in the shape of the
//! table's rows.
//!
//! Static rather than animated — enough to say "content is coming" without a
//! spinner that blocks or a layout that jumps when the real rows replace it. The
//! page keeps the last rows visible on a refresh, so this only shows on the very
//! first load.

use gpui::{App, IntoElement, ParentElement as _, Styled as _, div, px};
use gpui_component::{ActiveTheme as _, v_flex};

/// `rows` placeholder bars, each the height of a table row.
pub fn loading_skeleton(rows: usize, cx: &App) -> impl IntoElement {
    let bar = cx.theme().muted;
    v_flex()
        .w_full()
        .gap_2()
        .p_3()
        .children((0..rows).map(move |_| {
            div()
                .w_full()
                .h(px(40.))
                .rounded(cx.theme().radius)
                .bg(bar.opacity(0.6))
        }))
}
