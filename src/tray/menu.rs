//! The status item's native menu, and the one table that turns a click on it
//! into something dodo does.

use tray_icon::menu::{Menu, MenuId, MenuItem};

use crate::i18n::{Str, t};

/// What a menu click means. Private to the tray: nothing outside dispatches
/// one, so there is no reason for the rest of dodo to be able to name it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrayCommand {
    /// Show the window, building one if the user has closed it.
    OpenDodo,
    /// End the process. The **only** way to quit dodo once the tray has taken
    /// the quit mode off `LastWindowClosed` — see [`crate::tray::init`].
    Quit,
}

/// The menu, and the item handles that keep it alive and addressable.
///
/// **Holding the items matters.** `muda`'s ids are handed out per item and the
/// `Menu` itself is `Rc`-based; dropping these would take the menu with them
/// and leave a status item that opens nothing.
pub struct TrayMenu {
    menu: Menu,
    open: MenuItem,
    quit: MenuItem,
}

impl TrayMenu {
    /// Builds the menu in the language dodo's interface is currently in.
    ///
    /// Must run on the main thread: `muda::Menu::new` *panics* off it rather
    /// than returning an error. Every caller is inside `App::run`'s callback or
    /// a foreground task, so that is structural — see [`crate::tray::init`].
    pub fn new(cx: &gpui::App) -> TrayMenu {
        let menu = Menu::new();
        let open = MenuItem::new(t(Str::TrayOpenDodo, cx), true, None);
        let quit = MenuItem::new(t(Str::TrayQuitDodo, cx), true, None);

        let this = TrayMenu { menu, open, quit };
        this.rebuild();
        this
    }

    /// Puts the items into the menu, in order.
    ///
    /// Separate from [`TrayMenu::new`] because appending is fallible per item
    /// and the failure is worth degrading through rather than propagating: a
    /// menu missing one row is still a usable menu, and there is nothing dodo
    /// could do about it anyway.
    fn rebuild(&self) {
        let items: [&dyn tray_icon::menu::IsMenuItem; 2] = [&self.open, &self.quit];
        for item in items {
            if let Err(error) = self.menu.append(item) {
                super::problem(&format!("could not add a menu item: {error}"));
            }
        }
    }

    /// The menu, for handing to `TrayIconBuilder::with_menu`.
    pub fn as_context_menu(&self) -> Box<dyn tray_icon::menu::ContextMenu> {
        Box::new(self.menu.clone())
    }

    /// **The single routing table.** Every menu event arrives here and leaves
    /// as a [`TrayCommand`] or as `None`; the drain task in
    /// [`crate::tray`] matches on the result and nothing else inspects ids.
    pub fn command_for(&self, id: &MenuId) -> Option<TrayCommand> {
        if id == self.open.id() {
            Some(TrayCommand::OpenDodo)
        } else if id == self.quit.id() {
            Some(TrayCommand::Quit)
        } else {
            None
        }
    }

    /// Re-reads every translated label.
    ///
    /// Called when the **interface** language changes. It deliberately touches
    /// nothing to do with
    /// [`InputLanguage`](crate::tray::input_language::InputLanguage): the
    /// menu's own wording is dodo's text and follows the Settings dialog, while
    /// the input language is a different setting that this must never move.
    #[allow(
        dead_code,
        reason = "called from the `i18n::Language` observer, which is phase 5. Remove the allow with that subscription."
    )]
    pub fn relabel(&self, cx: &gpui::App) {
        self.open.set_text(t(Str::TrayOpenDodo, cx));
        self.quit.set_text(t(Str::TrayQuitDodo, cx));
    }
}
