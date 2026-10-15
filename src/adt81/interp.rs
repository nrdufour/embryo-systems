//! The interpreter of ADT'81's core: expressions, control flow, modules,
//! lists, fuzzy values and coroutines. docs/adt81.md, "Running", gives
//! the semantics and the decisions behind them.
//!
//! A tree walk over the [`ast`](super::ast) of a program the checker has
//! passed. The a.d.t. half (classes, objects, links, their functors,
//! processes) needs the HBDS store, not built yet: reaching it is a
//! runtime error that says so.
//!
//! Each coroutine runs on its own thread, and control passes by channel,
//! so that only one thread ever runs: RESUME continues a coroutine where
//! it paused, its "Local Sequence Counter" (page 112), and PAUSE gives
//! control back to whoever resumed it.

use std::collections::HashMap;
use std::fmt;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;

use super::ast::*;
use super::builtins;
use super::lower;
use super::types::{Ty, Typed};
use super::value::{self, Computed, Fz, List, Scalar, Value};

/// Where READ reads and PRINT writes.
pub trait Io: Send {
    fn read_line(&mut self) -> Option<String>;
    fn write(&mut self, text: &str);
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeError {
    pub span: Span,
    pub message: String,
}

/// `line:column: runtime error: message`
impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Span { line, column } = self.span;
        write!(f, "{line}:{column}: runtime error: {}", self.message)
    }
}

/// Runs the file's first PROGRAM, then any it CHAINs to.
pub fn run(program: Program, io: Arc<Mutex<dyn Io>>) -> Result<(), RuntimeError> {
    let mut modules = HashMap::new();
    for (i, m) in program.modules.iter().enumerate() {
        if let ModuleName::Name(n) = &m.name {
            modules.entry(n.key.clone()).or_insert(i);
        }
    }
    let Some(mut current) = program
        .modules
        .iter()
        .position(|m| m.kind == ModuleKind::Program)
    else {
        return Err(RuntimeError {
            span: Span { line: 1, column: 1 },
            message: "no PROGRAM to run in this file".into(),
        });
    };
    let machine = Machine {
        program: Arc::new(program),
        modules: Arc::new(modules),
        io,
        coroutines: Arc::new(Mutex::new(HashMap::new())),
        link: None,
    };
    let result = loop {
        let module = &machine.program.modules[current];
        match machine.call(module, Vec::new(), None) {
            Ok(_) | Err(Flow::Return | Flow::Stop) => break Ok(()),
            Err(Flow::Restart) => {}
            Err(Flow::Chain(q)) => match machine.modules.get(&q.key) {
                Some(&i) if machine.program.modules[i].kind == ModuleKind::Program => current = i,
                _ => {
                    break Err(RuntimeError {
                        span: q.span,
                        message: format!("`{}` is not a program of this file", q.spelling),
                    });
                }
            },
            Err(Flow::Error(e)) => break Err(e),
            Err(Flow::Exit(_) | Flow::Next(_) | Flow::Reloop(_)) => {
                break Err(RuntimeError {
                    span: module.span,
                    message: "EXIT, NEXT or RELOOP outside a loop".into(),
                });
            }
        }
    };
    // Paused coroutines wait for a RESUME that will not come: let them go.
    machine.coroutines.lock().unwrap().clear();
    result
}

/// Why execution leaves the straight line.
#[derive(Debug)]
enum Flow {
    /// Leave this many loops.
    Exit(i64),
    /// Next iteration of the n-th enclosing loop.
    Next(i64),
    /// Restart the n-th enclosing loop.
    Reloop(i64),
    Return,
    Stop,
    Restart,
    Chain(Name),
    Error(RuntimeError),
}

type Exec<T> = Result<T, Flow>;

fn error(span: Span, message: impl Into<String>) -> Flow {
    Flow::Error(RuntimeError {
        span,
        message: message.into(),
    })
}

/// A computation's fault, placed at `span`.
fn at<T>(r: Computed<T>, span: Span) -> Exec<T> {
    r.map_err(|f| error(span, f.0))
}

fn store(span: Span, what: &str) -> Flow {
    error(span, format!("{what} needs the HBDS store, not built yet"))
}

/// A variable: its value if it has one, the type it is declared with, and
/// the value RESET gives it back.
struct Var {
    value: Option<Value>,
    ty: Typed,
    initial: Option<Value>,
    constant: bool,
}

type Slot = Arc<Mutex<Var>>;

fn slot(value: Option<Value>, ty: Typed, constant: bool) -> Slot {
    Arc::new(Mutex::new(Var {
        initial: value.clone(),
        value,
        ty,
        constant,
    }))
}

#[derive(Default)]
struct Frame {
    vars: HashMap<String, Slot>,
    /// A function's RESULT.
    result: Option<Value>,
}

/// An argument: a value for IN, the caller's variable for OUT and INOUT.
enum Arg {
    Value(Value),
    Ref(Slot),
}

