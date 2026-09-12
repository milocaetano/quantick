//! The layout strip and the delete confirmation it opens: reloading the
//! book after an import, renaming from the strip, drawing the strip and
//! applying what a click on it asked for.

use super::*;

impl QuantickApp {
    // ------------------------------------------------------------------
    // Reload, rename, strip
    // ------------------------------------------------------------------

    /// Read the layouts file again and put it on every pane — after a
    /// workspace import replaced the file under the running app. Every pane
    /// reopens on the layout the imported workspace named for it, else the
    /// book's default.
    ///
    /// `imported` names the stores the import wrote. A bundle from before
    /// layouts existed carries an `indicators` section and no `layouts`
    /// section; reading the cockpit's own layouts file back would show the
    /// cockpit's indicators under a toast saying the bundle's were restored,
    /// so that case migrates the imported set into "Layout 1" instead —
    /// the same migration a launch performs.
    pub(in crate::app) fn reload_layouts(&mut self, imported: &[&str]) {
        self.clear_indicators();
        for tab in &mut self.tabs {
            for pane in tab.panes_mut() {
                pane.drawings.take_all();
                pane.drawings_key = None;
                pane.layout_seeded = false;
            }
        }
        let legacy = crate::indicators::state_file::default_path();
        let migrate_imported_indicators =
            imported.contains(&"indicators") && !imported.contains(&"layouts");
        let (book, blocked) = if migrate_imported_indicators {
            let set = crate::indicators::state_file::load(&legacy);
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "LAYOUTS_MIGRATED",
                indicators = set.len(),
                action = "imported_indicator_state_moved_into_layout_1",
                "the imported bundle predates layouts; its indicator set became Layout 1"
            );
            (LayoutBook::starter(set), false)
        } else {
            Self::load_layouts(self.workspace.layouts_path(), &legacy)
        };
        self.workspace.layouts_mut().set_book(book);
        self.workspace.layouts_mut().set_blocked(blocked);
        self.chrome.layout_rename = None;
        self.chrome.layout_delete_confirm = None;
        // A pane whose named layout is not in the new book opens on the
        // default, exactly as a pane with no name would.
        for tab in &mut self.tabs {
            for pane in tab.panes_mut() {
                let book = self.workspace.layouts().book();
                if pane.layout.is_some_and(|id| book.get(id).is_none()) {
                    pane.layout = None;
                }
            }
        }
        self.seed_new_panes();
        // Last, after the seeding, and `LayoutStore::settle` says why.
        self.workspace
            .layouts_mut()
            .settle(migrate_imported_indicators, Instant::now());
    }

    /// Open the rename box on `id`, seeded with its current name.
    pub(crate) fn begin_layout_rename(&mut self, id: LayoutId) {
        if let Some(layout) = self.layouts().get(id) {
            self.chrome.layout_rename = Some((id, layout.name.clone()));
        }
    }

    /// Draw the strip and apply what it asked for.
    pub(in crate::app) fn draw_layout_strip(&mut self, ctx: &eframe::egui::Context) {
        let actions = {
            let can_add = self.layouts().layouts().len() < layouts::MAX_LAYOUTS;
            let can_delete = self.layouts().layouts().len() > 1;
            let active = self.focused_pane_layout();
            let owner = self.active_tab().focused_side().title();
            let Self {
                chrome: ChromeState { layout_rename, .. },
                workspace,
                ..
            } = self;
            crate::layout_strip::draw(
                ctx,
                crate::layout_strip::StripModel {
                    layouts: workspace.layouts().book().layouts(),
                    active,
                    owner: &owner,
                    rename: layout_rename,
                    can_add,
                    can_delete,
                },
            )
        };
        for action in actions {
            self.apply_strip_action(action);
        }
    }

    /// One door for the strip, the menu and the keyboard's rename box.
    pub(crate) fn apply_strip_action(&mut self, action: crate::layout_strip::StripAction) {
        use crate::layout_strip::StripAction;
        let outcome: Result<(), LayoutError> = match action {
            StripAction::Switch(id) => self.switch_layout(id).map(|_| ()),
            StripAction::Create => self.create_layout(None).map(|_| ()),
            StripAction::BeginRename(id) => {
                self.begin_layout_rename(id);
                Ok(())
            }
            StripAction::CommitRename(id, name) => {
                self.chrome.layout_rename = None;
                // An empty box is a cancelled rename, not an error to show.
                match layouts::clean_name(&name) {
                    Some(_) => self.rename_layout(id, &name).map(|_| ()),
                    None => Ok(()),
                }
            }
            StripAction::CancelRename => {
                self.chrome.layout_rename = None;
                Ok(())
            }
            // Deleting takes the layout's drawings with it, on disk as well
            // as on screen: the one strip action that asks first.
            StripAction::Delete(id) => {
                if self.layouts().get(id).is_some() {
                    self.chrome.layout_delete_confirm = Some(id);
                }
                Ok(())
            }
        };
        if let Err(error) = outcome {
            self.note_workspace(error.to_string());
        }
    }
    /// The confirmation a delete waits on: the layout's name, what goes with
    /// it, and the two buttons. Enter deletes and Escape cancels — both
    /// consumed here, so neither reaches the chart's own key handling —
    /// and nothing else on the window is touched while it is up.
    pub(in crate::app) fn draw_layout_delete_confirm(&mut self, ctx: &eframe::egui::Context) {
        use eframe::egui;
        let Some(id) = self.chrome.layout_delete_confirm else {
            return;
        };
        let Some(layout) = self.layouts().get(id) else {
            self.chrome.layout_delete_confirm = None;
            return;
        };
        let name = layout.name.clone();
        let drawings: usize = layout.drawing_count();
        // Counted, not collected: the window repaints continuously and only
        // the number is read.
        let showing = self.panes_on_count(id);
        let mut decision: Option<bool> = None;
        egui::Window::new("Delete layout")
            .id(egui::Id::new("layout_delete_confirm"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(format!("Delete \"{name}\"?"));
                let mut what = if drawings == 0 {
                    "Its indicator set goes with it. Nothing is drawn under it.".to_owned()
                } else {
                    format!(
                        "Its indicator set and the {drawings} drawing(s) kept under it go with it. This cannot be undone."
                    )
                };
                if showing > 0 {
                    what.push_str(&format!(
                        " {showing} chart(s) showing it move to the layout beside it."
                    ));
                }
                ui.label(
                    egui::RichText::new(what)
                        .small()
                        .color(crate::theme::TEXT_SUPPORT),
                );
                ui.horizontal(|ui| {
                    if ui.button("Delete").clicked() {
                        decision = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        decision = Some(false);
                    }
                });
            });
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            decision = Some(false);
        } else if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        {
            decision = Some(true);
        }
        match decision {
            Some(true) => self.confirm_layout_delete(),
            Some(false) => self.chrome.layout_delete_confirm = None,
            None => {}
        }
    }

    /// The confirmed half of a delete: what the dialog's Delete button does.
    pub(crate) fn confirm_layout_delete(&mut self) {
        let Some(id) = self.chrome.layout_delete_confirm.take() else {
            return;
        };
        if let Err(error) = self.delete_layout(id) {
            self.note_workspace(error.to_string());
        }
    }
}
