//! The VM (docs/architecture.md, decisions 2 and 3): runs A-code. A run
//! is fibers, the main program, coroutines and processes' bodies, each
//! its frames and operand stack as data: no thread, no Rust recursion
//! between them, so that a paused flow of control is kept. Code an
//! engine calls back (a COND, an ACTION) runs nested, to completion.
//! What a value does is hbds-values'; lists are hbds-lists'; the store
//! is GTAD's.

use std::collections::HashMap;
use std::fmt;
use std::ops::DerefMut;
use std::rc::Rc;

use hbds_acode::{
    BUDGET_SPENT, Due, Instr, Kind, ListOp, Module, Place, SlotType, Span, StateOp, Unit, too_far,
};
use hbds_core::{AdtRef, Id, fold, same};
use hbds_gtad::{ClassKind, Code, State as AdtState, Store, Stored};
mod saved;

use hbds_simulation::When;
/// The sequencing set a turn is lent, and who runs in it.
pub use hbds_simulation::{Agent, Sequencing};
use hbds_values::{Fault, Fz, List, Op, Scalar, Value};

/// How deep CONDs and ACTIONs may nest, an ACTION's change running others.
const DEPTH: usize = 64;

/// The language a run's code is in: it compiles the code the store
/// keeps, and tells SES what a rule's part uses.
pub trait Language: hbds_acode::Language + hbds_rules::Compiler {}

impl<L: hbds_acode::Language + hbds_rules::Compiler> Language for L {}

/// Where the program reads its input and writes its output.
pub trait Io: Send {
    fn read_line(&mut self) -> Option<String>;
    fn write(&mut self, text: &str);
    /// Whether `read_line` would answer now, a line or the end of the
    /// input; a session's turn ends where it would wait.
    fn ready(&mut self) -> bool {
        true
    }

    /// The kernel runs `process` for `owner`, the agent that activated
    /// it: what it writes may go there.
    fn running(&mut self, _process: Agent, _owner: Option<Agent>) {}
}

/// `line:column: runtime error: message`, as the reference writes it.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeError {
    pub span: Span,
    pub message: String,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Span { line, column } = self.span;
        write!(f, "{line}:{column}: runtime error: {}", self.message)
    }
}

/// The stack of the thread a run takes: code the store keeps runs
/// nested, as deep as 64 CONDs and ACTIONs, as the reference's does.
const STACK: usize = 256 << 20;

/// Runs the unit's first module, a program, then those it CHAINs to, on
/// `store`: the statements it executed.
pub fn run(
    unit: &Unit,
    store: &mut Store,
    io: &mut dyn Io,
    language: &(dyn Language + Sync),
    budget: Option<u64>,
) -> Result<u64, RuntimeError> {
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(STACK)
            .spawn_scoped(s, || {
                let mut session = Session::new(unit.clone(), budget);
                let mut sim = Sequencing::new();
                loop {
                    match session.turn(store, &mut sim, io, language, None) {
                        Turn::Done(statements) => return Ok(statements),
                        Turn::Failed(e) => return Err(e),
                        // No slice, an Io always ready, a standalone run's
                        // HOLD running its processes in place.
                        _ => {}
                    }
                }
            })
            .expect("a thread for the run")
            .join()
            .expect("the run's thread")
    })
}

/// How a turn ends.
#[derive(Debug, Clone, PartialEq)]
pub enum Turn {
    /// The run ended: the statements it executed.
    Done(u64),
    Failed(RuntimeError),
    /// Its slice of instructions is spent, between two statements.
    Yield,
    /// It waits for a line its Io does not have yet.
    Input,
    /// A client's main program holds: it waits until the kernel says it
    /// is due.
    Held,
    /// The kernel has nothing due within its bound.
    Idle,
    /// The kernel reached a client's time: that client runs on.
    Due(u64),
}

/// How a session stands to the store's time (docs/architecture.md,
/// decision 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// A standalone run: the main program a process, its HOLD running the
    /// processes due in place.
    Standalone,
    /// A daemon's client: an agent of the store's set when it holds; its
    /// processes are the kernel's.
    Client(u64),
    /// The daemon's kernel: no main program; it runs the processes the
    /// store's set says are due.
    Kernel,
}

/// How far the kernel may take time in a turn: up to `until`, and at
/// most `events` agents due, if counted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bound {
    pub until: f64,
    pub events: Option<u64>,
}

/// Why a dispatch ends a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    Held,
    Idle,
    Due(u64),
}

/// A run that goes in turns (docs/architecture.md, decision 3): what the
/// VM keeps between them. The store, the Io and the language are lent to
/// each turn; a turn runs on a thread with the VM's large stack.
pub struct Session {
    /// The units loaded, and where each one's modules start.
    units: Vec<(Unit, usize)>,
    o: Owned,
}

/// The VM's own state.
struct Owned {
    /// The unit's modules, then those compiled from the store's code.
    modules: Vec<Rc<Module>>,
    /// Where each module comes from: its unit's hash and its place there;
    /// none for code the store keeps, compiled. What a saved fiber names
    /// its modules by.
    origin: Vec<Option<(String, usize)>>,
    /// Code the store keeps, compiled: by part and text.
    compiled: HashMap<(String, String), usize>,
    /// The functions that compute attributes, by class and attribute.
    computed: HashMap<(String, String), usize>,
    /// The bodies of process classes, by class (page 112).
    bodies: HashMap<String, usize>,
    budget: Option<u64>,
    statements: u64,
    frames: Vec<Frame>,
    stack: Vec<Item>,
    /// Transactions begun and not yet ended.
    open: usize,
    ses: Ses,
    mode: Mode,
    /// The kernel's bound for this turn, and the process it runs.
    bound: Bound,
    running: Option<Id>,
    /// The process the kernel's last turn ended on an error, if one did.
    failed: Option<Id>,
    /// Whether the store's records wait for the running process's pause.
    holding: bool,
    /// Every fiber; the current one's frames and stack are `frames` and
    /// `stack`.
    fibers: Vec<Fiber>,
    current: usize,
    /// The coroutines started, by module, and the processes' bodies, by
    /// object: their fibers.
    coroutines: HashMap<usize, usize>,
    processes: HashMap<Id, usize>,
    /// Variables shared with a coroutine by its first RESUME.
    shared: Vec<Var>,
    /// Where the main program last held: the sequencing set's errors go
    /// there, as the reference's `give_way` places them.
    held_at: Span,
    /// The program running, and whether it has started.
    program: usize,
    started: bool,
    /// How many callbacks run nested: a turn ends only outside them.
    nesting: usize,
    /// The instructions this turn may still execute, if it has a slice.
    slice: Option<u64>,
}

impl Session {
    pub fn new(unit: Unit, budget: Option<u64>) -> Session {
        let mut session = Session::empty(budget, Mode::Standalone);
        session.load(unit);
        session
    }

    /// A daemon's client's session: `id` is its agent's.
    pub fn client(unit: Unit, id: u64) -> Session {
        let mut session = Session::empty(None, Mode::Client(id));
        session.load(unit);
        session
    }

    /// The daemon's kernel: units are loaded into it as clients bring
    /// them.
    pub fn kernel() -> Session {
        Session::empty(None, Mode::Kernel)
    }

    fn empty(budget: Option<u64>, mode: Mode) -> Session {
        let o = Owned {
            modules: Vec::new(),
            origin: Vec::new(),
            compiled: HashMap::new(),
            computed: HashMap::new(),
            bodies: HashMap::new(),
            budget,
            statements: 0,
            frames: Vec::new(),
            stack: Vec::new(),
            open: 0,
            ses: Ses::default(),
            mode,
            bound: Bound {
                until: f64::INFINITY,
                events: None,
            },
            running: None,
            failed: None,
            holding: false,
            fibers: vec![Fiber::new(Role::Main, None)],
            current: 0,
            coroutines: HashMap::new(),
            processes: HashMap::new(),
            shared: Vec::new(),
            held_at: Span::default(),
            program: 0,
            started: false,
            nesting: 0,
            slice: None,
        };
        Session {
            units: Vec::new(),
            o,
        }
    }

    /// Adds a unit's modules, the instructions that name modules moved by
    /// where they start: a new version of a process's body or of a
    /// computed attribute's function serves from now on, while fibers
    /// already running keep theirs (decision 10). Where they start.
    pub fn load(&mut self, unit: Unit) -> usize {
        let offset = self.o.modules.len();
        let hash = hbds_core::sha1::hex(unit.text().as_bytes());
        for (i, m) in unit.modules.iter().enumerate() {
            self.o.origin.push(Some((hash.clone(), i)));
            let mut m = m.clone();
            relocate(&mut m, offset);
            if let Some(key) = &m.computes {
                self.o.computed.insert(key.clone(), offset + i);
            }
            if let Some(class) = &m.body {
                self.o.bodies.insert(class.clone(), offset + i);
            }
            self.o.modules.push(Rc::new(m));
        }
        self.units.push((unit, offset));
        offset
    }

    /// The A-code this session runs, the last loaded.
    pub fn unit(&self) -> &Unit {
        &self.units.last().expect("a unit").0
    }

    /// Runs on until the run ends, or `slice` instructions are spent, or
    /// it waits for input.
    pub fn turn(
        &mut self,
        store: &mut Store,
        sim: &mut Sequencing,
        io: &mut dyn Io,
        language: &dyn Language,
        slice: Option<u64>,
    ) -> Turn {
        let (unit, offset) = self.units.last().expect("a unit");
        let mut vm = Vm {
            unit,
            offset: *offset,
            o: &mut self.o,
            store,
            sim,
            io,
            language,
        };
        vm.o.slice = slice;
        if let Mode::Client(id) = vm.o.mode {
            vm.sim.run_as(Agent::Client(id));
        }
        match vm.turn() {
            Ok(Some(turn)) => turn,
            Ok(None) => Turn::Done(vm.o.statements),
            Err(e) => Turn::Failed(e),
        }
    }

