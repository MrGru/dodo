//! The English column of the tray item.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::OpenDodo => "Open Dodo".into(),
        Text::KeyboardInput => "Keyboard Input".into(),
        Text::QuitDodo => "Quit Dodo".into(),
    }
}
