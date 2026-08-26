//! Opens one window containing nothing but the Mermaid workspace.
//!
//! `cargo run -p dodo-mermaid --example mermaid --locked`

use std::{borrow::Cow, path::PathBuf};

use dodo_mermaid::MermaidView;
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

struct MermaidWindow {
    workspace: Entity<MermaidView>,
}

impl Render for MermaidWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.workspace.clone())
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
                window_min_size: Some(size(px(900.), px(600.))),
                ..Default::default()
            };

            cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| MermaidWindow {
                    workspace: cx.new(|cx| MermaidView::new(window, cx)),
                });
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open window");
        });
}
