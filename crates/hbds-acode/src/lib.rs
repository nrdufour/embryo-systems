//! A-code, HBDS's machine code (docs/architecture.md, decisions 1 and 2):
//! stack bytecode on hbds-core's values, which every language compiles to
//! and the VM runs. A [`Unit`] is modules; a module is its slots (the
//! variables, parameters and the compiler's own), its instructions, and
//! where each comes from in the source, so that a runtime error says
//! where. Each instruction fails at one place at most.
//!
//! The text form ([`Unit::text`], [`Unit::read`]) is a line per
//! declaration or instruction under an `acode` header with the version;
//! values are written as hbds-core's terms. A newer version is refused
//! (decision 10).

use std::fmt::{self, Write as _};

use hbds_core::spec::Spec;
use hbds_core::terms::{self, Reader, Term};
use hbds_values::{Base, Computed, Fz, Op, Rank, Unary, Value};

/// MAY, MUST or CANNOT, as an instruction holds two objects to a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modal {
    May,
    Must,
    Cannot,
}

impl Modal {
    pub fn word(self) -> &'static str {
        match self {
            Modal::May => "may",
            Modal::Must => "must",
            Modal::Cannot => "cannot",
        }
    }
}

/// The version this crate writes and reads.
pub const VERSION: u32 = 1;

/// What a unit needs from the language it was compiled from: its own
/// functions (decision 4's support module), how it reads input, and its
/// compiler for the code the store keeps, which runs when it is needed.
pub trait Language {
    /// As the store records it beside the code: `ADT'81`.
    fn name(&self) -> &str;
    /// A function of the language on one value, by its folded name.
    fn native(&self, name: &str, x: &Fz) -> Computed<Fz>;
    /// A line of input, read as a value (not as text).
    fn read(&self, line: &str) -> Result<Value, String>;
    /// Code the store keeps, a COND, an ACTION or another part, compiled
    /// against `unit`'s modules; `what` names its carrier in messages.
    /// `REPL` is a daemon's REPL entry: statements, as an ACTION's.
    fn compile(&self, part: &str, text: &str, what: &str, unit: &Unit) -> Result<Module, String>;
    /// Whether a property's code is good to keep: refused when written,
    /// as a COND that does not parse (page 80).
    fn check(&self, part: &str, text: &str) -> Result<(), String>;
}

/// A place in the source, 1-based; line 0 is nowhere.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Span {
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    /// The language its code is in, as the store records it beside code
    /// (`ADT'81`): it also compiles the code the store keeps.
    pub language: String,
    pub modules: Vec<Module>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Program,
    Subroutine,
    Function,
    /// A functor of the user's: its prefix and its result are slots.
    Functor,
    Coroutine,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    /// Folded, as calls name it.
    pub name: String,
    /// As written, for messages.
    pub spelling: String,
    pub kind: Kind,
    pub span: Span,
    /// The slots a call's arguments go to, in order.
    pub params: Vec<Param>,
    pub slots: Vec<Slot>,
    /// A functor's prefix and result slots. A functor takes its prefix
    /// as its first argument, which `Prefix` stores if it names one.
    pub prefix: Option<usize>,
    /// The class and attribute (folded) a function computes, pages 127
    /// and 128.
    pub computes: Option<(String, String)>,
    /// The class (folded) a coroutine is the body of, page 112.
    pub body: Option<String>,
    pub result: Option<usize>,
    pub code: Vec<Instr>,
    /// Where each instruction comes from.
    pub spans: Vec<Span>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Param {
    pub slot: usize,
    pub mode: Mode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    In,
    Out,
    InOut,
}

/// A variable: its name as written, what it holds, and whether it is a
/// constant. The compiler's own slots have names starting with `#`.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub name: String,
    pub ty: SlotType,
    pub constant: bool,
}

/// What a value stored in a slot converts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotType {
    /// Stored as it is.
    Any,
    Base(Base),
    List,
    /// Cells converting to the element's base, if it has one.
    Array(Rank, Option<Base>),
}

/// Where INSERT puts what it inserts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    End,
    After,
    Before,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListOp {
    Union,
    Inter,
    Compl,
}

/// When ACTIVATE or REACTIVATE makes a process due: now, AT a time,
/// BEFORE or AFTER another process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due {
    Now,
    At,
    Before,
    After,
}

/// HIBERN, AWAKE, DESTROY, RESUR, PURGE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateOp {
    Hibern,
    Awake,
    Destroy,
    Resur,
    Purge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leave {
    Exit,
    Next,
    Reloop,
}

/// What a run is refused past its statement budget, if it has one.
pub const BUDGET_SPENT: &str = "the statement budget is spent";

/// Why `n` loops cannot be left, `loops` being around: EXIT, NEXT and
/// RELOOP count those of their own module.
pub fn too_far(leave: Leave, n: i64, loops: usize) -> String {
    let word = match leave {
        Leave::Exit => "EXIT",
        Leave::Next => "NEXT",
        Leave::Reloop => "RELOOP",
    };
    let around = match loops {
        0 => "no loop is around it".to_string(),
        1 => "one loop is around it".to_string(),
        k => format!("{k} loops are around it"),
    };
    format!("{word} {n} counts {n} loop(s), and {around} in its module")
}

