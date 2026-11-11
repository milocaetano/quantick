//! Paints the order-flow settings panel inside the flow pane, right of its price
//! axis, in the chart's own width. What it remembers and decides is
//! [`quantick_workspace::orderflow_panel`]'s; this file holds the shapes, the
//! pointer and the port its named calls come in by.

use eframe::egui;
use quantick_chart::pane_header::HeaderControl;
use quantick_control::{error::ControlError, wire::WireU64};
use quantick_control_handlers::orderflow_settings::{OrderflowSettingsPort, PanelSeat};
use quantick_workspace::orderflow_panel::{PANEL_MAX_WIDTH_PX, PANEL_MIN_WIDTH_PX, PanelControl};

use crate::app::control_host::ControlPort;
use crate::pane_header::{PaintedControls, record};
use crate::tab::Tab;
use crate::theme;
use crate::ui_state::{AssetFolds, OrderFlowPanel, PanelSection};

/// The title line's words, and a header's state on its right.
const TITLE_SIZE: f32 = 15.0;
const STATE_SIZE: f32 = 11.0;
/// What a state leaves between itself and its title; a long preset name is cut.
const STATE_GAP_PX: f32 = 8.0;

/// Whether the tab has the pane its panel attaches to: no flow pane, no gear.
pub(crate) fn flow_pane_shown(tab: &Tab) -> bool {
    if !tab.canvas.column_painted() {
        return tab
            .canvas
            .drawn_bands
            .iter()
            .any(|(side, _)| *side == crate::pane::PaneSide::Flow);
    }
    tab.canvas.layout.shows_flow() && !tab.canvas.flow_collapsed
}

/// Open the tab's panel on `section`.
pub(crate) fn open_on(tab: &mut Tab, section: PanelSection) {
    if flow_pane_shown(tab) {
        tab.tape_mut().settings_panel.open_on(Some(section));
    }
}

/// The panel's key: the gear's left click, wherever focus sits.
pub(crate) fn toggle(tab: &mut Tab) {
    if flow_pane_shown(tab) {
        tab.tape_mut().settings_panel.toggle();
    }
}

/// The panel at `chart`'s right end, when it is open and there is room for a
/// chart beside it; answers the chart that is left.
pub(crate) fn draw(
    tab: &mut Tab,
    ui: &mut egui::Ui,
    chart: egui::Rect,
    folds: &mut AssetFolds,
) -> egui::Rect {
    let palette = theme::get(ui.ctx());
    let symbol = tab.symbol.as_str();
    let pane = &mut tab.flow_pane;
    let id = pane.id;
    let tick_chart = pane.state.tick_membership().is_some();
    let painted = &mut pane.frame.header_controls;
    let view = pane.orderflow.as_mut().expect("the flow pane keeps a tape");
    if !view.settings_panel.is_open() {
        return chart;
    }
    if !OrderFlowPanel::fits(chart.width()) {
        painted
            .extend(PanelSection::ALL.map(|at| (HeaderControl::section(at), egui::Rect::NOTHING)));
        return chart;
    }
    let mut seat = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(("orderflow_panel_seat", id))
            .max_rect(chart),
    );
    let landing = view.settings_panel.take_request(symbol, folds);
    let mut close = false;
    let mut restart_capture = false;
    let shown = egui::SidePanel::right(egui::Id::new(("orderflow_panel", id)))
        .default_width(view.settings_panel.width())
        .width_range(PANEL_MIN_WIDTH_PX..=PANEL_MAX_WIDTH_PX)
        .frame(
            egui::Frame::none()
                .fill(palette.chrome())
                .inner_margin(egui::Margin::symmetric(10.0, 8.0)),
        )
        .show_inside(&mut seat, |ui| {
            close = title_line(ui, symbol, painted);
            let (top, gap) = (ui.cursor().top(), ui.spacing().item_spacing.y);
            egui::ScrollArea::vertical()
                .id_salt("orderflow_panel_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for section in PanelSection::ALL {
                        let open = folds.section_open(symbol, section);
                        let state = view.panel_section_state(section);
                        let (toggled, restart) = section_block(
                            ui,
                            section,
                            open,
                            &state,
                            |ui| match section {
                                PanelSection::L2 => view.draw_l2_tab(ui),
                                PanelSection::Bubbles => view.draw_bubbles_tab(ui, tick_chart),
                            },
                            landing.map(|asked| (asked, top, gap)),
                            painted,
                        );
                        restart_capture |= restart;
                        if toggled {
                            folds.set(symbol, section, !open);
                        }
                    }
                });
        });
    let left = shown.response.rect.left();
    let moved = ui.input(|i| i.pointer.any_down() || i.raw_scroll_delta != egui::Vec2::ZERO);
    view.settings_panel
        .hand_moved(moved && ui.rect_contains_pointer(shown.response.rect));
    view.settings_panel
        .remember_width(shown.response.rect.width());
    if close {
        view.settings_panel.close();
    }
    if restart_capture {
        tab.restart_book_capture();
    }
    chart.with_max_x(left)
}

