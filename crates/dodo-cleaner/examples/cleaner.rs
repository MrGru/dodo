//! Opens one window containing nothing but the Cleaner.
//!
//! `cargo run -p dodo-cleaner --example cleaner --locked`
//!
//! This is a launcher, not a second application. It exists so that working on
//! the Cleaner costs a Cleaner-sized launch instead of dodo's whole window —
//! the payoff of the crate boundary. It is an `examples/` target rather than a
//! `[[bin]]` precisely so nothing a real build links can reach it, and
//! everything it needs is a `[dev-dependency]`: the shipped app's dependency
//! graph is exactly what it was before this file existed.
//!
//! It deliberately invents nothing. There is no fixture mode and no argument
//! parsing: the view is constructed the way `layout.rs` constructs it, reads
//! the same `data_dir()` through [`dodo_cleaner::paths`] and scans the real
//! machine, so what you see here is what the app shows.
//!
//! Two pieces of `src/app.rs`'s wiring are genuinely required and are the only
//! reason this file is more than a `cx.new`. `Root` is what `open_dialog`
//! pushes onto, and `Root::render_dialog_layer` — which belongs to the first
//! view *under* `Root` — is what actually paints it; without both, the uninstall
//! review dialog opens in state and never appears. The asset source is the
//! other: every category glyph is an `icons/<name>.svg` that some source has to
//! resolve, with `gpui_component_assets` behind it for the library's own carets
//! and check marks. It reads `assets/` off disk rather than embedding a copy, so
//! an edited SVG needs a restart and not a rebuild.

use std::{borrow::Cow, path::PathBuf};

use dodo_cleaner::CleanerView;
use gpui::{
    AppContext, AssetSource, Context, Entity, IntoElement, ParentElement, QuitMode, Render,
    SharedString, Styled, Window, WindowOptions, div, px, size,
};
use gpui_component::{ActiveTheme, Root};

/// `assets/` at the repository root, with the library's icon set behind it.
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

/// The first view under [`Root`], holding the Cleaner and the dialog layer.
struct CleanerWindow {
    cleaner: Entity<CleanerView>,
}

impl Render for CleanerWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.cleaner.clone())
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
                let view = cx.new(|cx| CleanerWindow {
                    cleaner: cx.new(|cx| CleanerView::new(window, cx)),
                });
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open window");
        });
}
