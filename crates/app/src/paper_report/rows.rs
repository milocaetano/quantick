//! One row of the ledger, painted.
//!
//! A day header, a group header, an open position, a closed trade, and the
//! "N more" row that reveals the next page. Each is a painter call and a hit
//! test, with no state of its own - what to draw arrives as an argument, and
//! what the pointer did leaves as a return value. Every row stands in the
//! day card and session box `paper::ledger_frame` gives it; [`framed`]
//! paints those behind it.

use std::sync::Arc;

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_appearance::paper::{
    CARET_PX, DAY_PAD, DETAIL_PX, LINE_GAP_PX, Margin, RowFrame, RowKind, RowLook, SIDE_GAP_PX,
    TITLE_PX, TRADE_EDGE_PX, TRADE_GAP_PX, TRADE_PAD, ledger_row,
};
use quantick_engine::Side;
use quantick_sim::ClosedTrade;
use rust_decimal::Decimal;

use super::sessions::source_glyph;
use super::{LEDGER_PAGE_TRADES, LEDGER_ROW_HEIGHT_PX, OpenRow};
use crate::paper_calendar::CivilDate;
use crate::paper_chrome::signed_text;
use crate::theme::{self, BOLD_STRIKE_PX};
use crate::timezone::TzOffset;

pub(super) use quantick_paper::report::ledger_rows::{
    GroupView, Item, LedgerRow, TradeMeta, TradeWords, push_groups,
};

/// What one painted ledger row reported back.
#[derive(Default)]
pub(super) struct LedgerRowResponse {
    pub(super) clicked: bool,
    pub(super) navigate: bool,
    /// The row's own response, for its context menu.
    pub(super) response: Option<egui::Response>,
}

/// Draw one row inside `frame`'s content margin, then paint the frame's
/// card and box slices behind it, clipped to the row so a container's
/// border crosses only the rows that open and close it.
pub(super) fn framed<R>(
    ui: &mut egui::Ui,
    frame: &RowFrame,
    draw: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let palette = theme::get(ui.ctx());
    let back = ui.painter().add(egui::Shape::Noop);
    let full = ui.available_rect_before_wrap();
    let top = ui.cursor().top();
    let content = inset(
        full.with_min_y(top).with_max_y(full.bottom().max(top)),
        frame.content,
    );
    let inner = ui
        .allocate_new_ui(egui::UiBuilder::new().max_rect(content), draw)
        .inner;
    ui.add_space(frame.content.bottom);
    let rect = egui::Rect::from_x_y_ranges(full.x_range(), top..=ui.cursor().top());
    let mut shapes = Vec::new();
    for layer in frame.layers.iter().flatten() {
        let slice = inset(rect, layer.inset);
        let fill = *palette.get(layer.fill);
        shapes.push(egui::Shape::rect_filled(
            slice,
            corners(layer.corners()),
            fill,
        ));
        let [up, down] = layer.reach();
        let outline = slice
            .with_min_y(slice.top() - up)
            .with_max_y(slice.bottom() + down);
        shapes.push(egui::Shape::rect_stroke(
            outline.shrink(0.5),
            layer.radius,
            egui::Stroke::new(1.0_f32, *palette.get(layer.stroke)),
        ));
    }
    ui.painter()
        .with_clip_rect(rect)
        .set(back, egui::Shape::Vec(shapes));
    inner
}

/// Rounded `above` at the top corners and `below` at the bottom ones.
fn corners([above, below]: [f32; 2]) -> egui::Rounding {
    egui::Rounding {
        nw: above,
        ne: above,
        sw: below,
        se: below,
    }
}

fn inset(rect: egui::Rect, margin: Margin) -> egui::Rect {
    egui::Rect::from_min_max(
        rect.min + egui::vec2(margin.left, margin.top),
        rect.max - egui::vec2(margin.right, margin.bottom),
    )
}