enum Event {
    Paused,
    Ended(Exec<()>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Running,
    Paused,
    Ended,
}

struct Coroutine {
    go: Sender<()>,
    back: Arc<Mutex<Receiver<Event>>>,
    state: State,
}

/// How a coroutine's thread gives control back.
struct Link {
    back: Sender<Event>,
    go: Mutex<Receiver<()>>,
}

#[derive(Clone)]
struct Machine {
    program: Arc<Program>,
    modules: Arc<HashMap<String, usize>>,
    io: Arc<Mutex<dyn Io>>,
    coroutines: Arc<Mutex<HashMap<String, Coroutine>>>,
    /// Inside a coroutine, its way back.
    link: Option<Arc<Link>>,
}

/// One iteration's outcome, for the loop that runs it.
enum Step {
    Go,
    Leave,
    Again,
}

impl Machine {
    fn module(&self, n: &Name) -> Option<&Module> {
        self.modules.get(&n.key).map(|&i| &self.program.modules[i])
    }

    fn var(&self, n: &Name, f: &Frame) -> Exec<Slot> {
        f.vars
            .get(&n.key)
            .cloned()
            .ok_or_else(|| error(n.span, format!("`{}` is not a variable here", n.spelling)))
    }

    // Modules

    fn call(&self, m: &Module, args: Vec<Arg>, prefix: Option<Value>) -> Exec<Option<Value>> {
        let mut f = Frame::default();
        for (p, arg) in m.parameters.iter().zip(args) {
            let spec = m.specifications.iter().find(|s| s.names.contains(p));
            let ty = spec.map_or(Typed::UNKNOWN, |s| Typed::of(&s.ty));
            let s = match arg {
                Arg::Value(v) => slot(Some(at(value::convert(v, &ty.ty), p.span)?), ty, false),
                Arg::Ref(s) => s,
            };
            f.vars.insert(p.key.clone(), s);
        }
        if let (ModuleKind::Functor { returns, .. }, ModuleName::Name(n)) = (&m.kind, &m.name) {
            f.vars
                .insert(n.key.clone(), slot(None, Typed::of(returns), false));
        }
        for d in &m.locals {
            let ty = Typed::of(&d.ty);
            let init = match &d.init {
                Some(Init::Initial(c) | Init::Constant(c)) => {
                    Some(at(value::convert(constant(c), &ty.ty), d.span)?)
                }
                None => None,
            };
            let constant = matches!(d.init, Some(Init::Constant(_)));
            for n in &d.names {
                let value = match &d.ty {
                    DataType::Array { shape, element, .. } => {
                        let element = Typed::of_scalar(element).ty;
                        let fill = init.clone().unwrap_or_else(|| value::zero(&element));
                        Some(self.array(shape, fill, &f, n.span)?)
                    }
                    DataType::List(_) => Some(Value::List(List::new(Vec::new()))),
                    _ => init.clone(),
                };
                f.vars
                    .insert(n.key.clone(), slot(value, ty.clone(), constant));
            }
        }
        if let (
            ModuleKind::Functor {
                prefix: Some(p), ..
            },
            Some(v),
        ) = (&m.kind, prefix)
        {
            let s = self.var(p, &f)?;
            s.lock().unwrap().value = Some(v);
        }
        match self.block(&m.body, &mut f) {
            Ok(()) | Err(Flow::Return) => {}
            Err(flow) => return Err(flow),
        }
        match (&m.kind, &m.name) {
            (ModuleKind::Function { .. }, _) => f
                .result
                .map(Some)
                .ok_or_else(|| error(m.span, "the function ended without a RESULT")),
            (ModuleKind::Functor { .. }, ModuleName::Name(n)) => {
                let s = self.var(n, &f)?;
                let v = s.lock().unwrap().value.clone();
                v.map(Some)
                    .ok_or_else(|| error(m.span, "the functor ended without a result"))
            }
            _ => Ok(None),
        }
    }

    /// An array of the declared shape, every cell `fill`. Without bounds,
    /// an array has no cells until a whole array is assigned to it.
    fn array(&self, shape: &Shape, fill: Value, f: &Frame, span: Span) -> Exec<Value> {
        let mut lows = Vec::new();
        let mut dims = Vec::new();
        for b in &shape.bounds {
            let end = |e: &Bound| -> Exec<i64> {
                match e {
                    Bound::Integer(i) => Ok(*i),
                    Bound::Name(n) => {
                        let s = self.var(n, f)?;
                        let v = s.lock().unwrap().value.clone();
                        let v = v.ok_or_else(|| {
                            error(n.span, format!("`{}` has no value", n.spelling))
                        })?;
                        at(v.as_integer(), n.span)
                    }
                }
            };
            let (low, high) = (end(&b.low)?, end(&b.high)?);
            if high < low {
                return Err(error(span, format!("the bounds {low}:{high} are empty")));
            }
            lows.push(low);
            dims.push((high - low + 1) as usize);
        }
        let size = if dims.is_empty() {
            0
        } else {
            dims.iter().product()
        };
        Ok(Value::Array(value::Array {
            rank: shape.rank,
            lows,
            dims,
            cells: vec![fill; size],
        }))
    }

