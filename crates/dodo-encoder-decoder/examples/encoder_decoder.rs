//! Opens one window containing nothing but the Encoder/Decoder.
//!
//! `cargo run -p dodo-encoder-decoder --example encoder_decoder --locked`

use std::{borrow::Cow, path::PathBuf};

use dodo_encoder_decoder::EncoderDecoder;
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

struct EncoderDecoderWindow {
    encoder_decoder: Entity<EncoderDecoder>,
}

impl Render for EncoderDecoderWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.encoder_decoder.clone())
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
                let view = cx.new(|cx| EncoderDecoderWindow {
                    encoder_decoder: cx.new(|cx| EncoderDecoder::new(window, cx)),
                });
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open window");
        });
}
