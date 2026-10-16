//! ADT'81 to A-code (docs/architecture.md, decisions 1 and 2): the
//! checked tree compiled to `hbds-acode`'s instructions, which the VM
//! runs. The compiled program does what the reference interpreter does,
//! failing where it fails, with the same message at the same place:
//! `crates/adt81/src/interp.rs` is the specification, and the gate
//! (`tests/vm.rs`) holds the two together.
//!
//! What the VM cannot run yet is [`NotYet`]: the store, the engines,
//! coroutines and processes (step 4's slices b and c).

use std::collections::HashMap;

use hbds_acode::{
    Instr, Kind, Leave, ListOp, Mode as AMode, Module as AModule, Param, Place, Slot, SlotType,
    Span as ASpan, Support, Unit,
};
use hbds_values::{Base, Computed, Fz, Op, Value};

use super::ast::*;
use super::builtins::{self, Takes};
use super::interp::{constant, input, literal, math, string};
use super::typeck::too_far;
use super::types::{Ty, Typed};
use super::value;

/// What this slice of the compiler leaves to the next, and where.
#[derive(Debug, Clone, PartialEq)]
pub struct NotYet {
    pub span: Span,
    pub what: String,
}

type Compiled<T> = Result<T, NotYet>;

fn not_yet<T>(span: Span, what: impl Into<String>) -> Compiled<T> {
    Err(NotYet {
        span,
        what: what.into(),
    })
}

fn at(s: Span) -> ASpan {
    ASpan {
        line: s.line as u32,
        column: s.column as u32,
    }
}

/// ADT'81's support module: its math functions, and READ's reading.
pub struct Adt81;

impl Support for Adt81 {
    fn native(&self, name: &str, x: &Fz) -> Computed<Fz> {
        math(name, x)
    }

    fn read(&self, line: &str) -> Result<Value, String> {
        input(line, &Ty::Unknown)
    }
}

/// The program compiled; its first PROGRAM runs first.
pub fn compile(program: &Program) -> Compiled<Unit> {
    let mut index = HashMap::new();
    for (i, m) in program.modules.iter().enumerate() {
        match &m.name {
            ModuleName::Name(n) => {
                index.entry(n.key.clone()).or_insert(i);
            }
            ModuleName::Attribute { class, .. } | ModuleName::Body(class) => {
                return not_yet(class.span, "computed attributes and process bodies");
            }
        }
    }
    let Some(first) = (program.modules.iter()).position(|m| m.kind == ModuleKind::Program) else {
        return not_yet(Span { line: 1, column: 1 }, "a file without a PROGRAM");
    };
    let mut modules = Vec::new();
    for m in &program.modules {
        modules.push(Compiler::new(program, &index, m).module()?);
    }
    // The first program first: the VM runs module 0.
    modules.swap(0, first);
    let renumber = |i: usize| match i {
        0 => first,
        i if i == first => 0,
        i => i,
    };
    for m in &mut modules {
        for i in &mut m.code {
            if let Instr::Call(callee, _) | Instr::Chain(callee) = i {
                *callee = renumber(*callee);
            }
        }
    }
    Ok(Unit { modules })
}

struct Compiler<'p> {
    program: &'p Program,
    index: &'p HashMap<String, usize>,
    module: &'p Module,
    slots: Vec<Slot>,
    names: HashMap<String, usize>,
    code: Vec<Instr>,
    spans: Vec<ASpan>,
    /// The loops around, innermost last: the ways out of each, patched
    /// when it ends, and where RELOOP starts it again.
    loops: Vec<Vec<(usize, Leave)>>,
    agains: Vec<usize>,
}

/// A target to patch: an instruction, and which of its targets.
const PENDING: usize = usize::MAX;

fn base(ty: &Ty) -> Option<Base> {
    Some(match ty {
        Ty::Integer => Base::Integer,
        Ty::Rational => Base::Rational,
        Ty::Float => Base::Float,
        Ty::Gauss => Base::Gauss,
        Ty::Complex => Base::Complex,
        Ty::Quaternion => Base::Quaternion,
        Ty::Boolean => Base::Boolean,
        Ty::Character => Base::Character,
        Ty::String => Base::String,
        _ => return None,
    })
}