    /// A turn of the kernel: the processes due, up to `bound`, each run
    /// until it holds, passivates or ends; a client due ends the turn.
    pub fn kernel_turn(
        &mut self,
        store: &mut Store,
        sim: &mut Sequencing,
        io: &mut dyn Io,
        language: &dyn Language,
        slice: Option<u64>,
        bound: Bound,
    ) -> Turn {
        let Some((unit, offset)) = self.units.last() else {
            return Turn::Idle;
        };
        let mut vm = Vm {
            unit,
            offset: *offset,
            o: &mut self.o,
            store,
            sim,
            io,
            language,
        };
        vm.o.slice = slice;
        vm.o.bound = bound;
        match vm.kernel_turn() {
            Ok(turn) => turn,
            Err(e) => Turn::Failed(e),
        }
    }

    /// Whether the kernel is in the middle of a process.
    pub fn in_process(&self) -> bool {
        self.o.running.is_some()
    }

    /// The process the kernel's last turn ended on an error, if one did.
    pub fn failed(&self) -> Option<Id> {
        self.o.failed
    }

    /// The agents due the kernel may still run, if they are counted.
    pub fn events_left(&self) -> Option<u64> {
        self.o.bound.events
    }
}

/// A module's instructions that name modules, moved by `offset`.
fn relocate(m: &mut Module, offset: usize) {
    if offset == 0 {
        return;
    }
    for i in &mut m.code {
        if let Instr::Call(callee, _)
        | Instr::Chain(callee)
        | Instr::ResumeStarted(callee, ..)
        | Instr::ResumeFirst(callee, _) = i
        {
            *callee += offset;
        }
    }
}

impl Vm<'_> {
    /// A turn of the session: `None` once the run has ended well.
    fn turn(&mut self) -> Result<Option<Turn>, RuntimeError> {
        loop {
            if !self.o.started {
                self.o.started = true;
                self.o.frames.clear();
                self.o.stack.clear();
                let program = self.o.program;
                self.enter(program, Vec::new(), None, 0, false);
            }
            let exit = self.exec_from(0, 0);
            if let Ok(Exit::Yield | Exit::Input | Exit::Stop(_)) = exit {
                return Ok(Some(match exit {
                    Ok(Exit::Yield) => Turn::Yield,
                    Ok(Exit::Stop(Stop::Held)) => Turn::Held,
                    Ok(Exit::Stop(Stop::Idle)) => Turn::Idle,
                    Ok(Exit::Stop(Stop::Due(id))) => Turn::Due(id),
                    _ => Turn::Input,
                }));
            }
            // A statement's transaction a failure or a STOP left open is
            // refused, as the reference's `atomic` refuses it.
            for _ in 0..std::mem::take(&mut self.o.open) {
                let _ = self.store.end(false);
            }
            self.o.started = false;
            match exit? {
                Exit::Returned(_) | Exit::End(End::Done) => break,
                Exit::End(End::Restart) => {}
                Exit::End(End::Chain(m)) => self.o.program = m,
                Exit::Yield | Exit::Input | Exit::Stop(_) => unreachable!("returned above"),
            }
        }
        if self.o.mode != Mode::Standalone {
            return Ok(None);
        }
        // The processes as the run leaves them: implicitly persistent (page
        // 112), the sequencing set gone with the run.
        for (p, state, clock) in self.sim.processes() {
            if self.store.objects().contains_key(&p) {
                hbds_simulation::record(self.store, p, state, clock).map_err(|e| RuntimeError {
                    span: Span { line: 1, column: 1 },
                    message: e.0,
                })?;
            }
        }
        Ok(None)
    }

    fn kernel_turn(&mut self) -> Result<Turn, RuntimeError> {
        self.o.failed = None;
        if let Some(p) = self.o.running {
            self.sim.run_as(Agent::Process(p));
            let owner = self.sim.owner(Agent::Process(p));
            self.io.running(Agent::Process(p), owner);
        } else {
            match self.dispatch() {
                Ok(Some(stop)) => return Ok(stop_turn(stop)),
                Ok(None) => {}
                Err(f) => {
                    return Err(RuntimeError {
                        span: f.at.unwrap_or_default(),
                        message: f.message,
                    });
                }
            }
        }
        let exit = self.exec_from(0, 0);
        if exit.is_err() || matches!(exit, Ok(Exit::End(_))) {
            // A process that fails, or STOPs, ends; the kernel goes on.
            for _ in 0..std::mem::take(&mut self.o.open) {
                let _ = self.store.end(false);
            }
            if let Some(p) = self.o.running.take() {
                self.sim.run_as(Agent::Process(p));
                let _ = self.sim.terminate();
                self.o.failed = Some(p);
                let _ = self.checkpoint();
            }
        }
        match exit? {
            Exit::Yield => Ok(Turn::Yield),
            Exit::Stop(stop) => Ok(stop_turn(stop)),
            Exit::End(_) => Ok(Turn::Idle),
            Exit::Input => Ok(Turn::Input),
            Exit::Returned(_) => unreachable!("the kernel's fiber has no frames"),
        }
    }
}

fn stop_turn(stop: Stop) -> Turn {
    match stop {
        Stop::Held => Turn::Held,
        Stop::Idle => Turn::Idle,
        Stop::Due(id) => Turn::Due(id),
    }
}

/// A variable's value, and the one RESET gives back.
#[derive(Debug, Clone, Default)]
struct Var {
    value: Option<Value>,
    initial: Option<Value>,
}

/// A slot of a frame: its own variable, or a caller's (OUT and INOUT),
/// or one shared with a coroutine, which may outlive the caller.
#[derive(Debug, Clone)]
enum Cell {
    Own(Var),
    Ref(usize, usize),
    Shared(usize),
}

/// What the operand stack holds: values, and references to variables
/// on their way to an OUT or INOUT parameter.
#[derive(Debug, Clone)]
enum Item {
    Value(Value),
    Ref(usize, usize),
    Shared(usize),
}

/// Whom a fiber runs for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Main,
    /// A coroutine RESUME started.
    Coroutine,
    Process(Id),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FiberState {
    Running,
    Paused,
    Ended,
}

/// A flow of control (docs/architecture.md, decision 3): its frames and
/// operand stack, parked while another fiber runs.
struct Fiber {
    frames: Vec<Frame>,
    stack: Vec<Item>,
    role: Role,
    /// The fiber that resumed it, a coroutine's, to go back to.
    resumer: Option<usize>,
    state: FiberState,
}

impl Fiber {
    fn new(role: Role, resumer: Option<usize>) -> Fiber {
        Fiber {
            frames: Vec::new(),
            stack: Vec::new(),
            role,
            resumer,
            state: FiberState::Running,
        }
    }
}

struct Frame {
    module: usize,
    pc: usize,
    cells: Vec<Cell>,
    args: Vec<Item>,
    prefix: Option<Value>,
    /// The object a callback or a computed attribute runs for.
    this: Option<AdtRef>,
    /// How many CONDs and ACTIONs this frame runs within.
    depth: usize,
    /// Whether this frame runs code the store calls back, or code that
    /// code calls: it runs to completion.
    within: bool,
}

enum End {
    Done,
    Restart,
    Chain(usize),
}

/// How a run of frames ends: its outermost returned, or the program ends.
enum Exit {
    Returned(Option<Value>),
    End(End),
    /// The turn's slice is spent, between two statements.
    Yield,
    /// The Io has no line yet.
    Input,
    Stop(Stop),
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

/// A change that runs CONDs and ACTIONs.
enum Invoked {
    Created(Id),
    Set(Id, Id),
    Linked(Id),
}

/// What an instruction does to the flow.
enum Next {
    Go,
    Jump(usize),
    Return(Option<Value>),
    End(End),
    /// The instruction waits for input: it runs again next turn.
    Wait,
    /// A dispatch ends the turn.
    Stop(Stop),
}

struct Vm<'r> {
    /// The last unit loaded, and where its modules start: what code the
    /// store keeps is compiled against.
    unit: &'r Unit,
    offset: usize,
    o: &'r mut Owned,
    store: &'r mut Store,
    sim: &'r mut Sequencing,
    io: &'r mut dyn Io,
    language: &'r dyn Language,
}

/// Why an instruction fails, and where if not where it comes from.
struct Failure {
    message: String,
    at: Option<Span>,
}

impl Failure {
    /// Placed at `span` unless it has its own place.
    fn at(self, span: Span) -> Failure {
        Failure {
            message: self.message,
            at: self.at.or(Some(span)),
        }
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Failure {
        Failure { message, at: None }
    }
}

impl From<RuntimeError> for Failure {
    fn from(e: RuntimeError) -> Failure {
        Failure {
            message: e.message,
            at: Some(e.span),
        }
    }
}

impl From<&str> for Failure {
    fn from(message: &str) -> Failure {
        Failure {
            message: message.into(),
            at: None,
        }
    }
}

type Run<T> = Result<T, Failure>;

fn fault(f: Fault) -> Failure {
    f.0.into()
}

/// The string a name is, as Named left it.
fn text_of(v: &Value) -> String {
    match v.scalar() {
        Some(Scalar::String(s)) => s.to_string(),
        v => unreachable!("a name expected, not {v:?}"),
    }
}

/// A store's refusal.
fn stored<T>(r: Stored<T>) -> Run<T> {
    r.map_err(|e| e.0.into())
}

/// An error inside a COND or an ACTION, told once, from the outermost,
/// at the place of the change that ran it.
fn within(f: Failure, kind: &str, what: &str) -> Failure {
    if f.message.starts_with("in the ") {
        return f.message.into();
    }
    format!("in the {kind} of {what}: {}", f.message).into()
}

/// What went wrong in a rule's part, or in HYPOCHAV's hypotheses: an
/// error, or a jump out of it (`None`).
fn told(message: Option<String>, part: &str, rule: Option<&str>) -> String {
    let message = match message {
        Some(m) if m.starts_with("in ") => return m,
        Some(m) => m,
        None => "EXIT, NEXT, RELOOP, STOP, RESTART or CHAIN leaves it".into(),
    };
    match rule {
        Some(r) => format!("in the {part} of {r}: {message}"),
        None => format!("in HYPOCHAV's hypotheses: {message}"),
    }
}

/// The expert-system engines' way into the VM: they decide what runs
/// when, the VM runs it.
struct Engine<'a, 'r> {
    vm: &'a mut Vm<'r>,
}

impl hbds_rules::Compiler for Engine<'_, '_> {
    fn uses(&self, part: &str, code: &Code) -> Result<hbds_rules::Uses, String> {
        self.vm.language.uses(part, code)
    }
}

impl hbds_rules::Host for Engine<'_, '_> {
    fn store(&mut self) -> Box<dyn DerefMut<Target = Store> + '_> {
        Box::new(&mut *self.vm.store)
    }

