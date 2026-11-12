//! Native window menu geometry and settings/dialog host state.
use eframe::egui;
use quantick_control_schema::{settings_sections::SettingsSection, window_menu::HelpPage};

/// The button the menu hangs from. Both open the same menu; the door only
/// decides where it drops and which way its submenus open.
pub(in crate::app) use quantick_ui_model::chrome_menu_bar::Door as MenuDoor;

/// Whether the menu is up, and which submenus are open along the way.
#[derive(Default)]
pub(in crate::app) struct WindowMenuState {
    pub(in crate::app) open: bool,
    pub(in crate::app) viewport: Option<egui::ViewportId>,
    /// The button the open menu hangs from.
    door: MenuDoor,
    /// Where the toolbar's `≡` was drawn last frame, on the window it is on.
    pub(in crate::app) toolbar_door: Option<egui::Rect>,
    /// The window Settings and Help opened on, apart from the menu's own.
    pub(in crate::app) windows_viewport: Option<egui::ViewportId>,
    pub(in crate::app) settings_open: bool,
    /// The section Settings shows; the section list and every door write it.
    pub(in crate::app) settings_section: SettingsSection,
    pub(in crate::app) help: Option<HelpPage>,
    /// The chip's menu, the armed delete and the name dialog.
    pub(in crate::app) tab_workspace: quantick_session::workspace_menu::WorkspaceMenuState,
    pub(in crate::app) keyboard: bool,
}

impl WindowMenuState {
    /// The section on screen, while Settings is up.
    pub(in crate::app) fn settings_shown(&self) -> Option<SettingsSection> {
        self.settings_open.then_some(self.settings_section)
    }

    /// Bring Settings up on `section`, on this window.
    pub(in crate::app) fn open_settings(
        &mut self,
        viewport: egui::ViewportId,
        section: SettingsSection,
    ) {
        self.windows_viewport = Some(viewport);
        self.settings_open = true;
        self.settings_section = section;
    }

    /// Open or close the menu on this window. Opened from the keyboard, its first
    /// row is ready for the keys; opened by the mouse, no row wears a focus ring.
    pub(in crate::app) fn toggle(&mut self, viewport: egui::ViewportId, by_keyboard: bool) {
        self.toggle_from(viewport, MenuDoor::Kebab, by_keyboard);
    }

    /// [`Self::toggle`], hanging the menu from `door`.
    pub(in crate::app) fn toggle_from(
        &mut self,
        viewport: egui::ViewportId,
        door: MenuDoor,
        by_keyboard: bool,
    ) {
        // Open on another window, this press still opens it here.
        let open = !self.is_open_on(viewport);
        self.close();
        self.open = open;
        self.viewport = Some(viewport);
        self.door = door;
        self.keyboard = open && by_keyboard;
    }

    /// Whether the menu is up on this window and hangs from `door`.
    pub(in crate::app) fn opened_from(&self, viewport: egui::ViewportId, door: MenuDoor) -> bool {
        self.is_open_on(viewport) && self.door == door
    }

    /// The button the menu hangs from.
    pub(in crate::app) const fn door(&self) -> MenuDoor {
        self.door
    }

    pub(in crate::app) fn is_open_on(&self, viewport: egui::ViewportId) -> bool {
        self.open && self.viewport == Some(viewport)
    }

    pub(in crate::app) fn close(&mut self) {
        self.open = false;
        self.keyboard = false;
    }
}

pub(super) fn flat_rows(ui: &mut egui::Ui) {
    crate::surfaces::chrome_menu_bar::flat_rows(ui);
}
