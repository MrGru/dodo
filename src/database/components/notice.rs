//! An inline banner: an icon, a line of text, and a tone.
//!
//! Used for the three things this module has to keep saying without a dialog —
//! that saved passwords are not encrypted, that a statement was rejected, and
//! that a test connection worked. Each is one line the user must be able to
//! read without dismissing anything, which is what a banner is for and what a
//! toast is not.

use gpui::{App, Div, ParentElement as _, SharedString, Styled as _, div};
use gpui_component::{ActiveTheme as _, Icon, Sizable as _, h_flex};

use crate::app_icon::AppIcon;

/// How loud a notice is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Something the user should know and cannot act on. The password-storage
    /// notice, which is never hidden.
    Info,
    /// Something worth a second look before continuing.
    Warning,
    /// Something went wrong.
    Danger,
    /// Something worked.
    Success,
}

/// A one-line banner.
pub fn notice(tone: Tone, text: SharedString, cx: &App) -> Div {
    let (icon, colour) = match tone {
        Tone::Info => (AppIcon::Info, cx.theme().muted_foreground),
        Tone::Warning => (AppIcon::AlertTriangle, cx.theme().warning),
        Tone::Danger => (AppIcon::AlertTriangle, cx.theme().danger),
        Tone::Success => (AppIcon::CircleCheck, cx.theme().success),
    };

    div()
        .w_full()
        .rounded(cx.theme().radius)
        .bg(colour.opacity(0.08))
        .border_1()
        .border_color(colour.opacity(0.35))
        .px_2()
        .py_1p5()
        .child(
            h_flex()
                .gap_2()
                .items_start()
                .child(Icon::new(icon).small().text_color(colour))
                // `min_w_0` because the text is the flexible child: without it
                // a long driver message sets the banner's width and pushes the
                // panel it sits in off the edge.
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(cx.theme().foreground)
                        .child(text),
                ),
        )
}
