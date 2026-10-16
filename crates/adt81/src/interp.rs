//! The interpreter of ADT'81's core: expressions, control flow, modules,
//! lists, fuzzy values and coroutines. docs/adt81.md, "Running", gives
//! the semantics and the decisions behind them.
//!
//! A tree walk over the [`ast`](super::ast) of a program the checker has
//! passed. Classes, objects, attributes and links live in the HBDS
//! [`Store`], whose COND and ACTION run when they change, a statement
//! being a transaction; the expert-system engines (hbds-rules) run rules
//! through it, CALLed as CHAV and HYPOCHAV. Processes are the simulation
//! engine's (hbds-simulation): each body runs as a coroutine, and the
//! main program, a process too, runs the sequencing set when it holds.
//!
//! Each coroutine runs on its own thread, and control passes by channel,
//! so that only one thread ever runs: RESUME continues a coroutine where
//! it paused, its "Local Sequence Counter" (page 112), and PAUSE gives
//! control back to whoever resumed it.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;

use super::ast::*;
use super::builtins;
use super::lower;
use super::types::{Ty, Typed};
use super::uses::{self, Part};
use super::value::{self, Ask, Computed, Fz, List, Scalar, Value};
use hbds_core::{AdtRef, Id, fold};
use hbds_gtad::{ClassKind, Code, State as AdtState, Store, Stored};
use hbds_simulation::{Agent, Sequencing, State as Lifecycle, When as Due};

/// The language the store's COND and ACTION are tagged with, when ADT'81
/// writes them.
pub const LANGUAGE: &str = "ADT'81";

/// A SPECIFY's specification, as the store keeps it.
fn specification(s: &Specification) -> hbds_core::spec::Spec {
    use hbds_core::spec::{Base, Fuzziness as Z, Scalar as S, Spec};
    let fuzz = |f: &Fuzziness| match *f {
        Fuzziness::Fuzzy => Z {
            appr: true,
            prob: true,
            subj: true,
            mult: true,
        },
        Fuzziness::Kinds {
            appr,
            prob,
            subj,
            mult,
        } => Z {
            appr,
            prob,
            subj,
            mult,
        },
    };
    let scalar = |t: &ScalarType| {
        let (base, long) = match t.base {
            BaseType::Numeric { kind, long } => (
                match kind {
                    NumericKind::Integer => Base::Integer,
                    NumericKind::Float => Base::Float,
                    NumericKind::Complex => Base::Complex,
                    NumericKind::Gauss => Base::Gauss,
                    NumericKind::Quaternion => Base::Quaternion,
                    NumericKind::Rational => Base::Rational,
                },
                long,
            ),
            BaseType::Boolean => (Base::Boolean, false),
            BaseType::Character => (Base::Character, false),
            BaseType::String => (Base::String, false),
            BaseType::Adt(a) => (Base::Adt(super::types::hbds_type(a)), false),
        };
        S {
            base,
            long,
            fuzziness: fuzz(&t.fuzziness),
        }
    };
    match s {
        Specification::Scalar(t) => Spec::Scalar(scalar(t)),
        Specification::Array { rank, element } => Spec::Array {
            rank: value::rank(*rank),
            element: element.as_ref().map(scalar),
        },
        Specification::List(element) => Spec::List(element.as_ref().map(scalar)),
        Specification::Compound => Spec::Compound,
        Specification::Fuzziness(f) => Spec::Fuzziness(fuzz(f)),
    }
}

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

/// Runs the file's first PROGRAM, then any it CHAINs to, on `store`.
pub fn run(
    program: Program,
    io: Arc<Mutex<dyn Io>>,
    store: Arc<Mutex<Store>>,
) -> Result<(), RuntimeError> {
    run_counted(program, io, store).map(|_| ())
}

/// [`run`], and how many statements it executed, coroutines and code the
/// engines called back included: the benchmarks' count of the work done.
pub fn run_counted(
    program: Program,
    io: Arc<Mutex<dyn Io>>,
    store: Arc<Mutex<Store>>,
) -> Result<u64, RuntimeError> {
    let mut modules = HashMap::new();
    let mut computed = HashMap::new();
    let mut bodies = HashMap::new();
    for (i, m) in program.modules.iter().enumerate() {
        match &m.name {
            ModuleName::Name(n) => {
                modules.entry(n.key.clone()).or_insert(i);
            }
            ModuleName::Attribute { class, attribute } => {
                computed.insert((fold(&class.text), fold(&attribute.text)), i);
            }
            ModuleName::Body(class) => {
                bodies.insert(fold(&class.text), i);
            }
        }
    }
    let Some(current) = program
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
        store,
        computed: Arc::new(computed),
        ses: Arc::new(Mutex::new(Ses::default())),
        bodies: Arc::new(bodies),
        sim: Arc::new(Mutex::new(Sequencing::new())),
        processes: Arc::new(Mutex::new(HashMap::new())),
        role: Role::Main,
        statements: Arc::new(AtomicU64::new(0)),
    };
    // The interpreter recurses (statements, calls, CONDs and ACTIONs): it
    // runs on a thread of its own, with the stack that takes.
    let runner = machine.clone();
    let result = thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || runner.main(current))
        .expect("a thread for the program")
        .join()
        .expect("the program's thread");
    // Paused coroutines wait for a RESUME that will not come: let them go.
    machine.coroutines.lock().unwrap().clear();
    machine.processes.lock().unwrap().clear();
    result?;
    // The processes as the run leaves them: implicitly persistent (page
    // 112), the sequencing set gone with the run.
    let mut store = machine.store.lock().unwrap();
    for (p, state, clock) in machine.sim.lock().unwrap().processes() {
        if store.objects().contains_key(&p) {
            hbds_simulation::record(&mut store, p, state, clock).map_err(|e| RuntimeError {
                span: Span { line: 1, column: 1 },
                message: e.0,
            })?;
        }
    }
    Ok(machine.statements.load(Ordering::Relaxed))
}

/// The stack of each thread the interpreter runs on.
const STACK: usize = 256 << 20;

