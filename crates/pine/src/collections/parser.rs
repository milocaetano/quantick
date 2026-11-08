//! Generic collection declarations are parsed without weakening expression comparisons.
use super::{NodeId, Parser, Tok, VarMode};
impl Parser<'_> {
    pub(super) fn collection_declaration_ahead(&self) -> bool {
        let start = usize::from(matches!(self.peek(), Tok::KwVar | Tok::KwVarip));
        matches!(self.peek_at(start), Tok::Ident(name) if name == "array" || name == "map")
            && matches!(self.peek_at(start + 1), Tok::Lt)
    }
    pub(super) fn collection_declaration(&mut self) -> Option<NodeId> {
        let start = self.span();
        let mode = if self.eat(&Tok::KwVar) {
            VarMode::Var
        } else if self.eat(&Tok::KwVarip) {
            VarMode::Varip
        } else {
            VarMode::Plain
        };
        self.advance();
        self.expect(&Tok::Lt, "`<` after collection type");
        loop {
            if !matches!(self.peek(), Tok::Ident(_)) {
                self.expect(&Tok::Gt, "collection element type");
                return None;
            }
            self.advance();
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::Gt, "`>` after collection types");
        let Tok::Ident(name) = self.peek().clone() else {
            self.expect(&Tok::Eq, "variable name");
            return None;
        };
        self.advance();
        self.expect(&Tok::Eq, "`=`");
        let (value, block) = self.statement_value()?;
        if !block {
            self.end_of_statement();
        }
        Some(self.declare(mode, name, value, start))
    }
}
