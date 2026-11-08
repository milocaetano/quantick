//! Load-time inference, independent of runtime values and commit/preview state.
use super::model::{Base, Qualifier, Type, accepts, annotation, common};
use std::collections::BTreeMap;

use crate::ast::{Ast, BinOp, NodeId, NodeKind, UnOp};
use crate::error::{ErrorCode, PineError};
use crate::ir::{FunctionInfo, Resolution};

pub(crate) fn validate(
    version: crate::version::Version,
    ast: &mut Ast,
    root: NodeId,
    resolutions: &mut [Resolution],
    functions: &[FunctionInfo],
    globals: usize,
    errors: &mut Vec<PineError>,
) -> super::Checked {
    let node_count = ast.len();
    let mut checker = Checker {
        version,
        ast,
        resolutions,
        functions,
        globals: vec![Type::unknown(); globals],
        locals: Vec::new(),
        errors: Vec::new(),
        coercions: vec![0; node_count],
        integer_divisions: vec![false; node_count],
        control: Qualifier::Const,
        stack: Vec::new(),
        context: 0,
        contexts: vec![(vec![0; node_count], vec![false; node_count])],
        dispatch: BTreeMap::new(),
    };
    // Qualifiers only rise. Revisit earlier reads after later assignments until
    // stable, including assignments guarded by a series condition.
    for _ in 0..=globals.saturating_mul(3).saturating_add(3) {
        let before = checker.globals.clone();
        checker.errors.clear();
        checker.infer(root);
        if before == checker.globals {
            break;
        }
    }
    errors.extend(checker.errors);
    checker.contexts[0] = (checker.coercions, checker.integer_divisions);
    let (coercions, integer_divisions) = checker.contexts.into_iter().unzip();
    super::Checked {
        version,
        coercions,
        integer_divisions,
        dispatch: checker.dispatch,
    }
}
pub(crate) struct Checker<'a> {
    pub version: crate::version::Version,
    pub ast: &'a mut Ast,
    pub resolutions: &'a mut [Resolution],
    pub functions: &'a [FunctionInfo],
    globals: Vec<Type>,
    locals: Vec<Type>,
    errors: Vec<PineError>,
    pub coercions: Vec<u8>,
    integer_divisions: Vec<bool>,
    control: Qualifier,
    stack: Vec<u32>,
    context: u32,
    contexts: Vec<(Vec<u8>, Vec<bool>)>,
    dispatch: crate::ir::FunctionDispatch,
}
impl Checker<'_> {
    pub fn error(&mut self, id: NodeId, message: impl Into<String>) {
        let error = PineError::new(ErrorCode::PineType, self.ast.node(id).span, message);
        if !self
            .errors
            .iter()
            .any(|old| old.span == error.span && old.message == error.message)
        {
            self.errors.push(error);
        }
    }
    fn read(&self, resolution: Resolution) -> Type {
        match resolution {
            Resolution::Global(i) => self
                .globals
                .get(i as usize)
                .cloned()
                .unwrap_or_else(Type::unknown),
            Resolution::Local(i) => self
                .locals
                .get(i as usize)
                .cloned()
                .unwrap_or_else(Type::unknown),
            Resolution::ColorConst(_) => Type::new(Base::Color, Qualifier::Const),
            Resolution::EnumConst(_) => Type::new(Base::Handle("enum"), Qualifier::Const),
            Resolution::Builtin(builtin) => super::calls::variable(builtin),
            _ => Type::unknown(),
        }
    }
    fn write(&mut self, resolution: Resolution, ty: Type) {
        let slot = match resolution {
            Resolution::Global(i) => self.globals.get_mut(i as usize),
            Resolution::Local(i) => self.locals.get_mut(i as usize),
            _ => None,
        };
        if let Some(slot) = slot {
            *slot = ty;
        }
    }
    fn annotated(&mut self, id: NodeId, value: NodeId, actual: Type, words: &[String]) -> Type {
        match annotation(words) {
            Ok(Some((base, qualifier))) => {
                if !self.accepts(&base, &actual.base) {
                    self.error(id, format!("expected {base:?}, found {:?}", actual.base));
                }
                if qualifier.is_some_and(|q| actual.qualifier > q) {
                    self.error(id, "value is stronger than its declared qualifier");
                }
                self.coercions[value.index()] = match base {
                    Base::Bool => self.version.bool_coercion(),
                    Base::Color => 2,
                    Base::String => 3,
                    _ => 0,
                };
                Type::new(base, qualifier.unwrap_or(actual.qualifier))
            }
            Ok(None) => actual,
            Err(message) => {
                self.error(id, message);
                actual
            }
        }
    }
    fn condition(&mut self, id: NodeId) -> Type {
        let ty = self.infer(id);
        if !self.valid_condition(&ty) {
            self.error(id, "expected a boolean or numeric condition");
        }
        ty
    }
    fn under(&mut self, id: NodeId, q: Qualifier) -> Type {
        let saved = self.control;
        self.control = self.control.max(q);
        let ty = self.infer(id).lift(self.control);
        self.control = saved;
        ty
    }
    fn join(&mut self, id: NodeId, a: Type, b: Type) -> Type {
        common(&a, &b).unwrap_or_else(|| {
            self.error(
                id,
                format!("incompatible types {:?} and {:?}", a.base, b.base),
            );
            Type::unknown()
        })
    }
    pub fn infer(&mut self, id: NodeId) -> Type {
        let ty = self.infer_value(id);
        // Preserve scalar identity when a branch, empty loop or history gap
        // returns a generic missing value. Context tables keep calls separate.
        match ty.base {
            Base::Bool => self.coercions[id.index()] = self.version.bool_coercion(),
            Base::Color => self.coercions[id.index()] = 2,
            Base::String => self.coercions[id.index()] = 3,
            _ => {}
        }
        ty
    }
    fn infer_value(&mut self, id: NodeId) -> Type {
        let node = self.ast.node(id).kind.clone();
        match node {
            NodeKind::Int(_) => Type::new(Base::Int, Qualifier::Const),
            NodeKind::Float(_) => Type::new(Base::Float, Qualifier::Const),
            NodeKind::Bool(_) => Type::new(Base::Bool, Qualifier::Const),
            NodeKind::Str(_) => Type::new(Base::String, Qualifier::Const),
            NodeKind::Color(_) => Type::new(Base::Color, Qualifier::Const),
            NodeKind::Na => Type::new(Base::Na, Qualifier::Const),
            NodeKind::Name(_) | NodeKind::Member { .. } => {
                let ty = self.read(self.resolutions[id.index()]);
                if ty.base == Base::Unknown
                    && matches!(
                        self.resolutions[id.index()],
                        Resolution::Builtin(_) | Resolution::Function(_)
                    )
                {
                    self.error(id, "function names require a call");
                }
                ty
            }
            NodeKind::Script { statements } | NodeKind::Block { statements } => {
                let mut last = Type::new(Base::Na, Qualifier::Const);
                for statement in statements {
                    last = self.infer(statement);
                }
                last
            }
            NodeKind::ExprStmt { expr } => self.infer(expr),
            other => self.infer_statement(id, other),
        }
    }
    fn infer_statement(&mut self, id: NodeId, node: NodeKind) -> Type {
        match node {
            NodeKind::Declare { value, .. } => {
                let mut actual = self.infer(value);
                let previous = self.read(self.resolutions[id.index()]);
                if actual.base == Base::Na
                    && previous.base != Base::Unknown
                    && !(self.version.two_state_bool()
                        && matches!(previous.base, Base::Bool | Base::Handle("enum")))
                {
                    actual.base = previous.base;
                }
                let words = self
                    .ast
                    .annotations(id)
                    .first()
                    .cloned()
                    .unwrap_or_default();
                if words.is_empty() && actual.base == Base::Na {
                    self.error(id, "a declaration initialized with na requires a type");
                }
                let mut ty = self.annotated(id, value, actual, &words);
                ty.qualifier = ty
                    .qualifier
                    .max(self.read(self.resolutions[id.index()]).qualifier);
                if let Ok(Some((_, Some(limit)))) = annotation(&words)
                    && ty.qualifier > limit
                {
                    self.error(id, "reassignment exceeds the declared qualifier");
                }
                self.write(self.resolutions[id.index()], ty.clone());
                ty
            }
            NodeKind::Reassign { value, .. } => {
                let actual = self.infer(value).lift(Qualifier::Series).lift(self.control);
                let mut old = self.read(self.resolutions[id.index()]);
                if matches!(old.base, Base::Na | Base::Unknown) {
                    old.base = actual.base.clone();
                }
                if !self.accepts(&old.base, &actual.base) {
                    self.error(id, "reassignment changes the variable's type");
                }
                self.coercions[value.index()] = match old.base {
                    Base::Bool => self.version.bool_coercion(),
                    Base::Color => 2,
                    Base::String => 3,
                    _ => 0,
                };
                let ty = Type::new(old.base, old.qualifier.max(actual.qualifier));
                self.write(self.resolutions[id.index()], ty.clone());
                ty
            }
            NodeKind::Tuple { items } => {
                let items: Vec<_> = items.into_iter().map(|n| self.infer(n)).collect();
                let qualifier = items
                    .iter()
                    .fold(Qualifier::Const, |q, t| q.max(t.qualifier));
                Type::new(Base::Tuple(items), qualifier).lift(qualifier)
            }
            NodeKind::TupleDeclare { names, value } => {
                let ty = self.infer(value);
                if let Base::Tuple(items) = &ty.base {
                    if names.len() != items.len() {
                        self.error(id, "tuple arity does not match declaration");
                    }
                    for (offset, item) in items.iter().enumerate() {
                        let offset = u32::try_from(offset).expect("bounded AST");
                        let slot = match self.resolutions[id.index()] {
                            Resolution::Global(i) => Resolution::Global(i + offset),
                            Resolution::Local(i) => Resolution::Local(i + offset),
                            r => r,
                        };
                        self.write(slot, item.clone());
                    }
                } else if ty.base != Base::Unknown {
                    self.error(id, "expected a tuple");
                }
                ty
            }
            NodeKind::FunctionDef { params, .. } => {
                for words in self.ast.annotations(id).to_vec() {
                    if let Err(message) = annotation(&words) {
                        self.error(id, message);
                    }
                }
                if let Resolution::Function(index) = self.resolutions[id.index()] {
                    let args: Vec<_> = params
                        .iter()
                        .map(|_| crate::ast::Arg {
                            name: None,
                            value: id,
                            span: self.ast.node(id).span,
                        })
                        .collect();
                    self.function(
                        id,
                        index,
                        &args,
                        vec![Type::unknown(); params.len()],
                        (0..params.len()).collect(),
                    );
                }
                Type::unknown()
            }
            NodeKind::Call { callee, args } => {
                self.version_arguments(callee, &args);
                if self.version.two_state_bool() {
                    let mut names = std::collections::BTreeSet::new();
                    for arg in &args {
                        if let Some(name) = &arg.name
                            && !names.insert(name)
                        {
                            self.error(arg.value, "v6 prohibits duplicate named arguments");
                        }
                    }
                }
                let args = if let Resolution::Builtin(b) = self.resolutions[callee.index()] {
                    self.ordered(id, b, args)
                } else {
                    args
                };
                let values = args.iter().map(|a| self.infer(a.value)).collect::<Vec<_>>();
                match self.resolutions[callee.index()] {
                    Resolution::Builtin(builtin) => self.builtin(id, builtin, &args, &values),
                    Resolution::Function(index) => self.overload(id, callee, index, &args, values),
                    _ => self.method(id, callee, &args, &values),
                }
            }
            other => self.infer_operator(id, other),
        }
    }
    fn infer_operator(&mut self, id: NodeId, node: NodeKind) -> Type {
        match node {
            NodeKind::History { subject, offset } => {
                if self.version.two_state_bool()
                    && matches!(
                        self.ast.node(subject).kind,
                        NodeKind::Int(_)
                            | NodeKind::Float(_)
                            | NodeKind::Bool(_)
                            | NodeKind::Str(_)
                            | NodeKind::Color(_)
                            | NodeKind::Na
                            | NodeKind::Member { .. }
                    )
                    && !matches!(self.resolutions[subject.index()], Resolution::Builtin(_))
                {
                    self.error(
                        subject,
                        "v6 cannot reference literal or built-in constant history directly",
                    );
                }
                let ty = self.infer(subject);
                let index = self.infer(offset);
                if !accepts(&Base::Int, &index.base) {
                    self.error(offset, "history offset must be int");
                }
                ty.lift(Qualifier::Series)
            }
            NodeKind::Unary { op, operand } => {
                let ty = self.infer(operand);
                match op {
                    UnOp::Not => {
                        if !self.valid_condition(&ty) {
                            self.error(id, "not requires a condition");
                        }
                        Type::new(Base::Bool, ty.qualifier)
                    }
                    UnOp::Neg => {
                        if !ty.numeric() {
                            self.error(id, "negation requires a number");
                        }
                        ty
                    }
                }
            }
            NodeKind::Binary { op, lhs, rhs } => {
                let a = self.infer(lhs);
                let b = self.infer(rhs);
                let q = a.qualifier.max(b.qualifier);
                match op {
                    BinOp::And | BinOp::Or => {
                        if !self.valid_condition(&a) || !self.valid_condition(&b) {
                            self.error(id, "logical operators require conditions");
                        }
                        Type::new(Base::Bool, q)
                    }
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        self.join(id, a.clone(), b.clone());
                        if !matches!(op, BinOp::Eq | BinOp::Ne) && (!a.numeric() || !b.numeric()) {
                            self.error(id, "ordered comparison requires numbers");
                        }
                        Type::new(Base::Bool, q)
                    }
                    BinOp::Add if a.base == Base::String && b.base == Base::String => {
                        Type::new(Base::String, q)
                    }
                    _ => {
                        if !a.numeric() || !b.numeric() {
                            self.error(id, "arithmetic requires numeric operands");
                        }
                        let mut ty = self.join(id, a, b);
                        if op == BinOp::Div && ty.base == Base::Int {
                            let truncate =
                                self.version.truncate_const_division() && q == Qualifier::Const;
                            self.integer_divisions[id.index()] = truncate;
                            if !truncate {
                                ty.base = Base::Float;
                            }
                        }
                        ty
                    }
                }
            }
            other => self.infer_flow(id, other),
        }
    }
    fn infer_flow(&mut self, id: NodeId, node: NodeKind) -> Type {
        match node {
            NodeKind::If {
                condition,
                then_block,
                otherwise,
            } => {
                let q = self.condition(condition).qualifier;
                let a = self.under(then_block, q);
                let b = otherwise.map_or_else(|| Type::new(Base::Na, q), |n| self.under(n, q));
                self.unique_branches(id, &a, &b);
                self.join(id, a, b)
            }
            NodeKind::Ternary {
                condition,
                if_true,
                if_false,
            } => {
                let q = self.condition(condition).qualifier;
                let a = self.under(if_true, q);
                let b = self.under(if_false, q);
                self.unique_branches(id, &a, &b);
                self.join(id, a, b)
            }
            NodeKind::Switch { subject, arms } => {
                let subject = subject.map(|n| self.infer(n));
                let mut q = subject.as_ref().map_or(Qualifier::Const, |t| t.qualifier);
                for (test, _) in &arms {
                    if let Some(test) = test {
                        let ty = self.infer(*test);
                        q = q.max(ty.qualifier);
                        if let Some(subject) = &subject {
                            self.join(*test, subject.clone(), ty);
                        } else if !self.valid_condition(&ty) {
                            self.error(*test, "switch arm requires a condition");
                        }
                    }
                }
                let mut missing_arm = !arms.iter().any(|(test, _)| test.is_none());
                let mut ty = Type::new(Base::Na, q);
                for (_, body) in arms {
                    let arm = self.under(body, q);
                    missing_arm |= arm.base == Base::Na;
                    ty = self.join(id, ty, arm);
                }
                if missing_arm {
                    self.unique_branches(id, &ty, &Type::new(Base::Na, q));
                }
                ty
            }
            NodeKind::For {
                from, to, by, body, ..
            } => {
                let q = Qualifier::Series;
                let mut base = Base::Int;
                for n in [Some(from), Some(to), by].into_iter().flatten() {
                    let ty = self.infer(n);
                    if ty.base == Base::Float {
                        base = Base::Float;
                    }
                    if !ty.numeric() {
                        self.error(n, "for bounds and step must be numeric");
                    }
                }
                self.write(self.resolutions[id.index()], Type::new(base, q));
                self.under(body, q)
            }
            NodeKind::While { condition, body } => {
                let q = self.condition(condition).qualifier;
                self.under(body, q)
            }
            _ => unreachable!("all expression families dispatched"),
        }
    }
    fn overload(
        &mut self,
        id: NodeId,
        callee: NodeId,
        index: u32,
        args: &[crate::ast::Arg],
        values: Vec<Type>,
    ) -> Type {
        if values.iter().any(|value| value.base == Base::Unknown) {
            return Type::unknown();
        }
        let name = self.functions[index as usize].name.clone();
        let mut candidates = Vec::new();
        for (i, function) in self.functions.iter().enumerate() {
            if function.name != name || args.len() != function.param_count {
                continue;
            }
            let Some(def) = (0..self.ast.len()).filter_map(|i| self.ast.id_at(i)).find(|n| matches!(&self.ast.node(*n).kind, NodeKind::FunctionDef {body,..} if *body == function.body)) else { continue; };
            if self.ast.node(def).span.start > self.ast.node(id).span.start {
                continue;
            }
            let NodeKind::FunctionDef { params, .. } = &self.ast.node(def).kind else {
                continue;
            };
            let mut order = vec![usize::MAX; args.len()];
            let mut positional = 0;
            let mut valid = true;
            let mut score = 0;
            for (argument, (arg, value)) in args.iter().zip(&values).enumerate() {
                let target = if let Some(name) = &arg.name {
                    params.iter().position(|p| p == name)
                } else {
                    let target = positional;
                    positional += 1;
                    Some(target)
                };
                let Some(target) = target.filter(|t| *t < order.len() && order[*t] == usize::MAX)
                else {
                    valid = false;
                    break;
                };
                order[target] = argument;
                let words = self
                    .ast
                    .annotations(def)
                    .get(target)
                    .map_or(&[][..], Vec::as_slice);
                match annotation(words) {
                    Ok(Some((base, qualifier))) => {
                        if !self.accepts(&base, &value.base)
                            || qualifier.is_some_and(|q| value.qualifier > q)
                        {
                            valid = false;
                            break;
                        }
                        if base != value.base {
                            score += 1;
                        }
                    }
                    Ok(None) => score += 2,
                    Err(_) => {
                        valid = false;
                        break;
                    }
                }
            }
            if valid {
                candidates.push((score, i, order));
            }
        }
        candidates.sort_by_key(|(score, i, _)| (*score, *i));
        if candidates.len() > 1 && candidates[0].0 == candidates[1].0 {
            self.error(id, "ambiguous function overload");
            return Type::unknown();
        }
        let Some((_, index, order)) = candidates.into_iter().next() else {
            self.error(id, "no matching function overload");
            return Type::unknown();
        };
        let index = u32::try_from(index).expect("bounded functions");
        self.resolutions[callee.index()] = Resolution::Function(index);
        let args: Vec<_> = order.iter().map(|i| args[*i].clone()).collect();
        let values = order.iter().map(|i| values[*i].clone()).collect();
        self.function(id, index, &args, values, order)
    }
    fn function(
        &mut self,
        id: NodeId,
        index: u32,
        args: &[crate::ast::Arg],
        values: Vec<Type>,
        order: Vec<usize>,
    ) -> Type {
        if self.stack.contains(&index) {
            return Type::unknown();
        }
        let function = &self.functions[index as usize];
        if args.len() != function.param_count {
            self.error(id, "wrong number of function arguments");
            return Type::unknown();
        }
        let definition = self
            .ast
            .all()
            .find_map(|n| {
                if let NodeKind::FunctionDef { name, params, body } = &n.kind {
                    (name == &function.name && *body == function.body).then_some(params.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let def_id = (0..self.ast.len()).find_map(|i| self.ast.id_at(i).filter(|n| matches!(&self.ast.node(*n).kind, NodeKind::FunctionDef { body, .. } if *body == function.body)));
        let annotations = def_id
            .map(|n| self.ast.annotations(n).to_vec())
            .unwrap_or_default();
        // Bound metadata even for generated branching call graphs.
        const MAX_TYPE_CELLS: usize = 4_000_000;
        if self
            .contexts
            .len()
            .saturating_add(1)
            .saturating_mul(self.ast.len())
            > MAX_TYPE_CELLS
        {
            self.error(id, "type specialization budget exceeded");
            return Type::unknown();
        }
        let parent_context = self.context;
        let key = (parent_context, id.index());
        let context = self
            .dispatch
            .get(&key)
            .map(|(context, _, _)| *context)
            .unwrap_or_else(|| {
                let context = u32::try_from(self.contexts.len()).expect("bounded call contexts");
                self.contexts.push((Vec::new(), Vec::new()));
                context
            });
        self.dispatch.insert(key, (context, index, order));
        self.context = context;
        let mut saved_coercions = std::mem::replace(&mut self.coercions, vec![0; self.ast.len()]);
        let saved_divisions =
            std::mem::replace(&mut self.integer_divisions, vec![false; self.ast.len()]);
        let saved = std::mem::replace(
            &mut self.locals,
            vec![Type::unknown(); function.local_slots],
        );
        for (position, value) in values.into_iter().enumerate() {
            let target = args[position]
                .name
                .as_ref()
                .and_then(|name| definition.iter().position(|p| p == name))
                .unwrap_or(position);
            if target >= function.param_count {
                continue;
            }
            let words = annotations.get(target).cloned().unwrap_or_default();
            self.locals[target] = self.annotated(
                id,
                args[position].value,
                value.lift(Qualifier::Simple),
                &words,
            );
            let argument = args[position].value.index();
            saved_coercions[argument] = self.coercions[argument];
            self.coercions[argument] = 0;
        }
        self.stack.push(index);
        let body = function.body;
        let mut result = Type::unknown();
        let error_mark = self.errors.len();
        for _ in 0..=self.locals.len().saturating_mul(3).saturating_add(3) {
            self.errors.truncate(error_mark);
            let before = self.locals.clone();
            result = self.infer(body);
            if before == self.locals {
                break;
            }
        }
        self.stack.pop();
        self.locals = saved;
        self.contexts[context as usize] = (
            std::mem::replace(&mut self.coercions, saved_coercions),
            std::mem::replace(&mut self.integer_divisions, saved_divisions),
        );
        self.context = parent_context;
        result
    }
}
