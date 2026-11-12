//! Typed host access to the catalog-owned drawing chrome.
use super::*;

pub(crate) trait DrawingChromeExt {
    fn quick_range_read(&self) -> quick_range::Read<'_>;
    fn quick_range(&mut self) -> quick_range::QuickRange<'_>;

    fn manager_open(&self) -> bool;
    fn inspector_pinned(&self) -> bool;
    fn inline_text_editing(&self) -> Option<usize>;
    fn content_editing_target(&self) -> Option<(u64, PaneSide, usize)>;
    fn remembered_inspector_position(&self) -> Option<[f32; 2]>;
    fn copy_drawing(&mut self, drawing: &Drawing);
    fn next_drawing_paste(&mut self) -> Option<(Drawing, u32)>;
    fn set_manager_open(&mut self, open: bool);
    fn inspector_open(&self) -> bool;
    #[cfg(any(feature = "drawing-harness", feature = "scenario-harness", test))]
    fn carry_across_selection(&mut self);
    fn request_text_edit(&mut self);
    #[cfg(any(feature = "drawing-harness", test))]
    fn note_text_note_placed(&mut self);
    fn inline_edit_is_on(&self, tab: u64, side: PaneSide) -> bool;
    fn drop_edit_baseline_on(&mut self, tab: u64, side: PaneSide);
    #[cfg(any(feature = "drawing-harness", test))]
    fn begin_inline_text_edit(
        &mut self,
        tab: u64,
        side: PaneSide,
        index: usize,
        drawing: &Drawing,
    ) -> bool;
    fn commit_inline_text(&mut self, tabs: &mut crate::app::arrangement_host::ArrangementHost);
    fn sync_content_editing(&self, tabs: &mut crate::app::arrangement_host::ArrangementHost);
    #[cfg(test)]
    fn end_inline_text_edit(&mut self) -> Option<DrawingEdit>;
    fn restore_inspector_position(&mut self, remembered: Option<[f32; 2]>);