    fn premises(&mut self, rule: Id, code: &Code) -> Result<(bool, f64), String> {
        let this = AdtRef::Object(rule);
        let what = self.vm.store.describe(this);
        let m = (self
            .vm
            .compiled("PREMISES", &code.language, &code.text, &what))
        .map_err(|f| f.message)?;
        let (exit, ses) = self.vm.with_ses(
            |s| {
                s.inside += 1;
                s.premises = Some((1.0, false));
                s.stamp = None;
            },
            |vm| vm.callback(m, Some(this)),
        );
        let (reliability, unknown) = ses.premises.expect("in PREMISES");
        let holds = match exit {
            Ok(Exit::Returned(v)) => {
                hbds_values::truth(&v.expect("an expression's value")).map_err(|f| Some(f.0))
            }
            Ok(Exit::End(_)) => Err(None),
            Ok(Exit::Yield | Exit::Input | Exit::Stop(_)) => {
                unreachable!("code the store calls back runs to completion")
            }
            Err(e) => Err(Some(e.message)),
        };
        match holds {
            Ok(holds) => Ok((holds, reliability)),
            Err(_) if unknown => Ok((false, reliability)),
            Err(message) => Err(told(message, "PREMISES", Some(&what))),
        }
    }

    fn run(
        &mut self,
        rule: Option<Id>,
        part: &str,
        code: &Code,
        reliability: f64,
    ) -> Result<(), String> {
        let what = rule.map(|r| self.vm.store.describe(AdtRef::Object(r)));
        let named = what.as_deref().unwrap_or("HYPOCHAV");
        let m =
            (self.vm.compiled(part, &code.language, &code.text, named)).map_err(|f| f.message)?;
        let (exit, _) = self.vm.with_ses(
            |s| {
                s.inside += 1;
                s.premises = None;
                s.stamp = (reliability < 1.0).then_some(reliability);
            },
            |vm| vm.callback(m, rule.map(AdtRef::Object)),
        );
        match exit {
            Ok(Exit::Returned(_)) => Ok(()),
            Ok(Exit::End(_)) => Err(told(None, part, what.as_deref())),
            Ok(Exit::Yield | Exit::Input | Exit::Stop(_)) => {
                unreachable!("code the store calls back runs to completion")
            }
            Err(e) => Err(told(Some(e.message), part, what.as_deref())),
        }
    }

    fn holds(&mut self, code: &Code, this: AdtRef) -> Result<bool, String> {
        match self.vm.fact_holds("EDITABLE", code, this) {
            Ok(Ok(holds)) => Ok(holds),
            Ok(Err(_)) => Err("an EDITABLE leaves by a jump".into()),
            Err(f) => Err(f.message),
        }
    }

    fn edit(&mut self, line: &str) {
        self.vm.io.write(&format!("{line}\n"));
    }
}

impl<'r> Vm<'r> {
    fn enter(
        &mut self,
        module: usize,
        mut args: Vec<Item>,
        this: Option<AdtRef>,
        depth: usize,
        within: bool,
    ) {
        let m = self.o.modules[module].clone();
        let mut prefix = None;
        if m.kind == Kind::Functor && !args.is_empty() {
            prefix = match args.remove(0) {
                Item::Value(v) => Some(v),
                Item::Ref(..) | Item::Shared(_) => None,
            };
        }
        self.o.frames.push(Frame {
            module,
            pc: 0,
            cells: vec![Cell::Own(Var::default()); m.slots.len()],
            args,
            prefix,
            this,
            depth,
            within,
        });
    }

    fn module(&self) -> Rc<Module> {
        self.o.modules[self.o.frames.last().expect("a frame").module].clone()
    }

    fn frame(&self) -> &Frame {
        self.o.frames.last().expect("a frame")
    }

    /// The frame and slot a slot of the current frame stands for.
    fn resolve(&self, s: usize) -> (usize, usize) {
        let f = self.o.frames.len() - 1;
        match self.o.frames[f].cells[s] {
            Cell::Own(_) | Cell::Shared(_) => (f, s),
            Cell::Ref(g, t) => (g, t),
        }
    }

    fn var(&self, s: usize) -> &Var {
        let (f, t) = self.resolve(s);
        match &self.o.frames[f].cells[t] {
            Cell::Own(v) => v,
            Cell::Shared(i) => &self.o.shared[*i],
            Cell::Ref(..) => unreachable!("references are resolved when made"),
        }
    }

    fn var_mut(&mut self, s: usize) -> &mut Var {
        let (f, t) = self.resolve(s);
        match &mut self.o.frames[f].cells[t] {
            Cell::Own(v) => v,
            Cell::Shared(i) => &mut self.o.shared[*i],
            Cell::Ref(..) => unreachable!("references are resolved when made"),
        }
    }

    // Fibers

    /// Parks the current fiber and makes `to` current.
    fn switch(&mut self, to: usize) {
        let from = self.o.current;
        self.o.fibers[from].frames = std::mem::take(&mut self.o.frames);
        self.o.fibers[from].stack = std::mem::take(&mut self.o.stack);
        self.o.current = to;
        self.o.frames = std::mem::take(&mut self.o.fibers[to].frames);
        self.o.stack = std::mem::take(&mut self.o.fibers[to].stack);
    }

    fn role(&self) -> Role {
        self.o.fibers[self.o.current].role
    }

    /// After a failure or a jump out of a fiber: it ends, and so does each
    /// fiber between it and `entry`, as the reference's threads end with
    /// what they pass back.
    fn unwind(&mut self, entry: usize) {
        while self.o.current != entry {
            let f = self.o.current;
            self.o.fibers[f].state = FiberState::Ended;
            if let Role::Process(p) = self.o.fibers[f].role {
                self.o.processes.remove(&p);
            }
            let back = self.o.fibers[f].resumer.unwrap_or(0);
            self.switch(back);
        }
    }

    /// A fiber whose outermost frame returned: a coroutine goes back to
    /// its resumer; a process's body ends as TERMINATE ends it.
    fn fiber_ends(&mut self) -> Run<Option<Stop>> {
        let f = self.o.current;
        self.o.fibers[f].state = FiberState::Ended;
        match self.o.fibers[f].role {
            Role::Coroutine => {
                let back = self.o.fibers[f].resumer.expect("a coroutine's resumer");
                self.switch(back);
                Ok(None)
            }
            Role::Process(p) => {
                self.o.processes.remove(&p);
                self.o.running = None;
                let at = self.o.held_at;
                self.sim_op(|sim, _| sim.terminate())
                    .map_err(|f| f.at(at))?;
                self.dispatch()
            }
            Role::Main => unreachable!("the main program ends the run"),
        }
    }

    /// A pause in the kernel: what the process wrote since it last paused
    /// is journalled in one write with the fibers and the set as they now
    /// are, so that after a crash it resumes here, and nothing runs twice.
    fn checkpoint(&mut self) -> Run<()> {
        if !std::mem::take(&mut self.o.holding) {
            return Ok(());
        }
        let fibers = Session::save_fibers_of(self.o).map_err(Failure::from)?;
        let extra = vec![
            hbds_gtad::Change::Fibers { text: fibers },
            hbds_gtad::Change::Sequencing {
                text: self.sim.save(),
            },
        ];
        stored(self.store.release_records(extra))
    }

    /// A client changed the store's set: kept with what it did.
    fn keep_sequencing(&mut self) -> Run<()> {
        if let Mode::Client(_) = self.o.mode {
            stored(self.store.keep_sequencing(self.sim.save()))?;
        }
        Ok(())
    }

    /// The agent the main fiber is: the main program, or a client's.
    fn me(&self) -> Agent {
        match self.o.mode {
            Mode::Client(id) => Agent::Client(id),
            _ => Agent::Main,
        }
    }

    /// Who runs: the main program or a process; a coroutine does not hold,
    /// passivate nor terminate, nor does code the store calls back.
    fn running(&self, what: &str) -> Run<Agent> {
        if self.frame().within {
            return Err(format!(
                "{what} where the store calls back: COND, ACTION, rules and computed attributes run to completion"
            )
            .into());
        }
        match self.role() {
            Role::Main => Ok(self.me()),
            Role::Process(p) => Ok(Agent::Process(p)),
            Role::Coroutine => Err(
                "a coroutine RESUME started is not a process: it does not HOLD, PASSIVATE nor TERMINATE"
                    .into(),
            ),
        }
    }

    /// After HOLD or PASSIVATE, or a process's end: the sequencing set
    /// says who runs next, the main program or a process. A client's
    /// main program waits for the kernel instead; the kernel stops at
    /// its bound, or at a client due.
    fn dispatch(&mut self) -> Run<Option<Stop>> {
        let at = self.o.held_at;
        if self.o.mode == Mode::Kernel {
            self.checkpoint()?;
        }
        let next = match self.o.mode {
            Mode::Client(_) => return Ok(Some(Stop::Held)),
            Mode::Standalone => self.sim.advance(),
            Mode::Kernel => {
                let bound = self.o.bound;
                let due = self.sim.next_due().is_some_and(|t| t <= bound.until);
                if !due || bound.events == Some(0) {
                    self.o.running = None;
                    self.switch(0);
                    return Ok(Some(Stop::Idle));
                }
                if let Some(n) = &mut self.o.bound.events {
                    *n -= 1;
                }
                self.sim.advance()
            }
        };
        match next {
            Some(Agent::Process(p)) => {
                self.o.running = Some(p);
                if self.o.mode == Mode::Kernel {
                    let owner = self.sim.owner(Agent::Process(p));
                    self.io.running(Agent::Process(p), owner);
                    // Its writes wait for its pause, to be journalled with
                    // what it then is (decision 5).
                    self.store.hold_records();
                    self.o.holding = true;
                }
                let fiber = match self.o.processes.get(&p) {
                    Some(&f) => f,
                    None => {
                        let body = self.body(p).map_err(|f| f.at(at))?;
                        let f = self.o.fibers.len();
                        self.o.fibers.push(Fiber::new(Role::Process(p), None));
                        self.o.processes.insert(p, f);
                        self.switch(f);
                        self.enter(body, Vec::new(), Some(AdtRef::Object(p)), 0, false);
                        return Ok(None);
                    }
                };
                self.switch(fiber);
            }
            Some(Agent::Client(id)) => {
                self.o.running = None;
                self.switch(0);
                return Ok(Some(Stop::Due(id)));
            }
            Some(Agent::Main) | None => {
                self.o.running = None;
                self.switch(0);
                if self.o.mode == Mode::Kernel {
                    return Ok(Some(Stop::Idle));
                }
            }
        }
        Ok(None)
    }

