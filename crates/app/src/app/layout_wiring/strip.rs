//! The layout strip and the delete confirmation it opens: reloading the
//! book after an import, renaming from the strip, drawing the strip and
//! applying what a click on it asked for.

use super::LayoutAdapter;
use crate::app::chrome::LayoutRename;
use crate::canvas_layout::MAX_CANVAS_PANES;
use crate::layouts::{self, LayoutBook, LayoutError, LayoutId};
use crate::pane::PaneSide;
use smallvec::SmallVec;
use std::time::Instant;

impl LayoutAdapter<'_> {
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
        for tab in self.tabs.iter_mut() {
            for pane in tab.panes_mut() {
                pane.drawings.take_all();
                pane.drawings_key = None;
                pane.request_opening_layout(pane.layout_id());
                pane.layout_view = quantick_workspace::session::LayoutView::default();
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
            Self::load_layouts(self.store.path(), &legacy)
        };
        self.store.set_book(book);
        self.store.set_blocked(blocked);
        *self.rename = None;
        *self.delete_confirm = None;
        self.seed_new_panes();
        // Last, after the seeding, and `LayoutStore::settle` says why.
        self.store
            .settle(migrate_imported_indicators, Instant::now());
    }

    /// Open the rename box on `id`, seeded with its current name.
    pub(crate) fn begin_layout_rename(&mut self, id: LayoutId) {
        let (tab, pane) = self.focused_target();
        self.begin_layout_rename_at(tab, pane, id);
    }

    /// Add a layout and put it on the pane whose footer asked for it.
    pub(crate) fn create_layout_at(
        &mut self,
        tab: u64,
        side: PaneSide,
        name: Option<&str>,
    ) -> Result<LayoutId, LayoutError> {
        let pane = self.register_layout_pane(tab, side)?;
        let facts = self.pane_facts(tab, side);
        let id = self.store.session_mut().create_for(pane, name, facts)?;
        self.mark_layouts_dirty();
        self.switch_pane_layout(tab, side, id)?;
        Ok(id)
    }

    /// Open one pane footer's rename box on `id`.
    fn begin_layout_rename_at(&mut self, tab: u64, pane: PaneSide, id: LayoutId) {
        if let Some(layout) = self.layouts().get(id) {
            *self.rename = Some(LayoutRename {
                tab,
                pane,
                layout: id,
                draft: layout.name.clone(),
            });
        }
    }

    /// Draw one strip in every visible pane footer and apply their addressed
    /// actions after the shared catalogue borrow is released.
    pub(in crate::app) fn draw_layout_strips(&mut self, ui: &mut eframe::egui::Ui) {
        for (tab, pane, action) in pane_strip_actions(self, ui) {
            self.apply_strip_action_at(tab, pane, action);
        }
    }

    /// One door for the strip, the menu and the keyboard's rename box.
    pub(crate) fn apply_strip_action(&mut self, action: crate::layout_strip::StripAction) {
        let (tab, pane) = self.focused_target();
        self.apply_strip_action_at(tab, pane, action);
    }

    /// Apply an action from the pane-local strip that produced it.
    pub(in crate::app) fn apply_strip_action_at(
        &mut self,
        tab: u64,
        pane: PaneSide,
        action: crate::layout_strip::StripAction,
    ) {
        apply_addressed_strip_action(self, tab, pane, action);
    }
    /// The confirmation a delete waits on: the layout's name, what goes with
    /// it, and the two buttons. Enter deletes and Escape cancels — both
    /// consumed here, so neither reaches the chart's own key handling —
    /// and nothing else on the window is touched while it is up.
    pub(in crate::app) fn draw_layout_delete_confirm(&mut self, ctx: &eframe::egui::Context) {
        use eframe::egui;
        let Some(id) = *self.delete_confirm else {
            return;
        };
        let Some(layout) = self.layouts().get(id) else {
            *self.delete_confirm = None;
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
            Some(false) => *self.delete_confirm = None,
            None => {}
        }
    }

    /// The confirmed half of a delete: what the dialog's Delete button does.
    pub(crate) fn confirm_layout_delete(&mut self) {
        let Some(id) = self.delete_confirm.take() else {
            return;
        };
        if let Err(error) = self.delete_layout(id) {
            self.note_workspace(error.to_string());
        }
    }
}

type AddressedStripAction = (u64, PaneSide, crate::layout_strip::StripAction);
type PaneStripTarget = (u64, PaneSide, u64, eframe::egui::Rect, LayoutId);

/// Compose the shared catalogue into each visible pane without making the
/// root application implementation own the per-frame walk.
fn pane_strip_actions(
    app: &mut LayoutAdapter<'_>,
    ui: &mut eframe::egui::Ui,
) -> Vec<AddressedStripAction> {
    let targets: SmallVec<[PaneStripTarget; MAX_CANVAS_PANES]> = {
        let tab_id = app.tabs.active_id();
        let tab = app.active_tab();
        tab.panes()
            .filter_map(|(pane, side)| {
                pane.frame
                    .layout_strip
                    .map(|rect| (tab_id, side, pane.id, rect, app.pane_layout(tab_id, side)))
            })
            .collect()
    };
    let can_add = app.layouts().layouts().len() < layouts::MAX_LAYOUTS;
    let can_delete = app.layouts().layouts().len() > 1;
    let LayoutAdapter {
        rename: layout_rename,
        store,
        ..
    } = app;
    let layouts = store.book().layouts();
    let mut addressed = Vec::new();
    for (tab, pane, pane_id, rect, active) in targets {
        let rename = layout_rename
            .as_mut()
            .filter(|rename| rename.tab == tab && rename.pane == pane)
            .map(|rename| (rename.layout, &mut rename.draft));
        let actions = crate::layout_strip::draw(
            ui,
            rect,
            pane_id,
            crate::layout_strip::StripModel {
                layouts,
                active,
                rename,
                can_add,
                can_delete,
            },
        );
        addressed.extend(actions.into_iter().map(|action| (tab, pane, action)));
    }
    addressed
}

/// Dispatch one pane-local strip action after the drawing borrow ends.
fn apply_addressed_strip_action(
    app: &mut LayoutAdapter<'_>,
    tab: u64,
    pane: PaneSide,
    action: crate::layout_strip::StripAction,
) {
    use crate::layout_strip::StripAction;
    if !app.pane_is_real(tab, pane) {
        app.note_workspace(LayoutError::Unknown.to_string());
        return;
    }
    if let Some(tab) = app.tabs.by_id_mut(tab) {
        tab.focus = pane;
    }
    let outcome: Result<(), LayoutError> = match action {
        StripAction::Switch(id) => app.switch_pane_layout(tab, pane, id).map(|_| ()),
        StripAction::Create => app.create_layout_at(tab, pane, None).map(|_| ()),
        StripAction::BeginRename(id) => {
            app.begin_layout_rename_at(tab, pane, id);
            Ok(())
        }
        StripAction::CommitRename(id, name) => {
            *app.rename = None;
            match layouts::clean_name(&name) {
                Some(_) => app.rename_layout(id, &name).map(|_| ()),
                None => Ok(()),
            }
        }
        StripAction::CancelRename => {
            *app.rename = None;
            Ok(())
        }
        StripAction::Delete(id) => {
            if app.layouts().get(id).is_some() {
                *app.delete_confirm = Some(id);
            }
            Ok(())
        }
    };
    if let Err(error) = outcome {
        app.note_workspace(error.to_string());
    }
}
