//! Parser extensions kept beside the user-object language feature.
use super::*;
use crate::libraries::{Field, UserType};

impl Parser<'_> {
    pub(super) fn library_statement(&mut self, start: Span) -> Option<Option<NodeId>> {
        match self.peek() {
            Tok::Ident(word) if word == "type" => Some(self.user_type(start)),
            Tok::Ident(word) if word == "method" => {
                self.advance();
                let Tok::Ident(name) = self.peek().clone() else {
                    return Some(None);
                };
                let result = self.try_function_def(&name, start);
                if let Some(id) = result {
                    if self.ast.annotations(id).first().is_none_or(Vec::is_empty) {
                        self.errors.push(PineError::new(
                            ErrorCode::PineType,
                            start,
                            "method receiver requires an explicit type",
                        ));
                    }
                    self.ast.methods.push(name);
                }
                Some(result)
            }
            _ => None,
        }
    }

    fn user_type(&mut self, start: Span) -> Option<NodeId> {
        self.advance();
        let Tok::Ident(name) = self.peek().clone() else {
            self.expect(&Tok::Ident(String::new()), "a type name");
            return None;
        };
        self.advance();
        self.expect(&Tok::Newline, "a newline after the type name");
        if !self.expect(&Tok::Indent, "indented fields") {
            return None;
        }
        let mut fields = Vec::new();
        while !matches!(self.peek(), Tok::Dedent | Tok::Eof) {
            if self.eat(&Tok::Newline) {
                continue;
            }
            let varip = self.eat(&Tok::KwVarip);
            let mut words = self.parameter_words()?;
            let field_name = words.pop()?;
            let type_name = words.join(" ");
            let explicit_default = self.eat(&Tok::Eq);
            let default = if explicit_default {
                self.expression()?
            } else {
                self.ast.push(NodeKind::Na, self.span())
            };
            fields.push(Field {
                name: field_name,
                type_name,
                default,
                explicit_default,
                varip,
            });
            self.end_of_statement();
        }
        self.expect(&Tok::Dedent, "the end of the type fields");
        if fields.is_empty() || self.ast.user_types.iter().any(|t| t.name == name) {
            self.errors.push(PineError::new(
                ErrorCode::PineType,
                start,
                "user types require unique names and at least one field",
            ));
        }
        self.ast.user_types.push(UserType { name, fields });
        let value = self.ast.push(NodeKind::Na, start);
        Some(self.ast.push(NodeKind::ExprStmt { expr: value }, start))
    }

    pub(super) fn field_assignment(&mut self, target: NodeId, start: Span) -> Option<NodeId> {
        let NodeKind::Member { object, field } = self.ast.node(target).kind.clone() else {
            self.errors.push(PineError::new(
                ErrorCode::PineSyntax,
                start,
                "field assignment requires an object field",
            ));
            return None;
        };
        let value = self.expression()?;
        self.end_of_statement();
        let callee = self.ast.push(
            NodeKind::Member {
                object,
                field: format!("$set_{field}"),
            },
            start,
        );
        let expr = self.ast.push(
            NodeKind::Call {
                callee,
                args: vec![Arg {
                    name: None,
                    value,
                    span: self.ast.node(value).span,
                }],
            },
            start,
        );
        Some(
            self.ast
                .push(NodeKind::ExprStmt { expr }, self.spanned_from(start)),
        )
    }
}
