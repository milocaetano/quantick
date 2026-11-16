//! The layout picker's pixels and presses. `quantick_ui_model::layout_picker`
//! decides its size, every place, word and colour; this hangs a frame of
//! exactly that size where `place_under` says, paints the marks, and reports
//! what the pointer and keys did as a [`Seen`].

use eframe::egui::scroll_area::ScrollBarVisibility::{AlwaysHidden, AlwaysVisible};
use eframe::egui::{self, Color32, Modifiers, Pos2, Rect, Sense, vec2};
use quantick_ui_model::layout_picker::{self as model, Mark, Picker, Seen, Target};
use quantick_workspace::canvas::LayoutPreset;

/// The picker's area; its "keys" child holds the focus that keeps the
/// arrows, Enter, Esc and Ctrl+Z from the chart while it is open.
const ID: &str = "layout_popover";
/// The app's bold face, when the fonts are installed.
const BOLD: &str = "note-sans-bold";

/// What the frame drew for the scene: cells, header, "was" words, with rects.
#[derive(Default)]
pub struct Drawn {
    pub cells: Vec<(&'static LayoutPreset, Rect)>,
    pub header: Option<(Rect, String)>,
    pub undo: Option<(Rect, String)>,
}

fn rgba([r, g, b, a]: [u8; 4]) -> Color32 {
    Color32::from_rgba_premultiplied(r, g, b, a)
}

/// The model's `at` from `origin`, in window points.
fn place(origin: Pos2, at: model::Rect) -> Rect {
    Rect::from_min_size(origin + vec2(at.x, at.y), vec2(at.w, at.h))
}

/// Hang `picker` flush below `button` (`anchor` presses it), on `tab`: drawn, seen.
pub fn draw(
    ctx: &egui::Context,
    (anchor, button): (Rect, Rect),
    picker: &Picker,
    (current, tab): (Option<&'static LayoutPreset>, u64),
) -> (Drawn, Seen) {
    let ours = |r: Rect| model::Rect::new(r.left(), r.top(), r.width(), r.height());
    let spot = model::place_under(ours(button), ours(ctx.screen_rect()));
    let frame = egui::Frame::popup(&ctx.style()).inner_margin(model::BORDER_PT);
    let frame = frame.fill(rgba(picker.ground)).rounding(model::RADIUS_PT);
    let frame = frame.stroke((model::BORDER_PT, rgba(picker.edge)));
    let (mut drawn, mut over, at) = (Drawn::default(), None, egui::pos2(spot.x, spot.y));
    let (id, scroll) = (egui::Id::new(ID), egui::ScrollArea::vertical());
    let area = egui::Area::new(id).order(egui::Order::Foreground);
    let area = area.constrain(false).fixed_pos(at).show(ctx, |ui| {
        frame.show(ui, |ui| {
            let scroll = scroll.max_height(spot.height - 2.0 * model::BORDER_PT);
            let bar = [AlwaysHidden, AlwaysVisible][usize::from(spot.scrolls)];
            let (mut style, edge) = (egui::style::ScrollStyle::solid(), rgba(picker.edge));
            (style.bar_width, style.bar_inner_margin) = (4.0, 0.0); // A thin bar: clipped.
            ui.spacing_mut().scroll = style;
            let w = &mut ui.visuals_mut().widgets;
            (w.inactive.bg_fill, w.hovered.bg_fill, w.active.bg_fill) = (edge, edge, edge);
            scroll.scroll_bar_visibility(bar).show(ui, |ui| {
                let size = vec2(picker.size[0], picker.size[1]);
                let (whole, _) = ui.allocate_exact_size(size, Sense::hover());
                ui.interact(whole, id.with("keys"), Sense::focusable_noninteractive());
                let (origin, seen) = (whole.min, ui.clip_rect());
                paint(ui.painter(), origin, &picker.marks);
                let mut hit = |at: model::Rect, key: &str, target: Target| {
                    let response =
                        ui.interact(place(origin, at), ui.id().with(key), Sense::click());
                    over = over.or(response.contains_pointer().then_some(target));
                    response
                };
                for cell in &picker.cells {
                    paint(ui.painter(), origin, &cell.marks);
                    let tip = cell.preset.hint;
                    hit(cell.at, cell.preset.id, Target::Cell(cell.preset)).on_hover_text(tip);
                    let shown = place(origin, cell.at).intersect(seen);
                    if cell.focused && shown != place(origin, cell.at) {
                        ui.scroll_to_rect(place(origin, cell.at), None);
                    }
                    drawn.cells.push((cell.preset, shown));
                }
                let header = &picker.header;
                drawn.header = Some((place(origin, header.at).intersect(seen), header.label()));
                if let Some((at, words)) = &header.undo {
                    let reach = hit(*at, "undo", Target::Undo).rect.intersect(seen);
                    drawn.undo = Some((reach, words.clone()));
                }
            });
        });
    });
    let mut keep = egui::EventFilter::default();
    (keep.tab, keep.horizontal_arrows, keep.vertical_arrows) = (true, true, true);
    keep.escape = true;
    ctx.memory_mut(|memory| memory.request_focus(id.with("keys")));
    ctx.memory_mut(|memory| memory.set_focus_lock_filter(id.with("keys"), keep));
    let (frame, pointer) = (area.response.rect, ctx.input(|input| input.pointer.clone()));
    let away = |at: Pos2| !anchor.contains(at) && !frame.contains(at);
    let outside = pointer.any_pressed() && pointer.interact_pos().is_some_and(away);
    let pressed = ctx.input_mut(|input| {
        let shifted = |e: &_| matches!(e, egui::Event::Key { modifiers: m, .. } if m.shift);
        let shift = input.modifiers.shift || input.events.iter().any(shifted); // Shift+Z: redo.
        model::KEY_NAMES.map(|(name, command)| {
            let modifiers = [Modifiers::NONE, Modifiers::COMMAND][usize::from(command)];
            let key = egui::Key::from_name(name).filter(|_| !shift);
            key.is_some_and(|key| input.consume_key(modifiers, key))
        })
    });
    let (press, release) = (pointer.primary_pressed(), pointer.primary_released());
    let clicks = [press, release, outside];
    let seen = Seen::of((now_ms(ctx), tab), current, over, clicks, pressed);
    (drawn, seen)
}

/// The window's clock, in milliseconds.
pub fn now_ms(ctx: &egui::Context) -> u64 {
    (ctx.input(|input| input.time) * 1000.0) as u64
}

/// `marks` from `origin`: square fills as exact pixels, rounded ones and
/// edges as egui shapes, words in the proportional face or the bold one.
pub fn paint(painter: &egui::Painter, origin: Pos2, marks: &[Mark]) {
    let (bold, context) = (egui::FontFamily::Name(BOLD.into()), painter.ctx());
    let has_bold = context.fonts(|fonts| fonts.families().contains(&bold));
    let mut mesh = egui::Mesh::default();
    for mark in marks {
        if !matches!(mark, Mark::Fill(plate) if plate.radius == 0.0) {
            painter.add(std::mem::take(&mut mesh));
        }
        match mark {
            Mark::Fill(plate) if plate.radius == 0.0 => {
                mesh.add_colored_rect(place(origin, plate.at), rgba(plate.rgba));
            }
            Mark::Fill(plate) => {
                painter.rect_filled(place(origin, plate.at), plate.radius, rgba(plate.rgba));
            }
            Mark::Edge(plate, width) => {
                let at = place(origin, plate.at).shrink(width / 2.0);
                painter.rect_stroke(at, plate.radius - width / 2.0, (*width, rgba(plate.rgba)));
            }
            Mark::Words(words) => {
                let heavy = words.bold && has_bold;
                let family = if heavy {
                    bold.clone()
                } else {
                    egui::FontFamily::Proportional
                };
                let (font, color) = (egui::FontId::new(words.size, family), rgba(words.rgba));
                let mut format = egui::TextFormat::simple(font, color);
                format.extra_letter_spacing = words.tracking;
                format.underline = egui::Stroke::new(f32::from(u8::from(words.underline)), color);
                let mut job = egui::text::LayoutJob::default();
                if let Some((glyph, ink)) = &words.lead {
                    let mut lead = format.clone();
                    lead.color = rgba(*ink);
                    job.append(glyph, 0.0, lead);
                }
                job.append(&words.text, 0.0, format);
                job.wrap = egui::text::TextWrapping::truncate_at_width(words.width);
                let galley = painter.layout_job(job);
                let shift = [0.0, 0.5, 1.0][words.anchor as usize] * galley.size().x;
                let at = origin + vec2(words.x - shift, words.y - galley.size().y / 2.0);
                painter.galley(at.round(), galley, color);
            }
        }
    }
    painter.add(mesh);
}

#[cfg(test)]
mod tests {
    use crate::canvas_layout::LAYOUT_PRESETS;

    /// The popover reads the registry: no preset id is spelled here.
    #[test]
    fn the_popover_names_no_preset_of_its_own() {
        let source = include_str!("layout_popover.rs");
        for preset in LAYOUT_PRESETS {
            assert!(
                !source.contains(&format!("\"{}\"", preset.id)),
                "{}",
                preset.id
            );
        }
    }
}
