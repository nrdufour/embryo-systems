//! Kinds and types, the checker's second layer: operators get operands of
//! the right types, values go where they fit, conditions are boolean, the
//! a.d.t. statements get the right kinds, and fuzziness goes only where it
//! is declared (page 152). docs/adt81.md, "Kinds and types", gives the
//! rules.
//!
//! Runs on the tables of [`names`](super::names), and reports only what
//! it can prove: an unresolved name, an object's attribute, an unknown
//! functor's result are [`Ty::Unknown`], which fits anywhere.

use super::ast::*;
use super::builtins::{self, Gives, Takes};
use super::diagnostic::Diagnostic;
use super::names::{Resolution, Scope, SymbolKind};
use super::print::op_text;
use super::types::{AdtKind, Fuzz, Ty, Typed};

pub fn check_types(program: &Program, resolution: &Resolution) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (module, scope) in program.modules.iter().zip(&resolution.scopes) {
        let mut c = Checker {
            program,
            resolution,
            module,
            scope,
            diagnostics: &mut diagnostics,
        };
        c.module();
    }
    diagnostics.sort_by_key(|d| (d.span.line, d.span.column));
    diagnostics
}

struct Checker<'c> {
    program: &'c Program,
    resolution: &'c Resolution,
    module: &'c Module,
    scope: &'c Scope,
    diagnostics: &'c mut Vec<Diagnostic>,
}

