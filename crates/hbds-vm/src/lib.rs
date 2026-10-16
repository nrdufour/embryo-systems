//! The VM (docs/architecture.md, decisions 2 and 3): runs A-code. A run
//! is one fiber, its frames and its operand stack, as data: no thread,
//! no Rust recursion, so that a paused flow of control can be kept.
//! What a value does is hbds-values'; lists are hbds-lists'.

use std::fmt;

use hbds_acode::{
    BUDGET_SPENT, Instr, Kind, ListOp, Module, Place, SlotType, Span, Support, Unit, too_far,
};
use hbds_values::{Fault, Fz, List, Op, Scalar, Value};

/// Where the program reads its input and writes its output.
pub trait Io {
    fn read_line(&mut self) -> Option<String>;
    fn write(&mut self, text: &str);
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

/// Runs the unit's first module, a program, then those it CHAINs to:
/// the statements it executed.
pub fn run(
    unit: &Unit,
    io: &mut dyn Io,
    support: &dyn Support,
    budget: Option<u64>,
) -> Result<u64, RuntimeError> {
    let mut vm = Vm {
        unit,
        io,
        support,
        budget,
        statements: 0,
        frames: Vec::new(),
        stack: Vec::new(),
    };
    let mut current = 0;
    loop {
        vm.frames.clear();
        vm.stack.clear();
        vm.enter(current, Vec::new());
        match vm.exec()? {
            End::Done => return Ok(vm.statements),
            End::Restart => {}
            End::Chain(m) => current = m,
        }
    }
}

/// A variable's value, and the one RESET gives back.
#[derive(Debug, Clone, Default)]
struct Var {
    value: Option<Value>,
    initial: Option<Value>,
}

/// A slot of a frame: its own variable, or a caller's (OUT and INOUT).
#[derive(Debug, Clone)]
enum Cell {
    Own(Var),
    Ref(usize, usize),
}

/// What the operand stack holds: values, and references to variables
/// on their way to an OUT or INOUT parameter.
#[derive(Debug, Clone)]
enum Item {
    Value(Value),
    Ref(usize, usize),
}

struct Frame {
    module: usize,
    pc: usize,
    cells: Vec<Cell>,
    args: Vec<Item>,
    prefix: Option<Value>,
}

enum End {
    Done,
    Restart,
    Chain(usize),
}

/// What an instruction does to the flow.
enum Next {
    Go,
    Jump(usize),
    Return(Option<Value>),
    End(End),
}

struct Vm<'r> {
    unit: &'r Unit,
    io: &'r mut dyn Io,
    support: &'r dyn Support,
    budget: Option<u64>,
    statements: u64,
    frames: Vec<Frame>,
    stack: Vec<Item>,
}

/// Why an instruction fails, and where if not where it comes from.
struct Failure {
    message: String,
    at: Option<Span>,
}

impl From<String> for Failure {
    fn from(message: String) -> Failure {
        Failure { message, at: None }
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

/// A value as PRINT writes it.
fn show(v: &Value) -> String {
    match v {
        Value::List(l) => format!(
            "[{}]",
            l.items.iter().map(show).collect::<Vec<_>>().join(", ")
        ),
        v => v.to_string(),
    }
}

impl<'r> Vm<'r> {
    fn enter(&mut self, module: usize, mut args: Vec<Item>) {
        let m = &self.unit.modules[module];
        let mut prefix = None;
        if m.kind == Kind::Functor && !args.is_empty() {
            prefix = match args.remove(0) {
                Item::Value(v) => Some(v),
                Item::Ref(..) => None,
            };
        }
        self.frames.push(Frame {
            module,
            pc: 0,
            cells: vec![Cell::Own(Var::default()); m.slots.len()],
            args,
            prefix,
        });
    }

    fn module(&self) -> &'r Module {
        &self.unit.modules[self.frames.last().expect("a frame").module]
    }

    /// The frame and slot a slot of the current frame stands for.
    fn resolve(&self, s: usize) -> (usize, usize) {
        let f = self.frames.len() - 1;
        match self.frames[f].cells[s] {
            Cell::Own(_) => (f, s),
            Cell::Ref(g, t) => (g, t),
        }
    }

    fn var(&self, s: usize) -> &Var {
        let (f, t) = self.resolve(s);
        match &self.frames[f].cells[t] {
            Cell::Own(v) => v,
            Cell::Ref(..) => unreachable!("references are resolved when made"),
        }
    }

    fn var_mut(&mut self, s: usize) -> &mut Var {
        let (f, t) = self.resolve(s);
        match &mut self.frames[f].cells[t] {
            Cell::Own(v) => v,
            Cell::Ref(..) => unreachable!("references are resolved when made"),
        }
    }

    /// The declaration of the variable a slot stands for, in the module
    /// that declares it.
    fn decl(&self, s: usize) -> &'r hbds_acode::Slot {
        let (f, t) = self.resolve(s);
        &self.unit.modules[self.frames[f].module].slots[t]
    }

