//! Name resolution, the checker's first layer: every name a program uses
//! is declared, and known as a variable, a parameter, a module or a
//! built-in. docs/adt81.md, "Name resolution", gives the rules.
//!
//! The modules of a file see one another; EXTERNAL brings in the others.
//! An undeclared variable is an error; an unknown functor, function or
//! property only a warning, as the polycopié's catalogue is partial (see
//! [`builtins`](super::builtins)).

use std::collections::{HashMap, HashSet};
use std::fmt;

use super::ast::*;
use super::builtins;
use super::diagnostic::Diagnostic;

/// What a module's names stand for, by folded name.
pub type Scope = HashMap<String, Symbol>;

#[derive(Debug, Clone)]
pub struct Symbol {
    pub kind: SymbolKind,
    /// Where it is declared.
    pub span: Span,
    /// The declared type, for variables and specified parameters.
    pub ty: Option<DataType>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Local,
    Parameter(Mode),
    /// A functor's own name, assigned its result (page 76).
    Result,
    External(Kind),
}

/// What kind of module a name calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Program,
    Subroutine,
    Coroutine,
    Function,
    Functor,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Kind::Program => "program",
            Kind::Subroutine => "subroutine",
            Kind::Coroutine => "coroutine",
            Kind::Function => "function",
            Kind::Functor => "functor",
        })
    }
}

impl Kind {
    fn of_module(kind: &ModuleKind) -> Kind {
        match kind {
            ModuleKind::Program => Kind::Program,
            ModuleKind::Subroutine => Kind::Subroutine,
            ModuleKind::Coroutine => Kind::Coroutine,
            ModuleKind::Function { .. } => Kind::Function,
            ModuleKind::Functor { .. } => Kind::Functor,
        }
    }

    fn of_external(kind: &ExternalKind) -> Kind {
        match kind {
            ExternalKind::Program => Kind::Program,
            ExternalKind::Subroutine => Kind::Subroutine,
            ExternalKind::Coroutine => Kind::Coroutine,
            ExternalKind::Function(_) => Kind::Function,
            ExternalKind::Functor(_) => Kind::Functor,
        }
    }
}

pub struct Resolution {
    /// One scope per module, in the program's order.
    pub scopes: Vec<Scope>,
    /// The file's modules by folded name, as indices into the program.
    pub modules: HashMap<String, usize>,
    /// Sorted by position.
    pub diagnostics: Vec<Diagnostic>,
}

pub fn resolve(program: &Program) -> Resolution {
    let mut diagnostics = Vec::new();
    let mut modules = HashMap::new();
    let mut kinds = HashMap::new();
    for (i, m) in program.modules.iter().enumerate() {
        // A computed attribute's function is reached through its
        // attribute, never called by name.
        let ModuleName::Name(name) = &m.name else {
            continue;
        };
        if modules.contains_key(&name.key) {
            diagnostics.push(Diagnostic::error(
                name.span,
                format!("module `{}` is defined twice", name.spelling),
            ));
        } else {
            modules.insert(name.key.clone(), i);
            kinds.insert(name.key.clone(), Kind::of_module(&m.kind));
        }
    }
    let scopes = program
        .modules
        .iter()
        .map(|m| {
            let mut r = Resolver {
                scope: Scope::new(),
                modules: &kinds,
                diagnostics: &mut diagnostics,
                undeclared: HashSet::new(),
            };
            r.module(m);
            r.scope
        })
        .collect();
    diagnostics.sort_by_key(|d| (d.span.line, d.span.column));
    Resolution {
        scopes,
        modules,
        diagnostics,
    }
}

struct Resolver<'r> {
    scope: Scope,
    /// The file's modules, by folded name.
    modules: &'r HashMap<String, Kind>,
    diagnostics: &'r mut Vec<Diagnostic>,
    /// Undeclared names already reported in this module.
    undeclared: HashSet<String>,
}