    /// The body of a process: its class's coroutine, or the nearest
    /// hyperclass's.
    fn body(&self, p: Id) -> Run<usize> {
        let class = self.store.objects()[&p].class;
        for c in self.store.lineage(class) {
            if let Some(&i) = self.o.bodies.get(&fold(&self.store.classes()[&c].name)) {
                return Ok(i);
            }
        }
        let name = &self.store.classes()[&class].name;
        Err(format!("\"{name}\" has no body: COROUTINE \"{name}\" gives it one").into())
    }

    /// An operation of the sequencing set, agents named as PRINT shows
    /// them.
    fn sim_op(
        &mut self,
        op: impl FnOnce(&mut Sequencing, hbds_simulation::Describe) -> Result<(), String>,
    ) -> Run<()> {
        let store = &*self.store;
        let name = |a: Agent| match a {
            Agent::Main | Agent::Client(_) => "the main program".to_string(),
            Agent::Process(p) => store.describe(AdtRef::Object(p)),
        };
        op(self.sim, &name).map_err(Failure::from)
    }

    fn pop_time(&mut self) -> f64 {
        match self.pop().scalar() {
            Some(Scalar::Float(t)) => *t,
            v => unreachable!("AsTime comes first, not {v:?}"),
        }
    }

    /// HOLD or PASSIVATE done: a process gives control back to the
    /// sequencing set; the main program lets it run the processes due
    /// until it is due again.
    fn give_way(&mut self, span: Span) -> Run<Option<Stop>> {
        if self.role() == Role::Main {
            self.o.held_at = span;
        }
        self.dispatch()
    }

    fn pop_process(&mut self) -> Agent {
        Agent::Process(self.pop_object())
    }

    /// The first RESUME of coroutine `m`: its OUT and INOUT arguments
    /// become variables it shares with the caller.
    fn start(&mut self, m: usize, n: usize) {
        let mut args = self.o.stack.split_off(self.o.stack.len() - n);
        for a in &mut args {
            if let Item::Ref(f, t) = *a {
                let i = match &mut self.o.frames[f].cells[t] {
                    Cell::Shared(i) => *i,
                    cell => {
                        let Cell::Own(v) =
                            std::mem::replace(cell, Cell::Shared(self.o.shared.len()))
                        else {
                            unreachable!("references are resolved when made")
                        };
                        self.o.shared.push(v);
                        self.o.shared.len() - 1
                    }
                };
                *a = Item::Shared(i);
            }
        }
        let fiber = self.o.fibers.len();
        self.o
            .fibers
            .push(Fiber::new(Role::Coroutine, Some(self.o.current)));
        self.o.coroutines.insert(m, fiber);
        self.switch(fiber);
        self.enter(m, args, None, 0, false);
    }

    /// The declaration of the variable a slot stands for, in the module
    /// that declares it.
    fn decl(&self, s: usize) -> &hbds_acode::Slot {
        let (f, t) = self.resolve(s);
        &self.o.modules[self.o.frames[f].module].slots[t]
    }

    fn name(&self, s: usize) -> String {
        self.o.modules[self.frame().module].slots[s].name.clone()
    }

    fn pop(&mut self) -> Value {
        match self.o.stack.pop().expect("an operand") {
            Item::Value(v) => v,
            Item::Ref(..) | Item::Shared(_) => {
                unreachable!("a reference where a value is expected")
            }
        }
    }

    fn push(&mut self, v: Value) {
        self.o.stack.push(Item::Value(v));
    }

