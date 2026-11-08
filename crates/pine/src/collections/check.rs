//! Collection typing is separate from scalar overload policy.
use crate::{
    ast::{Arg, NodeId},
    collections::{Element, Op},
    types::{
        check::Checker,
        model::{Base, Qualifier, Type},
    },
};
fn base(element: Element) -> Base {
    match element {
        Element::Int => Base::Int,
        Element::Float => Base::Float,
        Element::Bool => Base::Bool,
        Element::String => Base::String,
        Element::Color => Base::Color,
    }
}
fn accepts(expected: &Base, actual: &Base) -> bool {
    expected == actual
        || matches!(actual, Base::Na | Base::Unknown)
        || matches!((expected, actual), (Base::Float, Base::Int))
}
impl Checker<'_> {
    pub(crate) fn collection_method(
        &mut self,
        id: NodeId,
        op: Op,
        receiver: Type,
        args: &[Arg],
        values: &[Type],
    ) -> Type {
        if matches!(op, Op::NewArray(_) | Op::NewMap(..) | Op::From) {
            self.error(id, "collection constructors are namespace functions");
            return Type::unknown();
        }
        let names = crate::collections::arguments(op);
        let names = names.get(1..).unwrap_or(&[]);
        let mut order = vec![None; names.len()];
        let mut positional = 0;
        for (i, arg) in args.iter().enumerate() {
            let target = if let Some(name) = &arg.name {
                names.iter().position(|candidate| *candidate == name)
            } else {
                let target = positional;
                positional += 1;
                Some(target)
            };
            let Some(target) =
                target.filter(|target| *target < order.len() && order[*target].is_none())
            else {
                self.error(
                    id,
                    "unknown, duplicate or excess collection method argument",
                );
                return Type::unknown();
            };
            order[target] = Some(i);
        }
        let ordered: Vec<_> = order.iter().flatten().copied().collect();
        if order.iter().take(ordered.len()).any(Option::is_none) {
            self.error(id, "missing collection method argument");
            return Type::unknown();
        }
        self.ast.order_args(id, &ordered);
        let mut all = vec![receiver];
        all.extend(ordered.iter().map(|i| values[*i].clone()));
        self.collection(id, op, args, &all)
    }

    pub(crate) fn collection(
        &mut self,
        id: NodeId,
        op: Op,
        _args: &[Arg],
        values: &[Type],
    ) -> Type {
        match self.collection_type(op, values) {
            Ok(base) => {
                if matches!(
                    op,
                    Op::Avg
                        | Op::Median
                        | Op::Stdev
                        | Op::Variance
                        | Op::PercentileLinear
                        | Op::PercentRank
                ) && base == Base::Int
                {
                    self.coercions[id.index()] = 5;
                }
                if op == Op::Standardize && base == Base::Array(Box::new(Base::Int)) {
                    self.coercions[id.index()] = 6;
                }
                Type::new(base, Qualifier::Series)
            }
            Err(message) => {
                self.error(id, message);
                Type::unknown()
            }
        }
    }
    fn collection_type(&self, op: Op, values: &[Type]) -> Result<Base, String> {
        use Op::*;
        let arity = |min, max| {
            if (min..=max).contains(&values.len()) {
                Ok(())
            } else {
                Err(format!("collection expects {min}..={max} arguments"))
            }
        };
        let require = |index: usize, expected: &Base| {
            if values.get(index).is_some_and(|v| {
                accepts(expected, &v.base)
                    && !(self.version.two_state_bool()
                        && *expected == Base::Bool
                        && v.base == Base::Na)
            }) {
                Ok(())
            } else {
                Err(format!(
                    "collection argument {} requires {expected:?}",
                    index + 1
                ))
            }
        };
        match op {
            NewArray(element) => {
                arity(0, 2)?;
                if !values.is_empty() {
                    require(0, &Base::Int)?;
                }
                if values.len() == 2 {
                    require(1, &base(element))?;
                }
                return Ok(Base::Array(Box::new(base(element))));
            }
            NewMap(key, value) => {
                arity(0, 0)?;
                return Ok(Base::Map(Box::new(base(key)), Box::new(base(value))));
            }
            From => {
                arity(1, usize::MAX)?;
                let mut element = values[0].base.clone();
                for value in values {
                    if matches!((&element, &value.base), (Base::Int, Base::Float)) {
                        element = Base::Float;
                    }
                    if !accepts(&element, &value.base)
                        || self.version.two_state_bool()
                            && element == Base::Bool
                            && value.base == Base::Na
                    {
                        return Err("array.from requires compatible element types".into());
                    }
                }
                if !matches!(
                    element,
                    Base::Int | Base::Float | Base::String | Base::Bool | Base::Color
                ) {
                    return Err("array.from requires a scalar element type".into());
                }
                return Ok(Base::Array(Box::new(element)));
            }
            _ => {}
        }
        let first = values.first().ok_or("collection receiver is missing")?;
        let map = matches!(
            op,
            Size(true)
                | Clear(true)
                | Copy(true)
                | Get(true)
                | Remove(true)
                | Contains(true)
                | Put
                | PutAll
                | Keys
                | Values
        );
        let (key, element) = match (&first.base, map) {
            (Base::Array(element), false) => (Base::Int, element.as_ref().clone()),
            (Base::Map(key, element), true) => (key.as_ref().clone(), element.as_ref().clone()),
            (Base::Unknown, _) => return Ok(Base::Unknown),
            _ => return Err("collection receiver has the wrong type".into()),
        };
        let result = match op {
            Size(_) => {
                arity(1, 1)?;
                Base::Int
            }
            Clear(_) | Reverse => {
                arity(1, 1)?;
                Base::Na
            }
            Copy(_) => {
                arity(1, 1)?;
                first.base.clone()
            }
            Keys | Values => {
                arity(1, 1)?;
                Base::Array(Box::new(if op == Keys { key } else { element }))
            }
            Get(_) | Remove(_) => {
                arity(2, 2)?;
                require(1, &key)?;
                element
            }
            Put | Set | Insert => {
                arity(3, 3)?;
                require(1, &key)?;
                require(2, &element)?;
                if op == Put { element } else { Base::Na }
            }
            Push | Unshift => {
                arity(2, 2)?;
                require(1, &element)?;
                Base::Na
            }
            Pop | Shift | First | Last => {
                arity(1, 1)?;
                element
            }
            Concat | PutAll => {
                arity(2, 2)?;
                require(1, &first.base)?;
                if op == Concat {
                    first.base.clone()
                } else {
                    Base::Na
                }
            }
            BinarySearch | BinaryLeft | BinaryRight => {
                arity(2, 2)?;
                if !matches!(element, Base::Int | Base::Float) {
                    return Err("binary search requires a numeric array".into());
                }
                require(1, &element)?;
                Base::Int
            }
            Contains(_) | IndexOf | LastIndexOf => {
                arity(2, 2)?;
                require(1, if map { &key } else { &element })?;
                if matches!(op, Contains(_)) {
                    Base::Bool
                } else {
                    Base::Int
                }
            }
            Fill => {
                arity(2, 4)?;
                require(1, &element)?;
                for i in 2..values.len() {
                    require(i, &Base::Int)?;
                }
                Base::Na
            }
            Sum | Avg | Min | Max | Abs | Covariance | Median | Mode | PercentileLinear
            | PercentileNearest | PercentRank | Range | Standardize | Stdev | Variance => {
                if !matches!(element, Base::Int | Base::Float) {
                    return Err("numeric array required".into());
                }
                match op {
                    Min | Max => {
                        arity(1, 2)?;
                        if values.len() == 2 {
                            require(1, &Base::Int)?;
                        }
                    }
                    Stdev | Variance => {
                        arity(1, 2)?;
                        if values.len() == 2 {
                            require(1, &Base::Bool)?;
                        }
                    }
                    Covariance => {
                        arity(2, 3)?;
                        require(1, &first.base)?;
                        if values.len() == 3 {
                            require(2, &Base::Bool)?;
                        }
                    }
                    PercentileLinear | PercentileNearest => {
                        arity(2, 2)?;
                        require(1, &Base::Float)?;
                    }
                    PercentRank => {
                        arity(2, 2)?;
                        require(1, &Base::Int)?;
                    }
                    _ => arity(1, 1)?,
                }
                match op {
                    Abs | Standardize => first.base.clone(),
                    Covariance => Base::Float,
                    _ => element,
                }
            }
            Slice => {
                arity(3, 3)?;
                require(1, &Base::Int)?;
                require(2, &Base::Int)?;
                first.base.clone()
            }
            Sort | SortIndices => {
                arity(1, 2)?;
                if !matches!(element, Base::Int | Base::Float | Base::String) {
                    return Err("sorting requires a numeric or string array".into());
                }
                if values.len() == 2 {
                    require(1, &Base::Handle("enum"))?;
                }
                if op == Sort {
                    Base::Na
                } else {
                    Base::Array(Box::new(Base::Int))
                }
            }
            Join => {
                if !matches!(element, Base::Int | Base::Float | Base::String) {
                    return Err("joining requires a numeric or string array".into());
                }
                arity(1, 2)?;
                if values.len() == 2 {
                    require(1, &Base::String)?;
                }
                Base::String
            }
            _ => unreachable!("constructors handled above"),
        };
        Ok(result)
    }
}
