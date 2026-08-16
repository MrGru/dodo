//! Opens one window containing nothing but the Flow Canvas.
//!
//! `cargo run -p dodo-flow --example flow --locked`
//!
//! This mounts the real view and adds only the dialog and asset wiring that the
//! full app supplies around it — the shape every dodo feature crate's launcher
//! takes, and `crates/dodo-docker/examples/docker.rs` is the reference. Assets
//! are read from the repository, so editing an SVG needs a restart, not a
//! rebuild.
//!
//! **This is the only way to run the canvas today.** The sidebar row
//! lands last, deliberately, so nobody meets a half-built tool inside dodo.

use std::{borrow::Cow, path::PathBuf};

use dodo_flow::FlowView;
use gpui::{
    AppContext, AssetSource, Context, Entity, IntoElement, ParentElement, QuitMode, Render,
    SharedString, Styled, Window, WindowOptions, div, px, size,
};
use gpui_component::{ActiveTheme, Root};

struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets")
            .join(path);

        match std::fs::read(file) {
            Ok(bytes) => Ok(Some(Cow::Owned(bytes))),
            Err(_) => gpui_component_assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        gpui_component_assets::Assets.list(path)
    }
}

struct FlowWindow {
    flow: Entity<FlowView>,
}

impl Render for FlowWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.flow.clone())
            .children(Root::render_dialog_layer(window, cx))
    }
}

fn main() {
    gpui_platform::application()
        .with_assets(Assets)
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(|cx| {
            gpui_component::init(cx);
            cx.activate(true);

            let options = WindowOptions {
                window_min_size: Some(size(px(720.), px(480.))),
                ..Default::default()
            };

            cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| {
                    let flow = cx.new(|cx| FlowView::new(window, cx));
                    FlowWindow { flow }
                });
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open window");
        });
}