fn slot_type(ty: &Ty) -> SlotType {
    match ty {
        Ty::List(_) => SlotType::List,
        Ty::Array(rank, e) => SlotType::Array(value::rank(*rank), base(e)),
        t => base(t).map_or(SlotType::Any, SlotType::Base),
    }
}

impl<'p> Compiler<'p> {
    fn new(program: &'p Program, index: &'p HashMap<String, usize>, module: &'p Module) -> Self {
        Compiler {
            program,
            index,
            module,
            slots: Vec::new(),
            names: HashMap::new(),
            code: Vec::new(),
            spans: Vec::new(),
            loops: Vec::new(),
            agains: Vec::new(),
        }
    }

    fn emit(&mut self, i: Instr, s: Span) -> usize {
        self.code.push(i);
        self.spans.push(at(s));
        self.code.len() - 1
    }

    fn here(&self) -> usize {
        self.code.len()
    }

    fn patch(&mut self, pc: usize, target: usize) {
        match &mut self.code[pc] {
            Instr::Jump(t)
            | Instr::JumpFalse(t)
            | Instr::JumpTrue(t)
            | Instr::EachNext(_, _, t) => *t = target,
            i => unreachable!("not a jump: {i:?}"),
        }
    }

    fn fail(&mut self, s: Span, message: impl Into<String>) {
        self.emit(Instr::Fail(message.into()), s);
    }

    /// A slot of the compiler's own.
    fn hidden(&mut self) -> usize {
        let n = self.slots.len();
        self.slots.push(Slot {
            name: format!("#{n}"),
            ty: SlotType::Any,
            constant: false,
        });
        n
    }

    fn declare(&mut self, n: &Name, ty: &Ty, constant: bool) -> usize {
        let s = self.slots.len();
        self.slots.push(Slot {
            name: n.spelling.clone(),
            ty: slot_type(ty),
            constant,
        });
        self.names.insert(n.key.clone(), s);
        s
    }

    fn slot(&self, n: &Name) -> Option<usize> {
        self.names.get(&n.key).copied()
    }

    fn module_named(&self, n: &Name) -> Option<(usize, &'p Module)> {
        let i = *self.index.get(&n.key)?;
        Some((i, &self.program.modules[i]))
    }

    // Modules: the prologue as `call` in the reference builds the frame.

    fn module(mut self) -> Compiled<AModule> {
        let m = self.module;
        let kind = match m.kind {
            ModuleKind::Program => Kind::Program,
            ModuleKind::Subroutine => Kind::Subroutine,
            ModuleKind::Function { .. } => Kind::Function,
            ModuleKind::Functor { .. } => Kind::Functor,
            ModuleKind::Coroutine => return not_yet(m.span, "coroutines"),
        };
        let ModuleName::Name(name) = &m.name else {
            unreachable!("only named modules reach here")
        };
        let mut params = Vec::new();
        for (i, p) in m.parameters.iter().enumerate() {
            let spec = m.specifications.iter().find(|s| s.names.contains(p));
            let ty = spec.map_or(Typed::UNKNOWN, |s| Typed::of(&s.ty));
            let mode = match spec.map_or(Mode::In, |s| s.mode) {
                Mode::In => AMode::In,
                Mode::Out => AMode::Out,
                Mode::InOut => AMode::InOut,
            };
            let s = self.declare(p, &ty.ty, false);
            params.push(Param { slot: s, mode });
            self.emit(Instr::Bind(i, s), p.span);
        }
        let mut result = None;
        if let ModuleKind::Functor { returns, .. } = &m.kind {
            result = Some(self.declare(name, &Typed::of(returns).ty, false));
        }
        for d in &m.locals {
            self.local(d)?;
        }
        let mut prefix = None;
        if let ModuleKind::Functor {
            prefix: Some(p), ..
        } = &m.kind
        {
            match self.slot(p) {
                Some(s) => {
                    prefix = Some(s);
                    self.emit(Instr::Prefix(s), p.span);
                }
                None => self.fail(p.span, format!("`{}` is not a variable here", p.spelling)),
            }
        }
        self.stmts(&m.body)?;
        let end = self.here();
        for i in 0..end {
            if self.code[i] == Instr::Jump(PENDING) {
                self.code[i] = Instr::Jump(end);
            }
        }
        match kind {
            Kind::Function => {
                self.emit(Instr::NoResult, m.span);
            }
            _ => {
                self.emit(Instr::Return, m.span);
            }
        }
        Ok(AModule {
            name: name.key.clone(),
            spelling: name.spelling.clone(),
            kind,
            span: at(m.span),
            params,
            slots: self.slots,
            prefix,
            result,
            code: self.code,
            spans: self.spans,
        })
    }