    fn args(&self, m: &Module, c: &Call, f: &mut Frame) -> Exec<Vec<Arg>> {
        if m.parameters.len() != c.args.len() {
            return Err(error(
                c.name.span,
                format!(
                    "`{}` takes {} argument(s), not {}",
                    c.name.spelling,
                    m.parameters.len(),
                    c.args.len()
                ),
            ));
        }
        let mut args = Vec::new();
        for (p, a) in m.parameters.iter().zip(&c.args) {
            let mode = m
                .specifications
                .iter()
                .find(|s| s.names.contains(p))
                .map_or(Mode::In, |s| s.mode);
            args.push(match (mode, &a.kind) {
                (Mode::In, _) => Arg::Value(self.eval(a, f)?),
                (_, ExprKind::Variable(v)) if v.indices.is_empty() => {
                    Arg::Ref(self.var(&v.name, f)?)
                }
                _ => {
                    return Err(error(
                        a.span,
                        "an OUT or INOUT argument is a whole variable",
                    ));
                }
            });
        }
        Ok(args)
    }

    fn resume(&self, c: &Call, f: &mut Frame) -> Exec<()> {
        let Some(m) = self
            .module(&c.name)
            .filter(|m| m.kind == ModuleKind::Coroutine)
        else {
            return Err(error(
                c.name.span,
                format!("`{}` is not a coroutine of this file", c.name.spelling),
            ));
        };
        let started = self.coroutines.lock().unwrap().contains_key(&c.name.key);
        let back = if started {
            if !c.args.is_empty() {
                return Err(error(
                    c.name.span,
                    "arguments go with the first RESUME only",
                ));
            }
            let mut map = self.coroutines.lock().unwrap();
            let co = map.get_mut(&c.name.key).expect("started");
            match co.state {
                State::Ended => {
                    return Err(error(
                        c.name.span,
                        format!("`{}` has ended", c.name.spelling),
                    ));
                }
                State::Running => {
                    return Err(error(
                        c.name.span,
                        format!("`{}` is running: it cannot be resumed", c.name.spelling),
                    ));
                }
                State::Paused => {}
            }
            co.state = State::Running;
            co.go
                .send(())
                .map_err(|_| error(c.name.span, "the coroutine is gone"))?;
            co.back.clone()
        } else {
            let args = self.args(m, c, f)?;
            let (go, go_rx) = channel();
            let (back_tx, back) = channel();
            let back = Arc::new(Mutex::new(back));
            let link = Arc::new(Link {
                back: back_tx,
                go: Mutex::new(go_rx),
            });
            let mut machine = self.clone();
            machine.link = Some(link.clone());
            self.coroutines.lock().unwrap().insert(
                c.name.key.clone(),
                Coroutine {
                    go,
                    back: back.clone(),
                    state: State::Running,
                },
            );
            let index = self.modules[&c.name.key];
            thread::spawn(move || {
                let module = &machine.program.modules[index];
                let ended = machine.call(module, args, None).map(|_| ());
                let _ = link.back.send(Event::Ended(ended));
            });
            back
        };
        let event = back.lock().unwrap().recv();
        let mut map = self.coroutines.lock().unwrap();
        let co = map.get_mut(&c.name.key).expect("started");
        match event {
            Ok(Event::Paused) => {
                co.state = State::Paused;
                Ok(())
            }
            Ok(Event::Ended(ended)) => {
                co.state = State::Ended;
                match ended {
                    Ok(()) | Err(Flow::Return) => Ok(()),
                    Err(flow) => Err(flow),
                }
            }
            Err(_) => Err(Flow::Stop),
        }
    }

    fn pause(&self) -> Exec<()> {
        match &self.link {
            Some(link) => {
                let _ = link.back.send(Event::Paused);
                // No one left to resume: the program has ended.
                link.go.lock().unwrap().recv().map_err(|_| Flow::Stop)
            }
            None => {
                let mut io = self.io.lock().unwrap();
                io.write("PAUSE\n");
                io.read_line();
                Ok(())
            }
        }
    }

    // Statements

    fn block(&self, stmts: &[Stmt], f: &mut Frame) -> Exec<()> {
        for s in stmts {
            self.stmt(s, f)?;
        }
        Ok(())
    }

    /// One iteration's body, and what the loop does next.
    fn iteration(&self, body: &[Stmt], f: &mut Frame) -> Exec<Step> {
        match self.block(body, f) {
            Ok(()) | Err(Flow::Next(1)) => Ok(Step::Go),
            Err(Flow::Exit(1)) => Ok(Step::Leave),
            Err(Flow::Reloop(1)) => Ok(Step::Again),
            Err(Flow::Exit(n)) => Err(Flow::Exit(n - 1)),
            Err(Flow::Next(n)) => Err(Flow::Next(n - 1)),
            Err(Flow::Reloop(n)) => Err(Flow::Reloop(n - 1)),
            Err(flow) => Err(flow),
        }
    }

    fn truth(&self, e: &Expr, f: &mut Frame) -> Exec<bool> {
        let v = self.eval(e, f)?;
        at(v.truth(), e.span)
    }