/// A group caption inside the ledger, before its measured trade rows.
pub(super) fn draw_group_header(ui: &mut egui::Ui, label: &str, count: usize) {
    let palette = theme::get(ui.ctx());
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, palette.layout(LEDGER_ROW_HEIGHT_PX)),
        egui::Sense::hover(),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.painter().text(
        egui::pos2(rect.left() + 2.0, rect.bottom() - 4.0),
        egui::Align2::LEFT_BOTTOM,
        format!("{label} · {count}"),
        palette.mono_font(10.0),
        palette.text_faint(),
    );
}

/// One line's words from the left and its figure flush right: on one line
/// where the words keep at least half of it, else the figure under them.
struct Line {
    left: Arc<egui::Galley>,
    right: Option<Arc<egui::Galley>>,
    stacked: bool,
}

impl Line {
    fn lay(
        painter: &egui::Painter,
        mut left: egui::text::LayoutJob,
        mut right: egui::text::LayoutJob,
        width: f32,
    ) -> Self {
        let right = (!right.text.is_empty()).then(|| {
            right.wrap.max_width = width.max(1.0);
            painter.layout_job(right)
        });
        let figure = right.as_ref().map_or(0.0, |right| right.size().x);
        let room = width - figure - SIDE_GAP_PX;
        let stacked = room < width * 0.5;
        left.break_on_newline = true;
        left.wrap.max_rows = usize::MAX;
        left.wrap.max_width = if stacked { width } else { room }.max(1.0);
        Self {
            left: painter.layout_job(left),
            right,
            stacked,
        }
    }

    fn height(&self) -> f32 {
        let right = self.right.as_ref().map_or(0.0, |right| right.size().y);
        if self.stacked {
            self.left.size().y + right
        } else {
            self.left.size().y.max(right)
        }
    }

    /// Paint at `at`, `width` wide; `bold` strikes the left words or the
    /// right figure twice (egui bundles one weight).
    fn paint(self, painter: &egui::Painter, at: egui::Pos2, width: f32, bold: Bold) {
        let strike = egui::vec2(BOLD_STRIKE_PX, 0.0);
        let ink = egui::Color32::PLACEHOLDER;
        if bold == Bold::Left {
            painter.galley(at + strike, self.left.clone(), ink);
        }
        let lines = self.left.size().y;
        let below = if self.stacked { lines } else { 0.0 };
        painter.galley(at, self.left, ink);
        if let Some(right) = self.right {
            let right_at = at + egui::vec2(width - right.size().x, below);
            if bold == Bold::Right {
                painter.galley(right_at + strike, right.clone(), ink);
            }
            painter.galley(right_at, right, ink);
        }
    }
}

/// Which side of a [`Line`] reads bold.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Bold {
    Left,
    Right,
    Neither,
}

/// `text` in `font` and `color`, appended to `job`.
fn append(
    job: &mut egui::text::LayoutJob,
    text: &str,
    font_id: egui::FontId,
    color: egui::Color32,
) {
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id,
            color,
            valign: egui::Align::Center,
            ..Default::default()
        },
    );
}

/// `text` in the row's secondary ink, its signed figures in its gain and
/// loss inks.
fn signed_figures(
    palette: &theme::Palette,
    look: &RowLook,
    text: &str,
    font: egui::FontId,
    ink: egui::Color32,
) -> egui::text::LayoutJob {
    signed_text(text, font, ink, |value| {
        *palette.get(look.pnl_ink(value.cmp(&Decimal::ZERO)))
    })
}