impl Machine {
    /// Runs program `current`, then those it CHAINs to.
    fn main(&self, mut current: usize) -> Result<(), RuntimeError> {
        let machine = self;
        loop {
            let module = &machine.program.modules[current];
            match machine.call(module, Vec::new(), None, None) {
                Ok(_) | Err(Flow::Return | Flow::Stop) => break Ok(()),
                Err(Flow::Restart) => {}
                Err(Flow::Chain(q)) => match machine.modules.get(&q.key) {
                    Some(&i) if machine.program.modules[i].kind == ModuleKind::Program => {
                        current = i
                    }
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
        }
    }
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

/// An error inside a COND or an ACTION, told from the statement whose
/// change ran it.
fn within(flow: Flow, kind: &str, what: &str, span: Span) -> Flow {
    match flow {
        // Told once, from the outermost: nested ACTIONs say no more.
        Flow::Error(e) if e.message.starts_with("in the ") => error(span, e.message),
        Flow::Error(e) => error(span, format!("in the {kind} of {what}: {}", e.message)),
        flow => flow,
    }
}

/// A store's refusal, placed at `span`.
fn stored<T>(r: Stored<T>, span: Span) -> Exec<T> {
    r.map_err(|e| error(span, e.0))
}

fn not_yet(span: Span, what: &str) -> Flow {
    error(span, format!("{what}: not yet"))
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
    /// The object a computed attribute is computed for, or the one whose
    /// change runs a COND or an ACTION.
    this: Option<AdtRef>,
    /// How many COND and ACTION this frame runs within.
    depth: usize,
}

/// The rule code running, for the expert-system engines (docs/hbds.md,
/// "Expert systems").
#[derive(Default, Clone)]
struct Ses {
    /// How many rules' code this runs within: their changes may touch
    /// facts that are not MODIFIABLE.
    inside: usize,
    /// In PREMISES: the lowest probability read, and whether a fact read
    /// was not known.
    premises: Option<(f64, bool)>,
    /// In YES or NO: the probability what is written takes.
    stamp: Option<f64>,
}

/// How deep CONDs and ACTIONs may nest, an ACTION's change running others.
const DEPTH: usize = 64;

/// A change that runs CONDs and ACTIONs (docs/hbds.md, "COND and ACTION").
enum Invoked {
    Created(Id),
    Set(Id, Id),
    Linked(Id),
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
    store: Arc<Mutex<Store>>,
    /// The functions that compute attributes, by class and attribute.
    computed: Arc<HashMap<(String, String), usize>>,
    ses: Arc<Mutex<Ses>>,
    /// The bodies of process classes, by class (page 112).
    bodies: Arc<HashMap<String, usize>>,
    sim: Arc<Mutex<Sequencing>>,
    /// Each process's body, a coroutine, once it started.
    processes: Arc<Mutex<HashMap<Id, Coroutine>>>,
    /// Who this thread runs for.
    role: Role,
    /// Statements executed, on every thread.
    statements: Arc<AtomicU64>,
}

/// Who a thread of the interpreter runs for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Main,
    Process(Id),
    /// A coroutine RESUME started.
    Coroutine,
}

/// The expert-system engines' way into ADT'81: they decide what runs
/// when, ADT'81 runs it.
struct Engine<'m> {
    m: &'m Machine,
}

/// What went wrong in a rule's part, or in HYPOCHAV's hypotheses.
fn told(flow: Flow, part: &str, rule: Option<&str>) -> String {
    let message = match flow {
        Flow::Error(e) if e.message.starts_with("in ") => return e.message,
        Flow::Error(e) => e.message,
        _ => "EXIT, NEXT, RELOOP, STOP, RESTART or CHAIN leaves it".into(),
    };
    match rule {
        Some(r) => format!("in the {part} of {r}: {message}"),
        None => format!("in HYPOCHAV's hypotheses: {message}"),
    }
}

impl hbds_rules::Compiler for Engine<'_> {
    fn uses(&self, part: &str, code: &Code) -> Result<hbds_rules::Uses, String> {
        hbds_rules::Compiler::uses(&uses::Adt81, part, code)
    }
}