    fn local(&mut self, d: &Declaration) -> Compiled<()> {
        let ty = Typed::of(&d.ty);
        let constant = matches!(d.init, Some(Init::Constant(_)));
        let init = match &d.init {
            Some(Init::Initial(c) | Init::Constant(c)) => Some(constant_value(c)),
            None => None,
        };
        for n in &d.names {
            let s = self.declare(n, &ty.ty, constant);
            match &d.ty {
                DataType::Array { shape, element, .. } => {
                    let element = Typed::of_scalar(element).ty;
                    // The fill: the initial value as written, an array's
                    // declared type converting nothing, or zero.
                    let fill = init.clone().unwrap_or_else(|| value::zero(&element));
                    self.emit(Instr::Const(fill), n.span);
                    for b in &shape.bounds {
                        for end in [&b.low, &b.high] {
                            match end {
                                Bound::Integer(i) => {
                                    self.emit(Instr::Const(Value::integer(*i)), n.span);
                                }
                                Bound::Name(b) => {
                                    self.load(b)?;
                                    self.emit(Instr::AsInteger, b.span);
                                }
                            }
                        }
                    }
                    self.emit(
                        Instr::InitArray(s, value::rank(shape.rank), shape.bounds.len()),
                        n.span,
                    );
                }
                DataType::List(_) => {
                    let empty = Value::List(value::List::new(Vec::new()));
                    self.emit(Instr::Const(empty), n.span);
                    self.emit(Instr::Init(s), n.span);
                }
                _ => {
                    if let Some(v) = &init {
                        self.emit(Instr::Const(v.clone()), d.span);
                        self.emit(Instr::Init(s), d.span);
                    }
                }
            }
        }
        Ok(())
    }

    // Statements

    fn stmts(&mut self, stmts: &[Stmt]) -> Compiled<()> {
        for s in stmts {
            self.stmt(s)?;
        }
        Ok(())
    }

    /// A variable's value, as the reference's `read`: TIME when no
    /// variable hides it.
    fn load(&mut self, n: &Name) -> Compiled<()> {
        match self.slot(n) {
            Some(s) => {
                self.emit(Instr::Load(s), n.span);
            }
            None if n.key == "TIME" => {
                self.emit(Instr::Time, n.span);
            }
            None => self.fail(n.span, format!("`{}` is not a variable here", n.spelling)),
        }
        Ok(())
    }

    fn indices(&mut self, v: &Variable) -> Compiled<()> {
        for i in &v.indices {
            self.expr(i)?;
            self.emit(Instr::AsInteger, i.span);
        }
        Ok(())
    }

    /// Stores the value on the stack in `target`, as the reference's
    /// `assign`: indices, then the constant check, then the conversion,
    /// which fails at `convert`.
    fn assign(&mut self, target: &Variable, convert: Span) -> Compiled<()> {
        let Some(s) = self.slot(&target.name) else {
            let message = format!("`{}` is not a variable here", target.name.spelling);
            self.fail(target.name.span, message);
            return Ok(());
        };
        self.indices(target)?;
        self.emit(Instr::CheckStore(s), target.name.span);
        if target.indices.is_empty() {
            self.emit(Instr::Store(s), convert);
        } else {
            let n = target.indices.len();
            self.emit(Instr::StoreIndex(s, n, at(convert)), target.name.span);
        }
        Ok(())
    }