/// "ORDER FLOW · <symbol>" and ×; answers whether × was clicked.
fn title_line(ui: &mut egui::Ui, symbol: &str, painted: &mut PaintedControls) -> bool {
    let palette = theme::get(ui.ctx());
    ui.horizontal(|ui| {
        let words = egui::RichText::new(format!("ORDER FLOW · {symbol}"))
            .size(TITLE_SIZE)
            .color(palette.text_primary());
        ui.add(egui::Label::new(words).truncate().selectable(false));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let close = ui.small_button("×").on_hover_text("close the panel");
            let control = HeaderControl::Settings(PanelControl::Close);
            record(ui, control, close.rect, painted);
            close.clicked()
        })
        .inner
    })
    .inner
}

/// One section under its header, its state in faint mono on the header's right.
/// Answers whether the header was clicked, and what its body answered.
fn section_block(
    ui: &mut egui::Ui,
    section: PanelSection,
    open: bool,
    state: &str,
    body: impl FnOnce(&mut egui::Ui) -> bool,
    landing: Option<(PanelSection, f32, f32)>,
    painted: &mut PaintedControls,
) -> (bool, bool) {
    let palette = theme::get(ui.ctx());
    let words = egui::RichText::new(section.title().to_uppercase()).color(palette.text_primary());
    let block = egui::CollapsingHeader::new(words)
        .id_salt(("orderflow_panel_section", section.id()))
        .open(Some(open))
        .show_unindented(ui, body);
    let header = &block.header_response;
    if quantick_workspace::orderflow_panel::still_landing(landing, section, header.rect.top()) {
        header.scroll_to_me(Some(egui::Align::TOP));
        ui.ctx().request_repaint();
    }
    let seen = Some(header.rect).filter(|rect| ui.clip_rect().contains_rect(*rect));
    painted.push((
        HeaderControl::section(section),
        seen.unwrap_or(egui::Rect::NOTHING),
    ));
    let right = ui.max_rect().right();
    let room = right - header.rect.right() - STATE_GAP_PX;
    if room > 0.0 {
        let font = palette.mono_font(STATE_SIZE);
        let mut job =
            egui::text::LayoutJob::simple_singleline(state.to_owned(), font, palette.text_faint());
        job.wrap = egui::text::TextWrapping::truncate_at_width(room);
        let galley = ui.fonts(|fonts| fonts.layout_job(job));
        let y = header.rect.center().y - galley.size().y / 2.0;
        let at = egui::pos2(right - galley.size().x, y);
        ui.painter().galley(at, galley, palette.text_faint());
    }
    (header.clicked(), block.body_returned.unwrap_or(false))
}

impl OrderflowSettingsPort for dyn ControlPort {
    fn panel_seat(&mut self, tab: u64) -> Result<PanelSeat<'_>, ControlError> {
        let target = crate::control::layout::TabTarget {
            tab_id: Some(WireU64::new(tab)),
        };
        let index = crate::control::layout::tab_index(self, target)?;
        let lights_offered = self.appearance().has_lit_flow();
        let (tab, folds) = self
            .tabs_mut()
            .tab_with_panel_folds(index)
            .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
        let flow_shown = flow_pane_shown(tab);
        let view = tab.flow_pane.orderflow.as_mut();
        Ok(PanelSeat {
            panel: &mut view.expect("the flow pane keeps a tape").settings_panel,
            folds,
            asset: &tab.symbol,
            flow_shown,
            lights_offered,
        })
    }
}