    fn ask_from_menu(&mut self, pane: u64, ask: DrawingChromeAsk);
    fn menu_target(&self) -> Option<u64>;
    fn delete_confirm(&self) -> bool;
    fn set_delete_confirm(&mut self, confirming: bool);
    fn take_inspector_position_dirty(&mut self) -> bool;
    #[cfg(test)]
    fn set_inspector_open(&mut self, open: bool);
    #[cfg(test)]
    fn place_inspector_by_hand(&mut self, position: egui::Pos2);
    #[cfg(test)]
    fn set_inspector_pinned(&mut self, pinned: bool);
    #[cfg(test)]
    fn set_inspector_tab(&mut self, tab: InspectorTab);
    #[cfg(test)]
    fn inspector_pos(&self) -> Option<egui::Pos2>;
    #[cfg(test)]
    fn set_inspector_pos(&mut self, position: Option<egui::Pos2>);
    #[cfg(test)]
    fn inspector_moved(&self) -> bool;
    #[cfg(test)]
    fn set_inspector_moved(&mut self, moved: bool);
    #[cfg(test)]
    fn inspector_pin_touched(&self) -> bool;
    #[cfg(test)]
    fn set_inspector_pin_touched(&mut self, touched: bool);
    #[cfg(test)]
    fn context_bar(&self) -> &crate::drawings::context_bar::ContextBar;
    #[cfg(test)]
    fn context_bar_mut(&mut self) -> &mut crate::drawings::context_bar::ContextBar;
    #[cfg(test)]
    fn set_pending_text_note(&mut self, pending: bool);
    #[cfg(test)]
    fn forget_context_bar_rect(&mut self);
    #[cfg(test)]
    fn open_edit_gesture(&mut self, tab: u64, side: PaneSide, index: usize, before: Drawing);
    #[cfg(test)]
    fn context_bar_rect(&self) -> Option<egui::Rect>;
    #[cfg(test)]
    fn inspector_pin_rect(&self) -> Option<egui::Rect>;
    #[cfg(test)]
    fn manager_action_rects(&self) -> &[(usize, &'static str, egui::Rect)];
    #[cfg(any(feature = "drawing-harness", test))]
    fn demos(&self) -> &super::demo::DrawingDemoState;
    #[cfg(any(feature = "drawing-harness", test))]
    fn demos_mut(&mut self) -> &mut super::demo::DrawingDemoState;
    #[cfg(any(feature = "drawing-harness", test))]
    fn queue_launch(&mut self, input: DrawingChromeLaunch);
}
impl DrawingChromeExt for Owner {
    fn quick_range_read(&self) -> quick_range::Read<'_> {
        quick_range::Read {
            state: &self.model_state.quick_range,
            view: &self.view.quick_range,
        }
    }

    fn quick_range(&mut self) -> quick_range::QuickRange<'_> {
        QuickRange::new(
            Session::Host(&mut self.model_state),
            &mut self.view.quick_range,
        )
    }
    fn manager_open(&self) -> bool {
        self.model_state.core.manager.open
    }
    fn inspector_pinned(&self) -> bool {
        self.model_state.core.shared.pinned
    }
    fn inline_text_editing(&self) -> Option<usize> {
        self.model_state.core.inline.editing_index()
    }
    fn content_editing_target(&self) -> Option<(u64, PaneSide, usize)> {
        self.model_state.core.inline.target()
    }
    fn remembered_inspector_position(&self) -> Option<[f32; 2]> {
        self.model_state.inspector.remembered_position()
    }
    fn copy_drawing(&mut self, drawing: &Drawing) {
        borrow(self).copy_drawing(drawing)
    }
    fn next_drawing_paste(&mut self) -> Option<(Drawing, u32)> {
        borrow(self).next_drawing_paste()
    }
    fn set_manager_open(&mut self, open: bool) {
        borrow(self).set_manager_open(open)
    }
    fn inspector_open(&self) -> bool {
        self.model_state.core.shared.open
    }
    #[cfg(any(feature = "drawing-harness", feature = "scenario-harness", test))]
    fn carry_across_selection(&mut self) {
        borrow(self).carry_across_selection()
    }
    fn request_text_edit(&mut self) {
        borrow(self).request_text_edit()
    }
    #[cfg(any(feature = "drawing-harness", test))]
    fn note_text_note_placed(&mut self) {
        borrow(self).note_text_note_placed()
    }
    fn inline_edit_is_on(&self, tab: u64, side: PaneSide) -> bool {
        self.model_state
            .core
            .inline
            .target()
            .is_some_and(|(edit_tab, edit_side, _)| edit_tab == tab && edit_side == side)
    }
    fn drop_edit_baseline_on(&mut self, tab: u64, side: PaneSide) {
        borrow(self).drop_edit_baseline_on(tab, side)
    }
    #[cfg(any(feature = "drawing-harness", test))]
    fn begin_inline_text_edit(
        &mut self,
        tab: u64,
        side: PaneSide,
        index: usize,
        drawing: &Drawing,
    ) -> bool {
        borrow(self).begin_inline_text_edit(tab, side, index, drawing)
    }
    fn commit_inline_text(&mut self, tabs: &mut crate::app::arrangement_host::ArrangementHost) {
        borrow(self).commit_inline_text(tabs)
    }
    fn sync_content_editing(&self, tabs: &mut crate::app::arrangement_host::ArrangementHost) {
        let editing = self.content_editing_target();
        for (tab_id, tab) in tabs.iter_with_ids_mut() {
            let target = editing
                .filter(|(id, _, _)| *id == tab_id)
                .map(|(_, side, index)| (side, index));
            tab.set_content_editing(target);
        }
    }
    #[cfg(test)]
    fn end_inline_text_edit(&mut self) -> Option<DrawingEdit> {
        borrow(self).end_inline_text_edit()
    }
    fn restore_inspector_position(&mut self, remembered: Option<[f32; 2]>) {
        borrow(self).restore_inspector_position(remembered)
    }