    fn whole(name: &Name) -> Variable {
        Variable {
            name: name.clone(),
            indices: Vec::new(),
        }
    }

    fn stmt(&mut self, s: &Stmt) -> Compiled<()> {
        self.emit(Instr::Stmt, s.span);
        match &s.kind {
            StmtKind::Assign { target, value } => {
                self.expr(value)?;
                self.assign(target, value.span)
            }
            StmtKind::Loop {
                range,
                condition,
                body,
                until,
            } => self.for_loop(range.as_ref(), condition.as_ref(), body, until.as_ref()),
            StmtKind::Vect { range, body } => self.for_loop(Some(range), None, body, None),
            StmtKind::Each {
                name,
                of,
                filter,
                body,
            } => {
                let Some(of) = of else {
                    self.fail(s.span, "not yet: EACH without OF");
                    return Ok(());
                };
                self.each(name, of, filter.as_ref(), body)
            }
            StmtKind::Parallel(branches) => {
                for b in branches {
                    self.stmts(b)?;
                }
                Ok(())
            }
            StmtKind::If {
                condition,
                then,
                otherwise,
            } => {
                self.expr(condition)?;
                let skip = self.emit(Instr::JumpFalse(PENDING), condition.span);
                self.stmts(then)?;
                match otherwise {
                    Some(o) => {
                        let over = self.emit(Instr::Jump(0), s.span);
                        let here = self.here();
                        self.patch(skip, here);
                        self.stmts(o)?;
                        let here = self.here();
                        self.patch(over, here);
                    }
                    None => {
                        let here = self.here();
                        self.patch(skip, here);
                    }
                }
                Ok(())
            }
            StmtKind::Case {
                subject,
                arms,
                others,
            } => {
                self.expr(subject)?;
                let v = self.hidden();
                self.emit(Instr::Store(v), subject.span);
                let mut entries = Vec::new();
                for (k, arm) in arms.iter().enumerate() {
                    for candidate in &arm.values {
                        self.emit(Instr::Load(v), candidate.span);
                        self.expr(candidate)?;
                        self.emit(Instr::Op(Op::Eq), candidate.span);
                        let j = self.emit(Instr::JumpTrue(PENDING), candidate.span);
                        entries.push((j, k));
                    }
                }
                if let Some(o) = others {
                    self.stmts(o)?;
                }
                let mut ends = vec![self.emit(Instr::Jump(0), s.span)];
                let mut starts = Vec::new();
                for arm in arms {
                    starts.push(self.here());
                    self.stmts(&arm.body)?;
                    ends.push(self.emit(Instr::Jump(0), s.span));
                }
                for (j, k) in entries {
                    self.patch(j, starts[k]);
                }
                let end = self.here();
                for j in ends {
                    self.patch(j, end);
                }
                Ok(())
            }
            StmtKind::Exit { kind, count } => {
                let leave = match kind {
                    ExitKind::Exit => Leave::Exit,
                    ExitKind::Next => Leave::Next,
                    ExitKind::Reloop => Leave::Reloop,
                };
                let written = match count.as_ref().map(|c| &c.kind) {
                    None => Some(1),
                    Some(ExprKind::Number(FuzzyNumber {
                        value: Number::Real(Real::Integer(n)),
                        approximation: None,
                        probability: None,
                        opinion: None,
                    })) => i64::try_from(*n).ok(),
                    _ => None,
                };
                let depth = self.loops.len();
                match (written, count) {
                    (Some(n), _) if n < 1 => self.fail(s.span, "a number of loops is at least 1"),
                    (Some(n), _) if n as usize > depth => {
                        self.fail(s.span, too_far(*kind, n, depth))
                    }
                    (Some(n), _) => {
                        let j = self.emit(Instr::Jump(PENDING - 1), s.span);
                        let loop_ = depth - n as usize;
                        self.loops[loop_].push((j, leave));
                    }
                    (None, Some(c)) => {
                        self.expr(c)?;
                        self.emit(Instr::AsInteger, c.span);
                        let j = self.emit(Instr::Leave(leave, vec![PENDING; depth]), s.span);
                        for l in &mut self.loops {
                            l.push((j, leave));
                        }
                    }
                    (None, None) => unreachable!("a count of one is written"),
                }
                Ok(())
            }
            StmtKind::Return => {
                self.emit(Instr::Jump(PENDING), s.span);
                Ok(())
            }
            StmtKind::Reproc => {
                self.fail(s.span, "the meaning of REPROC is not established");
                Ok(())
            }
            StmtKind::Pause => {
                self.emit(Instr::Pause, s.span);
                Ok(())
            }
            StmtKind::Stop => {
                self.emit(Instr::Stop, s.span);
                Ok(())
            }
            StmtKind::Restart => {
                self.emit(Instr::Restart, s.span);
                Ok(())
            }
            StmtKind::Reset(vs) => {
                for v in vs {
                    match self.slot(&v.name) {
                        Some(slot) => {
                            self.emit(Instr::Reset(slot), v.name.span);
                        }
                        None => {
                            let message = format!("`{}` is not a variable here", v.name.spelling);
                            self.fail(v.name.span, message);
                        }
                    }
                }
                Ok(())
            }
            StmtKind::Result(e) => {
                self.expr(e)?;
                self.emit(Instr::Result, s.span);
                Ok(())
            }
            StmtKind::Insert {
                items,
                list,
                position,
            } => {
                self.gather(items)?;
                let place = match position {
                    Some((p, e)) => {
                        self.expr(e)?;
                        self.emit(Instr::AsInteger, e.span);
                        match p {
                            Position::After => Place::After,
                            Position::Before => Place::Before,
                        }
                    }
                    None => Place::End,
                };
                let Some(l) = self.list(list) else {
                    return Ok(());
                };
                let span = position.as_ref().map_or(s.span, |(_, e)| e.span);
                self.emit(Instr::Insert(l, place), span);
                Ok(())
            }
            StmtKind::Extract { items, list } => {
                self.gather(items)?;
                if let Some(l) = self.list(list) {
                    self.emit(Instr::Extract(l), s.span);
                }
                Ok(())
            }
            StmtKind::Empty(list) => {
                if let Some(l) = self.list(list) {
                    self.emit(Instr::Empty(l), s.span);
                }
                Ok(())
            }
            StmtKind::Organize { kind, lists } => {
                for list in lists {
                    let Some(l) = self.list(list) else {
                        return Ok(());
                    };
                    match kind {
                        Organization::Circular => {
                            self.emit(Instr::Organize(l, true), s.span);
                        }
                        Organization::Linear => {
                            self.emit(Instr::Organize(l, false), s.span);
                        }
                        Organization::Single | Organization::Tuple => {}
                    }
                }
                Ok(())
            }
            StmtKind::Chain(q) => {
                match self.module_named(q) {
                    Some((i, m)) if m.kind == ModuleKind::Program => {
                        self.emit(Instr::Chain(i), q.span);
                    }
                    _ => self.fail(
                        q.span,
                        format!("`{}` is not a program of this file", q.spelling),
                    ),
                }
                Ok(())
            }
            StmtKind::Call(c) => {
                match self.module_named(&c.name) {
                    None if builtins::is_subroutine(&c.name.key) => {
                        return not_yet(c.name.span, "the expert-system engines");
                    }
                    Some((i, m)) if m.kind == ModuleKind::Subroutine => {
                        let n = self.args(m, c)?;
                        if let Some(n) = n {
                            self.emit(Instr::Call(i, n), c.name.span);
                        }
                    }
                    _ => {
                        let message =
                            format!("`{}` is not a subroutine of this file", c.name.spelling);
                        self.fail(c.name.span, message);
                    }
                }
                Ok(())
            }
            StmtKind::Read(vs) => {
                for v in vs {
                    let Some(slot) = self.slot(&v.name) else {
                        let message = format!("`{}` is not a variable here", v.name.spelling);
                        self.fail(v.name.span, message);
                        return Ok(());
                    };
                    let text = matches!(
                        self.slots[slot].ty,
                        SlotType::Base(Base::String | Base::Character)
                    );
                    self.emit(Instr::Read(text), v.name.span);
                    self.assign(v, v.name.span)?;
                }
                Ok(())
            }
            StmtKind::Print(es) => {
                for e in es {
                    self.expr(e)?;
                }
                self.emit(Instr::Print(es.len()), s.span);
                Ok(())
            }
            _ => not_yet(s.span, "this statement on the VM"),
        }
    }