    fn assign(&self, target: &Variable, v: Value, f: &mut Frame, span: Span) -> Exec<()> {
        let s = self.var(&target.name, f)?;
        let indices = target
            .indices
            .iter()
            .map(|i| {
                let v = self.eval(i, f)?;
                at(v.as_integer(), i.span)
            })
            .collect::<Exec<Vec<i64>>>()?;
        let mut var = s.lock().unwrap();
        if var.constant {
            return Err(error(
                target.name.span,
                format!("`{}` is a constant", target.name.spelling),
            ));
        }
        if indices.is_empty() {
            var.value = Some(at(value::convert(v, &var.ty.ty), span)?);
            return Ok(());
        }
        let element = match &var.ty.ty {
            Ty::Array(_, e) => (**e).clone(),
            _ => Ty::Unknown,
        };
        let v = at(value::convert(v, &element), span)?;
        match &mut var.value {
            Some(Value::Array(a)) => {
                let i = at(offset(a, &indices), target.name.span)?;
                a.cells[i] = v;
                Ok(())
            }
            _ => Err(error(
                target.name.span,
                format!("`{}` is not an array", target.name.spelling),
            )),
        }
    }

    /// A FOR loop's counter: set to `from`, then on while within `to`.
    fn counter(&self, r: &Range, f: &mut Frame) -> Exec<(Value, bool)> {
        let from = self.eval(&r.from, f)?;
        self.assign(
            &Variable {
                name: r.name.clone(),
                indices: Vec::new(),
            },
            from,
            f,
            r.from.span,
        )?;
        let step = match &r.step {
            Some(s) => self.eval(s, f)?,
            None => Value::integer(1),
        };
        let span = r.step.as_ref().map_or(r.from.span, |s| s.span);
        let zero = Value::integer(0);
        if at(value::binary(BinaryOp::Eq, &step, &zero), span)? == Value::boolean(true) {
            return Err(error(span, "a loop's STEP is not zero"));
        }
        let up = at(
            value::binary(BinaryOp::Gt, &step, &zero).and_then(|v| v.truth()),
            span,
        )?;
        Ok((step, up))
    }

    fn within(&self, r: &Range, up: bool, f: &mut Frame) -> Exec<bool> {
        let i = self.read(
            &Variable {
                name: r.name.clone(),
                indices: Vec::new(),
            },
            f,
        )?;
        let to = self.eval(&r.to, f)?;
        let op = if up { BinaryOp::Le } else { BinaryOp::Ge };
        at(
            value::binary(op, &i, &to).and_then(|v| v.truth()),
            r.to.span,
        )
    }

    fn advance(&self, r: &Range, step: &Value, f: &mut Frame) -> Exec<()> {
        let var = Variable {
            name: r.name.clone(),
            indices: Vec::new(),
        };
        let i = self.read(&var, f)?;
        let next = at(value::binary(BinaryOp::Add, &i, step), r.name.span)?;
        self.assign(&var, next, f, r.name.span)
    }

    fn for_loop(
        &self,
        range: Option<&Range>,
        condition: Option<&Expr>,
        body: &[Stmt],
        until: Option<&Expr>,
        f: &mut Frame,
    ) -> Exec<()> {
        'again: loop {
            let counter = match range {
                Some(r) => Some(self.counter(r, f)?),
                None => None,
            };
            loop {
                if let (Some(r), Some((_, up))) = (range, &counter)
                    && !self.within(r, *up, f)?
                {
                    return Ok(());
                }
                if let Some(c) = condition
                    && !self.truth(c, f)?
                {
                    return Ok(());
                }
                match self.iteration(body, f)? {
                    Step::Go => {}
                    Step::Leave => return Ok(()),
                    Step::Again => continue 'again,
                }
                if let Some(u) = until
                    && self.truth(u, f)?
                {
                    return Ok(());
                }
                if let (Some(r), Some((step, _))) = (range, &counter) {
                    self.advance(r, step, f)?;
                }
            }
        }
    }