    fn ask_from_menu(&mut self, pane: u64, ask: DrawingChromeAsk) {
        borrow(self).ask_from_menu(pane, ask)
    }
    fn menu_target(&self) -> Option<u64> {
        self.model_state
            .core
            .menu_ask
            .as_ref()
            .map(|(pane, _)| *pane)
    }
    fn delete_confirm(&self) -> bool {
        self.model_state.core.shared.delete_confirm
    }
    fn set_delete_confirm(&mut self, confirming: bool) {
        borrow(self).set_delete_confirm(confirming)
    }
    fn take_inspector_position_dirty(&mut self) -> bool {
        borrow(self).take_inspector_position_dirty()
    }
    #[cfg(test)]
    fn set_inspector_open(&mut self, open: bool) {
        borrow(self).set_inspector_open(open)
    }
    #[cfg(test)]
    fn place_inspector_by_hand(&mut self, position: egui::Pos2) {
        borrow(self).place_inspector_by_hand(position)
    }
    #[cfg(test)]
    fn set_inspector_pinned(&mut self, pinned: bool) {
        borrow(self).set_inspector_pinned(pinned)
    }
    #[cfg(test)]
    fn set_inspector_tab(&mut self, tab: InspectorTab) {
        borrow(self).set_inspector_tab(tab)
    }
    #[cfg(test)]
    fn inspector_pos(&self) -> Option<egui::Pos2> {
        self.model_state
            .inspector
            .pos
            .map(|[x, y]| egui::pos2(x, y))
    }
    #[cfg(test)]
    fn set_inspector_pos(&mut self, position: Option<egui::Pos2>) {
        borrow(self).set_inspector_pos(position)
    }
    #[cfg(test)]
    fn inspector_moved(&self) -> bool {
        self.model_state.inspector.moved
    }
    #[cfg(test)]
    fn set_inspector_moved(&mut self, moved: bool) {
        borrow(self).set_inspector_moved(moved)
    }
    #[cfg(test)]
    fn inspector_pin_touched(&self) -> bool {
        self.model_state.inspector.pin_touched
    }
    #[cfg(test)]
    fn set_inspector_pin_touched(&mut self, touched: bool) {
        borrow(self).set_inspector_pin_touched(touched)
    }
    #[cfg(test)]
    fn context_bar(&self) -> &crate::drawings::context_bar::ContextBar {
        &self.view.bar.bar
    }
    #[cfg(test)]
    fn context_bar_mut(&mut self) -> &mut crate::drawings::context_bar::ContextBar {
        &mut self.view.bar.bar
    }
    #[cfg(test)]
    fn set_pending_text_note(&mut self, pending: bool) {
        borrow(self).set_pending_text_note(pending)
    }
    #[cfg(test)]
    fn forget_context_bar_rect(&mut self) {
        borrow(self).forget_context_bar_rect()
    }
    #[cfg(test)]
    fn open_edit_gesture(&mut self, tab: u64, side: PaneSide, index: usize, before: Drawing) {
        borrow(self).open_edit_gesture(tab, side, index, before)
    }
    #[cfg(test)]
    fn context_bar_rect(&self) -> Option<egui::Rect> {
        self.view.bar.last_drawn_rect
    }
    #[cfg(test)]
    fn inspector_pin_rect(&self) -> Option<egui::Rect> {
        self.view.inspector.pin_rect
    }
    #[cfg(test)]
    fn manager_action_rects(&self) -> &[(usize, &'static str, egui::Rect)] {
        &self.view.manager_rects
    }
    #[cfg(any(feature = "drawing-harness", test))]
    fn demos(&self) -> &super::demo::DrawingDemoState {
        self.view.scenario.demos()
    }
    #[cfg(any(feature = "drawing-harness", test))]
    fn demos_mut(&mut self) -> &mut super::demo::DrawingDemoState {
        self.view.scenario.demos_mut()
    }
    #[cfg(any(feature = "drawing-harness", test))]
    fn queue_launch(&mut self, input: DrawingChromeLaunch) {
        borrow(self).queue_launch(input)
    }
}