    /// A list variable for INSERT, EXTRACT, EMPTY, CIRCULAR and LINEAR.
    fn list(&mut self, n: &Name) -> Option<usize> {
        match self.slot(n) {
            Some(s) => {
                self.emit(Instr::CheckList(s), n.span);
                Some(s)
            }
            None => {
                self.fail(n.span, format!("`{}` is not a variable here", n.spelling));
                None
            }
        }
    }

    fn gather(&mut self, items: &[Expr]) -> Compiled<()> {
        for i in items {
            self.expr(i)?;
        }
        self.emit(Instr::Gather(items.len()), Span::default());
        Ok(())
    }

    fn open_loop(&mut self, again: usize) {
        self.loops.push(Vec::new());
        self.agains.push(again);
    }

    /// The loop just compiled: its EXIT, NEXT and RELOOP patched.
    fn close_loop(&mut self, next: usize, exit: usize) {
        let pending = self.loops.pop().expect("a loop");
        let again = self.agains.pop().expect("a loop");
        let depth = self.loops.len();
        for (pc, leave) in pending {
            let target = match leave {
                Leave::Exit => exit,
                Leave::Next => next,
                Leave::Reloop => again,
            };
            match &mut self.code[pc] {
                Instr::Jump(j) => *j = target,
                // Its targets, innermost first: this loop's is the one
                // as many places in as the loops around it, outermost
                // being the last.
                Instr::Leave(_, targets) => {
                    let k = targets.len() - 1 - depth;
                    targets[k] = target;
                }
                i => unreachable!("not a way out: {i:?}"),
            }
        }
    }