    fn stmt(&self, s: &Stmt, f: &mut Frame) -> Exec<()> {
        match &s.kind {
            StmtKind::Assign { target, value } => {
                let v = self.eval(value, f)?;
                self.assign(target, v, f, value.span)
            }
            StmtKind::Loop {
                range,
                condition,
                body,
                until,
            } => self.for_loop(range.as_ref(), condition.as_ref(), body, until.as_ref(), f),
            StmtKind::Vect { range, body } => self.for_loop(Some(range), None, body, None, f),
            StmtKind::Each {
                name,
                of,
                filter,
                body,
            } => {
                let Some(of) = of else {
                    return Err(store(s.span, "EACH without OF"));
                };
                'again: loop {
                    let items = match self.eval(of, f)? {
                        Value::List(l) => l.items,
                        v => return Err(error(of.span, format!("EACH goes over a list, not {v}"))),
                    };
                    let var = Variable {
                        name: name.clone(),
                        indices: Vec::new(),
                    };
                    for item in items {
                        self.assign(&var, item, f, name.span)?;
                        if let Some(c) = filter
                            && !self.truth(c, f)?
                        {
                            continue;
                        }
                        match self.iteration(body, f)? {
                            Step::Go => {}
                            Step::Leave => return Ok(()),
                            Step::Again => continue 'again,
                        }
                    }
                    return Ok(());
                }
            }
            StmtKind::Parallel(branches) => {
                for b in branches {
                    self.block(b, f)?;
                }
                Ok(())
            }
            StmtKind::If {
                condition,
                then,
                otherwise,
            } => {
                if self.truth(condition, f)? {
                    self.block(then, f)
                } else if let Some(o) = otherwise {
                    self.block(o, f)
                } else {
                    Ok(())
                }
            }
            StmtKind::Case {
                subject,
                arms,
                others,
            } => {
                let v = self.eval(subject, f)?;
                for arm in arms {
                    for candidate in &arm.values {
                        let c = self.eval(candidate, f)?;
                        let equal = value::binary(BinaryOp::Eq, &v, &c).and_then(|e| e.truth());
                        if at(equal, candidate.span)? {
                            return self.block(&arm.body, f);
                        }
                    }
                }
                match others {
                    Some(o) => self.block(o, f),
                    None => Ok(()),
                }
            }
            StmtKind::Exit { kind, count } => {
                let n = match count {
                    Some(c) => {
                        let v = self.eval(c, f)?;
                        at(v.as_integer(), c.span)?
                    }
                    None => 1,
                };
                if n < 1 {
                    return Err(error(s.span, "a number of loops is at least 1"));
                }
                Err(match kind {
                    ExitKind::Exit => Flow::Exit(n),
                    ExitKind::Next => Flow::Next(n),
                    ExitKind::Reloop => Flow::Reloop(n),
                })
            }
            StmtKind::Return => Err(Flow::Return),
            StmtKind::Reproc => Err(error(s.span, "the meaning of REPROC is not established")),
            StmtKind::Pause => self.pause(),
            StmtKind::Stop => Err(Flow::Stop),
            StmtKind::Restart => Err(Flow::Restart),
            StmtKind::Reset(vs) => {
                for v in vs {
                    let s = self.var(&v.name, f)?;
                    let mut var = s.lock().unwrap();
                    var.value = var.initial.clone();
                }
                Ok(())
            }
            StmtKind::Result(e) => {
                f.result = Some(self.eval(e, f)?);
                Err(Flow::Return)
            }
            StmtKind::Insert {
                items,
                list,
                position,
            } => {
                let mut new = Vec::new();
                for i in items {
                    match self.eval(i, f)? {
                        Value::List(l) => new.extend(l.items),
                        v => new.push(v),
                    }
                }
                let position = match position {
                    Some((p, e)) => {
                        let v = self.eval(e, f)?;
                        Some((*p, at(v.as_integer(), e.span)?, e.span))
                    }
                    None => None,
                };
                self.with_list(list, f, |l| {
                    let at = match position {
                        None => l.items.len(),
                        Some((p, k, span)) => {
                            let k = usize::try_from(k).ok().filter(|k| *k <= l.items.len());
                            let k =
                                k.ok_or_else(|| error(span, "this position is outside the list"))?;
                            match p {
                                Position::After => k,
                                Position::Before => k.saturating_sub(1),
                            }
                        }
                    };
                    l.items.splice(at..at, new);
                    Ok(())
                })
            }
            StmtKind::Extract { items, list } => {
                let mut out = Vec::new();
                for i in items {
                    match self.eval(i, f)? {
                        Value::List(l) => out.extend(l.items),
                        v => out.push(v),
                    }
                }
                self.with_list(list, f, |l| {
                    l.items.retain(|x| !out.contains(x));
                    l.cursor = l.cursor.min(l.items.len().saturating_sub(1));
                    Ok(())
                })
            }
            StmtKind::Empty(list) => self.with_list(list, f, |l| {
                l.items.clear();
                l.cursor = 0;
                Ok(())
            }),
            StmtKind::Organize { kind, lists } => {
                for list in lists {
                    self.with_list(list, f, |l| {
                        match kind {
                            Organization::Circular => l.circular = true,
                            Organization::Linear => l.circular = false,
                            Organization::Single | Organization::Tuple => {}
                        }
                        Ok(())
                    })?;
                }
                Ok(())
            }
            StmtKind::Create(_) => Err(store(s.span, "CREATE")),
            StmtKind::StateChange(..) => Err(store(s.span, "a change of state")),
            StmtKind::Design(_) => Err(store(s.span, "DESIGN")),
            StmtKind::Let { .. } => Err(store(s.span, "LET")),
            StmtKind::Assert(_) => Err(store(s.span, "ASSERT")),
            StmtKind::Specify { .. } => Err(store(s.span, "SPECIFY")),
            StmtKind::Include { .. } => Err(store(s.span, "INCLUDE")),
            StmtKind::Exclude { .. } => Err(store(s.span, "EXCLUDE")),
            StmtKind::Associate(_) => Err(store(s.span, "ASSOC")),
            StmtKind::Dissociate(_) => Err(store(s.span, "DISSOC")),
            StmtKind::Compose { .. } => Err(store(s.span, "COMPOSE")),
            StmtKind::Build { .. } => Err(store(s.span, "BUILD")),
            StmtKind::Activate(_)
            | StmtKind::Reactivate { .. }
            | StmtKind::Cancel(_)
            | StmtKind::Kill(_)
            | StmtKind::Delay { .. }
            | StmtKind::Hold { .. }
            | StmtKind::Passivate
            | StmtKind::Terminate => Err(store(s.span, "a process")),
            StmtKind::Chain(n) => Err(Flow::Chain(n.clone())),
            StmtKind::Call(c) => {
                let Some(m) = self
                    .module(&c.name)
                    .filter(|m| m.kind == ModuleKind::Subroutine)
                else {
                    return Err(error(
                        c.name.span,
                        format!("`{}` is not a subroutine of this file", c.name.spelling),
                    ));
                };
                let args = self.args(m, c, f)?;
                self.call(m, args, None).map(|_| ())
            }
            StmtKind::Resume(c) => self.resume(c, f),
            StmtKind::Read(vs) => {
                for v in vs {
                    let line = self.io.lock().unwrap().read_line();
                    let line =
                        line.ok_or_else(|| error(v.name.span, "READ finds no more input"))?;
                    let ty = self.var(&v.name, f)?.lock().unwrap().ty.ty.clone();
                    let value = self.parse_input(&line, &ty, v.name.span)?;
                    self.assign(v, value, f, v.name.span)?;
                }
                Ok(())
            }
            StmtKind::Print(es) => {
                let mut texts = Vec::new();
                for e in es {
                    texts.push(self.eval(e, f)?.to_string());
                }
                self.io.lock().unwrap().write(&(texts.join(" ") + "\n"));
                Ok(())
            }
        }
    }

    fn with_list(
        &self,
        n: &Name,
        f: &mut Frame,
        change: impl FnOnce(&mut List) -> Exec<()>,
    ) -> Exec<()> {
        let s = self.var(n, f)?;
        let mut var = s.lock().unwrap();
        match &mut var.value {
            Some(Value::List(l)) => change(l),
            _ => Err(error(n.span, format!("`{}` is not a list", n.spelling))),
        }
    }

    /// A line of input as a value of type `ty`: text as it is, anything
    /// else read as a literal, fuzzy ones included.
    fn parse_input(&self, line: &str, ty: &Ty, span: Span) -> Exec<Value> {
        if matches!(ty, Ty::String | Ty::Character) {
            return Ok(Value::exact(Scalar::String(line.to_string())));
        }
        let parsed = lower::parse_fragment(&format!("x := {line}"));
        let value = match parsed.ok().as_ref().map(|p| p.items.as_slice()) {
            Some(
                [
                    Item::Stmt(Stmt {
                        kind: StmtKind::Assign { value, .. },
                        ..
                    }),
                ],
            ) => value.clone(),
            _ => return Err(error(span, format!("READ cannot read {line:?}"))),
        };
        self.eval(&value, &mut Frame::default())
            .map_err(|_| error(span, format!("READ cannot read {line:?}")))
    }

    // Expressions

    fn read(&self, v: &Variable, f: &mut Frame) -> Exec<Value> {
        let s = self.var(&v.name, f)?;
        let indices = v
            .indices
            .iter()
            .map(|i| {
                let x = self.eval(i, f)?;
                at(x.as_integer(), i.span)
            })
            .collect::<Exec<Vec<i64>>>()?;
        let var = s.lock().unwrap();
        let Some(value) = &var.value else {
            return Err(error(
                v.name.span,
                format!("`{}` has no value", v.name.spelling),
            ));
        };
        if indices.is_empty() {
            return Ok(value.clone());
        }
        match value {
            Value::Array(a) => {
                let i = at(offset(a, &indices), v.name.span)?;
                Ok(a.cells[i].clone())
            }
            _ => Err(error(
                v.name.span,
                format!("`{}` is not an array", v.name.spelling),
            )),
        }
    }

    fn eval(&self, e: &Expr, f: &mut Frame) -> Exec<Value> {
        match &e.kind {
            ExprKind::Number(n) => Ok(Value::One(at(literal(n), e.span)?)),
            ExprKind::Multiple(values) => {
                let mut out = Vec::new();
                for (negative, n) in values {
                    let fz = at(literal(n), e.span)?;
                    out.push(if *negative {
                        at(value::unary_fz(UnaryOp::Neg, &fz), e.span)?
                    } else {
                        fz
                    });
                }
                Ok(Value::Many(out))
            }
            ExprKind::String(s) => {
                let mut chars = s.chars();
                Ok(Value::exact(match (chars.next(), chars.next()) {
                    (Some(c), None) => Scalar::Character(c),
                    _ => Scalar::String(s.clone()),
                }))
            }
            ExprKind::Boolean(b) => Ok(Value::boolean(*b)),
            ExprKind::Variable(v) => self.read(v, f),
            ExprKind::Call(c) => self.function(c, f, e.span),
            ExprKind::Unary(op, x) => {
                let v = self.eval(x, f)?;
                at(value::unary(*op, &v), e.span)
            }
            ExprKind::Binary(op, l, r) => {
                let a = self.eval(l, f)?;
                let b = self.eval(r, f)?;
                at(value::binary(*op, &a, &b), e.span)
            }
            ExprKind::Functor(adt, calls) => {
                let (mut v, mut s) = self.prefix(adt, f, e.span)?;
                for c in calls {
                    v = self.apply(v, s.take(), c, f)?;
                }
                Ok(v)
            }
            ExprKind::Attribute(..) => Err(store(e.span, "an object's attribute")),
            ExprKind::Link(..) => Err(store(e.span, "a link between objects")),
        }
    }

    fn function(&self, c: &Call, f: &mut Frame, span: Span) -> Exec<Value> {
        if let Some(m) = self.module(&c.name) {
            if !matches!(m.kind, ModuleKind::Function { .. }) {
                return Err(error(
                    c.name.span,
                    format!("`{}` is not a function", c.name.spelling),
                ));
            }
            let args = self.args(m, c, f)?;
            let v = self.call(m, args, None)?;
            return Ok(v.expect("a function's result"));
        }
        if !builtins::is_function(&c.name.key) {
            return Err(error(
                c.name.span,
                format!("`{}` is not a function of this file", c.name.spelling),
            ));
        }
        let [arg] = c.args.as_slice() else {
            return Err(error(
                c.name.span,
                format!("`{}` takes one argument", c.name.spelling),
            ));
        };
        let v = self.eval(arg, f)?;
        let Value::One(x) = v else {
            return Err(error(
                arg.span,
                format!("`{}` takes one number", c.name.spelling),
            ));
        };
        at(math(&c.name.key, &x).map(Value::One), span)
    }

    /// A functor's prefix: a list, held by a variable (whose slot the
    /// functors that move the cursor or take an element out act on) or
    /// computed.
    fn prefix(&self, adt: &Adt, f: &mut Frame, span: Span) -> Exec<(Value, Option<Slot>)> {
        match (&adt.designation, &adt.part) {
            (Designation::Named(AdtName::Name(n)), AdtPart::None) => {
                let v = self.read(
                    &Variable {
                        name: n.clone(),
                        indices: Vec::new(),
                    },
                    f,
                )?;
                Ok((v, Some(self.var(n, f)?)))
            }
            (Designation::Named(AdtName::Expr(e)), AdtPart::None) => Ok((self.eval(e, f)?, None)),
            _ => Err(store(span, "an a.d.t. designation")),
        }
    }

    fn apply(&self, prefix: Value, held: Option<Slot>, c: &Call, f: &mut Frame) -> Exec<Value> {
        let span = c.name.span;
        if let Some(m) = self.module(&c.name) {
            if !matches!(m.kind, ModuleKind::Functor { .. }) {
                return Err(error(
                    span,
                    format!("`{}` is not a functor", c.name.spelling),
                ));
            }
            let args = self.args(m, c, f)?;
            let v = self.call(m, args, Some(prefix))?;
            return Ok(v.expect("a functor's result"));
        }
        let Some(functor) = builtins::functor(&c.name.key) else {
            return Err(error(
                span,
                format!("`{}` is not a functor of this file", c.name.spelling),
            ));
        };
        if functor.takes != builtins::Takes::Lists {
            return Err(store(span, &format!("the functor `{}`", c.name.spelling)));
        }
        let args = c
            .args
            .iter()
            .map(|a| self.eval(a, f))
            .collect::<Exec<Vec<Value>>>()?;
        let Value::List(mut list) = prefix else {
            return Err(error(
                span,
                format!("`{}` applies to a list", c.name.spelling),
            ));
        };
        let result = at(list_functor(&c.name.key, &mut list, &args), span)?;
        // The functors that move the cursor or take an element out change
        // the list a variable holds.
        if let Some(s) = held {
            s.lock().unwrap().value = Some(Value::List(list));
        }
        Ok(result)
    }
}

