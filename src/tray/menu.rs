//! The status item's native menu, and the one table that turns a click on it
//! into something dodo does.

use tray_icon::menu::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};

use crate::i18n::{Str, t};
use crate::tray::input_language::InputLanguage;

/// Logs a failed append and carries on.
///
/// Every one of these is fallible per item, and losing one row is not worth
/// refusing to build a menu over — there is nothing dodo could do about it, and
/// Quit lives in the same menu.
fn report(result: tray_icon::menu::Result<()>) {
    if let Err(error) = result {
        super::problem(&format!("could not add a menu item: {error}"));
    }
}

/// What a menu click means. Private to the tray: nothing outside dispatches
/// one, so there is no reason for the rest of dodo to be able to name it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrayCommand {
    /// Show the window, building one if the user has closed it.
    OpenDodo,
    /// Pick the keyboard input language the menu bar mark shows.
    SelectInputLanguage(InputLanguage),
    /// Show the window and open the Settings dialog in it.
    OpenSettings,
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
    /// The **"Keyboard Input"** submenu. Its title is deliberately not
    /// "Language": in a status menu that word reads as dodo's *interface*
    /// language, which is the Settings dialog's and lives somewhere else
    /// entirely. macOS's own term, "Input Source", is also avoided — it names
    /// the system-wide input source, which dodo does not change.
    keyboard_input: Submenu,
    /// One row per [`InputLanguage`], built by iterating
    /// [`InputLanguage::ALL`]. **Adding a language does not touch this code.**
    languages: Vec<(InputLanguage, CheckMenuItem)>,
    settings: MenuItem,
    quit: MenuItem,
}

impl TrayMenu {
    /// Builds the menu, with `selected` checked.
    ///
    /// Must run on the main thread: `muda::Menu::new` *panics* off it rather
    /// than returning an error. Every caller is inside `App::run`'s callback or
    /// a foreground task, so that is structural — see [`crate::tray::init`].
    pub fn new(selected: InputLanguage, cx: &gpui::App) -> TrayMenu {
        let languages = InputLanguage::ALL
            .into_iter()
            .map(|language| {
                (
                    language,
                    // The label is the language's endonym and is deliberately
                    // not a `Str` — see `InputLanguage::label`.
                    CheckMenuItem::new(language.label(), true, language == selected, None),
                )
            })
            .collect::<Vec<_>>();

        let this = TrayMenu {
            menu: Menu::new(),
            open: MenuItem::new(t(Str::TrayOpenDodo, cx), true, None),
            keyboard_input: Submenu::new(t(Str::TrayKeyboardInput, cx), true),
            languages,
            settings: MenuItem::new(t(Str::Settings, cx), true, None),
            quit: MenuItem::new(t(Str::TrayQuitDodo, cx), true, None),
        };
        this.assemble();
        this
    }

    /// Puts the items into the menu, in order.
    ///
    /// Every append is fallible per item, and the failure is worth degrading
    /// through rather than propagating: a menu missing one row is still a
    /// usable menu, and there is nothing dodo could do about it anyway.
    fn assemble(&self) {
        for (_, item) in &self.languages {
            report(self.keyboard_input.append(item));
        }

        // Two separators, so the verbs at the ends stay visually apart from the
        // mode selector between them.
        let separator = PredefinedMenuItem::separator();
        let items: [&dyn tray_icon::menu::IsMenuItem; 6] = [
            &self.open,
            &separator,
            &self.keyboard_input,
            &separator,
            &self.settings,
            &self.quit,
        ];
        for item in items {
            report(self.menu.append(item));
        }
    }

    /// The menu, for handing to `TrayIconBuilder::with_menu`.
    pub fn as_context_menu(&self) -> Box<dyn tray_icon::menu::ContextMenu> {
        Box::new(self.menu.clone())
    }

    /// **The single routing table.** Every menu event arrives here and leaves
    /// as a [`TrayCommand`] or as `None`; the drain task in [`crate::tray`]
    /// matches on the result and nothing else inspects ids.
    ///
    /// The language arm is a lookup over [`InputLanguage::ALL`] rather than a
    /// per-language branch, which is what keeps adding a language down to one
    /// variant and one asset.
    pub fn command_for(&self, id: &MenuId) -> Option<TrayCommand> {
        if id == self.open.id() {
            return Some(TrayCommand::OpenDodo);
        }
        if id == self.settings.id() {
            return Some(TrayCommand::OpenSettings);
        }
        if id == self.quit.id() {
            return Some(TrayCommand::Quit);
        }
        self.languages
            .iter()
            .find(|(_, item)| item.id() == id)
            .map(|(language, _)| TrayCommand::SelectInputLanguage(*language))
    }

    /// Makes `selected` the only checked row.
    ///
    /// **`muda` has no radio group**, and worse, it toggles a `CheckMenuItem`
    /// for you *before* emitting the event (`MenuChild::set_checked` from its
    /// `fire_menu_item_click`). So a click on the already-selected language
    /// arrives with that row switched **off**, and a click on a different one
    /// arrives with two rows on. Re-asserting the whole group on every event is
    /// what turns a set of checkboxes into a radio group; it is three
    /// `-[NSMenuItem setState:]` calls, which is nothing.
    ///
    /// This runs even when the selection did not change, which is why
    /// [`crate::tray::Tray::set_input_language`]'s early return is *after* it.
    pub fn check_only(&self, selected: InputLanguage) {
        for (language, item) in &self.languages {
            item.set_checked(*language == selected);
        }
    }

    /// Re-reads every translated label.
    ///
    /// Called when the **interface** language changes. It deliberately leaves
    /// the [`InputLanguage`] rows alone: the menu's own wording is dodo's text
    /// and follows the Settings dialog, while the language names are endonyms
    /// and the *selection* is a different setting that this must never move.
    #[allow(
        dead_code,
        reason = "called from the `i18n::Language` observer, which is phase 5. Remove the allow with that subscription."
    )]
    pub fn relabel(&self, cx: &gpui::App) {
        self.open.set_text(t(Str::TrayOpenDodo, cx));
        self.keyboard_input.set_text(t(Str::TrayKeyboardInput, cx));
        self.settings.set_text(t(Str::Settings, cx));
        self.quit.set_text(t(Str::TrayQuitDodo, cx));
    }
}