    fn for_loop(
        &mut self,
        range: Option<&Range>,
        condition: Option<&Expr>,
        body: &[Stmt],
        until: Option<&Expr>,
    ) -> Compiled<()> {
        let again = self.here();
        let mut counter = None;
        if let Some(r) = range {
            self.expr(&r.from)?;
            self.assign(&Self::whole(&r.name), r.from.span)?;
            match &r.step {
                Some(s) => self.expr(s)?,
                None => {
                    self.emit(Instr::Const(Value::integer(1)), r.from.span);
                }
            }
            let (step, up) = (self.hidden(), self.hidden());
            let span = r.step.as_ref().map_or(r.from.span, |s| s.span);
            self.emit(Instr::LoopStep(step, up), span);
            counter = Some((r, step, up));
        }
        self.open_loop(again);
        let top = self.here();
        let mut exits = Vec::new();
        if let Some((r, _, up)) = counter {
            self.load(&r.name)?;
            self.expr(&r.to)?;
            self.emit(Instr::Within(up), r.to.span);
            exits.push(self.emit(Instr::JumpFalse(PENDING), r.to.span));
        }
        if let Some(c) = condition {
            self.expr(c)?;
            exits.push(self.emit(Instr::JumpFalse(PENDING), c.span));
        }
        self.stmts(body)?;
        let next = self.here();
        if let Some(u) = until {
            self.expr(u)?;
            exits.push(self.emit(Instr::JumpTrue(PENDING), u.span));
        }
        if let Some((r, step, _)) = counter {
            self.load(&r.name)?;
            self.emit(Instr::Load(step), r.name.span);
            self.emit(Instr::Op(Op::Add), r.name.span);
            self.assign(&Self::whole(&r.name), r.name.span)?;
        }
        self.emit(Instr::Jump(top), Span::default());
        let end = self.here();
        for j in exits {
            self.patch(j, end);
        }
        self.close_loop(next, end);
        Ok(())
    }