/// The instructions. "Pops" and "pushes" are the operand stack's; `s` is
/// a slot of the module, `t` an instruction's index.
#[derive(Debug, Clone, PartialEq)]
pub enum Instr {
    /// A statement begins: counted, and refused past the budget.
    Stmt,
    Const(Value),
    Pop,
    /// Pushes `s`'s value; fails if it has none.
    Load(usize),
    /// Pops n indices, pushes the cell of the array in `s`.
    LoadIndex(usize, usize),
    /// Fails if `s` is a constant.
    CheckStore(usize),
    /// Pops a value, converts it as `s` holds, stores it.
    Store(usize),
    /// Pops n indices and a value under them, converts it to `s`'s cell
    /// type (failing at the span given) and stores the cell.
    StoreIndex(usize, usize, Span),
    /// Pops a value, converts it as `s` holds, makes it `s`'s value and
    /// what RESET gives back.
    Init(usize),
    /// Pops 2n bounds (low, high) and a fill value: an array in `s`.
    InitArray(usize, Rank, usize),
    /// `s` back to its first value.
    Reset(usize),
    /// Pushes a reference to `s`, for an OUT or INOUT argument.
    Ref(usize),
    /// In a callee's prologue: the i-th argument into slot `s`.
    Bind(usize, usize),
    /// In a functor's prologue: the prefix it applies to into slot `s`.
    Prefix(usize),
    /// Pops a value; fails unless it is an integer, which it pushes back.
    AsInteger,
    Op(Op),
    Unary(Unary),
    ListOp(ListOp),
    Jump(usize),
    /// Pops a condition; jumps if false; fails unless a boolean.
    JumpFalse(usize),
    JumpTrue(usize),
    /// Pops a count of loops, jumps to the target of that many: EXIT,
    /// NEXT, RELOOP with a computed count.
    Leave(Leave, Vec<usize>),
    /// Pops a STEP: fails if zero; stores it in the first slot, and
    /// whether it goes up in the second.
    LoopStep(usize, usize),
    /// Pops the bound, then the counter; pushes whether the counter is
    /// within it, as the second slot of LoopStep says.
    Within(usize),
    /// Pops a list; its items and a position into the two slots: EACH.
    EachStart(usize, usize),
    /// Pushes the next item of EachStart's slots, or jumps to `t`.
    EachNext(usize, usize, usize),
    /// Pops n arguments and calls module `m`; pushes its result if it is
    /// a function or a functor (the prefix first, for a functor).
    Call(usize, usize),
    /// Pops a value: the function's result; returns.
    Result,
    Return,
    /// The end of a function that gave no RESULT: fails.
    NoResult,
    /// Pops a value; fails unless it is one value, not several.
    ExpectOne(String),
    /// Pops a value, pushes the language's function of it.
    Native(String),
    /// Pops n values, writes them on a line.
    Print(usize),
    /// Reads a line: as text if true, else as a value; pushes it.
    Read(bool),
    /// Pops n values, pushes a list of them, lists spread.
    Gather(usize),
    /// Fails unless `s` holds a list.
    CheckList(usize),
    /// Pops a position (unless at the end) and a list of items: INSERT.
    Insert(usize, Place),
    /// Pops a list of items: EXTRACT.
    Extract(usize),
    Empty(usize),
    /// CIRCULAR (true) or LINEAR (false).
    Organize(usize, bool),
    /// Pops n arguments and a list; pushes the list functor (folded
    /// name, then as written) applied, storing the list it changed back
    /// in `s` if any.
    Functor(String, String, usize, Option<usize>),
    /// The simulation's time.
    Time,
    Pause,
    Stop,
    Restart,
    /// Ends this program and runs module `m`.
    Chain(usize),
    Fail(String),