/// A day's or a session's header: a large caret, an optional glyph and the
/// title, `figures` flush right, inside `pad`. Hovered, or `lit` (the
/// session F2 acts on), it takes the hover fill inside its container's
/// border; `folded`, it closes the container, so its fill rounds there too.
pub(super) fn draw_header(
    ui: &mut egui::Ui,
    look: &RowLook,
    (title, glyph): (&str, Option<(&str, egui::Color32)>),
    figures: &str,
    (pad, bold): (Margin, Bold),
    (folded, lit): (bool, bool),
) -> egui::Response {
    let palette = theme::get(ui.ctx());
    let ink = *palette.get(look.ink);
    let mut left = egui::text::LayoutJob::default();
    let caret = format!("{} ", caret(folded));
    append(&mut left, &caret, palette.body_font(CARET_PX), ink);
    if let Some((glyph, color)) = glyph {
        append(
            &mut left,
            &format!("{glyph} "),
            palette.body_font(10.0),
            color,
        );
    }
    append(&mut left, title, palette.mono_font(TITLE_PX), ink);
    let (small, detail) = (palette.mono_font(DETAIL_PX), *palette.get(look.detail));
    let right = signed_figures(&palette, look, figures, small, detail);
    let width = (ui.available_width() - pad.left - pad.right).max(1.0);
    let line = Line::lay(ui.painter(), left, right, width);
    let size = egui::vec2(ui.available_width(), line.height() + pad.top + pad.bottom);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        if let Some(fill) = look.hover_fill.filter(|_| lit || response.hovered()) {
            let radius = (look.radius - 1.0).max(0.0);
            let rounding = corners([radius, if folded { radius } else { 0.0 }]);
            ui.painter()
                .rect_filled(rect.shrink(1.0), rounding, *palette.get(fill));
        }
        let at = rect.min + egui::vec2(pad.left, pad.top);
        line.paint(ui.painter(), at, width, bold);
    }
    response
}

/// A day's header, the top of its card: a large caret and the bold date,
/// its complete count and net flush right (`paper::RowKind::Day`). A click
/// folds the day; folded, the header closes its card.
pub(super) fn draw_day_header(
    ui: &mut egui::Ui,
    date: CivilDate,
    count: usize,
    net: &(Decimal, String),
    folded: bool,
) -> bool {
    let palette = theme::get(ui.ctx());
    let look = ledger_row(palette.resolved.dark, RowKind::Day);
    let figures = format!("{count} \u{00b7} {}", net.1);
    let title = date.long();
    draw_header(
        ui,
        &look,
        (&title, None),
        &figures,
        (DAY_PAD, Bold::Left),
        (folded, false),
    )
    .on_hover_text(format!(
        "{} · {count} trade(s) closed · {} on the day - click to {}",
        date.iso(),
        net.1,
        if folded { "open it" } else { "fold it shut" },
    ))
    .clicked()
}

/// The fold caret a day or session row leads with.
pub(super) fn caret(folded: bool) -> &'static str {
    if folded {
        icons::CARET_RIGHT
    } else {
        icons::CARET_DOWN
    }
}

/// The "show older" control at the foot of the saved history: it names how
/// many trades are still held back, so the end of the list is never
/// mistaken for the end of the history.
pub(super) fn draw_more_row(ui: &mut egui::Ui, remaining: usize) -> bool {
    let palette = theme::get(ui.ctx());
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, palette.layout(LEDGER_ROW_HEIGHT_PX)),
        egui::Sense::click(),
    );
    if !ui.is_rect_visible(rect) {
        return response.clicked();
    }
    let body = rect.shrink2(egui::vec2(6.0, 4.0));
    ui.painter().rect_filled(
        body,
        egui::Rounding::same(4.0),
        if response.hovered() {
            palette.hover_fill()
        } else {
            palette.control()
        },
    );
    ui.painter().text(
        body.center(),
        egui::Align2::CENTER_CENTER,
        format!("{}  show older · {remaining} more saved", icons::CARET_DOWN),
        palette.mono_font(10.0),
        palette.text_muted(),
    );
    response
        .on_hover_text(format!(
            "reveal the next {LEDGER_PAGE_TRADES} saved trade(s) - {remaining} still held back"
        ))
        .clicked()
}

/// The pinned open-position row uses the current mark as its provisional exit.
pub(super) fn draw_open_row(ui: &mut egui::Ui, open: &OpenRow, symbol: Option<&str>) {
    let words = TradeWords::open(open, symbol);
    paint_trade(ui, &words, false, egui::Sense::hover());
}