fn offset(a: &value::Array, indices: &[i64]) -> Computed<usize> {
    if indices.len() != a.dims.len() {
        return Err(value::Fault(format!(
            "{} index(es) for an array of {}",
            indices.len(),
            a.dims.len()
        )));
    }
    let mut at = 0;
    for ((i, low), dim) in indices.iter().zip(&a.lows).zip(&a.dims) {
        let k = i - low;
        if k < 0 || k >= *dim as i64 {
            return Err(value::Fault(format!(
                "the index {i} is outside {low}:{}",
                low + *dim as i64 - 1
            )));
        }
        at = at * dim + k as usize;
    }
    Ok(at)
}

fn real(r: &Real) -> f64 {
    match r {
        Real::Integer(i) => *i as f64,
        Real::Float(f) => *f,
    }
}

fn literal(n: &FuzzyNumber) -> Computed<Fz> {
    let int = |i: u64| i64::try_from(i).map_err(|_| value::Fault("integer overflow".into()));
    let x = match n.value {
        Number::Real(Real::Integer(i)) => Scalar::Integer(int(i)?),
        Number::Real(Real::Float(f)) => Scalar::Float(f),
        Number::Imaginary(Real::Integer(i), _) => Scalar::Gauss(0, int(i)?),
        Number::Imaginary(Real::Float(f), Unit::I) => Scalar::Complex(0.0, f),
        Number::Imaginary(Real::Float(f), Unit::J) => Scalar::Quaternion([0.0, 0.0, f, 0.0]),
        Number::Imaginary(Real::Float(f), Unit::K) => Scalar::Quaternion([0.0, 0.0, 0.0, f]),
    };
    Ok(Fz {
        x,
        appr: n.approximation.as_ref().map(real),
        prob: n.probability.as_ref().map(real),
        subj: n.opinion.clone(),
    })
}