/// What a called name stands for.
enum Callee<'c> {
    Module(&'c Module),
    External(&'c ExternalKind),
    Unknown,
}

fn returns_of_module(m: &Module) -> Typed {
    match &m.kind {
        ModuleKind::Function { returns } => Typed::of_scalar(returns),
        ModuleKind::Functor { returns, .. } => Typed::of(returns),
        _ => Typed::UNKNOWN,
    }
}

fn ordered(t: &Ty) -> bool {
    matches!(
        t,
        Ty::Unknown | Ty::Integer | Ty::Rational | Ty::Float | Ty::Character | Ty::String
    )
}

fn text(t: &Ty) -> bool {
    matches!(t, Ty::Unknown | Ty::Character | Ty::String)
}

fn list_element(t: &Ty) -> Option<Ty> {
    match t {
        Ty::List(e) => Some((**e).clone()),
        Ty::Unknown => Some(Ty::Unknown),
        _ => None,
    }
}

impl<'c> Checker<'c> {
    fn error(&mut self, span: Span, message: String) {
        self.diagnostics.push(Diagnostic::error(span, message));
    }

    /// The declared type of a name; Unknown if it is not a variable.
    fn symbol(&self, n: &Name) -> Typed {
        match self.scope.get(&n.key) {
            Some(s) if s.kind == SymbolKind::Result => returns_of_module(self.module),
            Some(s) => s.ty.as_ref().map_or(Typed::UNKNOWN, Typed::of),
            None => Typed::UNKNOWN,
        }
    }

    fn is_in_parameter(&self, n: &Name) -> bool {
        self.scope
            .get(&n.key)
            .is_some_and(|s| s.kind == SymbolKind::Parameter(Mode::In))
    }

    fn callee(&self, n: &Name) -> Callee<'c> {
        if let Some(s) = self.scope.get(&n.key) {
            if let SymbolKind::External(_) = s.kind {
                let external = self.module.externals.iter().find(|e| e.names.contains(n));
                return external.map_or(Callee::Unknown, |e| Callee::External(&e.kind));
            }
            return Callee::Unknown;
        }
        match self.resolution.modules.get(&n.key) {
            Some(&i) => Callee::Module(&self.program.modules[i]),
            None => Callee::Unknown,
        }
    }

    fn returns(&self, callee: &Callee) -> Typed {
        match callee {
            Callee::Module(m) => returns_of_module(m),
            Callee::External(ExternalKind::Function(t)) => Typed::of_scalar(t),
            Callee::External(ExternalKind::Functor(t)) => Typed::of(t),
            _ => Typed::UNKNOWN,
        }
    }

    // Where values go

    /// `value` goes into `what`, of type `target`: an assignment, a result,
    /// a parameter.
    fn flows(&mut self, value: &Typed, target: &Typed, what: &str, span: Span) {
        if !value.ty.widens_to(&target.ty) {
            let message = format!(
                "cannot assign {} to {what}, {}",
                value.ty.a(),
                target.ty.a()
            );
            self.error(span, message);
            return;
        }
        let beyond = value.fuzz.beyond(target.fuzz);
        if !beyond.is_empty() {
            let message = format!(
                "{what} is not declared {}, yet receives such a value (page 152)",
                beyond.join(" ")
            );
            self.error(span, message);
        }
    }

    fn expect(&mut self, t: &Typed, want: &Ty, what: &str, span: Span) {
        if !t.ty.widens_to(want) {
            let message = format!("{what} is {}, not {}", t.ty.a(), want.a());
            self.error(span, message);
        }
    }

    fn condition(&mut self, e: &Expr) {
        let t = self.expr(e);
        self.expect(&t, &Ty::Boolean, "the condition", e.span);
    }

    fn number(&mut self, e: &Expr, what: &str) {
        let t = self.expr(e);
        if !t.ty.is_numeric() && t.ty != Ty::Unknown {
            let message = format!("{what} is {}, not a number", t.ty.a());
            self.error(e.span, message);
        }
    }

    // Modules and statements

    fn module(&mut self) {
        if let ModuleKind::Functor {
            prefix: Some(p), ..
        } = &self.module.kind
        {
            let t = self.symbol(p).ty;
            if t != Ty::Unknown && t.adt_kind().is_none() && !matches!(t, Ty::List(_)) {
                let message = format!(
                    "the functor's prefix `{}` is {}, not an a.d.t. or a list",
                    p.spelling,
                    t.a()
                );
                self.error(p.span, message);
            }
        }
        self.stmts(&self.module.body);
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn assign_to(&mut self, target: &Variable, value: &Typed, span: Span) {
        let t = self.variable(target);
        if self.is_in_parameter(&target.name) {
            let message = format!(
                "`{}` is an IN parameter: it cannot be assigned",
                target.name.spelling
            );
            self.error(target.name.span, message);
            return;
        }
        let what = format!("`{}`", target.name.spelling);
        self.flows(value, &t, &what, span);
    }

    fn range(&mut self, r: &Range) {
        let t = self.symbol(&r.name);
        if !t.ty.is_numeric() && t.ty != Ty::Unknown {
            let message = format!(
                "the loop variable `{}` is {}, not a number",
                r.name.spelling,
                t.ty.a()
            );
            self.error(r.name.span, message);
            return;
        }
        let what = format!("`{}`", r.name.spelling);
        for e in [Some(&r.from), Some(&r.to), r.step.as_ref()]
            .into_iter()
            .flatten()
        {
            let v = self.expr(e);
            if !v.ty.widens_to(&t.ty) {
                let message = format!("cannot assign {} to {what}, {}", v.ty.a(), t.ty.a());
                self.error(e.span, message);
            }
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Assign { target, value } => {
                let v = self.expr(value);
                self.assign_to(target, &v, value.span);
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
                if let Some(c) = condition {
                    self.condition(c);
                }
                self.stmts(body);
                if let Some(u) = until {
                    self.condition(u);
                }
            }
            StmtKind::Each {
                name,
                of,
                filter,
                body,
            } => {
                if let Some(of) = of {
                    let list = self.expr(of);
                    match list_element(&list.ty) {
                        Some(e) => {
                            let var = self.symbol(name);
                            if !e.widens_to(&var.ty) {
                                let message = format!(
                                    "`{}` is {}, and goes over a list of {e}",
                                    name.spelling,
                                    var.ty.a()
                                );
                                self.error(name.span, message);
                            }
                        }
                        None => {
                            let message = format!("EACH goes over a list, not {}", list.ty.a());
                            self.error(of.span, message);
                        }
                    }
                }
                if let Some(f) = filter {
                    self.condition(f);
                }
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
                self.condition(condition);
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
                let t = self.expr(subject);
                for arm in arms {
                    for v in &arm.values {
                        let vt = self.expr(v);
                        self.comparable(&t, &vt, BinaryOp::Eq, v.span);
                    }
                    self.stmts(&arm.body);
                }
                if let Some(o) = others {
                    self.stmts(o);
                }
            }
            StmtKind::Exit { count, .. } => {
                if let Some(c) = count {
                    let t = self.expr(c);
                    self.expect(&t, &Ty::Integer, "the number of loops", c.span);
                }
            }
            StmtKind::Return
            | StmtKind::Reproc
            | StmtKind::Pause
            | StmtKind::Stop
            | StmtKind::Restart
            | StmtKind::Passivate
            | StmtKind::Terminate
            | StmtKind::Chain(_) => {}
            StmtKind::Reset(vs) => {
                for v in vs {
                    self.variable(v);
                }
            }
            StmtKind::Read(vs) => {
                for v in vs {
                    self.assign_to(v, &Typed::UNKNOWN, v.name.span);
                }
            }
            StmtKind::Print(es) => {
                for e in es {
                    self.expr(e);
                }
            }
            StmtKind::Result(e) => {
                let v = self.expr(e);
                match &self.module.kind {
                    ModuleKind::Function { returns } => {
                        let target = Typed::of_scalar(returns);
                        self.flows(&v, &target, "the function's result", e.span);
                    }
                    _ => self.error(s.span, "RESULT is for functions only".into()),
                }
            }
            StmtKind::Insert {
                items,
                list,
                position,
            } => {
                for i in items {
                    self.expr(i);
                }
                self.list(list);
                if let Some((_, e)) = position {
                    self.expr(e);
                }
            }
            StmtKind::Extract { items, list } => {
                for i in items {
                    self.expr(i);
                }
                self.list(list);
            }
            StmtKind::Empty(list) => self.list(list),
            StmtKind::Organize { lists, .. } => {
                for l in lists {
                    self.list(l);
                }
            }
            StmtKind::Create(o) => {
                if let Operands::Designations(ds) = o {
                    self.homogeneous(ds);
                } else {
                    self.operands(o);
                }
            }
            StmtKind::StateChange(_, o) => self.operands(o),
            StmtKind::Design(o) => self.operands(o),
            StmtKind::Let { target, value } => {
                match target {
                    LetTarget::Variable(_) => {}
                    LetTarget::Attribute { object, .. } => self.object(object),
                    LetTarget::Property { adt, .. } => {
                        self.adt(adt);
                    }
                }
                self.expr(value);
            }
            StmtKind::Assert(Assertion::Link { from, to, .. }) => {
                self.object(from);
                self.object(to);
            }
            StmtKind::Assert(Assertion::Name(_)) => {}
            StmtKind::Specify { items, .. } => {
                for item in items {
                    match item {
                        Specified::Class(c) => self.class(c),
                        Specified::Attribute(a) => self.class(&a.class),
                        Specified::Link(l) => {
                            self.class(&l.from);
                            self.class(&l.to);
                        }
                    }
                }
            }
            StmtKind::Include { classes, into } => {
                self.classes(classes);
                self.classes(into);
            }
            StmtKind::Exclude { classes, from } => {
                self.classes(classes);
                self.classes(from);
            }
            StmtKind::Compose { class, with } => {
                self.class(class);
                self.classes(with);
            }
            StmtKind::Build { classes, model } => {
                self.classes(classes);
                self.class(model);
            }
            StmtKind::Associate(pairs) => self.pairs(pairs, "ASSOC", "WITH"),
            StmtKind::Dissociate(pairs) => self.pairs(pairs, "DISSOC", "FROM"),
            StmtKind::Activate(o) | StmtKind::Cancel(o) | StmtKind::Kill(o) => self.object(o),
            StmtKind::Reactivate { object, when } => {
                self.object(object);
                match when {
                    None => {}
                    Some(When::At(e)) => self.number(e, "the time"),
                    Some(When::Before(o) | When::After(o)) => self.object(o),
                }
            }
            StmtKind::Delay { object, duration } | StmtKind::Hold { object, duration } => {
                self.object(object);
                self.number(duration, "the duration");
            }
            StmtKind::Call(c) | StmtKind::Resume(c) => self.args(c),
        }
    }

    /// A name that must be a list variable.
    fn list(&mut self, n: &Name) {
        let t = self.symbol(n);
        if list_element(&t.ty).is_none() {
            let message = format!("`{}` is {}, not a list", n.spelling, t.ty.a());
            self.error(n.span, message);
        }
    }

    /// Arguments against a module of this file's parameters: their number,
    /// their types, and a variable for each OUT or INOUT.
    fn args(&mut self, c: &Call) {
        let types: Vec<Typed> = c.args.iter().map(|a| self.expr(a)).collect();
        let Callee::Module(m) = self.callee(&c.name) else {
            return;
        };
        if m.parameters.len() != c.args.len() {
            let message = format!(
                "`{}` takes {} argument(s), not {}",
                c.name.spelling,
                m.parameters.len(),
                c.args.len()
            );
            self.error(c.name.span, message);
            return;
        }
        for (i, ((p, arg), value)) in m.parameters.iter().zip(&c.args).zip(&types).enumerate() {
            let Some(spec) = m.specifications.iter().find(|s| s.names.contains(p)) else {
                continue;
            };
            let param = Typed::of(&spec.ty);
            let what = format!("argument {} of `{}`", i + 1, c.name.spelling);
            match spec.mode {
                Mode::In => self.flows(value, &param, &what, arg.span),
                mode => {
                    if !matches!(arg.kind, ExprKind::Variable(_)) {
                        let mode = if mode == Mode::Out { "OUT" } else { "INOUT" };
                        let message = format!("{what} is {mode}: it must be a variable");
                        self.error(arg.span, message);
                        continue;
                    }
                    if mode == Mode::InOut {
                        self.flows(value, &param, &what, arg.span);
                    }
                    self.flows(&param, value, &what, arg.span);
                }
            }
        }
    }

    // A.d.t. designations

    /// The type a designation stands for: a class, an object, or a list of
    /// them when a variable holds one.
    fn designation(&mut self, d: &Designation) -> Ty {
        match d {
            Designation::This(_) => Ty::Adt(AdtType::Object),
            Designation::Classed { name, class } => {
                self.named(name);
                self.class(class);
                Ty::Adt(AdtType::Object)
            }
            Designation::Named(n) => self.named(n),
        }
    }

    /// A class, an object, or a list of them, by a name; a string names a
    /// class (page 23).
    fn named(&mut self, n: &AdtName) -> Ty {
        let (t, what, span) = match n {
            AdtName::String(_) => return Ty::Adt(AdtType::Class),
            AdtName::Name(name) => (
                self.symbol(name).ty,
                format!("`{}`", name.spelling),
                name.span,
            ),
            AdtName::Expr(e) => (self.expr(e).ty, "this name".into(), e.span),
        };
        match t {
            Ty::Unknown => Ty::Unknown,
            Ty::Character | Ty::String => Ty::Adt(AdtType::Class),
            t if t.adt_kind().is_some() => t,
            t => {
                let message = format!("{what} is {}, not an a.d.t. or its name", t.a());
                self.error(span, message);
                Ty::Unknown
            }
        }
    }

    fn kind_is(&mut self, t: &Ty, want: AdtKind, span: Span) {
        if let Some(k) = t.adt_kind()
            && (k != want || matches!(t, Ty::List(_)))
        {
            let message = format!("{} is used where {} is expected", t.a(), want.a());
            self.error(span, message);
        }
    }

    fn class(&mut self, n: &AdtName) {
        let t = self.named(n);
        self.kind_is(&t, AdtKind::Class, adt_name_span(n));
    }

    fn classes(&mut self, ns: &[AdtName]) {
        for n in ns {
            self.class(n);
        }
    }

    fn object(&mut self, d: &Designation) {
        let t = self.designation(d);
        self.kind_is(&t, AdtKind::Object, designation_span(d));
    }

    fn adt(&mut self, a: &Adt) -> Ty {
        let base = self.designation(&a.designation);
        let span = designation_span(&a.designation);
        let kind = base.adt_kind();
        let (class, object) = match &a.part {
            AdtPart::None => return base,
            AdtPart::Attribute(n) => {
                self.named_attribute(n);
                (AdtType::CAttribute, AdtType::OAttribute)
            }
            AdtPart::Link(_, to) => {
                let to_t = self.designation(to);
                if let (Some(k), Some(to_k)) = (kind, to_t.adt_kind())
                    && k != to_k
                {
                    let message = format!("a link joins {} to {}", k.a(), to_k.a());
                    self.error(designation_span(to), message);
                }
                (AdtType::CLink, AdtType::OLink)
            }
        };
        match kind {
            None => Ty::Unknown,
            Some(AdtKind::Class) => Ty::Adt(class),
            Some(AdtKind::Object) => Ty::Adt(object),
            Some(k) => {
                let message = format!("{} has no attributes or links", k.a());
                self.error(span, message);
                Ty::Unknown
            }
        }
    }

    /// An attribute's name: a string, or something giving one.
    fn named_attribute(&mut self, n: &AdtName) {
        let (t, span) = match n {
            AdtName::String(_) => return,
            AdtName::Name(name) => (self.symbol(name).ty, name.span),
            AdtName::Expr(e) => (self.expr(e).ty, e.span),
        };
        if !text(&t) && t.adt_kind() != Some(AdtKind::ClassAttribute) {
            let message = format!("an attribute is named by a string, not {}", t.a());
            self.error(span, message);
        }
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
                    self.class(&a.class);
                    self.named_attribute(&a.attribute);
                }
            }
            Operands::Links(links) => {
                for l in links {
                    self.class(&l.link.from);
                    self.class(&l.link.to);
                }
            }
        }
    }

    /// CREATE's designations: all classes or all objects.
    fn homogeneous(&mut self, ds: &[Designation]) {
        let mut first = None;
        for d in ds {
            let t = self.designation(d);
            let Some(k) = t.adt_kind() else { continue };
            match first {
                None => first = Some(k),
                Some(f) if f != k => {
                    let message = format!("CREATE mixes {} and {}", f.plural(), k.plural());
                    self.error(designation_span(d), message);
                }
                Some(_) => {}
            }
        }
    }

    /// ASSOC and DISSOC pairs, page 24: a class's attribute or link with a
    /// class, an object's with an object, an object with a class.
    fn pairs(&mut self, pairs: &[(Adt, Designation)], statement: &str, word: &str) {
        use AdtKind::*;
        for (a, d) in pairs {
            let left = self.adt(a).adt_kind();
            let right = self.designation(d).adt_kind();
            let (Some(l), Some(r)) = (left, right) else {
                continue;
            };
            let legal = matches!(
                (l, r),
                (ClassAttribute | ClassLink, Class)
                    | (ObjectAttribute | ObjectLink, Object)
                    | (Object, Class)
            );
            if !legal {
                let message = format!("{statement} cannot pair {} {word} {}", l.a(), r.a());
                self.error(designation_span(&a.designation), message);
            }
        }
    }

    // Expressions

    fn expr(&mut self, e: &Expr) -> Typed {
        match &e.kind {
            ExprKind::Number(n) => number(n),
            ExprKind::Multiple(values) => {
                let mut ty = Ty::Integer;
                let mut fuzz = Fuzz {
                    mult: true,
                    ..Fuzz::NONE
                };
                for (_, v) in values {
                    let t = number(v);
                    ty = ty.join(&t.ty).unwrap_or(Ty::Unknown);
                    fuzz = fuzz.union(t.fuzz);
                }
                Typed { ty, fuzz }
            }
            ExprKind::String(s) if s.chars().count() == 1 => Typed::plain(Ty::Character),
            ExprKind::String(_) => Typed::plain(Ty::String),
            ExprKind::Boolean(_) => Typed::plain(Ty::Boolean),
            ExprKind::Variable(v) => self.variable(v),
            ExprKind::Call(c) => {
                self.args(c);
                let callee = self.callee(&c.name);
                self.returns(&callee)
            }
            ExprKind::Unary(op, operand) => {
                let t = self.expr(operand);
                match op {
                    UnaryOp::Not => {
                        self.expect(&t, &Ty::Boolean, "NOT's operand", operand.span);
                        Typed {
                            ty: Ty::Boolean,
                            fuzz: t.fuzz,
                        }
                    }
                    _ => {
                        if !t.ty.is_numeric() && t.ty != Ty::Unknown {
                            let message = format!("a sign applies to a number, not {}", t.ty.a());
                            self.error(operand.span, message);
                        }
                        t
                    }
                }
            }
            ExprKind::Binary(op, left, right) => self.binary(*op, left, right),
            ExprKind::Functor(adt, calls) => {
                let mut t = self.adt(adt);
                for c in calls {
                    t = self.apply(&t, c);
                }
                Typed::plain(t)
            }
            ExprKind::Attribute(object, attribute) => {
                self.object(object);
                self.named_attribute(attribute);
                Typed::UNKNOWN
            }
            ExprKind::Link(from, _, to) => {
                self.object(from);
                self.object(to);
                Typed::plain(Ty::Boolean)
            }
        }
    }

    fn variable(&mut self, v: &Variable) -> Typed {
        let t = self.symbol(&v.name);
        if v.indices.is_empty() {
            return t;
        }
        for i in &v.indices {
            let it = self.expr(i);
            self.expect(&it, &Ty::Integer, "an index", i.span);
        }
        match t.ty {
            Ty::Unknown => Typed::UNKNOWN,
            Ty::Array(rank, element) => {
                let n = v.indices.len();
                let fits = match rank {
                    Rank::Vector => n == 1,
                    Rank::Matrix => n == 2,
                    Rank::Tensor => n >= 3,
                };
                if !fits {
                    let wants = match rank {
                        Rank::Vector => "a vector takes 1 index",
                        Rank::Matrix => "a matrix takes 2 indices",
                        Rank::Tensor => "a tensor takes 3 indices or more",
                    };
                    let message = format!("`{}` has {n} index(es): {wants}", v.name.spelling);
                    self.error(v.name.span, message);
                }
                Typed {
                    ty: *element,
                    fuzz: t.fuzz,
                }
            }
            ty => {
                let message = format!("`{}` is {}, not an array", v.name.spelling, ty.a());
                self.error(v.name.span, message);
                Typed::UNKNOWN
            }
        }
    }

    /// A functor applied to a prefix of type `prefix`.
    fn apply(&mut self, prefix: &Ty, c: &Call) -> Ty {
        for a in &c.args {
            self.expr(a);
        }
        let callee = self.callee(&c.name);
        if !matches!(callee, Callee::Unknown) || self.scope.contains_key(&c.name.key) {
            return self.returns(&callee).ty;
        }
        let Some(f) = builtins::functor(&c.name.key) else {
            return Ty::Unknown;
        };
        let span = c.name.span;
        let applies = match f.takes {
            Takes::Any => true,
            Takes::Lists => list_element(prefix).is_some(),
            takes => {
                let want = match takes {
                    Takes::Classes => AdtKind::Class,
                    Takes::ClassAttributes => AdtKind::ClassAttribute,
                    Takes::ClassLinks => AdtKind::ClassLink,
                    Takes::Objects => AdtKind::Object,
                    Takes::ObjectAttributes => AdtKind::ObjectAttribute,
                    _ => AdtKind::ObjectLink,
                };
                match prefix.adt_kind() {
                    Some(k) => k == want,
                    None => {
                        matches!(prefix, Ty::Unknown) || *prefix == Ty::List(Box::new(Ty::Unknown))
                    }
                }
            }
        };
        if !applies {
            let wants = match f.takes {
                Takes::Classes => "classes",
                Takes::ClassAttributes => "class attributes",
                Takes::ClassLinks => "class links",
                Takes::Objects => "objects",
                Takes::ObjectAttributes => "object attributes",
                Takes::ObjectLinks => "object links",
                _ => "a list",
            };
            let message = format!(
                "`{}` applies to {wants} (page {}), not to {}",
                c.name.spelling,
                f.page,
                prefix.a()
            );
            self.error(span, message);
            return Ty::Unknown;
        }
        match f.gives {
            Gives::ListOf(t) => Ty::List(Box::new(Ty::Adt(t))),
            Gives::Integer => Ty::Integer,
            Gives::Element => list_element(prefix).unwrap_or(Ty::Unknown),
            Gives::Same => prefix.clone(),
            Gives::Unknown => Ty::Unknown,
        }
    }

    /// `=` and `/=` take any two values that compare; the others, ordered
    /// ones, never complex numbers (page 27).
    fn comparable(&mut self, l: &Typed, r: &Typed, op: BinaryOp, span: Span) {
        let (a, b) = (&l.ty, &r.ty);
        if *a == Ty::Unknown || *b == Ty::Unknown {
            return;
        }
        let equality = matches!(op, BinaryOp::Eq | BinaryOp::Ne);
        let same = (a.is_numeric() && b.is_numeric() && a.join(b).is_some())
            || (text(a) && text(b))
            || a.widens_to(b)
            || b.widens_to(a)
            || (a.adt_kind().is_some() && a.adt_kind() == b.adt_kind());
        if !same {
            let message = format!("`{}` cannot compare {} with {}", op_text(op), a.a(), b.a());
            self.error(span, message);
        } else if !equality && (!ordered(a) || !ordered(b)) {
            let unordered = if ordered(a) { b } else { a };
            let message = format!(
                "`{}` does not order {}: only = and /= compare these (page 27)",
                op_text(op),
                unordered.a()
            );
            self.error(span, message);
        }
    }

    fn binary(&mut self, op: BinaryOp, left: &Expr, right: &Expr) -> Typed {
        use BinaryOp::*;
        let l = self.expr(left);
        let r = self.expr(right);
        let fuzz = l.fuzz.union(r.fuzz);
        let word = op_text(op);
        let typed = |ty| Typed { ty, fuzz };
        // Checks that both sides satisfy `ok`, reporting the first that
        // does not.
        let both = |c: &mut Self, ok: &dyn Fn(&Ty) -> bool, takes: &str| {
            for (t, e) in [(&l, left), (&r, right)] {
                if t.ty != Ty::Unknown && !ok(&t.ty) {
                    let message = format!("`{word}` takes {takes}, not {}", t.ty.a());
                    c.error(e.span, message);
                    return false;
                }
            }
            true
        };
        match op {
            Or | Nor | Xor | And | Nand => {
                both(self, &|t| *t == Ty::Boolean, "booleans");
                typed(Ty::Boolean)
            }
            Eq | Ne | Lt | Le | Gt | Ge => {
                self.comparable(&l, &r, op, left.span);
                typed(Ty::Boolean)
            }
            In | NotIn => {
                if list_element(&r.ty).is_none() {
                    let message = format!("`{word}` looks in a list, not in {}", r.ty.a());
                    self.error(right.span, message);
                }
                typed(Ty::Boolean)
            }
            Union | Inter | Compl => {
                if !both(self, &|t| matches!(t, Ty::List(_)), "lists") {
                    return typed(Ty::Unknown);
                }
                let element = match (list_element(&l.ty), list_element(&r.ty)) {
                    (Some(a), Some(b)) => a.join(&b).unwrap_or(Ty::Unknown),
                    _ => Ty::Unknown,
                };
                typed(Ty::List(Box::new(element)))
            }
            Concat => {
                both(self, &text, "characters and strings");
                typed(Ty::String)
            }
            Add | Sub | Mul | Div => {
                if !both(self, &Ty::is_numeric, "numbers") {
                    return typed(Ty::Unknown);
                }
                if op == Div && l.ty == Ty::Integer && r.ty == Ty::Integer {
                    return typed(Ty::Integer);
                }
                typed(l.ty.join(&r.ty).unwrap_or(Ty::Unknown))
            }
            Mod | Ratio => {
                both(self, &|t| *t == Ty::Integer, "integers");
                typed(if op == Mod { Ty::Integer } else { Ty::Rational })
            }
            Pow => {
                if l.ty != Ty::Unknown && !l.ty.is_numeric() {
                    let message = format!("`^` raises a number, not {}", l.ty.a());
                    self.error(left.span, message);
                }
                if !r.ty.widens_to(&Ty::Float) {
                    let message = format!(
                        "an exponent is an integer, a rational or a float, not {}",
                        r.ty.a()
                    );
                    self.error(right.span, message);
                }
                typed(l.ty.clone())
            }
            ArrayAdd | ArrayMul | ArrayDot | ArrayPow => self.array_op(op, (&l, left), &r, fuzz),
        }
    }

    /// Page 153: `[+]` adds arrays, `[*]` multiplies matrices or an array
    /// by a scalar, `[.]` and `[^]` are the scalar and vectorial products.
    fn array_op(
        &mut self,
        op: BinaryOp,
        (l, left): (&Typed, &Expr),
        r: &Typed,
        fuzz: Fuzz,
    ) -> Typed {
        use BinaryOp::*;
        let typed = |ty| Typed { ty, fuzz };
        if l.ty == Ty::Unknown || r.ty == Ty::Unknown {
            return typed(Ty::Unknown);
        }
        let join = |a: &Ty, b: &Ty| a.join(b).unwrap_or(Ty::Unknown);
        let result = match (op, &l.ty, &r.ty) {
            (ArrayAdd, Ty::Array(a, x), Ty::Array(b, y)) if a == b => {
                Some(Ty::Array(*a, Box::new(join(x, y))))
            }
            (ArrayMul, Ty::Array(Rank::Matrix, x), Ty::Array(Rank::Matrix, y)) => {
                Some(Ty::Array(Rank::Matrix, Box::new(join(x, y))))
            }
            (ArrayMul, s, Ty::Array(rank, x)) | (ArrayMul, Ty::Array(rank, x), s)
                if s.is_numeric() =>
            {
                Some(Ty::Array(*rank, Box::new(join(s, x))))
            }
            (ArrayDot, Ty::Array(Rank::Vector, x), Ty::Array(Rank::Vector, y)) => Some(join(x, y)),
            (ArrayPow, Ty::Array(Rank::Vector, x), Ty::Array(Rank::Vector, y)) => {
                Some(Ty::Array(Rank::Vector, Box::new(join(x, y))))
            }
            _ => None,
        };
        result.map(typed).unwrap_or_else(|| {
            let takes = match op {
                ArrayAdd => "two arrays of one rank",
                ArrayMul => "two matrices, or a number and an array",
                _ => "two vectors",
            };
            let message = format!(
                "`{}` takes {takes} (page 153), not {} and {}",
                op_text(op),
                l.ty.a(),
                r.ty.a()
            );
            self.error(left.span, message);
            typed(Ty::Unknown)
        })
    }
}

/// A number literal's type and fuzziness.
fn number(n: &FuzzyNumber) -> Typed {
    let ty = match n.value {
        Number::Real(Real::Integer(_)) => Ty::Integer,
        Number::Real(Real::Float(_)) => Ty::Float,
        Number::Imaginary(Real::Integer(_), _) => Ty::Gauss,
        Number::Imaginary(Real::Float(_), Unit::I) => Ty::Complex,
        Number::Imaginary(Real::Float(_), _) => Ty::Quaternion,
    };
    Typed {
        ty,
        fuzz: Fuzz {
            appr: n.approximation.is_some(),
            prob: n.probability.is_some(),
            subj: n.opinion.is_some(),
            mult: false,
        },
    }
}

fn adt_name_span(n: &AdtName) -> Span {
    match n {
        AdtName::Name(n) => n.span,
        AdtName::String(s) => s.span,
        AdtName::Expr(e) => e.span,
    }
}

fn designation_span(d: &Designation) -> Span {
    match d {
        Designation::Named(n) | Designation::Classed { name: n, .. } => adt_name_span(n),
        Designation::This(span) => *span,
    }
}