    fn each(
        &mut self,
        name: &Name,
        of: &Expr,
        filter: Option<&Expr>,
        body: &[Stmt],
    ) -> Compiled<()> {
        let again = self.here();
        self.expr(of)?;
        let (items, position) = (self.hidden(), self.hidden());
        self.emit(Instr::EachStart(items, position), of.span);
        self.open_loop(again);
        let top = self.here();
        let next = self.emit(Instr::EachNext(items, position, PENDING), name.span);
        self.assign(&Self::whole(name), name.span)?;
        if let Some(f) = filter {
            self.expr(f)?;
            self.emit(Instr::JumpFalse(top), f.span);
        }
        self.stmts(body)?;
        self.emit(Instr::Jump(top), Span::default());
        let end = self.here();
        self.patch(next, end);
        self.close_loop(top, end);
        Ok(())
    }

    /// A call's arguments, as the reference's `args`: their number
    /// first, then each in order, a value for IN, the caller's variable
    /// for OUT and INOUT. None if it fails before the call.
    fn args(&mut self, m: &Module, c: &Call) -> Compiled<Option<usize>> {
        if m.parameters.len() != c.args.len() {
            let message = format!(
                "`{}` takes {} argument(s), not {}",
                c.name.spelling,
                m.parameters.len(),
                c.args.len()
            );
            self.fail(c.name.span, message);
            return Ok(None);
        }
        for (p, a) in m.parameters.iter().zip(&c.args) {
            let mode = (m.specifications.iter())
                .find(|s| s.names.contains(p))
                .map_or(Mode::In, |s| s.mode);
            match (mode, &a.kind) {
                (Mode::In, _) => self.expr(a)?,
                (_, ExprKind::Variable(v)) if v.indices.is_empty() => match self.slot(&v.name) {
                    Some(s) => {
                        self.emit(Instr::Ref(s), a.span);
                    }
                    None => {
                        let message = format!("`{}` is not a variable here", v.name.spelling);
                        self.fail(v.name.span, message);
                        return Ok(None);
                    }
                },
                _ => {
                    self.fail(a.span, "an OUT or INOUT argument is a whole variable");
                    return Ok(None);
                }
            }
        }
        Ok(Some(c.args.len()))
    }

    // Expressions

    fn expr(&mut self, e: &Expr) -> Compiled<()> {
        match &e.kind {
            ExprKind::Number(n) => match literal(n) {
                Ok(fz) => {
                    self.emit(Instr::Const(Value::One(fz)), e.span);
                }
                Err(f) => self.fail(e.span, f.0),
            },
            ExprKind::Multiple(values) => {
                let mut out = Vec::new();
                for (negative, n) in values {
                    let fz = literal(n).and_then(|fz| match negative {
                        true => value::unary_fz(UnaryOp::Neg, &fz),
                        false => Ok(fz),
                    });
                    match fz {
                        Ok(fz) => out.push(fz),
                        Err(f) => {
                            self.fail(e.span, f.0);
                            return Ok(());
                        }
                    }
                }
                self.emit(Instr::Const(Value::Many(out)), e.span);
            }
            ExprKind::String(s) => {
                self.emit(Instr::Const(string(s)), e.span);
            }
            ExprKind::Boolean(b) => {
                self.emit(Instr::Const(Value::boolean(*b)), e.span);
            }
            ExprKind::This => self.fail(
                e.span,
                "THIS is the object a computed attribute, a COND, a rule or a process runs for",
            ),
            ExprKind::Variable(v) => {
                if v.indices.is_empty() {
                    self.load(&v.name)?;
                } else if let Some(s) = self.slot(&v.name) {
                    self.indices(v)?;
                    self.emit(Instr::LoadIndex(s, v.indices.len()), v.name.span);
                } else {
                    let message = format!("`{}` is not a variable here", v.name.spelling);
                    self.fail(v.name.span, message);
                }
            }
            ExprKind::Call(c) => self.function(c, e.span)?,
            ExprKind::Unary(op, x) => {
                self.expr(x)?;
                let u = match op {
                    UnaryOp::Neg => hbds_values::Unary::Neg,
                    UnaryOp::Plus => hbds_values::Unary::Number,
                    UnaryOp::Not => hbds_values::Unary::Not,
                };
                self.emit(Instr::Unary(u), e.span);
            }
            ExprKind::Binary(op, l, r) => {
                self.expr(l)?;
                self.expr(r)?;
                let i = match value::op(*op) {
                    Some(op) => Instr::Op(op),
                    None => Instr::ListOp(match op {
                        BinaryOp::Union => ListOp::Union,
                        BinaryOp::Inter => ListOp::Inter,
                        _ => ListOp::Compl,
                    }),
                };
                self.emit(i, e.span);
            }
            ExprKind::Functor(adt, calls) => self.functors(adt, calls, e.span)?,
            ExprKind::Attribute(..) | ExprKind::Link(..) => {
                return not_yet(e.span, "the store on the VM");
            }
        }
        Ok(())
    }