    fn pop_integers(&mut self, n: usize) -> Run<Vec<i64>> {
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(hbds_values::integer(&self.pop()).map_err(fault)?);
        }
        out.reverse();
        Ok(out)
    }

    fn convert(v: Value, ty: SlotType) -> Run<Value> {
        match ty {
            SlotType::Base(b) => hbds_values::convert(v, b).map_err(fault),
            _ => Ok(v),
        }
    }

    fn list_of(&mut self, s: usize) -> &mut List {
        match &mut self.var_mut(s).value {
            Some(Value::List(l)) => l,
            _ => unreachable!("CheckList comes first"),
        }
    }

    /// Runs until the frames are back to `base` deep, the outermost
    /// above it having returned, or the program ends.
    fn exec(&mut self, base: usize) -> Result<Exit, RuntimeError> {
        let entry = self.o.current;
        self.exec_from(entry, base)
    }

    /// [`exec`](Vm::exec) from fiber `entry`, the main program's for a
    /// turn, whichever fiber is current.
    fn exec_from(&mut self, entry: usize, base: usize) -> Result<Exit, RuntimeError> {
        let exit = self.run_fibers(base, entry);
        if matches!(exit, Err(_) | Ok(Exit::End(_))) {
            self.unwind(entry);
        }
        exit
    }

    /// [`exec`](Vm::exec)'s loop: `base` counts on fiber `entry`; other
    /// fibers run as they are switched to, until control is back.
    fn run_fibers(&mut self, base: usize, entry: usize) -> Result<Exit, RuntimeError> {
        loop {
            let m = self.module();
            let pc = self.o.frames.last().expect("a frame").pc;
            // A turn ends between statements, outside any callback and
            // transaction (decision 3).
            if let Some(left) = &mut self.o.slice {
                if *left == 0
                    && matches!(m.code[pc], Instr::Stmt)
                    && self.o.nesting == 0
                    && self.o.open == 0
                    && self.o.stack.is_empty()
                {
                    return Ok(Exit::Yield);
                }
                *left = left.saturating_sub(1);
            }
            self.o.frames.last_mut().expect("a frame").pc += 1;
            let span = m.spans[pc];
            let placed = |f: Failure| RuntimeError {
                span: f.at.unwrap_or(span),
                message: f.message,
            };
            let next = self.step(&m.code[pc], &m, span).map_err(placed)?;
            match next {
                Next::Go => {}
                Next::Jump(t) => self.o.frames.last_mut().expect("a frame").pc = t,
                Next::End(end) => return Ok(Exit::End(end)),
                Next::Wait => {
                    self.o.frames.last_mut().expect("a frame").pc = pc;
                    return Ok(Exit::Input);
                }
                Next::Stop(stop) => return Ok(Exit::Stop(stop)),
                Next::Return(value) => {
                    let callee = self.o.frames.pop().expect("a frame");
                    let kind = self.o.modules[callee.module].kind;
                    let result = match kind {
                        Kind::Function => value,
                        Kind::Functor => {
                            let m = &self.o.modules[callee.module];
                            let r = m.result.expect("a functor's result slot");
                            let v = match &callee.cells[r] {
                                Cell::Own(v) => v.value.clone(),
                                Cell::Shared(i) => self.o.shared[*i].value.clone(),
                                Cell::Ref(..) => None,
                            };
                            let v = v.ok_or_else(|| RuntimeError {
                                span: m.span,
                                message: "the functor ended without a result".into(),
                            })?;
                            Some(v)
                        }
                        _ => None,
                    };
                    if self.o.current == entry && self.o.frames.len() == base {
                        return Ok(Exit::Returned(result));
                    }
                    if self.o.frames.is_empty() {
                        if let Some(stop) = self.fiber_ends().map_err(placed)? {
                            return Ok(Exit::Stop(stop));
                        }
                        continue;
                    }
                    if let Some(v) = result {
                        self.push(v);
                    }
                }
            }
        }
    }

    /// Module `m` run nested, to completion, THIS `this`: a callback.
    fn nested(
        &mut self,
        m: usize,
        this: Option<AdtRef>,
        depth: usize,
    ) -> Result<Exit, RuntimeError> {
        let base = self.o.frames.len();
        let height = self.o.stack.len();
        self.enter(m, Vec::new(), this, depth, true);
        self.o.nesting += 1;
        let exit = self.exec(base);
        self.o.nesting -= 1;
        self.o.frames.truncate(base);
        self.o.stack.truncate(height);
        exit
    }

    /// A rule's part run for an engine, THIS the rule: the transactions
    /// it leaves open, failing or jumping out, are refused, as the
    /// reference's `atomic` refuses them on the way out.
    fn callback(&mut self, m: usize, this: Option<AdtRef>) -> Result<Exit, RuntimeError> {
        let open = self.o.open;
        let exit = self.nested(m, this, 0);
        if !matches!(exit, Ok(Exit::Returned(_))) {
            for _ in open..self.o.open {
                let _ = self.store.end(false);
            }
            self.o.open = open;
        }
        exit
    }

    /// Runs `body` with the rule state `set` makes, then puts it back;
    /// gives what `body` gives and the state it ended with.
    fn with_ses<T>(
        &mut self,
        set: impl FnOnce(&mut Ses),
        body: impl FnOnce(&mut Self) -> T,
    ) -> (T, Ses) {
        let saved = self.o.ses.clone();
        set(&mut self.o.ses);
        let result = body(self);
        let ended = std::mem::replace(&mut self.o.ses, saved);
        (result, ended)
    }

    /// Whether a fact's ASKABLE, EDITABLE or MODIFIABLE holds, THIS the
    /// object concerned; what it reads is not the PREMISES'.
    fn fact_holds(&mut self, part: &str, code: &Code, this: AdtRef) -> Run<Result<bool, End>> {
        let what = self.store.describe(this);
        let (holds, _) = self.with_ses(
            |s| s.premises = None,
            |vm| vm.holds(part, code, this, &what, 0),
        );
        holds
    }

    /// A value PREMISES read: the lowest probability is theirs.
    fn read_fact(&mut self, probability: Option<f64>) {
        if let Some((low, _)) = &mut self.o.ses.premises {
            *low = low.min(probability.unwrap_or(1.0));
        }
    }

    /// What YES or NO writes takes their reliability as its probability.
    fn stamped(&self, v: Value) -> Value {
        match (self.o.ses.stamp, v) {
            (Some(p), Value::One(mut fz)) => {
                fz.prob = Some(p);
                Value::One(fz)
            }
            (_, v) => v,
        }
    }

    /// A value not there: in PREMISES, asked if the fact is ASKABLE, and
    /// otherwise not known, which makes them false; elsewhere, an error.
    fn unknown(&mut self, o: Id, a: Id) -> Run<Next> {
        let what = self.store.describe(AdtRef::ObjectAttribute(o, a));
        if self.o.ses.premises.is_none() {
            return Err(format!("{what} has no value").into());
        }
        if let Some(code) = self.property(AdtRef::ClassAttribute(a), "ASKABLE") {
            let asked = match self.fact_holds("ASKABLE", &code, AdtRef::Object(o))? {
                Ok(asked) => asked,
                Err(end) => return Ok(Next::End(end)),
            };
            if asked {
                self.io.write(&format!("{what} ? "));
                if let Some(line) = self.io.read_line() {
                    let line = line.trim();
                    let v = (self.language.read(line))
                        .unwrap_or_else(|_| Value::exact(Scalar::String(line.into())));
                    stored(self.store.set(o, a, v.clone()))?;
                    if let Next::End(end) = self.trigger(Invoked::Set(o, a), 0)? {
                        return Ok(Next::End(end));
                    }
                    if let Value::One(fz) = &v {
                        self.read_fact(fz.prob);
                    }
                    self.push(v);
                    return Ok(Next::Go);
                }
            }
        }
        if let Some((_, unknown)) = &mut self.o.ses.premises {
            *unknown = true;
        }
        Err(format!("{what} is not known").into())
    }

    /// Code the store keeps, compiled the first time it runs.
    fn compiled(&mut self, part: &str, language: &str, text: &str, what: &str) -> Run<usize> {
        if language != self.language.name() {
            return Err(format!(
                "the {part} of {what} is written in {language}, which {} does not run",
                self.language.name()
            )
            .into());
        }
        let key = (part.to_string(), text.to_string());
        if let Some(&m) = self.o.compiled.get(&key) {
            return Ok(m);
        }
        let mut module = self.language.compile(part, text, what, self.unit)?;
        relocate(&mut module, self.offset);
        self.o.modules.push(Rc::new(module));
        self.o.origin.push(None);
        let m = self.o.modules.len() - 1;
        self.o.compiled.insert(key, m);
        Ok(m)
    }

    /// An expression the store keeps, its truth, THIS `this`; or the
    /// program's end, if it ends there.
    fn holds(
        &mut self,
        part: &str,
        code: &Code,
        this: AdtRef,
        what: &str,
        depth: usize,
    ) -> Run<Result<bool, End>> {
        let m = self.compiled(part, &code.language, &code.text, what)?;
        match self
            .nested(m, Some(this), depth)
            .map_err(|e| within(e.into(), part, what))?
        {
            Exit::Returned(v) => {
                let v = v.expect("an expression's value");
                let t = hbds_values::truth(&v).map_err(|f| within(fault(f), part, what))?;
                Ok(Ok(t))
            }
            Exit::End(end) => Ok(Err(end)),
            Exit::Yield | Exit::Input | Exit::Stop(_) => {
                unreachable!("code the store calls back runs to completion")
            }
        }
    }

    fn property(&self, r: AdtRef, name: &str) -> Option<Code> {
        self.store.props(r)?.properties.get(name).cloned()
    }

    /// The CONDs and ACTIONs a change runs: the class's, class
    /// attribute's or class link's, then the object's own, THIS being the
    /// object (pages 156 to 158). A false COND refuses the change.
    fn trigger(&mut self, event: Invoked, depth: usize) -> Run<Next> {
        let (adts, this) = match event {
            Invoked::Created(o) => (AdtRef::Class(self.store.objects()[&o].class), o),
            Invoked::Set(o, a) => (AdtRef::ClassAttribute(a), o),
            Invoked::Linked(l) => {
                let link = &self.store.object_links()[&l];
                (AdtRef::ClassLink(link.class_link), link.from)
            }
        };
        let object = AdtRef::Object(this);
        if self.o.ses.inside == 0
            && let Some(code) = self.property(adts, "MODIFIABLE")
        {
            match self.fact_holds("MODIFIABLE", &code, object)? {
                Ok(true) => {}
                Ok(false) => {
                    let what = self.store.describe(adts);
                    return Err(format!("{what} is not MODIFIABLE but by a rule").into());
                }
                Err(end) => return Ok(Next::End(end)),
            }
        }
        for r in [adts, object] {
            let (cond, action) = (self.property(r, "COND"), self.property(r, "ACTION"));
            if cond.is_none() && action.is_none() {
                continue;
            }
            if depth >= DEPTH {
                return Err("COND and ACTION nest too deep".into());
            }
            let what = self.store.describe(r);
            if let Some(code) = cond {
                match self.holds("COND", &code, object, &what, depth + 1)? {
                    Ok(true) => {}
                    Ok(false) => {
                        let refused =
                            format!("the COND of {what} refuses this change: {}", code.text);
                        return Err(refused.into());
                    }
                    Err(end) => return Ok(Next::End(end)),
                }
            }
            if let Some(code) = action {
                let m = self.compiled("ACTION", &code.language, &code.text, &what)?;
                let exit = self.nested(m, Some(object), depth + 1);
                if let Exit::End(end) = exit.map_err(|e| within(e.into(), "ACTION", &what))? {
                    return Ok(Next::End(end));
                }
            }
        }
        Ok(Next::Go)
    }

    // The store's names and designations, as the reference's `named`,
    // `text`, `class` and `designate`.

    /// FACT, RULE and PROCESS, created the first time a program names them.
    fn system(&mut self, name: &str) -> Run<()> {
        let rules = hbds_rules::is_system(name);
        if !rules && !hbds_simulation::is_system(name) {
            return Ok(());
        }
        if self.store.find_class(name).is_some() {
            return Ok(());
        }
        match rules {
            true => stored(hbds_rules::install(self.store)),
            false => stored(hbds_simulation::install(self.store)),
        }
    }

    fn pop_adt(&mut self) -> AdtRef {
        match self.pop() {
            Value::Adt(r) => r,
            v => unreachable!("an a.d.t. expected, not {v}"),
        }
    }

    fn pop_text(&mut self) -> String {
        match self.pop().scalar() {
            Some(Scalar::String(s)) => s.to_string(),
            v => unreachable!("a name expected, not {v:?}"),
        }
    }

    fn pop_class(&mut self) -> Id {
        match self.pop_adt() {
            AdtRef::Class(c) => c,
            r => unreachable!("a class expected, not {r:?}"),
        }
    }

    fn pop_object(&mut self) -> Id {
        match self.pop_adt() {
            AdtRef::Object(o) => o,
            r => unreachable!("an object expected, not {r:?}"),
        }
    }

    /// The class named so: any state if `any`, else active.
    fn class_named(&self, name: &str, any: bool) -> Run<Id> {
        match (any, self.store.find_class(name)) {
            (true, Some(c)) => Ok(c),
            _ => stored(self.store.class(name)),
        }
    }

    /// An object's attribute: computed by its function (pages 127 and
    /// 128), with THIS the object, or stored.
    fn attribute_value(&mut self, o: Id, name: &str) -> Run<Next> {
        let c = self.store.objects()[&o].class;
        let class = self.store.classes()[&c].name.clone();
        let a = stored(self.store.attribute(c, name))?;
        let computed = self.store.attributes()[&a].props.computed;
        if let Some(&m) = self.o.computed.get(&(fold(&class), fold(name))) {
            self.enter(m, Vec::new(), Some(AdtRef::Object(o)), 0, true);
            return Ok(Next::Go);
        }
        if computed {
            return Err(format!(
                "\"{class}\" . \"{name}\" is COMPUTED, but no FUNCTION computes it"
            )
            .into());
        }
        if let Some(v) = self.live(o, a) {
            self.push(v);
            return Ok(Next::Go);
        }
        match self.store.get(o, a).cloned() {
            Some(v) => {
                if let Value::One(fz) = &v {
                    self.read_fact(fz.prob);
                }
                self.push(v);
            }
            None => return self.unknown(o, a),
        }
        Ok(Next::Go)
    }

    /// STATE and CLOCK as the simulation has them, or as the store kept
    /// them, or passive at 0 for a process never run.
    fn live(&self, o: Id, a: Id) -> Option<Value> {
        if !hbds_simulation::is_live(self.store, a) {
            return None;
        }
        let state = same(&self.store.attributes()[&a].name, hbds_simulation::STATE);
        let kept = self.store.get(o, a).cloned();
        let p = Agent::Process(o);
        if !self.sim.knows(p) && kept.is_some() {
            return kept;
        }
        Some(if state {
            let name = hbds_simulation::state_name(self.sim.state(p));
            Value::exact(Scalar::String(name.into()))
        } else {
            Value::exact(Scalar::Float(self.sim.clock(p)))
        })
    }

    fn step(&mut self, i: &Instr, m: &Module, span: Span) -> Run<Next> {
        match i {
            Instr::Stmt => {
                self.o.statements += 1;
                if self.o.budget.is_some_and(|b| self.o.statements > b) {
                    return Err(BUDGET_SPENT.into());
                }
            }
            Instr::Const(v) => self.push(v.clone()),
            Instr::Pop => {
                self.o.stack.pop();
            }
            Instr::Load(s) => {
                let v = self.var(*s).value.clone();
                let v =
                    v.ok_or_else(|| Failure::from(format!("`{}` has no value", self.name(*s))))?;
                self.push(v);
            }
            Instr::LoadIndex(s, n) => {
                let indices = self.pop_integers(*n)?;
                let name = self.name(*s);
                let cell = match &self.var(*s).value {
                    None => return Err(format!("`{name}` has no value").into()),
                    Some(Value::Array(a)) => {
                        a.cells[hbds_values::offset(a, &indices).map_err(fault)?].clone()
                    }
                    Some(_) => return Err(format!("`{name}` is not an array").into()),
                };
                self.push(cell);
            }
            Instr::CheckStore(s) => {
                if self.decl(*s).constant {
                    return Err(format!("`{}` is a constant", self.name(*s)).into());
                }
            }
            Instr::Store(s) => {
                let v = Self::convert(self.pop(), self.decl(*s).ty)?;
                self.var_mut(*s).value = Some(v);
            }
            Instr::StoreIndex(s, n, convert_at) => {
                let indices = self.pop_integers(*n)?;
                let v = self.pop();
                let v = match self.decl(*s).ty {
                    // The conversion fails where the value was written.
                    SlotType::Array(_, Some(b)) => {
                        hbds_values::convert(v, b).map_err(|f| Failure {
                            message: f.0,
                            at: Some(*convert_at),
                        })?
                    }
                    _ => v,
                };
                let name = self.name(*s);
                match &mut self.var_mut(*s).value {
                    Some(Value::Array(a)) => {
                        let at = hbds_values::offset(a, &indices).map_err(fault)?;
                        std::sync::Arc::make_mut(&mut a.cells)[at] = v;
                    }
                    _ => return Err(format!("`{name}` is not an array").into()),
                }
            }
            Instr::Init(s) => {
                let v = Self::convert(self.pop(), m.slots[*s].ty)?;
                *self.var_mut(*s) = Var {
                    value: Some(v.clone()),
                    initial: Some(v),
                };
            }
            Instr::InitArray(s, rank, n) => {
                let bounds = self.pop_integers(2 * n)?;
                let fill = self.pop();
                let (mut lows, mut dims) = (Vec::new(), Vec::new());
                for b in bounds.chunks(2) {
                    let (low, high) = (b[0], b[1]);
                    if high < low {
                        return Err(format!("the bounds {low}:{high} are empty").into());
                    }
                    lows.push(low);
                    dims.push((high - low + 1) as usize);
                }
                let size = if dims.is_empty() {
                    0
                } else {
                    dims.iter().product()
                };
                let a = Value::Array(hbds_values::Array {
                    rank: *rank,
                    lows,
                    dims,
                    cells: vec![fill; size].into(),
                });
                *self.var_mut(*s) = Var {
                    value: Some(a.clone()),
                    initial: Some(a),
                };
            }
            Instr::Reset(s) => {
                let v = self.var_mut(*s);
                v.value = v.initial.clone();
            }
            Instr::Ref(s) => {
                let (f, t) = self.resolve(*s);
                self.o.stack.push(Item::Ref(f, t));
            }
            Instr::Bind(a, s) => {
                let f = self.o.frames.len() - 1;
                let arg = self.o.frames[f].args[*a].clone();
                self.o.frames[f].cells[*s] = match arg {
                    Item::Value(v) => {
                        let v = Self::convert(v, m.slots[*s].ty)?;
                        Cell::Own(Var {
                            value: Some(v.clone()),
                            initial: Some(v),
                        })
                    }
                    Item::Ref(g, t) => Cell::Ref(g, t),
                    Item::Shared(i) => Cell::Shared(i),
                };
            }
            Instr::Prefix(s) => {
                let f = self.o.frames.len() - 1;
                let v = self.o.frames[f].prefix.take();
                self.var_mut(*s).value = v;
            }
            Instr::AsInteger => {
                let v = self.pop();
                hbds_values::integer(&v).map_err(fault)?;
                self.push(v);
            }
            Instr::Op(op) => {
                let b = self.pop();
                let a = self.pop();
                let v = hbds_values::binary(*op, &a, &b).map_err(fault)?;
                self.push(v);
            }
            Instr::Unary(u) => {
                let a = self.pop();
                let v = hbds_values::unary(*u, &a).map_err(fault)?;
                self.push(v);
            }
            Instr::ListOp(op) => {
                let b = self.pop();
                let a = self.pop();
                let (Value::List(l), Value::List(k)) = (&a, &b) else {
                    return Err("UNION, INTER and COMPL take lists".into());
                };
                self.push(Value::List(match op {
                    ListOp::Union => hbds_lists::union(l, k),
                    ListOp::Inter => hbds_lists::inter(l, k),
                    ListOp::Compl => hbds_lists::compl(l, k),
                }));
            }
            Instr::Jump(t) => return Ok(Next::Jump(*t)),
            Instr::JumpFalse(t) => {
                if !hbds_values::truth(&self.pop()).map_err(fault)? {
                    return Ok(Next::Jump(*t));
                }
            }
            Instr::JumpTrue(t) => {
                if hbds_values::truth(&self.pop()).map_err(fault)? {
                    return Ok(Next::Jump(*t));
                }
            }
            Instr::Leave(leave, targets) => {
                let n = hbds_values::integer(&self.pop()).map_err(fault)?;
                if n < 1 {
                    return Err("a number of loops is at least 1".into());
                }
                if n as usize > targets.len() {
                    return Err(too_far(*leave, n, targets.len()).into());
                }
                return Ok(Next::Jump(targets[n as usize - 1]));
            }
            Instr::LoopStep(step, up) => {
                let v = self.pop();
                let zero = Value::integer(0);
                let is_zero = hbds_values::binary(Op::Eq, &v, &zero).map_err(fault)?;
                if is_zero == Value::boolean(true) {
                    return Err("a loop's STEP is not zero".into());
                }
                let gt = hbds_values::binary(Op::Gt, &v, &zero)
                    .and_then(|g| hbds_values::truth(&g))
                    .map_err(fault)?;
                self.var_mut(*step).value = Some(v);
                self.var_mut(*up).value = Some(Value::boolean(gt));
            }
            Instr::Within(up) => {
                let to = self.pop();
                let i = self.pop();
                let up = self.var(*up).value == Some(Value::boolean(true));
                let op = if up { Op::Le } else { Op::Ge };
                let within = hbds_values::binary(op, &i, &to)
                    .and_then(|w| hbds_values::truth(&w))
                    .map_err(fault)?;
                self.push(Value::boolean(within));
            }
            Instr::EachStart(items, position) => {
                let v = self.pop();
                let Value::List(_) = v else {
                    return Err(format!("EACH goes over a list, not {v}").into());
                };
                self.var_mut(*items).value = Some(v);
                self.var_mut(*position).value = Some(Value::integer(0));
            }
            Instr::EachNext(items, position, t) => {
                let k = match &self.var(*position).value {
                    Some(v) => hbds_values::integer(v).map_err(fault)? as usize,
                    None => 0,
                };
                let item = match &self.var(*items).value {
                    Some(Value::List(l)) => l.items.get(k).cloned(),
                    _ => None,
                };
                match item {
                    Some(v) => {
                        self.var_mut(*position).value = Some(Value::integer(k as i64 + 1));
                        self.push(v);
                    }
                    None => return Ok(Next::Jump(*t)),
                }
            }
            Instr::Call(callee, n) => {
                let args = self.o.stack.split_off(self.o.stack.len() - n);
                let within = self.frame().within;
                self.enter(*callee, args, None, 0, within);
            }
            Instr::Result => return Ok(Next::Return(Some(self.pop()))),
            Instr::Return => return Ok(Next::Return(None)),
            Instr::NoResult => return Err("the function ended without a RESULT".into()),
            Instr::ExpectOne(message) => {
                let v = self.pop();
                if !matches!(v, Value::One(_)) {
                    return Err(message.clone().into());
                }
                self.push(v);
            }
            Instr::Native(name) => {
                let Value::One(x) = self.pop() else {
                    unreachable!("ExpectOne comes first")
                };
                let y: Fz = self.language.native(name, &x).map_err(fault)?;
                self.push(Value::One(y));
            }
            Instr::Print(n) => {
                let values = self.o.stack.split_off(self.o.stack.len() - n);
                let texts: Vec<String> = (values.iter())
                    .map(|i| match i {
                        Item::Value(v) => self.store.show(v),
                        Item::Ref(..) | Item::Shared(_) => unreachable!("a value to print"),
                    })
                    .collect();
                self.io.write(&(texts.join(" ") + "\n"));
            }
            Instr::Read(as_text) => {
                if self.o.nesting == 0 && !self.io.ready() {
                    return Ok(Next::Wait);
                }
                let line = self.io.read_line();
                let line = line.ok_or("READ finds no more input")?;
                let v = match as_text {
                    true => Value::exact(Scalar::String(line.into())),
                    false => self.language.read(&line)?,
                };
                self.push(v);
            }
            Instr::Gather(n) => {
                let values = self.o.stack.split_off(self.o.stack.len() - n);
                let mut items = Vec::new();
                for i in values {
                    match i {
                        Item::Value(Value::List(l)) => items.extend(l.items.iter().cloned()),
                        Item::Value(v) => items.push(v),
                        Item::Ref(..) | Item::Shared(_) => unreachable!("a value to gather"),
                    }
                }
                self.push(Value::List(List::new(items)));
            }
            Instr::CheckList(s) => {
                if !matches!(self.var(*s).value, Some(Value::List(_))) {
                    return Err(format!("`{}` is not a list", self.name(*s)).into());
                }
            }
            Instr::Insert(s, place) => {
                let at = match place {
                    Place::End => None,
                    _ => {
                        let k = hbds_values::integer(&self.pop()).map_err(fault)?;
                        let k = usize::try_from(k).unwrap_or(usize::MAX);
                        Some(match place {
                            Place::After => k,
                            _ => k.saturating_sub(1),
                        })
                    }
                };
                let Value::List(items) = self.pop() else {
                    unreachable!("Gather comes first")
                };
                let items = (*items.items).clone();
                hbds_lists::insert(self.list_of(*s), items, at)?;
            }
            Instr::Extract(s) => {
                let Value::List(items) = self.pop() else {
                    unreachable!("Gather comes first")
                };
                hbds_lists::extract(self.list_of(*s), &items.items);
            }
            Instr::Empty(s) => hbds_lists::empty(self.list_of(*s)),
            Instr::Organize(s, circular) => self.list_of(*s).circular = *circular,
            Instr::Functor(key, spelling, n, held) => {
                let args: Vec<Value> = (self.o.stack.split_off(self.o.stack.len() - n))
                    .into_iter()
                    .map(|i| match i {
                        Item::Value(v) => v,
                        Item::Ref(..) | Item::Shared(_) => unreachable!("a functor's argument"),
                    })
                    .collect();
                let mut prefix = self.pop();
                // SIGMA adds attributes' values (page 78).
                if let (Value::List(l), "SIGMA") = (&mut prefix, key.as_str()) {
                    for item in std::sync::Arc::make_mut(&mut l.items).iter_mut() {
                        if let Value::Adt(AdtRef::ObjectAttribute(o, a)) = item {
                            let v = self.store.get(*o, *a).cloned();
                            *item = v.ok_or_else(|| {
                                let what = self.store.describe(AdtRef::ObjectAttribute(*o, *a));
                                Failure::from(format!("{what} has no value"))
                            })?;
                        }
                    }
                }
                let Value::List(mut list) = prefix else {
                    return Err(format!("`{spelling}` applies to a list").into());
                };
                let result = hbds_lists::apply(key, &mut list, &args)?;
                if let Some(s) = held {
                    self.var_mut(*s).value = Some(Value::List(list));
                }
                self.push(result);
            }
            Instr::Time => self.push(Value::exact(Scalar::Float(self.sim.now()))),
            Instr::Pause => match self.role() {
                Role::Main => {
                    if self.o.nesting == 0 && !self.io.ready() {
                        return Ok(Next::Wait);
                    }
                    self.io.write("PAUSE\n");
                    self.io.read_line();
                }
                role => {
                    if self.frame().within {
                        return Err("PAUSE where the store calls back: COND, ACTION, rules and computed attributes run to completion".into());
                    }
                    if role != Role::Coroutine {
                        return Err("PAUSE in a process: it HOLDs or PASSIVATEs".into());
                    }
                    let f = self.o.current;
                    self.o.fibers[f].state = FiberState::Paused;
                    let back = self.o.fibers[f].resumer.expect("a coroutine's resumer");
                    self.switch(back);
                }
            },
            Instr::Stop => return Ok(Next::End(End::Done)),
            Instr::Restart => return Ok(Next::End(End::Restart)),
            Instr::Chain(m) => return Ok(Next::End(End::Chain(*m))),
            Instr::Fail(message) => return Err(message.clone().into()),
            Instr::Named => {
                let v = self.pop();
                let named = match v {
                    Value::Adt(_) => v,
                    Value::One(Fz {
                        x: Scalar::String(ref s),
                        ..
                    }) => {
                        self.system(&s.clone())?;
                        Value::exact(Scalar::String(s.clone()))
                    }
                    Value::One(Fz {
                        x: Scalar::Character(c),
                        ..
                    }) => Value::exact(Scalar::String(c.to_string().into())),
                    v => return Err(format!("{v} names no a.d.t.").into()),
                };
                self.push(named);
            }
            Instr::Text => {
                if let Some(Item::Value(Value::Adt(r))) = self.o.stack.last() {
                    return Err(format!("{} is not a name", self.store.describe(*r)).into());
                }
            }
            Instr::ToClass => {
                let c = match self.pop() {
                    Value::Adt(AdtRef::Class(c)) => c,
                    Value::Adt(r) => {
                        return Err(format!("{} is not a class", self.store.describe(r)).into());
                    }
                    v => {
                        let name = text_of(&v);
                        stored(self.store.class(&name))?
                    }
                };
                self.push(Value::Adt(AdtRef::Class(c)));
            }
            Instr::Designate(any) => {
                let r = match self.pop() {
                    Value::Adt(r) => r,
                    v => AdtRef::Class(self.class_named(&text_of(&v), *any)?),
                };
                self.push(Value::Adt(r));
            }
            Instr::ObjectNamed(any) => {
                let class = self.pop_text();
                let name = self.pop_text();
                let c = self.class_named(&class, *any)?;
                let o = match (any, self.store.find_object(c, &name)) {
                    (true, Some(o)) => o,
                    _ => stored(self.store.object(c, &name))?,
                };
                self.push(Value::Adt(AdtRef::Object(o)));
            }
            Instr::IsObject => {
                let r = self.pop_adt();
                if !matches!(r, AdtRef::Object(_)) {
                    return Err(format!("{} is not an object", self.store.describe(r)).into());
                }
                self.push(Value::Adt(r));
            }
            Instr::AttrOf => {
                let name = self.pop_text();
                let r = match self.pop_adt() {
                    AdtRef::Class(c) => {
                        AdtRef::ClassAttribute(stored(self.store.attribute(c, &name))?)
                    }
                    AdtRef::Object(o) => {
                        let c = self.store.objects()[&o].class;
                        AdtRef::ObjectAttribute(o, stored(self.store.attribute(c, &name))?)
                    }
                    r => {
                        return Err(format!("{} has no attributes", self.store.describe(r)).into());
                    }
                };
                self.push(Value::Adt(r));
            }
            Instr::LinkOf(relation) => {
                let to = self.pop_adt();
                let from = self.pop_adt();
                let r = match (from, to) {
                    (AdtRef::Class(a), AdtRef::Class(b)) => {
                        AdtRef::ClassLink(stored(self.store.class_link(a, relation, b))?)
                    }
                    (AdtRef::Object(a), AdtRef::Object(b)) => self
                        .store
                        .object_link(a, relation, b)
                        .map(AdtRef::ObjectLink)
                        .ok_or("these objects are not linked so")?,
                    _ => return Err("a link joins two classes or two objects".into()),
                };
                self.push(Value::Adt(r));
            }
            Instr::ClassAttr => {
                let name = self.pop_text();
                let c = self.pop_class();
                let a = stored(self.store.attribute(c, &name))?;
                self.push(Value::Adt(AdtRef::ClassAttribute(a)));
            }
            Instr::FindAttribute => {
                let name = self.pop_text();
                let c = self.pop_class();
                let a = self.store.find_attribute(c, &name);
                let a = a.ok_or_else(|| Failure::from(format!("no attribute \"{name}\"")))?;
                self.push(Value::Adt(AdtRef::ClassAttribute(a)));
            }
            Instr::FindLink(relation) => {
                let to = self.pop_class();
                let from = self.pop_class();
                let l = self.store.find_class_link(from, relation, to);
                let l = l.ok_or("no such link")?;
                self.push(Value::Adt(AdtRef::ClassLink(l)));
            }
            Instr::This(message) => match self.frame().this {
                Some(r) => self.push(Value::Adt(r)),
                None => return Err(message.clone().into()),
            },
            Instr::Begin => {
                self.store.begin();
                self.o.open += 1;
            }
            Instr::Commit => {
                self.o.open -= 1;
                stored(self.store.end(true))?;
            }
            Instr::CreateClass(embryo) => {
                let name = self.pop_text();
                let kind = if *embryo {
                    ClassKind::Embryo
                } else {
                    ClassKind::Class
                };
                stored(self.store.create_class(&name, kind))?;
            }
            Instr::CreateObject => {
                let name = self.pop_text();
                let c = self.pop_class();
                let o = stored(self.store.create_object(c, &name))?;
                return self.trigger(Invoked::Created(o), self.frame().depth);
            }
            Instr::CreateAttribute => {
                let name = self.pop_text();
                let c = self.pop_class();
                stored(self.store.create_attribute(c, &name))?;
            }
            Instr::CreateLink(relation, named, reflexive) => {
                let reverse = named.then(|| self.pop_text());
                let to = self.pop_class();
                let from = self.pop_class();
                let r = self.store.create_class_link(
                    from,
                    relation,
                    to,
                    reverse.as_deref(),
                    *reflexive,
                );
                stored(r)?;
            }
            Instr::StateChange(op, n) => {
                let targets: Vec<Value> = (self.o.stack.split_off(self.o.stack.len() - n))
                    .into_iter()
                    .map(|i| match i {
                        Item::Value(v) => v,
                        Item::Ref(..) | Item::Shared(_) => unreachable!("an a.d.t."),
                    })
                    .collect();
                for t in targets {
                    let Value::Adt(t) = t else {
                        unreachable!("an a.d.t.")
                    };
                    let r = match op {
                        StateOp::Hibern => {
                            self.store
                                .change_state(t, AdtState::Active, AdtState::Hibernating)
                        }
                        StateOp::Awake => {
                            self.store
                                .change_state(t, AdtState::Hibernating, AdtState::Active)
                        }
                        StateOp::Destroy => {
                            self.store
                                .change_state(t, AdtState::Active, AdtState::Destroyed)
                        }
                        StateOp::Resur => {
                            self.store
                                .change_state(t, AdtState::Destroyed, AdtState::Active)
                        }
                        StateOp::Purge => self.store.purge_adt(t),
                    };
                    stored(r)?;
                }
            }
            Instr::SetAttribute => {
                let name = self.pop_text();
                let o = self.pop_object();
                let v = self.pop();
                let v = self.stamped(v);
                let c = self.store.objects()[&o].class;
                let a = stored(self.store.attribute(c, &name))?;
                if hbds_simulation::is_live(self.store, a) {
                    return Err(format!("{name} is the simulation engine's").into());
                }
                stored(self.store.set(o, a, v))?;
                return self.trigger(Invoked::Set(o, a), self.frame().depth);
            }
            Instr::SetHeld(variable, at) => {
                let held = self.pop();
                let v = self.pop();
                let Value::Adt(AdtRef::ObjectAttribute(o, a)) = held else {
                    return Err(format!("`{variable}` holds no object's attribute").into());
                };
                let v = self.stamped(v);
                let elsewhere = |f: Failure| Failure {
                    message: f.message,
                    at: f.at.or(Some(*at)),
                };
                stored(self.store.set(o, a, v)).map_err(elsewhere)?;
                return self
                    .trigger(Invoked::Set(o, a), self.frame().depth)
                    .map_err(elsewhere);
            }
            Instr::SetProperty(key, at) => {
                let target = self.pop_adt();
                let v = self.pop();
                let text = match v.scalar() {
                    Some(Scalar::String(s)) => s.to_string(),
                    Some(Scalar::Character(c)) => c.to_string(),
                    _ => return Err(format!("a property is a string, not {v}").into()),
                };
                if hbds_gtad::is_property(key) || hbds_rules::is_property(key) {
                    self.language
                        .check(key, &text)
                        .map_err(|e| Failure::from(format!("this {key} does not parse: {e}")))?;
                }
                let name = self.language.name().to_string();
                stored(self.store.set_property(target, key, &name, &text)).map_err(|f| {
                    Failure {
                        message: f.message,
                        at: Some(*at),
                    }
                })?;
            }
            Instr::Assert(relation, probability, negated) => {
                let b = self.pop_object();
                let a = self.pop_object();
                if *negated {
                    stored(self.store.retract_link(a, relation, b))?;
                } else {
                    let p = probability.or(self.o.ses.stamp);
                    let l = stored(self.store.assert_link(a, relation, b, p))?;
                    return self.trigger(Invoked::Linked(l), self.frame().depth);
                }
            }
            Instr::Specify(spec, unit, computed) => {
                let target = self.pop_adt();
                stored(
                    self.store
                        .specify(target, Some(*spec), unit.clone(), *computed),
                )?;
            }
            Instr::Include(n, m, include) => {
                let hypers: Vec<Id> = (0..*m).map(|_| self.pop_class()).collect::<Vec<_>>();
                let classes: Vec<Id> = (0..*n).map(|_| self.pop_class()).collect::<Vec<_>>();
                for c in classes.iter().rev() {
                    for h in hypers.iter().rev() {
                        let r = match include {
                            true => self.store.include(*c, *h),
                            false => self.store.exclude(*c, *h),
                        };
                        stored(r)?;
                    }
                }
            }
            Instr::Assoc(assoc) => {
                let b = self.pop_adt();
                let a = self.pop_adt();
                stored(match assoc {
                    true => self.store.assoc(a, b),
                    false => self.store.dissoc(a, b),
                })?;
            }
            Instr::Compose(n) => {
                let mut embryos: Vec<Id> = (0..*n).map(|_| self.pop_class()).collect();
                embryos.reverse();
                let name = self.pop_text();
                stored(
                    self.store
                        .create_class(&name, ClassKind::Prototype(embryos)),
                )?;
            }
            Instr::Build(n) => {
                let mut names: Vec<String> = (0..*n).map(|_| self.pop_text()).collect();
                names.reverse();
                let prototype = self.pop_class();
                if !matches!(
                    self.store.classes()[&prototype].kind,
                    ClassKind::Prototype(_)
                ) {
                    return Err("BUILD builds structures AS a prototype".into());
                }
                for name in names {
                    stored(
                        self.store
                            .create_class(&name, ClassKind::Structure(prototype)),
                    )?;
                }
            }
            Instr::AttrValue => {
                let name = self.pop_text();
                let o = self.pop_object();
                return self.attribute_value(o, &name);
            }
            Instr::Linked(relation) => {
                let b = self.pop_object();
                let a = self.pop_object();
                let linked = self.store.object_link(a, relation, b).is_some();
                self.push(Value::boolean(linked));
            }
            Instr::KeepOr(t) => {
                let top = match self.o.stack.last() {
                    Some(Item::Value(v)) => v,
                    _ => unreachable!("a value"),
                };
                if !matches!(top, Value::List(_) | Value::Adt(_)) {
                    self.o.stack.pop();
                    return Ok(Next::Jump(*t));
                }
            }
            Instr::AdtFunctor(key, spelling, n) => {
                let args: Vec<Value> = (self.o.stack.split_off(self.o.stack.len() - n))
                    .into_iter()
                    .map(|i| match i {
                        Item::Value(v) => v,
                        Item::Ref(..) | Item::Shared(_) => unreachable!("a functor's argument"),
                    })
                    .collect();
                let prefix = self.pop();
                let not_adt = |v: &Value| format!("`{spelling}` applies to a.d.t., not to {v}");
                let refs = match &prefix {
                    Value::Adt(r) => vec![*r],
                    Value::List(l) => (l.items.iter())
                        .map(|v| match v {
                            Value::Adt(r) => Ok(*r),
                            v => Err(Failure::from(not_adt(v))),
                        })
                        .collect::<Run<Vec<_>>>()?,
                    v => return Err(not_adt(v).into()),
                };
                let args = (args.iter())
                    .map(|a| match a.scalar() {
                        Some(Scalar::String(s)) => Ok(s.to_string()),
                        Some(Scalar::Character(c)) => Ok(c.to_string()),
                        _ => Err(Failure::from(format!("`{spelling}` takes strings"))),
                    })
                    .collect::<Run<Vec<String>>>()?;
                let found = if hbds_rules::functor(key).is_some() {
                    let compiler: &dyn hbds_rules::Compiler = self.language;
                    hbds_rules::apply(self.store, compiler, key, &refs)?
                } else {
                    stored(hbds_functors::apply(self.store, key, &refs, &args))?
                };
                self.push(Value::List(List::new(
                    found.into_iter().map(Value::Adt).collect(),
                )));
            }
            Instr::Hypotheses => {
                let wrong = "HYPOCHAV takes its hypotheses first, statements in a string";
                let text = match self.pop().scalar() {
                    Some(Scalar::String(s)) => s.to_string(),
                    _ => return Err(wrong.into()),
                };
                (self.language.check("HYPOTHESES", &text))
                    .map_err(|_| Failure::from("HYPOCHAV's hypotheses do not parse"))?;
                self.push(Value::exact(Scalar::String(text.into())));
            }
            Instr::InstallRules => stored(hbds_rules::install(self.store))?,
            Instr::RuleClass(engine) => {
                let class = match self.pop() {
                    Value::Adt(AdtRef::Class(c)) => c,
                    v => match v.scalar() {
                        Some(Scalar::String(name)) => stored(self.store.class(name))?,
                        _ => return Err(format!("{engine} takes rule classes, not {v}").into()),
                    },
                };
                self.push(Value::Adt(AdtRef::Class(class)));
            }
            Instr::ResumeStarted(c, has_args, t) => {
                let Some(&fiber) = self.o.coroutines.get(c) else {
                    return Ok(Next::Go);
                };
                let name = &self.o.modules[*c].spelling;
                if *has_args {
                    return Err("arguments go with the first RESUME only".into());
                }
                match self.o.fibers[fiber].state {
                    FiberState::Ended => return Err(format!("`{name}` has ended").into()),
                    FiberState::Running => {
                        return Err(format!("`{name}` is running: it cannot be resumed").into());
                    }
                    FiberState::Paused => {}
                }
                self.o.frames.last_mut().expect("a frame").pc = *t;
                self.o.fibers[fiber].state = FiberState::Running;
                self.o.fibers[fiber].resumer = Some(self.o.current);
                self.switch(fiber);
            }
            Instr::ResumeFirst(c, n) => self.start(*c, *n),
            Instr::Process => {
                let o = self.pop_object();
                if !hbds_simulation::is_process(self.store, o) {
                    let what = self.store.describe(AdtRef::Object(o));
                    return Err(
                        format!("{what} is not a process: its class is not in PROCESS").into(),
                    );
                }
                self.push(Value::Adt(AdtRef::Object(o)));
            }
            Instr::HasBody => {
                let Some(Item::Value(Value::Adt(AdtRef::Object(p)))) = self.o.stack.last() else {
                    unreachable!("a process")
                };
                let body = self.body(*p)?;
                if !self.o.modules[body].params.is_empty() {
                    return Err("a process's body takes no parameters".into());
                }
            }
            Instr::AsTime => {
                let v = self.pop();
                let t = match v.scalar() {
                    Some(Scalar::Integer(i)) => *i as f64,
                    Some(Scalar::Float(x)) => *x,
                    Some(Scalar::Rational(n, d)) => *n as f64 / *d as f64,
                    _ => return Err(format!("a time is a real number, not {v}").into()),
                };
                self.push(Value::exact(Scalar::Float(t)));
            }
            Instr::Activate(due) => {
                let when = match due {
                    Due::Now => When::Now,
                    Due::At => When::At(self.pop_time()),
                    Due::Before => When::Before(self.pop_process()),
                    Due::After => When::After(self.pop_process()),
                };
                let p = self.pop_process();
                self.sim_op(|sim, d| sim.reactivate(p, when, d))?;
                self.keep_sequencing()?;
            }
            Instr::Cancel => {
                let p = self.pop_process();
                self.sim_op(|sim, d| sim.cancel(p, d))?;
                self.keep_sequencing()?;
            }
            Instr::Kill => {
                let p = self.pop_process();
                self.sim_op(|sim, d| sim.kill(p, d))?;
                self.keep_sequencing()?;
            }
            Instr::Delay => {
                let dt = self.pop_time();
                let p = self.pop_process();
                self.sim_op(|sim, d| sim.delay(p, dt, d))?;
                self.keep_sequencing()?;
            }
            Instr::Running(what) => {
                self.running(what)?;
            }
            Instr::IsRunning => {
                let p = self.pop_process();
                if p != self.running("HOLD")? {
                    let Agent::Process(o) = p else {
                        unreachable!("a process")
                    };
                    let what = self.store.describe(AdtRef::Object(o));
                    return Err(format!("HOLD holds the running process, not {what}").into());
                }
            }
            Instr::Hold => {
                let dt = self.pop_time();
                self.sim_op(|sim, _| sim.hold(dt))?;
                self.keep_sequencing()?;
                if let Some(stop) = self.give_way(span)? {
                    return Ok(Next::Stop(stop));
                }
            }
            Instr::Passivate => {
                self.sim_op(|sim, _| sim.passivate())?;
                if let Some(stop) = self.give_way(span)? {
                    return Ok(Next::Stop(stop));
                }
            }
            Instr::Terminate => {
                self.sim_op(|sim, _| sim.terminate())?;
                match self.role() {
                    Role::Process(p) => {
                        let f = self.o.current;
                        self.o.fibers[f].state = FiberState::Ended;
                        self.o.processes.remove(&p);
                        self.o.running = None;
                        if let Some(stop) = self.dispatch()? {
                            return Ok(Next::Stop(stop));
                        }
                    }
                    _ => return Ok(Next::End(End::Done)),
                }
            }
            Instr::Chav(hypothetical, n) => {
                let mut scope: Vec<Id> = (0..*n).map(|_| self.pop_class()).collect();
                scope.reverse();
                let hypotheses = hypothetical.then(|| Code {
                    language: self.language.name().into(),
                    text: self.pop_text(),
                });
                let mut engine = Engine { vm: self };
                match &hypotheses {
                    Some(h) => hbds_rules::hypochav(&mut engine, h, &scope)?,
                    None => hbds_rules::chav(&mut engine, &scope)?,
                };
            }
        }
        Ok(Next::Go)
    }
}
