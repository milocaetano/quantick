//! Scalar identities and the ordered Pine qualifier lattice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Base {
    Int,
    Float,
    Bool,
    String,
    Color,
    Na,
    Unknown,
    Tuple(Vec<Type>),
    Array(Box<Base>),
    Map(Box<Base>, Box<Base>),
    Handle(&'static str),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Qualifier {
    Const,
    Input,
    Simple,
    Series,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Type {
    pub base: Base,
    pub qualifier: Qualifier,
}
impl Type {
    pub fn new(base: Base, qualifier: Qualifier) -> Self {
        Self { base, qualifier }
    }
    pub fn unknown() -> Self {
        Self::new(Base::Unknown, Qualifier::Const)
    }
    pub fn numeric(&self) -> bool {
        matches!(
            self.base,
            Base::Int | Base::Float | Base::Na | Base::Unknown
        )
    }
    pub fn condition(&self) -> bool {
        self.numeric() || self.base == Base::Bool
    }
    pub fn lift(mut self, qualifier: Qualifier) -> Self {
        self.qualifier = self.qualifier.max(qualifier);
        if let Base::Tuple(items) = &mut self.base {
            for item in items {
                *item = item.clone().lift(qualifier);
            }
        }
        self
    }
}
pub(crate) fn accepts(expected: &Base, actual: &Base) -> bool {
    expected == actual
        || matches!(actual, Base::Na | Base::Unknown)
        || matches!(
            (expected, actual),
            (Base::Float, Base::Int) | (Base::Bool, Base::Int | Base::Float)
        )
}
pub(crate) fn common(a: &Type, b: &Type) -> Option<Type> {
    let base = match (&a.base, &b.base) {
        (a, b) if a == b => a.clone(),
        (Base::Na | Base::Unknown, b) => b.clone(),
        (a, Base::Na | Base::Unknown) => a.clone(),
        (Base::Int, Base::Float) | (Base::Float, Base::Int) => Base::Float,
        (Base::Tuple(a), Base::Tuple(b)) if a.len() == b.len() => Base::Tuple(
            a.iter()
                .zip(b)
                .map(|(a, b)| common(a, b))
                .collect::<Option<Vec<_>>>()?,
        ),
        _ => return None,
    };
    Some(Type::new(base, a.qualifier.max(b.qualifier)))
}
pub(crate) fn annotation(words: &[String]) -> Result<Option<(Base, Option<Qualifier>)>, String> {
    if words.is_empty() {
        return Ok(None);
    }
    if matches!(words.first().map(String::as_str), Some("array" | "map")) {
        let element = |name: &String| {
            annotation(std::slice::from_ref(name))
                .and_then(|v| v.map(|v| v.0).ok_or("missing element type".into()))
        };
        return match words {
            [kind, value] if kind == "array" => {
                Ok(Some((Base::Array(Box::new(element(value)?)), None)))
            }
            [kind, key, value] if kind == "map" => Ok(Some((
                Base::Map(Box::new(element(key)?), Box::new(element(value)?)),
                None,
            ))),
            _ => Err("invalid collection type annotation".into()),
        };
    }
    let (qualifier, name) = match words {
        [name] => (None, name.as_str()),
        [q, name] => (
            Some(match q.as_str() {
                "const" => Qualifier::Const,
                "input" => Qualifier::Input,
                "simple" => Qualifier::Simple,
                "series" => Qualifier::Series,
                _ => return Err(format!("unknown type qualifier `{q}`")),
            }),
            name.as_str(),
        ),
        _ => return Err("expected an optional qualifier and one type".into()),
    };
    let base = match name {
        "int" => Base::Int,
        "float" => Base::Float,
        "bool" => Base::Bool,
        "string" => Base::String,
        "color" => Base::Color,
        "line" => Base::Handle("line"),
        "box" => Base::Handle("box"),
        "label" => Base::Handle("label"),
        _ => return Err(format!("unknown type `{name}`")),
    };
    Ok(Some((base, qualifier)))
}
