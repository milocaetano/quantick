//! Load-time restrictions for persistent references and collection field graphs.
use super::{check::Checker, model::Base};

impl Checker<'_> {
    pub(super) fn varip_type(&self, base: &Base) -> bool {
        match base {
            // A persistent object reference does not persist its ordinary fields.
            Base::UserType(_) => true,
            Base::Array(_) | Base::Map(_, _) => self.varip_graph(base, &mut Vec::new()),
            Base::Handle("enum") => true,
            _ => self.varip_graph(base, &mut Vec::new()),
        }
    }

    fn varip_graph(&self, base: &Base, visited: &mut Vec<String>) -> bool {
        match base {
            Base::Int
            | Base::Float
            | Base::Bool
            | Base::Color
            | Base::String
            | Base::Na
            | Base::Unknown => true,
            Base::Array(value) => self.varip_graph(value, visited),
            Base::Map(key, value) => {
                self.varip_graph(key, visited) && self.varip_graph(value, visited)
            }
            Base::UserType(name) => {
                if visited.contains(name) {
                    return true;
                }
                visited.push(name.clone());
                self.ast
                    .user_types
                    .iter()
                    .find(|ty| &ty.name == name)
                    .is_some_and(|ty| {
                        ty.fields.iter().all(|field| {
                            self.annotation(std::slice::from_ref(&field.type_name))
                                .ok()
                                .flatten()
                                .is_some_and(|(base, _)| self.varip_graph(&base, visited))
                        })
                    })
            }
            _ => false,
        }
    }

    pub(super) fn validate_varip_fields(&mut self) {
        let invalid: Vec<_> = self
            .ast
            .user_types
            .iter()
            .flat_map(|ty| &ty.fields)
            .filter(|field| field.varip)
            .filter(|field| {
                self.annotation(std::slice::from_ref(&field.type_name))
                    .ok()
                    .flatten()
                    .is_some_and(|(base, _)| !self.varip_type(&base))
            })
            .map(|field| field.default)
            .collect();
        for id in invalid {
            self.error(
                id,
                "varip field requires a supported value or collection field graph",
            );
        }
    }
}