    // The store. Names are strings; a.d.t. are `Value::Adt`.
    /// Pops what a name in a designation gives: a string naming a class
    /// or an object, or an a.d.t.; FACT, RULE and PROCESS installed the
    /// first time a string names them.
    Named,
    /// Pops what Named gave; fails unless a string.
    Text,
    /// Pops what Named gave; pushes the active class it names or is.
    ToClass,
    /// Pops what Named gave; pushes the a.d.t. it names or is, any state
    /// if true.
    Designate(bool),
    /// Pops a class's name, then an object's: pushes the object, any
    /// state if true.
    ObjectNamed(bool),
    /// Fails unless the a.d.t. on top is an object.
    IsObject,
    /// Pops an attribute's name and an a.d.t.: pushes its attribute.
    AttrOf,
    /// Pops two a.d.t.: pushes the link between them.
    LinkOf(String),
    /// Pops an attribute's name and a class: pushes the class attribute.
    ClassAttr,
    /// As ClassAttr, whatever its state: HIBERN and the others.
    FindAttribute,
    /// Pops two classes: the class link between them, whatever its state.
    FindLink(String),
    /// Pushes THIS, or fails with the message.
    This(String),
    /// A statement's transaction begins.
    Begin,
    /// It ends, kept.
    Commit,
    /// Pops a name: a class, an embryo if true.
    CreateClass(bool),
    /// Pops a name and a class: an object, its CONDs and ACTIONs run.
    CreateObject,
    CreateAttribute,
    /// Pops a reverse's name if named, then two classes.
    CreateLink(String, bool, bool),
    /// Pops n a.d.t.
    StateChange(StateOp, usize),
    /// Pops an attribute's name, an object and a value: LET.
    SetAttribute,
    /// Pops a variable's object attribute and a value: LET through it;
    /// the store's refusal at the span given.
    SetHeld(String, Span),
    /// Pops an a.d.t. and a property's code; the store's refusal at the
    /// span given.
    SetProperty(String, Span),
    /// Pops two objects; the probability; RETRACT if true.
    Assert(String, Option<f64>, bool),
    /// Pops two objects: what holds them to the link (page 159).
    Modality(String, Modal),
    /// Pops an a.d.t.: its specification, unit, COMPUTED.
    Specify(Spec, Option<String>, bool),
    /// Pops m hyperclasses and n classes: INCLUDE if true, EXCLUDE else.
    Include(usize, usize, bool),
    /// Pops two a.d.t.: ASSOC if true, DISSOC else.
    Assoc(bool),
    /// Pops n embryos and a name: COMPOSE.
    Compose(usize),
    /// Pops n names and a prototype: BUILD.
    Build(usize),
    /// Pops an attribute's name and an object: pushes its value.
    AttrValue,
    /// Pops two objects: pushes whether they are linked so.
    Linked(String),
    /// Keeps a list or an a.d.t. on top; anything else is popped and
    /// the designation at `t` gives the prefix instead.
    KeepOr(usize),
    /// Pops n strings and a prefix, an a.d.t. or a list of them: pushes
    /// the functor's result, a list.
    AdtFunctor(String, String, usize),
    /// Pops HYPOCHAV's hypotheses: pushes them back once they are a
    /// string that compiles.
    Hypotheses,
    /// FACT and RULE, created if they are not.
    InstallRules,
    /// Pops a rule class or its name, for the engine named so: pushes
    /// the class.
    RuleClass(String),
    /// Pops n rule classes, and the hypotheses if HYPOCHAV: runs CHAV or
    /// HYPOCHAV over them.
    Chav(bool, usize),
    /// Pops a dialog or its name: installs the dialog engine's classes,
    /// starts a run of it, and pushes the run (figure 72).
    Dial,
    /// Takes the run on top one step, in a transaction of its own,
    /// reading a line if it waits for one: pushes whether it is done.
    DialStep,
    /// RESUME of coroutine m once started: refused with arguments, ended
    /// or running; else it goes on, and then the code at `t`.
    ResumeStarted(usize, bool, usize),
    /// The first RESUME of coroutine m: pops its n arguments.
    ResumeFirst(usize, usize),
    /// Pops an object: pushes it back if it is a process.
    Process,
    /// Pops a process: pushes it back once its class has a body that
    /// takes no parameters.
    HasBody,
    /// Pops a duration or a time: pushes it as a real.
    AsTime,
    /// Pops when, if not now, and a process: ACTIVATE or REACTIVATE.
    Activate(Due),
    /// Pops a process.
    Cancel,
    Kill,
    /// Pops a duration and a process.
    Delay,
    /// The statement named so may give way here: the main program or a
    /// process does, not a coroutine nor code the store calls back.
    Running(String),
    /// Pops a process: the running one, HOLD's.
    IsRunning,
    /// Pops a duration.
    Hold,
    Passivate,
    Terminate,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// The text form

const OPS: &[(&str, Op)] = &[
    ("or", Op::Or),
    ("nor", Op::Nor),
    ("xor", Op::Xor),
    ("and", Op::And),
    ("nand", Op::Nand),
    ("eq", Op::Eq),
    ("ne", Op::Ne),
    ("lt", Op::Lt),
    ("le", Op::Le),
    ("gt", Op::Gt),
    ("ge", Op::Ge),
    ("in", Op::In),
    ("notin", Op::NotIn),
    ("concat", Op::Concat),
    ("add", Op::Add),
    ("sub", Op::Sub),
    ("array.add", Op::ArrayAdd),
    ("mul", Op::Mul),
    ("div", Op::Div),
    ("mod", Op::Mod),
    ("ratio", Op::Ratio),
    ("array.mul", Op::ArrayMul),
    ("array.dot", Op::ArrayDot),
    ("array.pow", Op::ArrayPow),
    ("pow", Op::Pow),
];

const UNARIES: &[(&str, Unary)] = &[
    ("neg", Unary::Neg),
    ("number", Unary::Number),
    ("not", Unary::Not),
];

const LIST_OPS: &[(&str, ListOp)] = &[
    ("union", ListOp::Union),
    ("inter", ListOp::Inter),
    ("compl", ListOp::Compl),
];

const KINDS: &[(&str, Kind)] = &[
    ("program", Kind::Program),
    ("subroutine", Kind::Subroutine),
    ("function", Kind::Function),
    ("functor", Kind::Functor),
    ("coroutine", Kind::Coroutine),
];

const BASES: &[(&str, Base)] = &[
    ("integer", Base::Integer),
    ("rational", Base::Rational),
    ("float", Base::Float),
    ("gauss", Base::Gauss),
    ("complex", Base::Complex),
    ("quaternion", Base::Quaternion),
    ("boolean", Base::Boolean),
    ("character", Base::Character),
    ("string", Base::String),
];

const RANKS: &[(&str, Rank)] = &[
    ("vector", Rank::Vector),
    ("matrix", Rank::Matrix),
    ("tensor", Rank::Tensor),
];

const DUES: &[(&str, Due)] = &[
    ("now", Due::Now),
    ("at", Due::At),
    ("before", Due::Before),
    ("after", Due::After),
];

const STATE_OPS: &[(&str, StateOp)] = &[
    ("hibern", StateOp::Hibern),
    ("awake", StateOp::Awake),
    ("destroy", StateOp::Destroy),
    ("resur", StateOp::Resur),
    ("purge", StateOp::Purge),
];

const MODES: &[(&str, Mode)] = &[("in", Mode::In), ("out", Mode::Out), ("inout", Mode::InOut)];

const PLACES: &[(&str, Place)] = &[
    ("end", Place::End),
    ("after", Place::After),
    ("before", Place::Before),
];

const LEAVES: &[(&str, Leave)] = &[
    ("exit", Leave::Exit),
    ("next", Leave::Next),
    ("reloop", Leave::Reloop),
];

fn name_of<T: PartialEq + Copy>(table: &[(&'static str, T)], x: T) -> &'static str {
    table.iter().find(|(_, y)| *y == x).expect("in the table").0
}

fn named<T: Copy>(table: &[(&str, T)], w: &str, what: &str) -> Result<T, String> {
    (table.iter().find(|(n, _)| *n == w))
        .map(|(_, x)| *x)
        .ok_or_else(|| format!("{w:?} is not {what}"))
}

fn base_word(b: Base) -> Term {
    match BASES.iter().find(|(_, x)| *x == b) {
        Some((n, _)) => terms::word(n),
        None => terms::word("adt"),
    }
}

fn slot_type(t: SlotType) -> Vec<Term> {
    use terms::word;
    match t {
        SlotType::Any => vec![word("any")],
        SlotType::Base(b) => vec![base_word(b)],
        SlotType::List => vec![word("list")],
        SlotType::Array(rank, element) => vec![
            word("array"),
            word(name_of(RANKS, rank)),
            element.map_or(word("any"), base_word),
        ],
    }
}

/// An instruction as terms: its mnemonic, then its operands.
fn instr(i: &Instr) -> Vec<Term> {
    use terms::{text, word};
    let n = |x: &usize| word(x);
    match i {
        Instr::Stmt => vec![word("stmt")],
        Instr::Const(v) => vec![word("const"), terms::value(v)],
        Instr::Pop => vec![word("pop")],
        Instr::Load(s) => vec![word("load"), n(s)],
        Instr::LoadIndex(s, k) => vec![word("load.index"), n(s), n(k)],
        Instr::CheckStore(s) => vec![word("check.store"), n(s)],
        Instr::Store(s) => vec![word("store"), n(s)],
        Instr::StoreIndex(s, k, at) => {
            vec![word("store.index"), n(s), n(k), word(span_text(*at))]
        }
        Instr::Init(s) => vec![word("init"), n(s)],
        Instr::InitArray(s, rank, k) => {
            vec![word("init.array"), n(s), word(name_of(RANKS, *rank)), n(k)]
        }
        Instr::Reset(s) => vec![word("reset"), n(s)],
        Instr::Ref(s) => vec![word("ref"), n(s)],
        Instr::Bind(a, s) => vec![word("bind"), n(a), n(s)],
        Instr::Prefix(s) => vec![word("prefix"), n(s)],
        Instr::AsInteger => vec![word("as.integer")],
        Instr::Op(op) => vec![word("op"), word(name_of(OPS, *op))],
        Instr::Unary(u) => vec![word("unary"), word(name_of(UNARIES, *u))],
        Instr::ListOp(op) => vec![word("list.op"), word(name_of(LIST_OPS, *op))],
        Instr::Jump(t) => vec![word("jump"), n(t)],
        Instr::JumpFalse(t) => vec![word("jump.false"), n(t)],
        Instr::JumpTrue(t) => vec![word("jump.true"), n(t)],
        Instr::Leave(l, targets) => {
            let mut out = vec![word("leave"), word(name_of(LEAVES, *l))];
            out.extend(targets.iter().map(n));
            out
        }
        Instr::LoopStep(a, b) => vec![word("loop.step"), n(a), n(b)],
        Instr::Within(s) => vec![word("within"), n(s)],
        Instr::EachStart(a, b) => vec![word("each.start"), n(a), n(b)],
        Instr::EachNext(a, b, t) => vec![word("each.next"), n(a), n(b), n(t)],
        Instr::Call(m, k) => vec![word("call"), n(m), n(k)],
        Instr::Result => vec![word("result")],
        Instr::Return => vec![word("return")],
        Instr::NoResult => vec![word("no.result")],
        Instr::ExpectOne(f) => vec![word("expect.one"), text(f)],
        Instr::Native(f) => vec![word("native"), text(f)],
        Instr::Print(k) => vec![word("print"), n(k)],
        Instr::Read(as_text) => vec![word("read"), word(if *as_text { "text" } else { "value" })],
        Instr::Gather(k) => vec![word("gather"), n(k)],
        Instr::CheckList(s) => vec![word("check.list"), n(s)],
        Instr::Insert(s, p) => vec![word("insert"), n(s), word(name_of(PLACES, *p))],
        Instr::Extract(s) => vec![word("extract"), n(s)],
        Instr::Empty(s) => vec![word("empty"), n(s)],
        Instr::Organize(s, circular) => vec![
            word("organize"),
            n(s),
            word(if *circular { "circular" } else { "linear" }),
        ],
        Instr::Functor(f, spelling, k, held) => vec![
            word("functor"),
            text(f),
            text(spelling),
            n(k),
            terms::optional(held.map(word)),
        ],
        Instr::Time => vec![word("time")],
        Instr::Pause => vec![word("pause")],
        Instr::Stop => vec![word("stop")],
        Instr::Restart => vec![word("restart")],
        Instr::Chain(m) => vec![word("chain"), n(m)],
        Instr::Fail(m) => vec![word("fail"), text(m)],
        Instr::Named => vec![word("named")],
        Instr::Text => vec![word("text")],
        Instr::ToClass => vec![word("to.class")],
        Instr::Designate(any) => vec![word("designate"), word(any)],
        Instr::ObjectNamed(any) => vec![word("object.named"), word(any)],
        Instr::IsObject => vec![word("is.object")],
        Instr::AttrOf => vec![word("attr.of")],
        Instr::LinkOf(r) => vec![word("link.of"), text(r)],
        Instr::ClassAttr => vec![word("class.attr")],
        Instr::FindAttribute => vec![word("find.attribute")],
        Instr::FindLink(r) => vec![word("find.link"), text(r)],
        Instr::This(m) => vec![word("this"), text(m)],
        Instr::Begin => vec![word("begin")],
        Instr::Commit => vec![word("commit")],
        Instr::CreateClass(embryo) => vec![word("create.class"), word(embryo)],
        Instr::CreateObject => vec![word("create.object")],
        Instr::CreateAttribute => vec![word("create.attribute")],
        Instr::CreateLink(r, named, reflexive) => {
            vec![word("create.link"), text(r), word(named), word(reflexive)]
        }
        Instr::StateChange(op, k) => vec![word("state"), word(name_of(STATE_OPS, *op)), n(k)],
        Instr::SetAttribute => vec![word("set.attribute")],
        Instr::SetHeld(v, at) => vec![word("set.held"), text(v), word(span_text(*at))],
        Instr::SetProperty(k, at) => vec![word("set.property"), text(k), word(span_text(*at))],
        Instr::Modality(r, m) => vec![word("modality"), text(r), word(m.word())],
        Instr::Assert(r, p, negated) => vec![
            word("assert"),
            text(r),
            terms::optional(p.map(terms::float)),
            word(negated),
        ],
        Instr::Specify(spec, unit, computed) => vec![
            word("specify"),
            text(&spec.to_string()),
            terms::optional(unit.as_deref().map(text)),
            word(computed),
        ],
        Instr::Include(k, m, include) => vec![word("include"), n(k), n(m), word(include)],
        Instr::Assoc(assoc) => vec![word("assoc"), word(assoc)],
        Instr::Compose(k) => vec![word("compose"), n(k)],
        Instr::Build(k) => vec![word("build"), n(k)],
        Instr::AttrValue => vec![word("attr.value")],
        Instr::Linked(r) => vec![word("linked"), text(r)],
        Instr::KeepOr(t) => vec![word("keep.or"), n(t)],
        Instr::AdtFunctor(f, spelling, k) => {
            vec![word("adt.functor"), text(f), text(spelling), n(k)]
        }
        Instr::Hypotheses => vec![word("hypotheses")],
        Instr::InstallRules => vec![word("install.rules")],
        Instr::RuleClass(e) => vec![word("rule.class"), text(e)],
        Instr::Chav(hypotheses, k) => vec![word("chav"), word(hypotheses), n(k)],
        Instr::Dial => vec![word("dial")],
        Instr::DialStep => vec![word("dial.step")],
        Instr::ResumeStarted(m, args, t) => vec![word("resume.started"), n(m), word(args), n(t)],
        Instr::ResumeFirst(m, k) => vec![word("resume.first"), n(m), n(k)],
        Instr::Process => vec![word("process")],
        Instr::HasBody => vec![word("has.body")],
        Instr::AsTime => vec![word("as.time")],
        Instr::Activate(due) => vec![word("activate"), word(name_of(DUES, *due))],
        Instr::Cancel => vec![word("cancel")],
        Instr::Kill => vec![word("kill")],
        Instr::Delay => vec![word("delay")],
        Instr::Running(what) => vec![word("running"), text(what)],
        Instr::IsRunning => vec![word("is.running")],
        Instr::Hold => vec![word("hold")],
        Instr::Passivate => vec![word("passivate")],
        Instr::Terminate => vec![word("terminate")],
    }
}

fn read_instr(r: &mut Reader) -> Result<Instr, String> {
    let n = |r: &mut Reader| r.parse::<usize>();
    Ok(match r.word()? {
        "stmt" => Instr::Stmt,
        "const" => Instr::Const(r.value()?),
        "pop" => Instr::Pop,
        "load" => Instr::Load(n(r)?),
        "load.index" => Instr::LoadIndex(n(r)?, n(r)?),
        "check.store" => Instr::CheckStore(n(r)?),
        "store" => Instr::Store(n(r)?),
        "store.index" => Instr::StoreIndex(n(r)?, n(r)?, read_span(r.word()?)?),
        "init" => Instr::Init(n(r)?),
        "init.array" => Instr::InitArray(n(r)?, named(RANKS, r.word()?, "a rank")?, n(r)?),
        "reset" => Instr::Reset(n(r)?),
        "ref" => Instr::Ref(n(r)?),
        "bind" => Instr::Bind(n(r)?, n(r)?),
        "prefix" => Instr::Prefix(n(r)?),
        "as.integer" => Instr::AsInteger,
        "op" => Instr::Op(named(OPS, r.word()?, "an operation")?),
        "unary" => Instr::Unary(named(UNARIES, r.word()?, "an operation")?),
        "list.op" => Instr::ListOp(named(LIST_OPS, r.word()?, "a list operation")?),
        "jump" => Instr::Jump(n(r)?),
        "jump.false" => Instr::JumpFalse(n(r)?),
        "jump.true" => Instr::JumpTrue(n(r)?),
        "leave" => Instr::Leave(named(LEAVES, r.word()?, "a way out")?, r.rest(n)?),
        "loop.step" => Instr::LoopStep(n(r)?, n(r)?),
        "within" => Instr::Within(n(r)?),
        "each.start" => Instr::EachStart(n(r)?, n(r)?),
        "each.next" => Instr::EachNext(n(r)?, n(r)?, n(r)?),
        "call" => Instr::Call(n(r)?, n(r)?),
        "result" => Instr::Result,
        "return" => Instr::Return,
        "no.result" => Instr::NoResult,
        "expect.one" => Instr::ExpectOne(r.text()?),
        "native" => Instr::Native(r.text()?),
        "print" => Instr::Print(n(r)?),
        "read" => Instr::Read(match r.word()? {
            "text" => true,
            "value" => false,
            w => return Err(format!("{w:?} is not text nor value")),
        }),
        "gather" => Instr::Gather(n(r)?),
        "check.list" => Instr::CheckList(n(r)?),
        "insert" => Instr::Insert(n(r)?, named(PLACES, r.word()?, "a place")?),
        "extract" => Instr::Extract(n(r)?),
        "empty" => Instr::Empty(n(r)?),
        "organize" => Instr::Organize(
            n(r)?,
            match r.word()? {
                "circular" => true,
                "linear" => false,
                w => return Err(format!("{w:?} is not circular nor linear")),
            },
        ),
        "functor" => Instr::Functor(r.text()?, r.text()?, n(r)?, r.optional(n)?),
        "time" => Instr::Time,
        "pause" => Instr::Pause,
        "stop" => Instr::Stop,
        "restart" => Instr::Restart,
        "chain" => Instr::Chain(n(r)?),
        "fail" => Instr::Fail(r.text()?),
        "named" => Instr::Named,
        "text" => Instr::Text,
        "to.class" => Instr::ToClass,
        "designate" => Instr::Designate(r.parse()?),
        "object.named" => Instr::ObjectNamed(r.parse()?),
        "is.object" => Instr::IsObject,
        "attr.of" => Instr::AttrOf,
        "link.of" => Instr::LinkOf(r.text()?),
        "class.attr" => Instr::ClassAttr,
        "find.attribute" => Instr::FindAttribute,
        "find.link" => Instr::FindLink(r.text()?),
        "this" => Instr::This(r.text()?),
        "begin" => Instr::Begin,
        "commit" => Instr::Commit,
        "create.class" => Instr::CreateClass(r.parse()?),
        "create.object" => Instr::CreateObject,
        "create.attribute" => Instr::CreateAttribute,
        "create.link" => Instr::CreateLink(r.text()?, r.parse()?, r.parse()?),
        "state" => Instr::StateChange(named(STATE_OPS, r.word()?, "a state change")?, n(r)?),
        "set.attribute" => Instr::SetAttribute,
        "set.held" => Instr::SetHeld(r.text()?, read_span(r.word()?)?),
        "set.property" => Instr::SetProperty(r.text()?, read_span(r.word()?)?),
        "assert" => Instr::Assert(r.text()?, r.optional(|r| r.parse())?, r.parse()?),
        "modality" => Instr::Modality(
            r.text()?,
            match r.word()? {
                "may" => Modal::May,
                "must" => Modal::Must,
                "cannot" => Modal::Cannot,
                w => return Err(format!("no modality {w}")),
            },
        ),
        "specify" => {
            let spec = r.text()?;
            let spec = spec
                .parse()
                .map_err(|_| format!("{spec:?} is not a specification"))?;
            Instr::Specify(spec, r.optional(|r| r.text())?, r.parse()?)
        }
        "include" => Instr::Include(n(r)?, n(r)?, r.parse()?),
        "assoc" => Instr::Assoc(r.parse()?),
        "compose" => Instr::Compose(n(r)?),
        "build" => Instr::Build(n(r)?),
        "attr.value" => Instr::AttrValue,
        "linked" => Instr::Linked(r.text()?),
        "keep.or" => Instr::KeepOr(n(r)?),
        "adt.functor" => Instr::AdtFunctor(r.text()?, r.text()?, n(r)?),
        "hypotheses" => Instr::Hypotheses,
        "install.rules" => Instr::InstallRules,
        "rule.class" => Instr::RuleClass(r.text()?),
        "chav" => Instr::Chav(r.parse()?, n(r)?),
        "dial" => Instr::Dial,
        "dial.step" => Instr::DialStep,
        "resume.started" => Instr::ResumeStarted(n(r)?, r.parse()?, n(r)?),
        "resume.first" => Instr::ResumeFirst(n(r)?, n(r)?),
        "process" => Instr::Process,
        "has.body" => Instr::HasBody,
        "as.time" => Instr::AsTime,
        "activate" => Instr::Activate(named(DUES, r.word()?, "a due")?),
        "cancel" => Instr::Cancel,
        "kill" => Instr::Kill,
        "delay" => Instr::Delay,
        "running" => Instr::Running(r.text()?),
        "is.running" => Instr::IsRunning,
        "hold" => Instr::Hold,
        "passivate" => Instr::Passivate,
        "terminate" => Instr::Terminate,
        w => return Err(format!("{w:?} is not an instruction")),
    })
}

fn read_base(w: &str) -> Result<Option<Base>, String> {
    match w {
        "any" => Ok(None),
        "adt" => Err("an a.d.t. type does not convert".into()),
        w => named(BASES, w, "a type").map(Some),
    }
}

fn read_slot_type(r: &mut Reader) -> Result<SlotType, String> {
    Ok(match r.word()? {
        "any" => SlotType::Any,
        "list" => SlotType::List,
        "array" => {
            let rank = named(RANKS, r.word()?, "a rank")?;
            SlotType::Array(rank, read_base(r.word()?)?)
        }
        w => SlotType::Base(named(BASES, w, "a type")?),
    })
}

fn span_text(s: Span) -> String {
    if s.line == 0 {
        "-".into()
    } else {
        format!("{}:{}", s.line, s.column)
    }
}

fn read_span(w: &str) -> Result<Span, String> {
    if w == "-" {
        return Ok(Span::default());
    }
    let (l, c) = w
        .split_once(':')
        .ok_or_else(|| format!("{w:?} is not a place"))?;
    match (l.parse(), c.parse()) {
        (Ok(line), Ok(column)) => Ok(Span { line, column }),
        _ => Err(format!("{w:?} is not a place")),
    }
}

fn line(items: &[Term]) -> String {
    let mut out = String::new();
    for (i, t) in items.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        terms::write_term(t, &mut out);
    }
    out
}

impl Unit {
    /// The text form.
    pub fn text(&self) -> String {
        use terms::{text, word};
        let mut out = format!("acode {VERSION}\n");
        writeln!(out, "{}", line(&[word("language"), text(&self.language)])).unwrap();
        for m in &self.modules {
            let header = [
                word("module"),
                word(name_of(KINDS, m.kind)),
                text(&m.spelling),
                text(&m.name),
                word(span_text(m.span)),
            ];
            writeln!(out, "\n{}", line(&header)).unwrap();
            for s in &m.slots {
                let mut items = vec![word("slot"), text(&s.name)];
                items.extend(slot_type(s.ty));
                if s.constant {
                    items.push(word("constant"));
                }
                writeln!(out, "{}", line(&items)).unwrap();
            }
            for p in &m.params {
                let items = [word("param"), word(p.slot), word(name_of(MODES, p.mode))];
                writeln!(out, "{}", line(&items)).unwrap();
            }
            if let Some(p) = m.prefix {
                writeln!(out, "prefix {p}").unwrap();
            }
            if let Some(r) = m.result {
                writeln!(out, "result {r}").unwrap();
            }
            if let Some((c, a)) = &m.computes {
                writeln!(out, "{}", line(&[word("computes"), text(c), text(a)])).unwrap();
            }
            if let Some(c) = &m.body {
                writeln!(out, "{}", line(&[word("body"), text(c)])).unwrap();
            }
            for (pc, (i, s)) in m.code.iter().zip(&m.spans).enumerate() {
                writeln!(out, "{pc:>4} {:<8} {}", span_text(*s), line(&instr(i))).unwrap();
            }
        }
        out
    }

    /// A unit from its text form.
    pub fn read(text: &str) -> Result<Unit, Error> {
        let mut lines = text
            .lines()
            .enumerate()
            .filter(|(_, l)| !l.trim().is_empty());
        let fail = |n: usize, e: String| Error(format!("A-code line {}: {e}", n + 1));
        let (n, first) = lines.next().ok_or_else(|| Error("no A-code here".into()))?;
        let version: u32 = (first.strip_prefix("acode "))
            .and_then(|v| v.trim().parse().ok())
            .ok_or_else(|| fail(n, "an `acode VERSION` header expected".into()))?;
        if version > VERSION {
            return Err(Error(format!(
                "A-code version {version} is newer than this reader's, {VERSION}"
            )));
        }
        let (n, second) = lines.next().ok_or_else(|| Error("no A-code here".into()))?;
        let terms = terms::read_terms(second.trim()).map_err(|e| fail(n, e))?;
        let language = match terms.as_slice() {
            [Term::Word(w), Term::Text(l)] if w == "language" => l.clone(),
            _ => return Err(fail(n, "a `language \"NAME\"` line expected".into())),
        };
        let mut modules: Vec<Module> = Vec::new();
        for (n, l) in lines {
            let terms = terms::read_terms(l.trim()).map_err(|e| fail(n, e))?;
            let mut r = Reader::new(&terms);
            let at = |e| fail(n, e);
            let first = match terms.first() {
                Some(Term::Word(w)) => w.clone(),
                _ => return Err(at("a declaration or an instruction expected".into())),
            };
            if first == "module" {
                r.word().map_err(at)?;
                let kind = named(KINDS, r.word().map_err(at)?, "a module kind").map_err(at)?;
                let spelling = r.text().map_err(at)?;
                let name = r.text().map_err(at)?;
                let span = read_span(r.word().map_err(at)?).map_err(at)?;
                r.end().map_err(at)?;
                modules.push(Module {
                    name,
                    spelling,
                    kind,
                    span,
                    params: Vec::new(),
                    slots: Vec::new(),
                    prefix: None,
                    result: None,
                    computes: None,
                    body: None,
                    code: Vec::new(),
                    spans: Vec::new(),
                });
                continue;
            }
            let m = modules
                .last_mut()
                .ok_or_else(|| at("a module expected first".into()))?;
            match first.as_str() {
                "slot" => {
                    r.word().map_err(at)?;
                    let name = r.text().map_err(at)?;
                    let ty = read_slot_type(&mut r).map_err(at)?;
                    let rest = r.rest(|r| r.word().map(String::from)).map_err(at)?;
                    let constant = match rest.as_slice() {
                        [] => false,
                        [w] if w == "constant" => true,
                        _ => return Err(at(format!("{rest:?} after a slot's type"))),
                    };
                    m.slots.push(Slot { name, ty, constant });
                }
                "param" => {
                    r.word().map_err(at)?;
                    let slot = r.parse().map_err(at)?;
                    let mode = named(MODES, r.word().map_err(at)?, "a mode").map_err(at)?;
                    r.end().map_err(at)?;
                    m.params.push(Param { slot, mode });
                }
                "computes" => {
                    r.word().map_err(at)?;
                    m.computes = Some((r.text().map_err(at)?, r.text().map_err(at)?));
                    r.end().map_err(at)?;
                }
                "body" => {
                    r.word().map_err(at)?;
                    m.body = Some(r.text().map_err(at)?);
                    r.end().map_err(at)?;
                }
                "prefix" | "result" => {
                    r.word().map_err(at)?;
                    let slot = Some(r.parse().map_err(at)?);
                    r.end().map_err(at)?;
                    match first.as_str() {
                        "prefix" => m.prefix = slot,
                        _ => m.result = slot,
                    }
                }
                _ => {
                    let pc: usize = r.parse().map_err(at)?;
                    if pc != m.code.len() {
                        return Err(at(format!(
                            "instruction {pc} where {} is next",
                            m.code.len()
                        )));
                    }
                    let span = read_span(r.word().map_err(at)?).map_err(at)?;
                    let i = read_instr(&mut r).map_err(at)?;
                    r.end().map_err(at)?;
                    m.code.push(i);
                    m.spans.push(span);
                }
            }
        }
        Ok(Unit { language, modules })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hbds_values::Scalar;

    fn sample() -> Unit {
        let at = |line, column| Span { line, column };
        Unit {
            language: "ADT'81".into(),
            modules: vec![
                Module {
                    name: "P".into(),
                    spelling: "p".into(),
                    kind: Kind::Program,
                    span: at(1, 1),
                    params: Vec::new(),
                    slots: vec![
                        Slot {
                            name: "n".into(),
                            ty: SlotType::Base(Base::Integer),
                            constant: false,
                        },
                        Slot {
                            name: "v".into(),
                            ty: SlotType::Array(Rank::Vector, Some(Base::Float)),
                            constant: true,
                        },
                        Slot {
                            name: "#0".into(),
                            ty: SlotType::Any,
                            constant: false,
                        },
                    ],
                    prefix: None,
                    result: None,
                    computes: None,
                    body: None,
                    code: vec![
                        Instr::Stmt,
                        Instr::Const(Value::exact(Scalar::String("say \"hi\"".into()))),
                        Instr::Const(Value::One(Fz::new(
                            Scalar::Float(2.25),
                            Some(0.5),
                            None,
                            None,
                        ))),
                        Instr::Op(Op::ArrayDot),
                        Instr::Leave(Leave::Next, vec![3, 7]),
                        Instr::Functor("SUBLIST".into(), "sublist".into(), 2, Some(0)),
                        Instr::Functor("CARD".into(), "Card".into(), 0, None),
                        Instr::Fail("division by zero".into()),
                        Instr::Specify(
                            "LONG FLOAT APPR".parse().unwrap(),
                            Some("kelvin".into()),
                            true,
                        ),
                        Instr::Assert("crosses".into(), Some(0.9), false),
                        Instr::SetHeld("t".into(), at(7, 2)),
                        Instr::StateChange(StateOp::Resur, 2),
                        Instr::Hypotheses,
                        Instr::InstallRules,
                        Instr::RuleClass("hypochav".into()),
                        Instr::Chav(true, 1),
                        Instr::ResumeStarted(1, true, 20),
                        Instr::ResumeFirst(1, 2),
                        Instr::Activate(Due::Before),
                        Instr::Running("HOLD".into()),
                        Instr::Terminate,
                    ],
                    spans: vec![
                        at(3, 9),
                        Span::default(),
                        at(3, 14),
                        at(3, 14),
                        at(4, 1),
                        at(5, 2),
                        at(5, 2),
                        at(6, 3),
                        at(7, 1),
                        at(7, 1),
                        at(7, 1),
                        at(8, 1),
                        at(8, 1),
                        at(8, 1),
                        at(8, 9),
                        at(8, 1),
                        at(9, 1),
                        at(9, 1),
                        at(9, 2),
                        at(9, 3),
                        at(9, 4),
                    ],
                },
                Module {
                    name: "F".into(),
                    spelling: "f".into(),
                    kind: Kind::Functor,
                    span: at(9, 1),
                    params: vec![Param {
                        slot: 0,
                        mode: Mode::InOut,
                    }],
                    slots: vec![
                        Slot {
                            name: "x".into(),
                            ty: SlotType::List,
                            constant: false,
                        },
                        Slot {
                            name: "f".into(),
                            ty: SlotType::Any,
                            constant: false,
                        },
                    ],
                    prefix: Some(0),
                    result: Some(1),
                    computes: None,
                    body: Some("worker".into()),
                    code: vec![Instr::Return],
                    spans: vec![at(10, 1)],
                },
            ],
        }
    }

    #[test]
    fn text_round_trips() {
        let unit = sample();
        let text = unit.text();
        assert_eq!(Unit::read(&text), Ok(unit), "{text}");
    }

    #[test]
    fn a_newer_version_is_refused() {
        let text = sample().text().replacen("acode 1", "acode 2", 1);
        assert_eq!(
            Unit::read(&text),
            Err(Error(
                "A-code version 2 is newer than this reader's, 1".into()
            ))
        );
    }

    #[test]
    fn a_unit_says_its_language() {
        let text = sample().text().replacen("language \"ADT'81\"\n", "", 1);
        let e = Unit::read(&text).unwrap_err().0;
        assert!(e.contains("a `language \"NAME\"` line expected"), "{e}");
    }

    #[test]
    fn errors_say_where() {
        let text = sample().text().replace("op array.dot", "op array.cross");
        let e = Unit::read(&text).unwrap_err().0;
        assert!(e.contains("\"array.cross\" is not an operation"), "{e}");
        let text = sample().text().replace("   1 -", "   2 -");
        let e = Unit::read(&text).unwrap_err().0;
        assert!(e.contains("instruction 2 where 1 is next"), "{e}");
    }
}