impl hbds_rules::Host for Engine<'_> {
    fn store(&self) -> &Mutex<Store> {
        &self.m.store
    }

    fn premises(&mut self, rule: Id, code: &Code) -> Result<(bool, f64), String> {
        let what = self.m.describe(AdtRef::Object(rule));
        let Part::Expr(e) = self.m.part("PREMISES", code, &what)? else {
            unreachable!("PREMISES are an expression")
        };
        let mut f = Frame {
            this: Some(AdtRef::Object(rule)),
            ..Frame::default()
        };
        let (holds, ses) = self.m.with_ses(
            |s| {
                s.inside += 1;
                s.premises = Some((1.0, false));
                s.stamp = None;
            },
            || self.m.truth(&e, &mut f),
        );
        let (reliability, unknown) = ses.premises.expect("in PREMISES");
        match holds {
            Ok(holds) => Ok((holds, reliability)),
            Err(_) if unknown => Ok((false, reliability)),
            Err(flow) => Err(told(flow, "PREMISES", Some(&what))),
        }
    }

    fn run(
        &mut self,
        rule: Option<Id>,
        part: &str,
        code: &Code,
        reliability: f64,
    ) -> Result<(), String> {
        let what = rule.map(|r| self.m.describe(AdtRef::Object(r)));
        let named = what.as_deref().unwrap_or("HYPOCHAV");
        let Part::Stmts(fragment) = self.m.part(part, code, named)? else {
            unreachable!("{part} is statements")
        };
        let mut f = Frame {
            this: rule.map(AdtRef::Object),
            ..Frame::default()
        };
        let (done, _) = self.m.with_ses(
            |s| {
                s.inside += 1;
                s.premises = None;
                s.stamp = (reliability < 1.0).then_some(reliability);
            },
            || self.m.run_fragment(&fragment, &mut f),
        );
        done.map_err(|flow| told(flow, part, what.as_deref()))
    }

    fn holds(&mut self, code: &Code, this: AdtRef) -> Result<bool, String> {
        let at = Span { line: 1, column: 1 };
        self.m
            .fact_holds(code, "EDITABLE", this, at)
            .map_err(|flow| match flow {
                Flow::Error(e) => e.message,
                _ => "an EDITABLE leaves by a jump".into(),
            })
    }

    fn edit(&mut self, line: &str) {
        self.m.io.lock().unwrap().write(&format!("{line}\n"));
    }
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

    fn call(
        &self,
        m: &Module,
        args: Vec<Arg>,
        prefix: Option<Value>,
        this: Option<AdtRef>,
    ) -> Exec<Option<Value>> {
        let mut f = Frame {
            this,
            ..Frame::default()
        };
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
            rank: value::rank(shape.rank),
            lows,
            dims,
            cells: vec![fill; size].into(),
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
            machine.role = Role::Coroutine;
            self.coroutines.lock().unwrap().insert(
                c.name.key.clone(),
                Coroutine {
                    go,
                    back: back.clone(),
                    state: State::Running,
                },
            );
            let index = self.modules[&c.name.key];
            let spawned = thread::Builder::new().stack_size(STACK).spawn(move || {
                let module = &machine.program.modules[index];
                let ended = machine.call(module, args, None, None).map(|_| ());
                let _ = link.back.send(Event::Ended(ended));
            });
            spawned.map_err(|e| error(c.name.span, format!("no thread for the coroutine: {e}")))?;
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
                Arc::make_mut(&mut a.cells)[i] = v;
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
        self.statements.fetch_add(1, Ordering::Relaxed);
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
                    return Err(not_yet(s.span, "EACH without OF"));
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
                    for item in items.iter().cloned() {
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
            StmtKind::Pause if matches!(self.role, Role::Process(_)) => {
                Err(error(s.span, "PAUSE in a process: it HOLDs or PASSIVATEs"))
            }
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
                        Value::List(l) => new.extend(l.items.iter().cloned()),
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
                    let (at, span) = match position {
                        None => (None, s.span),
                        Some((p, k, span)) => {
                            let k = usize::try_from(k).unwrap_or(usize::MAX);
                            let k = match p {
                                Position::After => k,
                                Position::Before => k.saturating_sub(1),
                            };
                            (Some(k), span)
                        }
                    };
                    hbds_lists::insert(l, new, at).map_err(|m| error(span, m))
                })
            }
            StmtKind::Extract { items, list } => {
                let mut out = Vec::new();
                for i in items {
                    match self.eval(i, f)? {
                        Value::List(l) => out.extend(l.items.iter().cloned()),
                        v => out.push(v),
                    }
                }
                self.with_list(list, f, |l| {
                    hbds_lists::extract(l, &out);
                    Ok(())
                })
            }
            StmtKind::Empty(list) => self.with_list(list, f, |l| {
                hbds_lists::empty(l);
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
            StmtKind::Create(o) => {
                self.atomic(s.span, |m| m.create(o, ClassKind::Class, f, s.span))
            }
            StmtKind::Design(o) => self.create(o, ClassKind::Embryo, f, s.span),
            StmtKind::StateChange(op, o) => self.state_change(*op, o, f, s.span),
            StmtKind::Let { target, value } => {
                self.atomic(s.span, |m| m.let_(target, value, f, s.span))
            }
            StmtKind::Assert(Assertion::Link {
                from,
                negated,
                relation,
                probability,
                to,
            }) => {
                let a = self.object(from, f, s.span)?;
                let b = self.object(to, f, s.span)?;
                let p = probability.as_ref().map(|p| match p {
                    Real::Integer(i) => *i as f64,
                    Real::Float(x) => *x,
                });
                let p = p.or(self.ses.lock().unwrap().stamp);
                self.atomic(s.span, |m| {
                    let mut store = m.store.lock().unwrap();
                    if *negated {
                        return stored(store.retract_link(a, &relation.text, b), s.span);
                    }
                    let l = stored(store.assert_link(a, &relation.text, b, p), s.span)?;
                    drop(store);
                    m.trigger(Invoked::Linked(l), f, s.span)
                })
            }
            StmtKind::Assert(Assertion::Name(_)) => Err(not_yet(s.span, "ASSERT of a name")),
            StmtKind::Specify {
                items,
                spec,
                unit,
                computed,
            } => {
                let text = Some(specification(spec));
                for item in items {
                    let target = match item {
                        Specified::Class(c) => AdtRef::Class(self.class(c, f, s.span)?),
                        Specified::Attribute(a) => self.class_attribute(a, f, s.span)?,
                        Specified::Link(l) => {
                            let from = self.class(&l.from, f, s.span)?;
                            let to = self.class(&l.to, f, s.span)?;
                            let store = self.store.lock().unwrap();
                            AdtRef::ClassLink(stored(
                                store.class_link(from, &l.relation.text, to),
                                s.span,
                            )?)
                        }
                    };
                    let unit = unit.as_ref().map(|u| u.spelling.clone());
                    let mut store = self.store.lock().unwrap();
                    stored(store.specify(target, text, unit, *computed), s.span)?;
                }
                Ok(())
            }
            StmtKind::Include { classes, into }
            | StmtKind::Exclude {
                classes,
                from: into,
            } => {
                let include = matches!(s.kind, StmtKind::Include { .. });
                let classes = self.classes(classes, f, s.span)?;
                let hypers = self.classes(into, f, s.span)?;
                let mut store = self.store.lock().unwrap();
                for c in &classes {
                    for h in &hypers {
                        let r = if include {
                            store.include(*c, *h)
                        } else {
                            store.exclude(*c, *h)
                        };
                        stored(r, s.span)?;
                    }
                }
                Ok(())
            }
            StmtKind::Associate(pairs) | StmtKind::Dissociate(pairs) => {
                let assoc = matches!(s.kind, StmtKind::Associate(_));
                for (a, d) in pairs {
                    let a = self.adt(a, f, s.span)?;
                    let b = self.designate(d, f, s.span, false)?;
                    let mut store = self.store.lock().unwrap();
                    stored(
                        if assoc {
                            store.assoc(a, b)
                        } else {
                            store.dissoc(a, b)
                        },
                        s.span,
                    )?;
                }
                Ok(())
            }
            StmtKind::Compose { class, with } => {
                let name = self.text(class, f, s.span)?;
                let embryos = self.classes(with, f, s.span)?;
                let mut store = self.store.lock().unwrap();
                stored(
                    store.create_class(&name, ClassKind::Prototype(embryos)),
                    s.span,
                )
                .map(|_| ())
            }
            StmtKind::Build { classes, model } => {
                let prototype = self.class(model, f, s.span)?;
                let mut names = Vec::new();
                for c in classes {
                    names.push(self.text(c, f, s.span)?);
                }
                let mut store = self.store.lock().unwrap();
                if !matches!(store.classes()[&prototype].kind, ClassKind::Prototype(_)) {
                    return Err(error(s.span, "BUILD builds structures AS a prototype"));
                }
                for n in names {
                    stored(
                        store.create_class(&n, ClassKind::Structure(prototype)),
                        s.span,
                    )?;
                }
                Ok(())
            }
            StmtKind::Activate(o) => self.reactivate(o, None, f, s.span),
            StmtKind::Reactivate { object, when } => {
                self.reactivate(object, when.as_ref(), f, s.span)
            }
            StmtKind::Cancel(o) => {
                let p = self.process(o, f, s.span)?;
                self.sim_op(s.span, |sim, d| sim.cancel(p, d))
            }
            StmtKind::Kill(o) => {
                let p = self.process(o, f, s.span)?;
                self.sim_op(s.span, |sim, d| sim.kill(p, d))
            }
            StmtKind::Delay { object, duration } => {
                let p = self.process(object, f, s.span)?;
                let dt = self.time(duration, f)?;
                self.sim_op(s.span, |sim, d| sim.delay(p, dt, d))
            }
            StmtKind::Hold { object, duration } => {
                let running = self.running(s.span)?;
                if let Some(o) = object
                    && self.process(o, f, s.span)? != running
                {
                    let what = self.describe(self.designate(o, f, s.span, false)?);
                    return Err(error(
                        s.span,
                        format!("HOLD holds the running process, not {what}"),
                    ));
                }
                let dt = self.time(duration, f)?;
                self.sim_op(s.span, |sim, _| sim.hold(dt))?;
                self.give_way(s.span)
            }
            StmtKind::Passivate => {
                self.running(s.span)?;
                self.sim_op(s.span, |sim, _| sim.passivate())?;
                self.give_way(s.span)
            }
            StmtKind::Terminate => {
                self.running(s.span)?;
                self.sim_op(s.span, |sim, _| sim.terminate())?;
                // The body stops here; its thread ends.
                Err(Flow::Stop)
            }
            StmtKind::Chain(n) => Err(Flow::Chain(n.clone())),
            StmtKind::Call(c)
                if self.module(&c.name).is_none() && builtins::is_subroutine(&c.name.key) =>
            {
                self.atomic(s.span, |m| m.engine(c, f, s.span))
            }
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
                self.call(m, args, None, None).map(|_| ())
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
                    let v = self.eval(e, f)?;
                    texts.push(self.store.lock().unwrap().show(&v));
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
            return Ok(Value::exact(Scalar::String(line.into())));
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
        // TIME, the simulation's, unless a variable of that name hides it.
        if v.name.key == "TIME" && v.indices.is_empty() && self.var(&v.name, f).is_err() {
            return Ok(Value::exact(Scalar::Float(self.sim.lock().unwrap().now())));
        }
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
                    _ => Scalar::String(s.clone().into()),
                }))
            }
            ExprKind::Boolean(b) => Ok(Value::boolean(*b)),
            ExprKind::This => f.this.map(Value::Adt).ok_or_else(|| {
                error(
                    e.span,
                    "THIS is the object a computed attribute, a COND, a rule or a process runs for",
                )
            }),
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
            ExprKind::Attribute(object, attribute) => {
                let o = self.object(object, f, e.span)?;
                let name = self.text(attribute, f, e.span)?;
                self.attribute_value(o, &name, e.span)
            }
            ExprKind::Link(from, relation, to) => {
                let a = self.object(from, f, e.span)?;
                let b = self.object(to, f, e.span)?;
                let store = self.store.lock().unwrap();
                let link = store.object_link(a, &relation.text, b);
                if let Some(l) = link {
                    self.read_fact(store.object_links()[&l].probability);
                }
                Ok(Value::boolean(link.is_some()))
            }
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
            let v = self.call(m, args, None, None)?;
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
    /// computed, or an a.d.t.
    fn prefix(&self, adt: &Adt, f: &mut Frame, span: Span) -> Exec<(Value, Option<Slot>)> {
        let held = match (&adt.designation, &adt.part) {
            (Designation::Named(AdtName::Name(n)), AdtPart::None) => {
                let var = Variable {
                    name: n.clone(),
                    indices: Vec::new(),
                };
                Some((self.read(&var, f)?, self.var(n, f)?))
            }
            (Designation::Named(AdtName::Expr(e)), AdtPart::None) => {
                let v = self.eval(e, f)?;
                if matches!(v, Value::List(_) | Value::Adt(_)) {
                    return Ok((v, None));
                }
                None
            }
            _ => None,
        };
        match held {
            Some((v @ Value::List(_), slot)) => Ok((v, Some(slot))),
            Some((v @ Value::Adt(_), _)) => Ok((v, None)),
            _ => Ok((Value::Adt(self.adt(adt, f, span)?), None)),
        }
    }

    /// A functor of the catalogue on a.d.t.: the prefix one or a list of
    /// them, the arguments strings.
    fn adt_functor(&self, name: &Name, prefix: &Value, args: &[Value]) -> Exec<Value> {
        let span = name.span;
        let refs = match prefix {
            Value::Adt(r) => vec![*r],
            Value::List(l) => l
                .items
                .iter()
                .map(|v| match v {
                    Value::Adt(r) => Ok(*r),
                    v => Err(error(
                        span,
                        format!("`{}` applies to a.d.t., not to {v}", name.spelling),
                    )),
                })
                .collect::<Exec<_>>()?,
            v => {
                return Err(error(
                    span,
                    format!("`{}` applies to a.d.t., not to {v}", name.spelling),
                ));
            }
        };
        let args = args
            .iter()
            .map(|a| match a.scalar() {
                Some(Scalar::String(s)) => Ok(s.to_string()),
                Some(Scalar::Character(c)) => Ok(c.to_string()),
                _ => Err(error(span, format!("`{}` takes strings", name.spelling))),
            })
            .collect::<Exec<Vec<String>>>()?;
        let store = self.store.lock().unwrap();
        let result = if hbds_rules::functor(&name.key).is_some() {
            let found = hbds_rules::apply(&store, &uses::Adt81, &name.key, &refs);
            found.map_err(|e| error(span, e))?
        } else {
            stored(hbds_functors::apply(&store, &name.key, &refs, &args), span)?
        };
        Ok(Value::List(List::new(
            result.into_iter().map(Value::Adt).collect(),
        )))
    }

    // A.d.t.

    /// What a name in a designation gives: a string naming a class or an
    /// object, or an a.d.t. a variable holds.
    fn named(&self, n: &AdtName, f: &mut Frame, span: Span) -> Exec<Result<String, AdtRef>> {
        let v = match n {
            AdtName::String(s) => {
                self.system(&s.text, span)?;
                return Ok(Ok(s.text.clone()));
            }
            AdtName::Name(name) => self.read(
                &Variable {
                    name: name.clone(),
                    indices: Vec::new(),
                },
                f,
            )?,
            AdtName::Expr(e) => self.eval(e, f)?,
        };
        match v {
            Value::Adt(r) => Ok(Err(r)),
            Value::One(Fz {
                x: Scalar::String(s),
                ..
            }) => {
                self.system(&s, span)?;
                Ok(Ok(s.to_string()))
            }
            Value::One(Fz {
                x: Scalar::Character(c),
                ..
            }) => Ok(Ok(c.to_string())),
            v => Err(error(span, format!("{v} names no a.d.t."))),
        }
    }

    /// A name, as a string.
    fn text(&self, n: &AdtName, f: &mut Frame, span: Span) -> Exec<String> {
        match self.named(n, f, span)? {
            Ok(s) => Ok(s),
            Err(r) => {
                let what = self.store.lock().unwrap().describe(r);
                Err(error(span, format!("{what} is not a name")))
            }
        }
    }

    /// A class, by its name or a variable holding it.
    fn class(&self, n: &AdtName, f: &mut Frame, span: Span) -> Exec<Id> {
        match self.named(n, f, span)? {
            Ok(name) => stored(self.store.lock().unwrap().class(&name), span),
            Err(AdtRef::Class(c)) => Ok(c),
            Err(r) => {
                let what = self.store.lock().unwrap().describe(r);
                Err(error(span, format!("{what} is not a class")))
            }
        }
    }

    fn classes(&self, ns: &[AdtName], f: &mut Frame, span: Span) -> Exec<Vec<Id>> {
        ns.iter().map(|n| self.class(n, f, span)).collect()
    }

    fn class_attribute(&self, a: &ClassAttribute, f: &mut Frame, span: Span) -> Exec<AdtRef> {
        let c = self.class(&a.class, f, span)?;
        let name = self.text(&a.attribute, f, span)?;
        let store = self.store.lock().unwrap();
        Ok(AdtRef::ClassAttribute(stored(
            store.attribute(c, &name),
            span,
        )?))
    }

    /// The a.d.t. a designation names. `any` finds hibernating and
    /// destroyed ones too, for AWAKE, RESUR and PURGE.
    fn designate(&self, d: &Designation, f: &mut Frame, span: Span, any: bool) -> Exec<AdtRef> {
        match d {
            Designation::This(at) => f.this.ok_or_else(|| {
                error(
                    *at,
                    "THIS is the object a computed attribute is computed for",
                )
            }),
            Designation::Named(n) => match self.named(n, f, span)? {
                Err(r) => Ok(r),
                Ok(name) => {
                    let store = self.store.lock().unwrap();
                    match (any, store.find_class(&name)) {
                        (true, Some(c)) => Ok(AdtRef::Class(c)),
                        _ => stored(store.class(&name), span).map(AdtRef::Class),
                    }
                }
            },
            Designation::Classed { name, class } => {
                let name = self.text(name, f, span)?;
                let class_name = self.text(class, f, span)?;
                let store = self.store.lock().unwrap();
                let c = match (any, store.find_class(&class_name)) {
                    (true, Some(c)) => c,
                    _ => stored(store.class(&class_name), span)?,
                };
                match (any, store.find_object(c, &name)) {
                    (true, Some(o)) => Ok(AdtRef::Object(o)),
                    _ => stored(store.object(c, &name), span).map(AdtRef::Object),
                }
            }
        }
    }

    fn object(&self, d: &Designation, f: &mut Frame, span: Span) -> Exec<Id> {
        match self.designate(d, f, span, false)? {
            AdtRef::Object(o) => Ok(o),
            r => {
                let what = self.store.lock().unwrap().describe(r);
                Err(error(span, format!("{what} is not an object")))
            }
        }
    }

    /// A designation with its part: an attribute, or a link.
    fn adt(&self, a: &Adt, f: &mut Frame, span: Span) -> Exec<AdtRef> {
        let base = self.designate(&a.designation, f, span, false)?;
        match &a.part {
            AdtPart::None => Ok(base),
            AdtPart::Attribute(n) => {
                let name = self.text(n, f, span)?;
                let store = self.store.lock().unwrap();
                match base {
                    AdtRef::Class(c) => Ok(AdtRef::ClassAttribute(stored(
                        store.attribute(c, &name),
                        span,
                    )?)),
                    AdtRef::Object(o) => {
                        let c = store.objects()[&o].class;
                        Ok(AdtRef::ObjectAttribute(
                            o,
                            stored(store.attribute(c, &name), span)?,
                        ))
                    }
                    r => Err(error(
                        span,
                        format!("{} has no attributes", store.describe(r)),
                    )),
                }
            }
            AdtPart::Link(relation, to) => {
                let to = self.designate(to, f, span, false)?;
                let store = self.store.lock().unwrap();
                match (base, to) {
                    (AdtRef::Class(a), AdtRef::Class(b)) => Ok(AdtRef::ClassLink(stored(
                        store.class_link(a, &relation.text, b),
                        span,
                    )?)),
                    (AdtRef::Object(a), AdtRef::Object(b)) => store
                        .object_link(a, &relation.text, b)
                        .map(AdtRef::ObjectLink)
                        .ok_or_else(|| error(span, "these objects are not linked so")),
                    _ => Err(error(span, "a link joins two classes or two objects")),
                }
            }
        }
    }

    /// An object's attribute: computed by its function (pages 127 and
    /// 128), with THIS the object, or stored.
    fn attribute_value(&self, o: Id, name: &str, span: Span) -> Exec<Value> {
        let (class, attribute, computed) = {
            let store = self.store.lock().unwrap();
            let c = store.objects()[&o].class;
            let a = stored(store.attribute(c, name), span)?;
            (
                store.classes()[&c].name.clone(),
                a,
                store.attributes()[&a].props.computed,
            )
        };
        if let Some(&i) = self.computed.get(&(fold(&class), fold(name))) {
            let m = &self.program.modules[i];
            let v = self.call(m, Vec::new(), None, Some(AdtRef::Object(o)))?;
            return Ok(v.expect("a function's result"));
        }
        if computed {
            return Err(error(
                span,
                format!("\"{class}\" . \"{name}\" is COMPUTED, but no FUNCTION computes it"),
            ));
        }
        if let Some(v) = self.live(o, attribute) {
            return Ok(v);
        }
        let found = self.store.lock().unwrap().get(o, attribute).cloned();
        match found {
            Some(v) => {
                if let Value::One(fz) = &v {
                    self.read_fact(fz.prob);
                }
                Ok(v)
            }
            None => self.unknown(o, attribute, span),
        }
    }

    /// A value PREMISES read: the lowest probability is theirs.
    fn read_fact(&self, probability: Option<f64>) {
        if let Some((low, _)) = &mut self.ses.lock().unwrap().premises {
            *low = low.min(probability.unwrap_or(1.0));
        }
    }

    /// A value not there: in PREMISES, asked if the fact is ASKABLE, and
    /// otherwise not known, which makes them false; elsewhere, an error.
    fn unknown(&self, o: Id, a: Id, span: Span) -> Exec<Value> {
        let what = self.describe(AdtRef::ObjectAttribute(o, a));
        if self.ses.lock().unwrap().premises.is_none() {
            return Err(error(span, format!("{what} has no value")));
        }
        let askable = self.property(AdtRef::ClassAttribute(a), "ASKABLE");
        if let Some(code) = askable
            && self.fact_holds(&code, "ASKABLE", AdtRef::Object(o), span)?
        {
            self.io.lock().unwrap().write(&format!("{what} ? "));
            let line = self.io.lock().unwrap().read_line();
            if let Some(line) = line {
                let v = match self.parse_input(line.trim(), &Ty::Unknown, span) {
                    Ok(v) => v,
                    Err(_) => Value::exact(Scalar::String(line.trim().into())),
                };
                stored(self.store.lock().unwrap().set(o, a, v.clone()), span)?;
                self.trigger(Invoked::Set(o, a), &Frame::default(), span)?;
                if let Value::One(fz) = &v {
                    self.read_fact(fz.prob);
                }
                return Ok(v);
            }
        }
        if let Some((_, unknown)) = &mut self.ses.lock().unwrap().premises {
            *unknown = true;
        }
        Err(error(span, format!("{what} is not known")))
    }

    /// An a.d.t. as PRINT shows it.
    fn describe(&self, r: AdtRef) -> String {
        self.store.lock().unwrap().describe(r)
    }

    /// A property an a.d.t. carries, if it does.
    fn property(&self, r: AdtRef, name: &str) -> Option<Code> {
        let store = self.store.lock().unwrap();
        store.props(r)?.properties.get(name).cloned()
    }

    /// Runs `body` with the rule state `set` makes, then puts it back;
    /// gives what `body` gives and the state it ended with.
    fn with_ses<T>(&self, set: impl FnOnce(&mut Ses), body: impl FnOnce() -> T) -> (T, Ses) {
        let saved = {
            let mut ses = self.ses.lock().unwrap();
            let saved = ses.clone();
            set(&mut ses);
            saved
        };
        let result = body();
        let ended = std::mem::replace(&mut *self.ses.lock().unwrap(), saved);
        (result, ended)
    }

    /// A property's code, ADT'81's and parsed, or what is wrong with it.
    fn part(&self, part: &str, code: &Code, what: &str) -> Result<Part, String> {
        if code.language != LANGUAGE {
            return Err(format!(
                "the {part} of {what} is written in {}, which ADT'81 does not run",
                code.language
            ));
        }
        uses::parse(part, &code.text)
            .map_err(|e| format!("the {part} of {what} does not parse: {e}"))
    }

    /// Whether a fact's ASKABLE, EDITABLE or MODIFIABLE holds, THIS the
    /// object concerned; what it reads is not the PREMISES'.
    fn fact_holds(&self, code: &Code, kind: &str, this: AdtRef, span: Span) -> Exec<bool> {
        let what = self.describe(this);
        let Part::Expr(e) = self.part(kind, code, &what).map_err(|m| error(span, m))? else {
            unreachable!("{kind} is an expression")
        };
        let mut f = Frame {
            this: Some(this),
            ..Frame::default()
        };
        let (holds, _) = self.with_ses(|s| s.premises = None, || self.truth(&e, &mut f));
        holds.map_err(|flow| within(flow, kind, &what, span))
    }

    /// FACT and RULE, created the first time a program names them.
    fn system(&self, name: &str, span: Span) -> Exec<()> {
        let rules = hbds_rules::is_system(name);
        if !rules && !hbds_simulation::is_system(name) {
            return Ok(());
        }
        let mut store = self.store.lock().unwrap();
        if store.find_class(name).is_some() {
            return Ok(());
        }
        match rules {
            true => stored(hbds_rules::install(&mut store), span),
            false => stored(hbds_simulation::install(&mut store), span),
        }
    }

    // Processes (pages 112 to 115, 125)

    /// STATE and CLOCK as the engine has them, while the run lasts; as the
    /// store kept them otherwise; passive at 0 for a process never run.
    fn live(&self, o: Id, attribute: Id) -> Option<Value> {
        let (state, kept) = {
            let store = self.store.lock().unwrap();
            if !hbds_simulation::is_live(&store, attribute) {
                return None;
            }
            let state =
                hbds_core::same(&store.attributes()[&attribute].name, hbds_simulation::STATE);
            (state, store.get(o, attribute).cloned())
        };
        let sim = self.sim.lock().unwrap();
        let p = Agent::Process(o);
        if !sim.knows(p) && kept.is_some() {
            return kept;
        }
        Some(if state {
            let name = hbds_simulation::state_name(sim.state(p));
            Value::exact(Scalar::String(name.into()))
        } else {
            Value::exact(Scalar::Float(sim.clock(p)))
        })
    }

    /// How an agent is named in an error.
    fn agent_name(&self, a: Agent) -> String {
        match a {
            Agent::Main => "the main program".into(),
            Agent::Process(p) => self.describe(AdtRef::Object(p)),
        }
    }

    /// An operation of the sequencing set, its refusal placed at `span`.
    fn sim_op(
        &self,
        span: Span,
        op: impl FnOnce(&mut Sequencing, hbds_simulation::Describe) -> Result<(), String>,
    ) -> Exec<()> {
        let mut sim = self.sim.lock().unwrap();
        op(&mut sim, &|a| self.agent_name(a)).map_err(|m| error(span, m))
    }

    /// A process a designation names.
    fn process(&self, d: &Designation, f: &mut Frame, span: Span) -> Exec<Agent> {
        let o = self.object(d, f, span)?;
        if hbds_simulation::is_process(&self.store.lock().unwrap(), o) {
            return Ok(Agent::Process(o));
        }
        let what = self.describe(AdtRef::Object(o));
        Err(error(
            span,
            format!("{what} is not a process: its class is not in PROCESS"),
        ))
    }

    /// Who runs here: the main program or a process; a coroutine does not
    /// hold, passivate nor terminate.
    fn running(&self, span: Span) -> Exec<Agent> {
        match self.role {
            Role::Main => Ok(Agent::Main),
            Role::Process(p) => Ok(Agent::Process(p)),
            Role::Coroutine => Err(error(
                span,
                "a coroutine RESUME started is not a process: it does not HOLD, PASSIVATE nor TERMINATE",
            )),
        }
    }

    /// A duration or a time, as a number.
    fn time(&self, e: &Expr, f: &mut Frame) -> Exec<f64> {
        let v = self.eval(e, f)?;
        match v.scalar() {
            Some(Scalar::Integer(i)) => Ok(*i as f64),
            Some(Scalar::Float(x)) => Ok(*x),
            Some(Scalar::Rational(n, d)) => Ok(*n as f64 / *d as f64),
            _ => Err(error(e.span, format!("a time is a real number, not {v}"))),
        }
    }

    /// The body of a process: its class's coroutine, or the nearest
    /// hyperclass's.
    fn body(&self, p: Id, span: Span) -> Exec<usize> {
        let store = self.store.lock().unwrap();
        let class = store.objects()[&p].class;
        for c in store.lineage(class) {
            if let Some(&i) = self.bodies.get(&fold(&store.classes()[&c].name)) {
                return Ok(i);
            }
        }
        let name = store.classes()[&class].name.clone();
        Err(error(
            span,
            format!("\"{name}\" has no body: COROUTINE \"{name}\" gives it one"),
        ))
    }

    /// ACTIVATE, and REACTIVATE AT, BEFORE or AFTER.
    fn reactivate(
        &self,
        o: &Designation,
        when: Option<&When>,
        f: &mut Frame,
        span: Span,
    ) -> Exec<()> {
        let p = self.process(o, f, span)?;
        let Agent::Process(id) = p else {
            unreachable!("a process")
        };
        let i = self.body(id, span)?;
        if !self.program.modules[i].parameters.is_empty() {
            return Err(error(span, "a process's body takes no parameters"));
        }
        let due = match when {
            None => Due::Now,
            Some(When::At(e)) => Due::At(self.time(e, f)?),
            Some(When::Before(q)) => Due::Before(self.process(q, f, span)?),
            Some(When::After(q)) => Due::After(self.process(q, f, span)?),
        };
        self.sim_op(span, |sim, d| sim.reactivate(p, due, d))
    }

    /// After HOLD or PASSIVATE: a body gives control back to the
    /// sequencing set; the main program runs it until it is due again.
    fn give_way(&self, span: Span) -> Exec<()> {
        if let Role::Process(_) = self.role {
            return self.pause();
        }
        loop {
            let next = self.sim.lock().unwrap().advance();
            match next {
                Some(Agent::Process(p)) => self.run_process(p, span)?,
                Some(Agent::Main) | None => return Ok(()),
            }
        }
    }

    /// Runs a process's body from where it stopped, its LSC, until it
    /// holds, passivates or ends.
    fn run_process(&self, p: Id, span: Span) -> Exec<()> {
        let started = self.processes.lock().unwrap().contains_key(&p);
        let back = if started {
            let map = self.processes.lock().unwrap();
            let co = &map[&p];
            co.go
                .send(())
                .map_err(|_| error(span, "the process is gone"))?;
            co.back.clone()
        } else {
            let index = self.body(p, span)?;
            let (go, go_rx) = channel();
            let (back_tx, back) = channel();
            let back = Arc::new(Mutex::new(back));
            let link = Arc::new(Link {
                back: back_tx,
                go: Mutex::new(go_rx),
            });
            let mut machine = self.clone();
            machine.link = Some(link.clone());
            machine.role = Role::Process(p);
            self.processes.lock().unwrap().insert(
                p,
                Coroutine {
                    go,
                    back: back.clone(),
                    state: State::Running,
                },
            );
            let spawned = thread::Builder::new().stack_size(STACK).spawn(move || {
                let module = &machine.program.modules[index];
                let ended = machine.call(module, Vec::new(), None, Some(AdtRef::Object(p)));
                let _ = link.back.send(Event::Ended(ended.map(|_| ())));
            });
            spawned.map_err(|e| error(span, format!("no thread for the process: {e}")))?;
            back
        };
        let event = back.lock().unwrap().recv();
        match event {
            Ok(Event::Paused) => Ok(()),
            Ok(Event::Ended(ended)) => {
                self.processes.lock().unwrap().remove(&p);
                let terminated =
                    self.sim.lock().unwrap().state(Agent::Process(p)) == Lifecycle::Terminated;
                match ended {
                    // TERMINATE, or the end of the body.
                    Err(Flow::Stop) if terminated => Ok(()),
                    Ok(()) | Err(Flow::Return) => self.sim_op(span, |sim, _| sim.terminate()),
                    Err(flow) => Err(flow),
                }
            }
            Err(_) => Err(Flow::Stop),
        }
    }

    /// CALL CHAV(rule classes) and CALL HYPOCHAV(hypotheses, rule
    /// classes): the expert-system engines (pages 106 and 107).
    fn engine(&self, c: &Call, f: &mut Frame, span: Span) -> Exec<()> {
        let mut args = c.args.iter();
        let hypotheses = if c.name.key == "HYPOCHAV" {
            let wrong = || {
                error(
                    span,
                    "HYPOCHAV takes its hypotheses first, statements in a string",
                )
            };
            let v = self.eval(args.next().ok_or_else(wrong)?, f)?;
            let text = match v.scalar() {
                Some(Scalar::String(s)) => s.to_string(),
                _ => return Err(wrong()),
            };
            let code = Code {
                language: LANGUAGE.into(),
                text,
            };
            self.part("HYPOTHESES", &code, "HYPOCHAV")
                .map_err(|_| error(span, "HYPOCHAV's hypotheses do not parse"))?;
            Some(code)
        } else {
            None
        };
        stored(hbds_rules::install(&mut self.store.lock().unwrap()), span)?;
        let mut scope = Vec::new();
        for a in args {
            let class = match self.eval(a, f)? {
                Value::Adt(AdtRef::Class(c)) => c,
                v => match v.scalar() {
                    Some(Scalar::String(name)) => {
                        stored(self.store.lock().unwrap().class(name), a.span)?
                    }
                    _ => {
                        return Err(error(
                            a.span,
                            format!("{} takes rule classes, not {v}", c.name.spelling),
                        ));
                    }
                },
            };
            scope.push(class);
        }
        let mut engine = Engine { m: self };
        let run = match &hypotheses {
            Some(h) => hbds_rules::hypochav(&mut engine, h, &scope),
            None => hbds_rules::chav(&mut engine, &scope),
        };
        run.map(|_| ()).map_err(|e| error(span, e))
    }

    /// Runs `body` as one transaction of the store: its changes, and those
    /// of the ACTIONs it runs, are kept together or refused together.
    fn atomic(&self, span: Span, body: impl FnOnce(&Self) -> Exec<()>) -> Exec<()> {
        self.store.lock().unwrap().begin();
        let result = body(self);
        let ended = self.store.lock().unwrap().end(result.is_ok());
        result?;
        stored(ended, span)
    }

    /// The CONDs and ACTIONs a change runs: the class's, class
    /// attribute's or class link's, then the object's own, THIS being the
    /// object (pages 156 to 158). A false COND refuses the change.
    fn trigger(&self, event: Invoked, f: &Frame, span: Span) -> Exec<()> {
        let (adts, this) = {
            let store = self.store.lock().unwrap();
            match event {
                Invoked::Created(o) => (vec![AdtRef::Class(store.objects()[&o].class)], o),
                Invoked::Set(o, a) => (vec![AdtRef::ClassAttribute(a)], o),
                Invoked::Linked(l) => {
                    let link = &store.object_links()[&l];
                    (vec![AdtRef::ClassLink(link.class_link)], link.from)
                }
            }
        };
        if self.ses.lock().unwrap().inside == 0 {
            for r in &adts {
                if let Some(code) = self.property(*r, "MODIFIABLE")
                    && !self.fact_holds(&code, "MODIFIABLE", AdtRef::Object(this), span)?
                {
                    let what = self.describe(*r);
                    return Err(error(
                        span,
                        format!("{what} is not MODIFIABLE but by a rule"),
                    ));
                }
            }
        }
        let adts = adts.into_iter().chain([AdtRef::Object(this)]);
        for r in adts {
            let (cond, action, what) = {
                let store = self.store.lock().unwrap();
                let props = store.props(r).map(|p| &p.properties);
                let code = |k: &str| props.and_then(|p| p.get(k)).cloned();
                (code("COND"), code("ACTION"), store.describe(r))
            };
            if cond.is_none() && action.is_none() {
                continue;
            }
            if f.depth >= DEPTH {
                return Err(error(span, "COND and ACTION nest too deep"));
            }
            let mut inner = Frame {
                this: Some(AdtRef::Object(this)),
                depth: f.depth + 1,
                ..Frame::default()
            };
            if let Some(code) = cond {
                self.ours(&code, "COND", &what, span)?;
                let parsed = lower::parse_fragment(&format!("x := {}", code.text));
                let value = match parsed.ok().as_ref().map(|p| p.items.as_slice()) {
                    Some(
                        [
                            Item::Stmt(Stmt {
                                kind: StmtKind::Assign { value, .. },
                                ..
                            }),
                        ],
                    ) => value.clone(),
                    _ => return Err(error(span, format!("the COND of {what} does not parse"))),
                };
                let holds = self
                    .truth(&value, &mut inner)
                    .map_err(|flow| within(flow, "COND", &what, span))?;
                if !holds {
                    return Err(error(
                        span,
                        format!("the COND of {what} refuses this change: {}", code.text),
                    ));
                }
            }
            if let Some(code) = action {
                self.ours(&code, "ACTION", &what, span)?;
                let fragment = lower::parse_fragment(&code.text).map_err(|e| {
                    error(span, format!("the ACTION of {what} does not parse: {e}"))
                })?;
                self.run_fragment(&fragment, &mut inner)
                    .map_err(|flow| within(flow, "ACTION", &what, span))?;
            }
        }
        Ok(())
    }

    /// Code written in ADT'81, or an error saying in what it is written.
    fn ours(&self, code: &Code, kind: &str, what: &str, span: Span) -> Exec<()> {
        if code.language == LANGUAGE {
            return Ok(());
        }
        Err(error(
            span,
            format!(
                "the {kind} of {what} is written in {}, which ADT'81 does not run",
                code.language
            ),
        ))
    }

    /// An ACTION's statements, its declarations its own locals.
    fn run_fragment(&self, fragment: &Fragment, f: &mut Frame) -> Exec<()> {
        for item in &fragment.items {
            match item {
                Item::Local(d) => {
                    let ty = Typed::of(&d.ty);
                    for n in &d.names {
                        let value = matches!(d.ty, DataType::List(_))
                            .then(|| Value::List(List::new(Vec::new())));
                        f.vars.insert(n.key.clone(), slot(value, ty.clone(), false));
                    }
                }
                Item::Parameter(p) => {
                    return Err(error(p.span, "an ACTION has no parameters"));
                }
                Item::Stmt(s) => match self.stmt(s, f) {
                    Ok(()) => {}
                    Err(Flow::Return) => return Ok(()),
                    Err(flow) => return Err(flow),
                },
            }
        }
        Ok(())
    }

    /// CREATE and DESIGN: classes (embryos for DESIGN), objects,
    /// attributes, links.
    fn create(&self, o: &Operands, kind: ClassKind, f: &mut Frame, span: Span) -> Exec<()> {
        match o {
            Operands::Designations(ds) => {
                for d in ds {
                    match d {
                        Designation::Named(n) => {
                            let name = self.text(n, f, span)?;
                            let mut store = self.store.lock().unwrap();
                            stored(store.create_class(&name, kind.clone()), span)?;
                        }
                        Designation::Classed { name, class } => {
                            let c = self.class(class, f, span)?;
                            let name = self.text(name, f, span)?;
                            let mut store = self.store.lock().unwrap();
                            let o = stored(store.create_object(c, &name), span)?;
                            drop(store);
                            self.trigger(Invoked::Created(o), f, span)?;
                        }
                        Designation::This(at) => return Err(error(*at, "THIS exists already")),
                    }
                }
            }
            Operands::Attributes(attributes) => {
                for a in attributes {
                    let c = self.class(&a.class, f, span)?;
                    let name = self.text(&a.attribute, f, span)?;
                    let mut store = self.store.lock().unwrap();
                    stored(store.create_attribute(c, &name), span)?;
                }
            }
            Operands::Links(links) => {
                for l in links {
                    let from = self.class(&l.link.from, f, span)?;
                    let to = self.class(&l.link.to, f, span)?;
                    let reverse = match &l.reverse {
                        Reverse::Named(n) => Some(self.text(n, f, span)?),
                        _ => None,
                    };
                    let reflexive = l.reverse == Reverse::Reflexive;
                    let mut store = self.store.lock().unwrap();
                    let r = store.create_class_link(
                        from,
                        &l.link.relation.text,
                        to,
                        reverse.as_deref(),
                        reflexive,
                    );
                    stored(r, span)?;
                }
            }
        }
        Ok(())
    }

    /// HIBERN, AWAKE, DESTROY, RESUR, PURGE.
    fn state_change(&self, op: StateOp, o: &Operands, f: &mut Frame, span: Span) -> Exec<()> {
        let mut targets = Vec::new();
        match o {
            Operands::Designations(ds) => {
                for d in ds {
                    targets.push(self.designate(d, f, span, true)?);
                }
            }
            Operands::Attributes(attributes) => {
                for a in attributes {
                    let c = self.class(&a.class, f, span)?;
                    let name = self.text(&a.attribute, f, span)?;
                    let store = self.store.lock().unwrap();
                    let a = store.find_attribute(c, &name);
                    let a = a.ok_or_else(|| error(span, format!("no attribute \"{name}\"")))?;
                    targets.push(AdtRef::ClassAttribute(a));
                }
            }
            Operands::Links(links) => {
                for l in links {
                    let from = self.class(&l.link.from, f, span)?;
                    let to = self.class(&l.link.to, f, span)?;
                    let store = self.store.lock().unwrap();
                    let link = store.find_class_link(from, &l.link.relation.text, to);
                    let link = link.ok_or_else(|| error(span, "no such link"))?;
                    targets.push(AdtRef::ClassLink(link));
                }
            }
        }
        let mut store = self.store.lock().unwrap();
        for t in targets {
            let r = match op {
                StateOp::Hibern => store.change_state(t, AdtState::Active, AdtState::Hibernating),
                StateOp::Awake => store.change_state(t, AdtState::Hibernating, AdtState::Active),
                StateOp::Destroy => store.change_state(t, AdtState::Active, AdtState::Destroyed),
                StateOp::Resur => store.change_state(t, AdtState::Destroyed, AdtState::Active),
                StateOp::Purge => store.purge_adt(t),
            };
            stored(r, span)?;
        }
        Ok(())
    }

    /// LET: an object's attribute, or a property (COND, ACTION), which is
    /// stored as written, once it parses (pages 156 to 158).
    fn let_(&self, target: &LetTarget, value: &Expr, f: &mut Frame, span: Span) -> Exec<()> {
        let v = self.eval(value, f)?;
        match target {
            LetTarget::Attribute { object, attribute } => {
                let o = self.object(object, f, span)?;
                let name = self.text(attribute, f, span)?;
                let v = self.stamped(v);
                let mut store = self.store.lock().unwrap();
                let a = stored(store.attribute(store.objects()[&o].class, &name), span)?;
                if hbds_simulation::is_live(&store, a) {
                    return Err(error(span, format!("{name} is the simulation engine's")));
                }
                stored(store.set(o, a, v), span)?;
                drop(store);
                self.trigger(Invoked::Set(o, a), f, span)
            }
            LetTarget::Variable(n) => {
                let r = self.read(
                    &Variable {
                        name: n.clone(),
                        indices: Vec::new(),
                    },
                    f,
                )?;
                match r {
                    Value::Adt(AdtRef::ObjectAttribute(o, a)) => {
                        let v = self.stamped(v);
                        stored(self.store.lock().unwrap().set(o, a, v), span)?;
                        self.trigger(Invoked::Set(o, a), f, span)
                    }
                    _ => Err(error(
                        n.span,
                        format!("`{}` holds no object's attribute", n.spelling),
                    )),
                }
            }
            LetTarget::Property { adt, name } => {
                let target = self.adt(adt, f, span)?;
                let text = match v.scalar() {
                    Some(Scalar::String(s)) => s.to_string(),
                    Some(Scalar::Character(c)) => c.to_string(),
                    _ => {
                        return Err(error(
                            value.span,
                            format!("a property is a string, not {v}"),
                        ));
                    }
                };
                // Compiled at capture: a rule's part that does not parse is
                // refused, as a COND is (page 80).
                let parses = match builtins::is_property(&name.key) {
                    true => uses::parse(&name.key, &text).map(|_| ()),
                    false => Ok(()),
                };
                if let Err(e) = parses {
                    return Err(error(
                        value.span,
                        format!("this {} does not parse: {e}", name.key),
                    ));
                }
                let mut store = self.store.lock().unwrap();
                stored(store.set_property(target, &name.key, LANGUAGE, &text), span)
            }
        }
    }

    /// What YES or NO writes takes their reliability as its probability.
    fn stamped(&self, v: Value) -> Value {
        match (self.ses.lock().unwrap().stamp, v) {
            (Some(p), Value::One(mut fz)) => {
                fz.prob = Some(p);
                Value::One(fz)
            }
            (_, v) => v,
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
            let v = self.call(m, args, Some(prefix), None)?;
            return Ok(v.expect("a functor's result"));
        }
        let Some(functor) = builtins::functor(&c.name.key) else {
            return Err(error(
                span,
                format!("`{}` is not a functor of this file", c.name.spelling),
            ));
        };
        let args = c
            .args
            .iter()
            .map(|a| self.eval(a, f))
            .collect::<Exec<Vec<Value>>>()?;
        if functor.takes != builtins::Takes::Lists {
            return self.adt_functor(&c.name, &prefix, &args);
        }
        // SIGMA adds attributes' values (page 78).
        let prefix = match prefix {
            Value::List(mut l) if c.name.key == "SIGMA" => {
                let store = self.store.lock().unwrap();
                for item in Arc::make_mut(&mut l.items).iter_mut() {
                    if let Value::Adt(AdtRef::ObjectAttribute(o, a)) = item {
                        let v = store.get(*o, *a).cloned();
                        *item = v.ok_or_else(|| {
                            let what = store.describe(AdtRef::ObjectAttribute(*o, *a));
                            error(span, format!("{what} has no value"))
                        })?;
                    }
                }
                Value::List(l)
            }
            prefix => prefix,
        };
        let Value::List(mut list) = prefix else {
            return Err(error(
                span,
                format!("`{}` applies to a list", c.name.spelling),
            ));
        };
        let add = |a: &Value, b: &Value| value::binary(BinaryOp::Add, a, b).map_err(|f| f.0);
        let result = hbds_lists::apply(&c.name.key, &mut list, &args, &add);
        let result = result.map_err(|m| error(span, m))?;
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
        Constant::String(s) => Scalar::String(s.clone().into()),
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