    fn name(&self, s: usize) -> &'r str {
        &self.module().slots[s].name
    }

    fn pop(&mut self) -> Value {
        match self.stack.pop().expect("an operand") {
            Item::Value(v) => v,
            Item::Ref(..) => unreachable!("a reference where a value is expected"),
        }
    }

    fn push(&mut self, v: Value) {
        self.stack.push(Item::Value(v));
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

    fn exec(&mut self) -> Result<End, RuntimeError> {
        loop {
            let m = self.module();
            let f = self.frames.last_mut().expect("a frame");
            let pc = f.pc;
            f.pc += 1;
            let span = m.spans[pc];
            let next = self.step(&m.code[pc], m).map_err(|f| RuntimeError {
                span: f.at.unwrap_or(span),
                message: f.message,
            })?;
            match next {
                Next::Go => {}
                Next::Jump(t) => self.frames.last_mut().expect("a frame").pc = t,
                Next::End(end) => return Ok(end),
                Next::Return(value) => {
                    let callee = self.frames.pop().expect("a frame");
                    let kind = self.unit.modules[callee.module].kind;
                    if self.frames.is_empty() {
                        return Ok(End::Done);
                    }
                    let result = match kind {
                        Kind::Function => value,
                        Kind::Functor => {
                            let m = &self.unit.modules[callee.module];
                            let r = m.result.expect("a functor's result slot");
                            let v = match &callee.cells[r] {
                                Cell::Own(v) => v.value.clone(),
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
                    if let Some(v) = result {
                        self.push(v);
                    }
                }
            }
        }
    }

    fn step(&mut self, i: &Instr, m: &'r Module) -> Run<Next> {
        match i {
            Instr::Stmt => {
                self.statements += 1;
                if self.budget.is_some_and(|b| self.statements > b) {
                    return Err(BUDGET_SPENT.into());
                }
            }
            Instr::Const(v) => self.push(v.clone()),
            Instr::Pop => {
                self.stack.pop();
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
                self.stack.push(Item::Ref(f, t));
            }
            Instr::Bind(a, s) => {
                let f = self.frames.len() - 1;
                let arg = self.frames[f].args[*a].clone();
                self.frames[f].cells[*s] = match arg {
                    Item::Value(v) => {
                        let v = Self::convert(v, m.slots[*s].ty)?;
                        Cell::Own(Var {
                            value: Some(v.clone()),
                            initial: Some(v),
                        })
                    }
                    Item::Ref(g, t) => Cell::Ref(g, t),
                };
            }
            Instr::Prefix(s) => {
                let f = self.frames.len() - 1;
                let v = self.frames[f].prefix.take();
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
                let args = self.stack.split_off(self.stack.len() - n);
                self.enter(*callee, args);
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
                let y: Fz = self.support.native(name, &x).map_err(fault)?;
                self.push(Value::One(y));
            }
            Instr::Print(n) => {
                let values = self.stack.split_off(self.stack.len() - n);
                let texts: Vec<String> = (values.iter())
                    .map(|i| match i {
                        Item::Value(v) => show(v),
                        Item::Ref(..) => unreachable!("a value to print"),
                    })
                    .collect();
                self.io.write(&(texts.join(" ") + "\n"));
            }
            Instr::Read(as_text) => {
                let line = self.io.read_line();
                let line = line.ok_or("READ finds no more input")?;
                let v = match as_text {
                    true => Value::exact(Scalar::String(line.into())),
                    false => self.support.read(&line)?,
                };
                self.push(v);
            }
            Instr::Gather(n) => {
                let values = self.stack.split_off(self.stack.len() - n);
                let mut items = Vec::new();
                for i in values {
                    match i {
                        Item::Value(Value::List(l)) => items.extend(l.items.iter().cloned()),
                        Item::Value(v) => items.push(v),
                        Item::Ref(..) => unreachable!("a value to gather"),
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
                let args: Vec<Value> = (self.stack.split_off(self.stack.len() - n))
                    .into_iter()
                    .map(|i| match i {
                        Item::Value(v) => v,
                        Item::Ref(..) => unreachable!("a functor's argument"),
                    })
                    .collect();
                let Value::List(mut list) = self.pop() else {
                    return Err(format!("`{spelling}` applies to a list").into());
                };
                let result = hbds_lists::apply(key, &mut list, &args)?;
                if let Some(s) = held {
                    self.var_mut(*s).value = Some(Value::List(list));
                }
                self.push(result);
            }
            Instr::Time => self.push(Value::exact(Scalar::Float(0.0))),
            Instr::Pause => {
                self.io.write("PAUSE\n");
                self.io.read_line();
            }
            Instr::Stop => return Ok(Next::End(End::Done)),
            Instr::Restart => return Ok(Next::End(End::Restart)),
            Instr::Chain(m) => return Ok(Next::End(End::Chain(*m))),
            Instr::Fail(message) => return Err(message.clone().into()),
        }
        Ok(Next::Go)
    }
}
