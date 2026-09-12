//! The draw-object family: `line.new`, `box.new`, `label.new` and the
//! `handle.set_*` / `handle.delete()` methods on what they return.
//!
//! These are the built-ins that write to the shared `ObjectStore` instead
//! of to the plot row, so they own the argument coercions a drawing takes —
//! bar index, price, colour, text — and the defaults a script may omit.

use quantick_indicators::{BoxObj, LabelObj, LabelStyle, LineObj, ObjectId, Rgba8};

use crate::ast::NodeId;
use crate::error::{ErrorCode, PineError};

use super::{Eval, ObjKind, Value};

/// Default draw-object stroke/border/label color (a readable blue).
const DEFAULT_OBJECT_COLOR: Rgba8 = Rgba8::opaque(41, 98, 255);
/// Default box fill: the border color at ~85% transparency.
const DEFAULT_BOX_FILL: Rgba8 = Rgba8::new(41, 98, 255, 38);
/// Default label text color.
const DEFAULT_LABEL_TEXT: Rgba8 = Rgba8::opaque(255, 255, 255);
/// Default line width in points.
const DEFAULT_LINE_WIDTH: f32 = 1.0;

impl<'a> Eval<'a> {
    /// A named-or-positional argument, mirroring the compile-time helper.
    fn call_arg<'b>(
        args: &'b [crate::ast::Arg],
        position: usize,
        name: &str,
    ) -> Option<&'b crate::ast::Arg> {
        args.iter()
            .find(|a| a.name.as_deref() == Some(name))
            .or_else(|| args.iter().filter(|a| a.name.is_none()).nth(position))
    }

    fn arg_bar_index(
        &mut self,
        args: &[crate::ast::Arg],
        position: usize,
        name: &str,
    ) -> Result<i64, PineError> {
        match Self::call_arg(args, position, name) {
            Some(arg) => {
                let v = self.num(arg.value)?;
                #[allow(clippy::cast_possible_truncation)]
                Ok(if v.is_finite() {
                    v.round() as i64
                } else {
                    // `label.new(na, price)` has no bar to sit on. Zero would
                    // anchor it to the very first bar — a real label drawn
                    // out of a missing value, while the price side of the
                    // same call answers NaN and the renderer honestly skips
                    // it. `OFF_CHART_BAR` is negative, which the renderer's
                    // existing `x < 0` guards already drop.
                    quantick_indicators::OFF_CHART_BAR
                })
            }
            None => Ok(self.ctx.bar_index as i64),
        }
    }

    fn arg_price(
        &mut self,
        args: &[crate::ast::Arg],
        position: usize,
        name: &str,
    ) -> Result<f64, PineError> {
        match Self::call_arg(args, position, name) {
            Some(arg) => self.num(arg.value),
            None => Ok(f64::NAN),
        }
    }

    fn arg_color(
        &mut self,
        args: &[crate::ast::Arg],
        position: usize,
        name: &str,
        default: Rgba8,
    ) -> Result<Rgba8, PineError> {
        match Self::call_arg(args, position, name) {
            Some(arg) => {
                let v = self.eval(arg.value)?;
                match v {
                    Value::Color(rgba) => Ok(Rgba8::from_u32(rgba)),
                    Value::Na => Ok(default),
                    other => Err(self.type_err(arg.value, "a color", &other)),
                }
            }
            None => Ok(default),
        }
    }

    fn arg_text(
        &mut self,
        args: &[crate::ast::Arg],
        position: usize,
        name: &str,
    ) -> Result<String, PineError> {
        match Self::call_arg(args, position, name) {
            Some(arg) => {
                let v = self.eval(arg.value)?;
                match v {
                    Value::Str(s) => Ok(s.as_ref().clone()),
                    Value::Na => Ok(String::new()),
                    other => Err(self.type_err(arg.value, "a string", &other)),
                }
            }
            None => Ok(String::new()),
        }
    }

    pub(super) fn line_new(&mut self, args: &[crate::ast::Arg]) -> Result<Value, PineError> {
        let x1 = self.arg_bar_index(args, 0, "x1")?;
        let y1 = self.arg_price(args, 1, "y1")?;
        let x2 = self.arg_bar_index(args, 2, "x2")?;
        let y2 = self.arg_price(args, 3, "y2")?;
        let color = self.arg_color(args, 4, "color", DEFAULT_OBJECT_COLOR)?;
        let width = match Self::call_arg(args, 5, "width") {
            Some(arg) => {
                #[allow(clippy::cast_possible_truncation)]
                let w = self.num(arg.value)? as f32;
                w
            }
            None => DEFAULT_LINE_WIDTH,
        };
        let handle = self.state.objects.new_line(LineObj {
            x1,
            y1,
            x2,
            y2,
            color,
            width,
        });
        Ok(Value::Obj(ObjKind::Line, handle.0))
    }

    pub(super) fn box_new(&mut self, args: &[crate::ast::Arg]) -> Result<Value, PineError> {
        let left = self.arg_bar_index(args, 0, "left")?;
        let top = self.arg_price(args, 1, "top")?;
        let right = self.arg_bar_index(args, 2, "right")?;
        let bottom = self.arg_price(args, 3, "bottom")?;
        let border_color = self.arg_color(args, 4, "border_color", DEFAULT_OBJECT_COLOR)?;
        let bg_color = self.arg_color(args, 5, "bgcolor", DEFAULT_BOX_FILL)?;
        let handle = self.state.objects.new_box(BoxObj {
            left,
            top,
            right,
            bottom,
            border_color,
            bg_color,
            border_width: DEFAULT_LINE_WIDTH,
        });
        Ok(Value::Obj(ObjKind::Box, handle.0))
    }

    pub(super) fn label_new(&mut self, args: &[crate::ast::Arg]) -> Result<Value, PineError> {
        let x = self.arg_bar_index(args, 0, "x")?;
        let y = self.arg_price(args, 1, "y")?;
        let text = self.arg_text(args, 2, "text")?;
        let color = self.arg_color(args, 3, "color", DEFAULT_OBJECT_COLOR)?;
        let text_color = self.arg_color(args, 4, "textcolor", DEFAULT_LABEL_TEXT)?;
        let style = match Self::call_arg(args, 5, "style") {
            Some(arg) => {
                let v = self.eval(arg.value)?;
                match v {
                    Value::Str(tag) => match tag.as_str() {
                        "style_label_up" => LabelStyle::Up,
                        "style_label_down" => LabelStyle::Down,
                        _ => LabelStyle::None,
                    },
                    _ => LabelStyle::Down,
                }
            }
            None => LabelStyle::Down,
        };
        let handle = self.state.objects.new_label(LabelObj {
            x,
            y,
            text,
            color,
            text_color,
            style,
        });
        Ok(Value::Obj(ObjKind::Label, handle.0))
    }

    /// `handle.set_*(...)` / `handle.delete()`. A stale handle (collected
    /// by the cap, or deleted) is a silent no-op — Pine's behavior, and the
    /// honest one for GC'd objects.
    #[allow(clippy::too_many_lines)]
    pub(super) fn method_call(
        &mut self,
        id: NodeId,
        kind: ObjKind,
        handle: ObjectId,
        field: &str,
        args: &[crate::ast::Arg],
    ) -> Result<Value, PineError> {
        match (kind, field) {
            (ObjKind::Line, "delete") => self.state.objects.delete_line(handle),
            (ObjKind::Box, "delete") => self.state.objects.delete_box(handle),
            (ObjKind::Label, "delete") => self.state.objects.delete_label(handle),
            (ObjKind::Line, "set_xy1" | "set_xy2") => {
                let x = self.arg_bar_index(args, 0, "x")?;
                let y = self.arg_price(args, 1, "y")?;
                if let Some(line) = self.state.objects.line_mut(handle) {
                    if field == "set_xy1" {
                        line.x1 = x;
                        line.y1 = y;
                    } else {
                        line.x2 = x;
                        line.y2 = y;
                    }
                }
            }
            (ObjKind::Line, "set_x1" | "set_x2") => {
                let x = self.arg_bar_index(args, 0, "x")?;
                if let Some(line) = self.state.objects.line_mut(handle) {
                    if field == "set_x1" {
                        line.x1 = x;
                    } else {
                        line.x2 = x;
                    }
                }
            }
            (ObjKind::Line, "set_y1" | "set_y2") => {
                let y = self.arg_price(args, 0, "y")?;
                if let Some(line) = self.state.objects.line_mut(handle) {
                    if field == "set_y1" {
                        line.y1 = y;
                    } else {
                        line.y2 = y;
                    }
                }
            }
            (ObjKind::Line, "set_color") => {
                let color = self.arg_color(args, 0, "color", DEFAULT_OBJECT_COLOR)?;
                if let Some(line) = self.state.objects.line_mut(handle) {
                    line.color = color;
                }
            }
            (ObjKind::Line, "set_width") => {
                #[allow(clippy::cast_possible_truncation)]
                let width = self.arg_price(args, 0, "width")? as f32;
                if let Some(line) = self.state.objects.line_mut(handle) {
                    line.width = width;
                }
            }
            (ObjKind::Box, "set_left" | "set_right") => {
                let x = self.arg_bar_index(args, 0, "x")?;
                if let Some(object) = self.state.objects.box_mut(handle) {
                    if field == "set_left" {
                        object.left = x;
                    } else {
                        object.right = x;
                    }
                }
            }
            (ObjKind::Box, "set_top" | "set_bottom") => {
                let y = self.arg_price(args, 0, "y")?;
                if let Some(object) = self.state.objects.box_mut(handle) {
                    if field == "set_top" {
                        object.top = y;
                    } else {
                        object.bottom = y;
                    }
                }
            }
            (ObjKind::Box, "set_lefttop" | "set_rightbottom") => {
                let x = self.arg_bar_index(args, 0, "x")?;
                let y = self.arg_price(args, 1, "y")?;
                if let Some(object) = self.state.objects.box_mut(handle) {
                    if field == "set_lefttop" {
                        object.left = x;
                        object.top = y;
                    } else {
                        object.right = x;
                        object.bottom = y;
                    }
                }
            }
            (ObjKind::Box, "set_bgcolor") => {
                let color = self.arg_color(args, 0, "color", DEFAULT_BOX_FILL)?;
                if let Some(object) = self.state.objects.box_mut(handle) {
                    object.bg_color = color;
                }
            }
            (ObjKind::Box, "set_border_color") => {
                let color = self.arg_color(args, 0, "color", DEFAULT_OBJECT_COLOR)?;
                if let Some(object) = self.state.objects.box_mut(handle) {
                    object.border_color = color;
                }
            }
            (ObjKind::Label, "set_xy") => {
                let x = self.arg_bar_index(args, 0, "x")?;
                let y = self.arg_price(args, 1, "y")?;
                if let Some(label) = self.state.objects.label_mut(handle) {
                    label.x = x;
                    label.y = y;
                }
            }
            (ObjKind::Label, "set_x") => {
                let x = self.arg_bar_index(args, 0, "x")?;
                if let Some(label) = self.state.objects.label_mut(handle) {
                    label.x = x;
                }
            }
            (ObjKind::Label, "set_y") => {
                let y = self.arg_price(args, 0, "y")?;
                if let Some(label) = self.state.objects.label_mut(handle) {
                    label.y = y;
                }
            }
            (ObjKind::Label, "set_text") => {
                let text = self.arg_text(args, 0, "text")?;
                if let Some(label) = self.state.objects.label_mut(handle) {
                    label.text = text;
                }
            }
            (ObjKind::Label, "set_color") => {
                let color = self.arg_color(args, 0, "color", DEFAULT_OBJECT_COLOR)?;
                if let Some(label) = self.state.objects.label_mut(handle) {
                    label.color = color;
                }
            }
            (ObjKind::Label, "set_textcolor") => {
                let color = self.arg_color(args, 0, "textcolor", DEFAULT_LABEL_TEXT)?;
                if let Some(label) = self.state.objects.label_mut(handle) {
                    label.text_color = color;
                }
            }
            _ => {
                return Err(self.err(
                    id,
                    ErrorCode::PineType,
                    format!("unknown method `{field}` for this object kind"),
                ));
            }
        }
        Ok(Value::Na)
    }
}