fn constant(c: &Constant) -> Value {
    Value::exact(match c {
        Constant::Integer(i) => Scalar::Integer(*i),
        Constant::Float(x) => Scalar::Float(*x),
        Constant::Complex { re, im } => Scalar::Complex(*re, *im),
        Constant::Gauss { re, im } => Scalar::Gauss(*re, *im),
        Constant::Quaternion(q) => Scalar::Quaternion(*q),
        Constant::Rational(n, d) => {
            value::rational(*n as i128, *d as i128).unwrap_or(Scalar::Rational(*n, 1))
        }
        Constant::Boolean(b) => Scalar::Boolean(*b),
        Constant::String(s) => Scalar::String(s.clone()),
    })
}

/// Our math functions, each with its derivative for the approximation.
fn math(key: &str, x: &Fz) -> Computed<Fz> {
    let integral = |fz: Fz| -> Computed<Fz> {
        match fz.x {
            Scalar::Float(v) if v.abs() < 9.2e18 => Ok(Fz {
                x: Scalar::Integer(v as i64),
                appr: None,
                ..fz
            }),
            _ => Err(value::Fault("integer overflow".into())),
        }
    };
    match key {
        "SQRT" => value::real_function(x, f64::sqrt, |v| 0.5 / v.sqrt()),
        "ABS" => match x.x {
            Scalar::Integer(i) => Ok(Fz {
                x: Scalar::Integer(
                    i.checked_abs()
                        .ok_or_else(|| value::Fault("integer overflow".into()))?,
                ),
                ..x.clone()
            }),
            _ => value::real_function(x, f64::abs, |_| 1.0),
        },
        "EXP" => value::real_function(x, f64::exp, f64::exp),
        "LN" => value::real_function(x, f64::ln, |v| 1.0 / v),
        "SIN" => value::real_function(x, f64::sin, f64::cos),
        "COS" => value::real_function(x, f64::cos, f64::sin),
        "TAN" => value::real_function(x, f64::tan, |v| 1.0 / (v.cos() * v.cos())),
        "ATAN" => value::real_function(x, f64::atan, |v| 1.0 / (1.0 + v * v)),
        "FLOOR" => integral(value::real_function(x, f64::floor, |_| 0.0)?),
        "CEIL" => integral(value::real_function(x, f64::ceil, |_| 0.0)?),
        _ => integral(value::real_function(x, f64::round, |_| 0.0)?),
    }
}