/// A trade's plate in its card colour, its side rule, and its two lines:
/// the side, size and round trip with the leading figure flush right, the
/// detail with the figure behind it.
fn paint_trade(
    ui: &mut egui::Ui,
    words: &TradeWords<'_>,
    selected: bool,
    sense: egui::Sense,
) -> egui::Response {
    let palette = theme::get(ui.ctx());
    let long = words.side == Side::Buy;
    let look = ledger_row(palette.resolved.dark, RowKind::Trade { long });
    let ink = *palette.get(look.ink);
    let detail = *palette.get(look.detail_at(selected));
    let (title, small) = (palette.mono_font(TITLE_PX), palette.mono_font(DETAIL_PX));
    let mut head = egui::text::LayoutJob::default();
    append(&mut head, &words.head, title.clone(), ink);
    let route = format!("  {}", words.route);
    append(&mut head, &route, title.clone(), detail);
    if let Some(symbol) = words.symbol {
        append(&mut head, &format!("  {symbol}"), title.clone(), detail);
    }
    if let Some((glyph, color)) = source_glyph(&palette, words.source, &look) {
        append(&mut head, &format!("  {glyph}"), title.clone(), color);
    }
    let mut lower = egui::text::LayoutJob::default();
    append(&mut lower, &words.detail, small.clone(), detail);
    let width = (ui.available_width() - TRADE_EDGE_PX - TRADE_PAD.left - TRADE_PAD.right).max(1.0);
    let painter = ui.painter().clone();
    let first = Line::lay(
        &painter,
        head,
        signed_figures(&palette, &look, &words.pnl.0, title, ink),
        width,
    );
    let second = Line::lay(
        &painter,
        lower,
        signed_figures(&palette, &look, &words.pnl.1, small, detail),
        width,
    );
    let plate_height =
        TRADE_PAD.top + first.height() + LINE_GAP_PX + second.height() + TRADE_PAD.bottom;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), plate_height + TRADE_GAP_PX),
        sense,
    );
    if ui.is_rect_visible(rect) {
        let plate = rect.with_max_y(rect.top() + plate_height);
        if let Some(fill) = look.fill_at(response.hovered(), selected) {
            painter.rect_filled(plate, look.radius, *palette.get(fill));
        }
        // The side rule is the plate's rounded left edge, clipped.
        if let Some(edge) = look.edge {
            let rule = plate.with_max_x(plate.left() + TRADE_EDGE_PX);
            let clipped = painter.with_clip_rect(rule);
            clipped.rect_filled(plate, look.radius, *palette.get(edge));
        }
        let at = plate.min + egui::vec2(TRADE_EDGE_PX + TRADE_PAD.left, TRADE_PAD.top);
        let below = egui::vec2(0.0, first.height() + LINE_GAP_PX);
        first.paint(&painter, at, width, Bold::Right);
        second.paint(&painter, at + below, width, Bold::Neither);
    }
    response
}

/// How wide `text` paints in `font` - the measure the ledger's fitting
/// decisions in [`quantick_paper::report::fit`] are made with.
pub(super) fn text_width(painter: &egui::Painter, text: &str, font: &egui::FontId) -> f32 {
    let palette = theme::get(painter.ctx());
    painter
        .layout_no_wrap(text.to_owned(), font.clone(), palette.text_faint())
        .size()
        .x
}

/// One closed trade in the ledger. Every row selects on click (the tag
/// strip edits the selected one) and carries its tags on the detail line;
/// session rows also carry a separate jump-to-chart action. Rows from
/// earlier sessions never navigate — their tape is not the one on screen,
/// so the chart has nothing honest to point at. The day's card above
/// carries the date, and `symbol` comes only where the list mixes
/// instruments.
pub(super) fn draw_ledger_row(
    ui: &mut egui::Ui,
    trade: &ClosedTrade,
    meta: TradeMeta<'_>,
    symbol: Option<&str>,
    selected: bool,
    session: bool,
    tz: TzOffset,
) -> LedgerRowResponse {
    let words = TradeWords::closed(trade, meta, symbol, tz);
    let response = paint_trade(ui, &words, selected, egui::Sense::click());
    // Navigation gets its own row; it never covers the date or financial text.
    // An accent text action, not a pill: the row is the thing, this a link.
    let navigate = session
        && ui
            .link(egui::RichText::new(format!("{} Center on chart", icons::ARROW_UP_RIGHT)).small())
            .clicked();
    LedgerRowResponse {
        clicked: response.clicked() && !navigate,
        navigate,
        response: Some(response),
    }
}