    fn function(&mut self, c: &Call, span: Span) -> Compiled<()> {
        if let Some((i, m)) = self.module_named(&c.name) {
            if !matches!(m.kind, ModuleKind::Function { .. }) {
                self.fail(
                    c.name.span,
                    format!("`{}` is not a function", c.name.spelling),
                );
                return Ok(());
            }
            if let Some(n) = self.args(m, c)? {
                self.emit(Instr::Call(i, n), c.name.span);
            }
            return Ok(());
        }
        if !builtins::is_function(&c.name.key) {
            let message = format!("`{}` is not a function of this file", c.name.spelling);
            self.fail(c.name.span, message);
            return Ok(());
        }
        let [arg] = c.args.as_slice() else {
            self.fail(
                c.name.span,
                format!("`{}` takes one argument", c.name.spelling),
            );
            return Ok(());
        };
        self.expr(arg)?;
        let one = format!("`{}` takes one number", c.name.spelling);
        self.emit(Instr::ExpectOne(one), arg.span);
        self.emit(Instr::Native(c.name.key.clone()), span);
        Ok(())
    }

    /// `L ' F ' G(x)`: on the VM for now, a list a variable holds, the
    /// first functor changing it if it moves the cursor or takes an
    /// element out.
    fn functors(&mut self, adt: &Adt, calls: &[Call], span: Span) -> Compiled<()> {
        let held = match (&adt.designation, &adt.part) {
            (Designation::Named(AdtName::Name(n)), AdtPart::None) => match self.slot(n) {
                Some(s) if self.slots[s].ty == SlotType::List => {
                    self.load(n)?;
                    Some(s)
                }
                _ => return not_yet(span, "functors on a.d.t. on the VM"),
            },
            _ => return not_yet(span, "functors on a.d.t. on the VM"),
        };
        let mut held = held;
        for c in calls {
            let held_now = held.take();
            if let Some((i, m)) = self.module_named(&c.name) {
                if !matches!(m.kind, ModuleKind::Functor { .. }) {
                    self.fail(
                        c.name.span,
                        format!("`{}` is not a functor", c.name.spelling),
                    );
                    return Ok(());
                }
                if let Some(n) = self.args(m, c)? {
                    // The prefix, under the arguments, goes first.
                    self.emit(Instr::Call(i, n + 1), c.name.span);
                }
                continue;
            }
            let Some(functor) = builtins::functor(&c.name.key) else {
                let message = format!("`{}` is not a functor of this file", c.name.spelling);
                self.fail(c.name.span, message);
                return Ok(());
            };
            for a in &c.args {
                self.expr(a)?;
            }
            if functor.takes != Takes::Lists {
                return not_yet(c.name.span, "functors on a.d.t. on the VM");
            }
            let (key, spelling) = (c.name.key.clone(), c.name.spelling.clone());
            let i = Instr::Functor(key, spelling, c.args.len(), held_now);
            self.emit(i, c.name.span);
        }
        Ok(())
    }
}

fn constant_value(c: &Constant) -> Value {
    constant(c)
}