/// The list functors of page 80.
fn list_functor(key: &str, l: &mut List, args: &[Value]) -> Computed<Value> {
    let fault = |m: &str| Err(value::Fault(m.into()));
    let position = |v: &Value, l: &List| -> Computed<usize> {
        let p = v.as_integer()?;
        match usize::try_from(p) {
            Ok(p) if p >= 1 && p <= l.items.len() => Ok(p - 1),
            _ => Err(value::Fault(format!(
                "no element {p} in a list of {}",
                l.items.len()
            ))),
        }
    };
    if l.items.is_empty() && !matches!(key, "CARD" | "SUBLIST" | "OCCUR" | "POSLP" | "SIGMA") {
        return fault("the list is empty");
    }
    let last = l.items.len().saturating_sub(1);
    Ok(match (key, args) {
        ("FIRST", []) => {
            l.cursor = 0;
            l.items[0].clone()
        }
        ("LAST", []) => {
            l.cursor = last;
            l.items[last].clone()
        }
        ("CURRENT", []) => l.items[l.cursor].clone(),
        ("SUCC", []) => {
            if l.cursor == last {
                if !l.circular {
                    return fault("the current element has no successor");
                }
                l.cursor = 0;
            } else {
                l.cursor += 1;
            }
            l.items[l.cursor].clone()
        }
        ("PRED", []) => {
            if l.cursor == 0 {
                if !l.circular {
                    return fault("the current element has no predecessor");
                }
                l.cursor = last;
            } else {
                l.cursor -= 1;
            }
            l.items[l.cursor].clone()
        }
        ("FIRSTOUT", []) => {
            l.cursor = 0;
            l.items.remove(0)
        }
        ("LASTOUT", []) => {
            l.cursor = l.cursor.min(last.saturating_sub(1));
            l.items.remove(last)
        }
        ("CURRENTOUT", []) => {
            let v = l.items.remove(l.cursor);
            l.cursor = l.cursor.min(l.items.len().saturating_sub(1));
            v
        }
        ("CARD", []) => Value::integer(l.items.len() as i64),
        ("ELMT", [p]) => l.items[position(p, l)?].clone(),
        ("SUBLIST", [p, n]) => {
            let start = position(p, l)?;
            let n = usize::try_from(n.as_integer()?).unwrap_or(0);
            let end = (start + n).min(l.items.len());
            Value::List(List::new(l.items[start..end].to_vec()))
        }
        ("OCCUR", [x, p]) => {
            let start = position(p, l)?;
            Value::integer(l.items[start..].iter().filter(|v| *v == x).count() as i64)
        }
        ("POSLP", [x, p]) => {
            let start = position(p, l)?;
            let found = l.items[start..].iter().position(|v| v == x);
            Value::integer(found.map_or(0, |i| (start + i + 1) as i64))
        }
        ("SIGMA", []) => {
            let mut sum = Value::integer(0);
            for v in &l.items {
                sum = value::binary(BinaryOp::Add, &sum, v)?;
            }
            sum
        }
        (name, _) => {
            return Err(value::Fault(format!(
                "`{name}` with these arguments: not yet"
            )));
        }
    })
}
