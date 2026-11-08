//! Version-dependent scalar acceptance.
use super::{
    check::Checker,
    model::{Base, Type, accepts},
};
impl Checker<'_> {
    pub fn version_arguments(&mut self, callee: crate::ast::NodeId, args: &[crate::ast::Arg]) {
        use crate::builtins::Builtin::*;
        if !self.version.two_state_bool() {
            return;
        }
        let crate::ir::Resolution::Builtin(builtin) = self.resolutions[callee.index()] else {
            return;
        };
        // The ordinary argument canonicalizer already checks scalar/kernel calls.
        // These surfaces retain their own named-argument extraction and need the
        // same positional/named overlap check before their arguments are read.
        let names: &[&str] = match builtin {
            Indicator => &["title", "shorttitle", "overlay"],
            Plot => &["series", "title", "color", "linewidth", "style"],
            Hline => &["price", "title", "color", "linewidth"],
            PlotShape => &["series", "title", "style", "location", "color"],
            PlotChar => &["series", "title", "char", "location", "color"],
            PlotCandle | PlotBar => &["open", "high", "low", "close", "title", "color"],
            InputInt | InputFloat | InputBool | InputString | InputColor | InputSource => {
                &["defval", "title"]
            }
            Barcolor | Bgcolor => &["color"],
            Fill => &["plot1", "plot2", "color"],
            AlertCondition => &["condition", "title", "message"],
            LineNew => &["x1", "y1", "x2", "y2"],
            BoxNew => &["left", "top", "right", "bottom"],
            LabelNew => &["x", "y", "text"],
            _ => return,
        };
        let positional = args.iter().filter(|arg| arg.name.is_none()).count();
        for arg in args {
            if let Some(name) = &arg.name
                && names
                    .iter()
                    .position(|n| *n == name)
                    .is_some_and(|index| index < positional)
            {
                self.error(
                    arg.value,
                    "v6 prohibits supplying a parameter both positionally and by name",
                );
            }
        }
    }

    pub fn unique_branches(&mut self, id: crate::ast::NodeId, a: &Type, b: &Type) {
        if self.version.two_state_bool()
            && matches!(
                (&a.base, &b.base),
                (Base::Handle("enum"), Base::Na) | (Base::Na, Base::Handle("enum"))
            )
        {
            self.error(id, "v6 unique enum expressions require non-na branches");
        }
    }
    pub fn valid_condition(&self, ty: &Type) -> bool {
        if self.version.two_state_bool() {
            matches!(ty.base, Base::Bool | Base::Unknown)
        } else {
            ty.condition()
        }
    }
    pub fn accepts(&self, expected: &Base, actual: &Base) -> bool {
        if self.version.two_state_bool() && *expected == Base::Bool {
            matches!(actual, Base::Bool | Base::Unknown)
        } else if self.version.two_state_bool()
            && *expected == Base::Handle("enum")
            && *actual == Base::Na
        {
            false
        } else {
            accepts(expected, actual)
        }
    }
}