impl Resolver<'_> {
    fn error(&mut self, span: Span, message: String) {
        self.diagnostics.push(Diagnostic::error(span, message));
    }

    fn warning(&mut self, span: Span, message: String) {
        self.diagnostics.push(Diagnostic::warning(span, message));
    }

    fn declare(&mut self, name: &Name, kind: SymbolKind, ty: Option<DataType>) {
        if let Some(previous) = self.scope.get(&name.key) {
            let what = match previous.kind {
                SymbolKind::Parameter(_) => "a parameter",
                SymbolKind::Result => "the functor's result",
                _ => "declared",
            };
            let message = match what {
                "declared" => format!("`{}` is declared twice", name.spelling),
                what => format!("`{}` is already {what}", name.spelling),
            };
            self.error(name.span, message);
            return;
        }
        let symbol = Symbol {
            kind,
            span: name.span,
            ty,
        };
        self.scope.insert(name.key.clone(), symbol);
    }

    fn module(&mut self, m: &Module) {
        for p in &m.parameters {
            if self.scope.contains_key(&p.key) {
                let message = format!("parameter `{}` is listed twice", p.spelling);
                self.error(p.span, message);
            } else {
                self.declare(p, SymbolKind::Parameter(Mode::In), None);
            }
        }
        let mut specified = HashSet::new();
        for spec in &m.specifications {
            for n in &spec.names {
                match self.scope.get_mut(&n.key) {
                    Some(_) if !specified.insert(n.key.clone()) => {
                        let message = format!("parameter `{}` is specified twice", n.spelling);
                        self.error(n.span, message);
                    }
                    Some(symbol) => {
                        symbol.kind = SymbolKind::Parameter(spec.mode);
                        symbol.ty = Some(spec.ty.clone());
                    }
                    None => {
                        let message =
                            format!("`{}` is specified but is not a parameter", n.spelling);
                        self.error(n.span, message);
                    }
                }
            }
        }
        for p in &m.parameters {
            if !specified.contains(&p.key) {
                let message = format!("parameter `{}` has no specification", p.spelling);
                self.error(p.span, message);
            }
        }
        for d in &m.locals {
            for n in &d.names {
                self.declare(n, SymbolKind::Local, Some(d.ty.clone()));
            }
        }
        if let (ModuleKind::Functor { .. }, ModuleName::Name(name)) = (&m.kind, &m.name)
            && !self.scope.contains_key(&name.key)
        {
            self.declare(name, SymbolKind::Result, None);
        }
        for e in &m.externals {
            let kind = Kind::of_external(&e.kind);
            for n in &e.names {
                if let Some(&here) = self.modules.get(&n.key)
                    && here != kind
                {
                    let message =
                        format!("`{}` is a {here} in this file, not a {kind}", n.spelling);
                    self.error(n.span, message);
                    continue;
                }
                self.declare(n, SymbolKind::External(kind), None);
            }
        }
        // Bounds may name parameters and locals declared after them.
        let types = m.specifications.iter().map(|s| &s.ty);
        let types: Vec<&DataType> = types.chain(m.locals.iter().map(|d| &d.ty)).collect();
        for ty in types {
            self.data_type(ty);
        }
        if let ModuleKind::Functor {
            prefix: Some(prefix),
            ..
        } = &m.kind
        {
            let declared = self
                .scope
                .get(&prefix.key)
                .is_some_and(|s| matches!(s.kind, SymbolKind::Local | SymbolKind::Parameter(_)));
            if !declared {
                let message = format!("the functor's prefix `{}` is not declared", prefix.spelling);
                self.error(prefix.span, message);
            }
        }
        self.stmts(&m.body);
    }

    fn data_type(&mut self, ty: &DataType) {
        if let DataType::Array { shape, .. } = ty {
            for b in &shape.bounds {
                for end in [&b.low, &b.high] {
                    if let Bound::Name(n) = end {
                        self.variable(n);
                    }
                }
            }
        }
    }

    // Names

    /// A name used as a variable.
    fn variable(&mut self, n: &Name) {
        let kind = match self.scope.get(&n.key) {
            Some(s) => match s.kind {
                SymbolKind::External(kind) => Some(kind),
                _ => return,
            },
            None => self.modules.get(&n.key).copied(),
        };
        match kind {
            Some(kind) => {
                let message = format!("`{}` is a {kind}, not a variable", n.spelling);
                self.error(n.span, message);
            }
            None if self.undeclared.insert(n.key.clone()) => {
                let message = format!("undeclared name `{}`", n.spelling);
                self.error(n.span, message);
            }
            None => {}
        }
    }

    /// A name called as a module of kind `want`, or applied as a functor.
    fn callee(&mut self, n: &Name, want: Kind) {
        let found = match self.scope.get(&n.key) {
            Some(s) => match s.kind {
                SymbolKind::External(kind) => Some(kind),
                _ => {
                    let message = format!("`{}` is a variable, not a {want}", n.spelling);
                    self.error(n.span, message);
                    return;
                }
            },
            None => self.modules.get(&n.key).copied(),
        };
        match found {
            Some(kind) if kind == want => {}
            Some(kind) => {
                let message = format!("`{}` is a {kind}, not a {want}", n.spelling);
                self.error(n.span, message);
            }
            None if want == Kind::Functor && builtins::is_functor(&n.key) => {}
            None if want == Kind::Function && builtins::is_function(&n.key) => {}
            None if want == Kind::Subroutine && builtins::is_subroutine(&n.key) => {}
            None if matches!(want, Kind::Functor | Kind::Function) => {
                let message = format!("unknown {want} `{}`", n.spelling);
                self.warning(n.span, message);
            }
            None => {
                let message = format!("unknown {want} `{}`", n.spelling);
                self.error(n.span, message);
            }
        }
    }

    fn call(&mut self, c: &Call, want: Kind) {
        self.callee(&c.name, want);
        self.exprs(&c.args);
    }

    fn adt_name(&mut self, n: &AdtName) {
        match n {
            AdtName::Name(n) => self.variable(n),
            AdtName::String(_) => {}
            AdtName::Expr(e) => self.expr(e),
        }
    }

    fn adt_names(&mut self, names: &[AdtName]) {
        for n in names {
            self.adt_name(n);
        }
    }

    fn designation(&mut self, d: &Designation) {
        match d {
            Designation::Named(n) => self.adt_name(n),
            Designation::Classed { name, class } => {
                self.adt_name(name);
                self.adt_name(class);
            }
            Designation::This(_) => {}
        }
    }

    fn adt(&mut self, a: &Adt) {
        self.designation(&a.designation);
        match &a.part {
            AdtPart::None => {}
            AdtPart::Attribute(n) => self.adt_name(n),
            AdtPart::Link(_, to) => self.designation(to),
        }
    }

    fn class_attribute(&mut self, a: &ClassAttribute) {
        self.adt_name(&a.class);
        self.adt_name(&a.attribute);
    }

    fn class_link(&mut self, l: &ClassLink) {
        self.adt_name(&l.from);
        self.adt_name(&l.to);
    }

    fn operands(&mut self, o: &Operands) {
        match o {
            Operands::Designations(ds) => {
                for d in ds {
                    self.designation(d);
                }
            }
            Operands::Attributes(attributes) => {
                for a in attributes {
                    self.class_attribute(a);
                }
            }
            Operands::Links(links) => {
                for l in links {
                    self.class_link(&l.link);
                    if let Reverse::Named(n) = &l.reverse {
                        self.adt_name(n);
                    }
                }
            }
        }
    }

    // Expressions and statements

    fn exprs(&mut self, es: &[Expr]) {
        for e in es {
            self.expr(e);
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Number(_)
            | ExprKind::Multiple(_)
            | ExprKind::String(_)
            | ExprKind::Boolean(_) => {}
            ExprKind::Variable(v) => self.indexed(v),
            ExprKind::Call(c) => self.call(c, Kind::Function),
            ExprKind::Unary(_, operand) => self.expr(operand),
            ExprKind::Binary(_, left, right) => {
                self.expr(left);
                self.expr(right);
            }
            ExprKind::Functor(adt, calls) => {
                self.adt(adt);
                for c in calls {
                    self.call(c, Kind::Functor);
                }
            }
            ExprKind::Attribute(object, attribute) => {
                self.designation(object);
                self.adt_name(attribute);
            }
            ExprKind::Link(from, _, to) => {
                self.designation(from);
                self.designation(to);
            }
        }
    }

    fn indexed(&mut self, v: &Variable) {
        self.variable(&v.name);
        self.exprs(&v.indices);
    }

    fn range(&mut self, r: &Range) {
        self.variable(&r.name);
        self.expr(&r.from);
        self.expr(&r.to);
        if let Some(step) = &r.step {
            self.expr(step);
        }
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn opt(&mut self, e: &Option<Expr>) {
        if let Some(e) = e {
            self.expr(e);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Assign { target, value } => {
                self.indexed(target);
                self.expr(value);
            }
            StmtKind::Loop {
                range,
                condition,
                body,
                until,
            } => {
                if let Some(r) = range {
                    self.range(r);
                }
                self.opt(condition);
                self.stmts(body);
                self.opt(until);
            }
            StmtKind::Each {
                name,
                of,
                filter,
                body,
            } => {
                self.variable(name);
                self.opt(of);
                self.opt(filter);
                self.stmts(body);
            }
            StmtKind::Vect { range, body } => {
                self.range(range);
                self.stmts(body);
            }
            StmtKind::Parallel(branches) => {
                for b in branches {
                    self.stmts(b);
                }
            }
            StmtKind::If {
                condition,
                then,
                otherwise,
            } => {
                self.expr(condition);
                self.stmts(then);
                if let Some(o) = otherwise {
                    self.stmts(o);
                }
            }
            StmtKind::Case {
                subject,
                arms,
                others,
            } => {
                self.expr(subject);
                for arm in arms {
                    self.exprs(&arm.values);
                    self.stmts(&arm.body);
                }
                if let Some(o) = others {
                    self.stmts(o);
                }
            }
            StmtKind::Exit { count, .. } => self.opt(count),
            StmtKind::Return
            | StmtKind::Reproc
            | StmtKind::Pause
            | StmtKind::Stop
            | StmtKind::Restart
            | StmtKind::Passivate
            | StmtKind::Terminate => {}
            StmtKind::Reset(vs) | StmtKind::Read(vs) => {
                for v in vs {
                    self.indexed(v);
                }
            }
            StmtKind::Result(e) => self.expr(e),
            StmtKind::Insert {
                items,
                list,
                position,
            } => {
                self.exprs(items);
                self.variable(list);
                if let Some((_, e)) = position {
                    self.expr(e);
                }
            }
            StmtKind::Extract { items, list } => {
                self.exprs(items);
                self.variable(list);
            }
            StmtKind::Empty(list) => self.variable(list),
            StmtKind::Organize { lists, .. } => {
                for l in lists {
                    self.variable(l);
                }
            }
            StmtKind::Create(o) | StmtKind::StateChange(_, o) | StmtKind::Design(o) => {
                self.operands(o)
            }
            StmtKind::Let { target, value } => {
                match target {
                    LetTarget::Variable(n) => self.variable(n),
                    LetTarget::Attribute { object, attribute } => {
                        self.designation(object);
                        self.adt_name(attribute);
                    }
                    LetTarget::Property { adt, name } => {
                        self.adt(adt);
                        if !builtins::is_property(&name.key) {
                            let message = format!("unknown property `{}`", name.spelling);
                            self.warning(name.span, message);
                        }
                    }
                }
                self.expr(value);
            }
            StmtKind::Assert(Assertion::Name(n)) => self.variable(n),
            StmtKind::Assert(Assertion::Link { from, to, .. }) => {
                self.designation(from);
                self.designation(to);
            }
            StmtKind::Specify { items, .. } => {
                for item in items {
                    match item {
                        Specified::Class(c) => self.adt_name(c),
                        Specified::Attribute(a) => self.class_attribute(a),
                        Specified::Link(l) => self.class_link(l),
                    }
                }
            }
            StmtKind::Include { classes, into } => {
                self.adt_names(classes);
                self.adt_names(into);
            }
            StmtKind::Exclude { classes, from } => {
                self.adt_names(classes);
                self.adt_names(from);
            }
            StmtKind::Associate(pairs) | StmtKind::Dissociate(pairs) => {
                for (a, d) in pairs {
                    self.adt(a);
                    self.designation(d);
                }
            }
            StmtKind::Compose { class, with } => {
                self.adt_name(class);
                self.adt_names(with);
            }
            StmtKind::Build { classes, model } => {
                self.adt_names(classes);
                self.adt_name(model);
            }
            StmtKind::Activate(o) | StmtKind::Cancel(o) | StmtKind::Kill(o) => self.designation(o),
            StmtKind::Reactivate { object, when } => {
                self.designation(object);
                match when {
                    None => {}
                    Some(When::At(e)) => self.expr(e),
                    Some(When::Before(o) | When::After(o)) => self.designation(o),
                }
            }
            StmtKind::Hold { object, duration } => {
                if let Some(o) = object {
                    self.designation(o);
                }
                self.expr(duration);
            }
            StmtKind::Delay { object, duration } => {
                self.designation(object);
                self.expr(duration);
            }
            StmtKind::Chain(n) => self.callee(n, Kind::Program),
            StmtKind::Call(c) => self.call(c, Kind::Subroutine),
            StmtKind::Resume(c) => self.call(c, Kind::Coroutine),
            StmtKind::Print(es) => self.exprs(es),
        }
    }
}
