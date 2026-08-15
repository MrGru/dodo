//! The Vietnamese column of the tray item.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::OpenDodo => "Mở Dodo".into(),
        Text::KeyboardInput => "Bàn phím nhập".into(),
        Text::QuitDodo => "Thoát Dodo".into(),
    }
}
