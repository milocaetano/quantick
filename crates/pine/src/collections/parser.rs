//! Generic collection declarations are parsed without weakening expression comparisons.
use super::{NodeId, Parser, Tok, VarMode};
impl Parser<'_> {
    pub(super) fn collection_declaration_ahead(&self) -> bool {
        let start = usize::from(matches!(self.peek(), Tok::KwVar | Tok::KwVarip));
        (matches!(self.peek_at(start), Tok::Ident(name) if name == "array" || name == "map")
            && matches!(self.peek_at(start + 1), Tok::Lt))
            || (matches!(self.peek_at(start), Tok::Ident(_))
                && matches!(self.peek_at(start + 1), Tok::LBracket)
                && matches!(self.peek_at(start + 2), Tok::RBracket))
    }
    pub(super) fn parameter_words(&mut self) -> Option<Vec<String>> {
        let mut words = Vec::new();
        while let Tok::Ident(word) = self.peek().clone() {
            self.advance();
            if matches!(word.as_str(), "array" | "map") && self.eat(&Tok::Lt) {
                words.push(word);
                loop {
                    let Tok::Ident(element) = self.peek().clone() else {
                        return None;
                    };
                    self.advance();
                    words.push(element);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                if !self.eat(&Tok::Gt) {
                    return None;
                }
            } else if self.eat(&Tok::LBracket) {
                if !self.eat(&Tok::RBracket) {
                    return None;
                }
                words.extend(["array".into(), word]);
            } else {
                words.push(word);
            }
        }
        if words.is_empty() { None } else { Some(words) }
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
        if self.eat(&Tok::LBracket) {
            self.expect(&Tok::RBracket, "`]` after legacy array type");
        } else {
            self.expect(&Tok::Lt, "`<` after collection type");
            loop {
                if !matches!(self.peek(), Tok::Ident(_)) {
                    return None;
                }
                self.advance();
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
            self.expect(&Tok::Gt, "`>` after collection types");
        }
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
