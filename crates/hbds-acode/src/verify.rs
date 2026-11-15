//! The verifier (docs/architecture.md, decisions 1 and 10): A-code comes
//! from files and from clients, and a machine that runs it as it is
//! written would index out of its slots, jump out of its code or pop an
//! empty stack. A unit is checked once, when it is read: every slot,
//! jump and call within bounds, the operand stack the same along every
//! path to an instruction, and no path that falls off a module's end.
//! What a value is (a name, a class, a list) stays the machine's to
//! check as it runs.

use std::collections::VecDeque;

use crate::{Error, Instr, Kind, Module, Place, Unit};

/// What the operand stack holds: a value, or a reference to a variable
/// on its way to an OUT or INOUT parameter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Item {
    Value,
    Ref,
}

/// The operands before an instruction, top last, and the transactions
/// its module has begun and not ended.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
struct Stack {
    items: Vec<Item>,
    open: usize,
}

impl Stack {
    fn len(&self) -> usize {
        self.items.len()
    }

    fn push(&mut self, i: Item) {
        self.items.push(i);
    }
}

/// What an instruction leaves of the stack, for each place it goes on to.
type Steps = Result<Vec<(To, Stack)>, String>;

/// Pops `n` operands: values, or references too if `any`.
fn pop(s: &mut Stack, n: usize, any: bool) -> Result<(), String> {
    if s.len() < n {
        return Err(format!(
            "{n} operand(s) popped, and the stack holds {}",
            s.len()
        ));
    }
    let popped = s.items.split_off(s.len() - n);
    if !any && popped.contains(&Item::Ref) {
        return Err("a reference where a value is expected".into());
    }
    Ok(())
}

/// Where an instruction goes on to.
enum To {
    /// The next instruction.
    Next,
    At(usize),
}

impl Unit {
    /// Refuses a unit the machine cannot run safely: why, and where.
    pub fn verify(&self) -> Result<(), Error> {
        match self.modules.first().map(|m| m.kind) {
            Some(Kind::Program) => {}
            Some(_) => return Err(Error("A-code: a unit's first module is its program".into())),
            None => return Err(Error("A-code: a unit has a module, its program".into())),
        }
        let modules = |c: usize| self.modules.get(c);
        for m in &self.modules {
            let modules = &modules;
            Verifier { modules, m }
                .module()
                .map_err(|e| Error(format!("A-code module \"{}\": {e}", m.spelling)))?;
        }
        Ok(())
    }
}

/// How many operands module `m` holds before its instruction `pc`: what
/// a saved fiber's stack is checked against. `modules` gives the modules
/// its instructions name. `None` if no path leads there.
pub fn depth_at<'a>(
    m: &'a Module,
    pc: usize,
    modules: &'a dyn Fn(usize) -> Option<&'a Module>,
) -> Option<usize> {
    let stacks = Verifier { modules, m }.stacks().ok()?;
    stacks.get(pc)?.as_ref().map(Stack::len)
}

struct Verifier<'a> {
    /// The modules instructions name, by their number.
    modules: &'a dyn Fn(usize) -> Option<&'a Module>,
    m: &'a Module,
}

impl Verifier<'_> {
    fn module(&self) -> Result<(), String> {
        let m = self.m;
        if m.spans.len() != m.code.len() {
            return Err("as many places as instructions are expected".into());
        }
        for p in &m.params {
            self.slot(p.slot)?;
        }
        for s in m.prefix.iter().chain(&m.result) {
            self.slot(*s)?;
        }
        match m.kind {
            Kind::Program if !m.params.is_empty() => {
                return Err("a program takes no parameters".into());
            }
            Kind::Functor if m.result.is_none() => {
                return Err("a functor has a result slot".into());
            }
            _ => {}
        }
        self.stacks().map(|_| ())
    }

    fn slot(&self, s: usize) -> Result<(), String> {
        match s < self.m.slots.len() {
            true => Ok(()),
            false => Err(format!(
                "slot {s}, and the module has {}",
                self.m.slots.len()
            )),
        }
    }

    fn target(&self, t: usize) -> Result<(), String> {
        match t < self.m.code.len() {
            true => Ok(()),
            false => Err(format!(
                "a jump to {t}, and the module has {} instructions",
                self.m.code.len()
            )),
        }
    }

    fn callee(&self, c: usize) -> Result<&Module, String> {
        (self.modules)(c).ok_or_else(|| format!("module {c}, and the unit has no such module"))
    }

    /// The stack before each instruction a path leads to.
    fn stacks(&self) -> Result<Vec<Option<Stack>>, String> {
        let code = &self.m.code;
        if code.is_empty() {
            return Err("no instructions".into());
        }
        let mut before: Vec<Option<Stack>> = vec![None; code.len()];
        before[0] = Some(Stack::default());
        let mut work = VecDeque::from([0]);
        while let Some(pc) = work.pop_front() {
            let stack = before[pc].clone().expect("queued with its stack");
            let at = |e: String| format!("instruction {pc}: {e}");
            for (to, after) in self.step(&code[pc], stack).map_err(at)? {
                let t = match to {
                    To::Next => pc + 1,
                    To::At(t) => t,
                };
                if t >= code.len() {
                    return Err(at("the module ends without a return".into()));
                }
                match &before[t] {
                    None => {
                        before[t] = Some(after);
                        work.push_back(t);
                    }
                    Some(known) if *known == after => {}
                    Some(known) => {
                        return Err(at(format!(
                            "instruction {t} is reached with {} operand(s) and {} \
                             transaction(s) open, and with {} and {}",
                            known.len(),
                            known.open,
                            after.len(),
                            after.open
                        )));
                    }
                }
            }
        }
        // What no path leads to is not run; its operands are checked all
        // the same, so that nothing the unit holds is out of bounds.
        for (pc, i) in code.iter().enumerate() {
            if before[pc].is_none() {
                self.operands(i)
                    .map_err(|e| format!("instruction {pc}: {e}"))?;
            }
        }
        Ok(before)
    }

    /// The slots, targets and modules an instruction names.
    fn operands(&self, i: &Instr) -> Result<(), String> {
        use Instr::*;
        match i {
            Load(s)
            | CheckStore(s)
            | Store(s)
            | Init(s)
            | Reset(s)
            | Ref(s)
            | Prefix(s)
            | Within(s)
            | LoopAdd(s)
            | CheckList(s)
            | Insert(s, _)
            | Extract(s)
            | Empty(s)
            | Organize(s, _)
            | LoadIndex(s, _)
            | StoreIndex(s, _, _)
            | InitArray(s, _, _) => self.slot(*s),
            Bind(a, s) => {
                if *a >= self.m.params.len() {
                    return Err(format!(
                        "argument {a}, and the module takes {}",
                        self.m.params.len()
                    ));
                }
                self.slot(*s)
            }
            LoopStep(a, b) | EachStart(a, b) => self.slot(*a).and_then(|()| self.slot(*b)),
            EachNext(a, b, t) => {
                self.slot(*a)?;
                self.slot(*b)?;
                self.target(*t)
            }
            Functor(_, _, _, held) => held.map_or(Ok(()), |s| self.slot(s)),
            Jump(t) | JumpFalse(t) | JumpTrue(t) | KeepOr(t) => self.target(*t),
            Leave(_, targets) => targets.iter().try_for_each(|t| self.target(*t)),
            Call(c, n) => {
                let callee = self.callee(*c)?;
                // A functor takes its prefix first.
                let takes = callee.params.len() + (callee.kind == Kind::Functor) as usize;
                if *n != takes {
                    return Err(format!(
                        "a call with {n} argument(s) to \"{}\", which takes {takes}",
                        callee.spelling
                    ));
                }
                Ok(())
            }
            Chain(c) => match self.callee(*c)?.kind {
                Kind::Program => Ok(()),
                _ => Err("CHAIN runs a program".into()),
            },
            ResumeStarted(c, _, t) => {
                self.coroutine(*c)?;
                self.target(*t)
            }
            ResumeFirst(c, n) => {
                let callee = self.coroutine(*c)?;
                if *n != callee.params.len() {
                    return Err(format!(
                        "a RESUME with {n} argument(s) of \"{}\", which takes {}",
                        callee.spelling,
                        callee.params.len()
                    ));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn coroutine(&self, c: usize) -> Result<&Module, String> {
        let callee = self.callee(c)?;
        match callee.kind {
            Kind::Coroutine => Ok(callee),
            _ => Err(format!("\"{}\" is not a coroutine", callee.spelling)),
        }
    }

    /// What an instruction leaves of `stack`, for each place it goes on
    /// to; none for one that ends the module or the run.
    fn step(&self, i: &Instr, mut s: Stack) -> Steps {
        use Instr::*;
        self.operands(i)?;
        // Pops `n` values, pushes `m`, and goes on to the next.
        let go = |mut s: Stack, n: usize, m: usize| -> Steps {
            pop(&mut s, n, false)?;
            s.items.extend(std::iter::repeat_n(Item::Value, m));
            Ok(vec![(To::Next, s)])
        };
        match i {
            Stmt | CheckStore(_) | Reset(_) | Bind(..) | Prefix(_) | CheckList(_) | Empty(_)
            | Organize(..) | Pause | InstallRules | Running(_) | Passivate | Terminate => {
                go(s, 0, 0)
            }
            Begin => {
                s.open += 1;
                Ok(vec![(To::Next, s)])
            }
            Commit => {
                if s.open == 0 {
                    return Err("a commit, and no transaction is begun".into());
                }
                s.open -= 1;
                Ok(vec![(To::Next, s)])
            }
            Const(_) | Load(_) | Read(_) | Time | This(_) => go(s, 0, 1),
            Pop => {
                pop(&mut s, 1, true)?;
                Ok(vec![(To::Next, s)])
            }
            Ref(_) => {
                s.push(Item::Ref);
                Ok(vec![(To::Next, s)])
            }
            LoadIndex(_, n) => go(s, *n, 1),
            Store(_) | Init(_) | LoopStep(..) | EachStart(..) | Extract(_) | CreateClass(_)
            | Specify(..) | Cancel | Kill | IsRunning | Hold => go(s, 1, 0),
            StoreIndex(_, n, _) => go(s, n + 1, 0),
            InitArray(_, _, n) => go(s, 2 * n + 1, 0),
            AsInteger | Unary(_) | ExpectOne(_) | Native(_) | Named | ToClass | Designate(_)
            | IsObject | Hypotheses | RuleClass(_) | Dial | Process | AsTime => go(s, 1, 1),
            Op(_) | ListOp(_) | Within(_) | LoopAdd(_) | ObjectNamed(_) | AttrOf | LinkOf(_)
            | ClassAttr | FindAttribute | FindLink(_) | AttrValue | Linked(_) => go(s, 2, 1),
            // These look at the top, and leave it.
            Text | HasBody => go(s, 1, 1),
            DialStep => go(s, 1, 2),
            Jump(t) => Ok(vec![(To::At(*t), s)]),
            JumpFalse(t) | JumpTrue(t) => {
                pop(&mut s, 1, false)?;
                Ok(vec![(To::Next, s.clone()), (To::At(*t), s)])
            }
            Leave(_, targets) => {
                pop(&mut s, 1, false)?;
                Ok(targets.iter().map(|t| (To::At(*t), s.clone())).collect())
            }
            EachNext(_, _, t) => {
                let mut next = s.clone();
                next.push(Item::Value);
                Ok(vec![(To::Next, next), (To::At(*t), s)])
            }
            KeepOr(t) => {
                let kept = s.clone();
                pop(&mut s, 1, false)?;
                Ok(vec![(To::Next, kept), (To::At(*t), s)])
            }
            Call(c, n) => {
                pop(&mut s, *n, true)?;
                if matches!(self.callee(*c)?.kind, Kind::Function | Kind::Functor) {
                    s.push(Item::Value);
                }
                Ok(vec![(To::Next, s)])
            }
            ResumeStarted(_, _, t) => Ok(vec![(To::Next, s.clone()), (To::At(*t), s)]),
            ResumeFirst(_, n) => {
                pop(&mut s, *n, true)?;
                Ok(vec![(To::Next, s)])
            }
            // A module's end: nothing of its own is left on the stack,
            // which its caller goes on with.
            Result => {
                if self.m.kind != Kind::Function {
                    return Err("a result where no function is".into());
                }
                pop(&mut s, 1, false)?;
                self.left(&s)
            }
            Return => {
                if self.m.kind == Kind::Function {
                    return Err("a function returns a RESULT".into());
                }
                self.left(&s)
            }
            NoResult | Stop | Restart | Chain(_) | Fail(_) => Ok(Vec::new()),
            Print(n) | StateChange(_, n) => go(s, *n, 0),
            Gather(n) => go(s, *n, 1),
            Insert(_, place) => go(s, 1 + (*place != Place::End) as usize, 0),
            Functor(_, _, n, _) | AdtFunctor(_, _, n) => go(s, n + 1, 1),
            CreateObject | CreateAttribute | SetHeld(..) | SetProperty(..) | Assert(..)
            | Modality(..) | Assoc(_) | Delay => go(s, 2, 0),
            CreateLink(_, named, _) => go(s, 2 + *named as usize, 0),
            SetAttribute => go(s, 3, 0),
            Include(n, m, _) => go(s, n + m, 0),
            Compose(n) | Build(n) => go(s, n + 1, 0),
            Chav(hypothetical, n) => go(s, n + *hypothetical as usize, 0),
            Activate(due) => go(s, 1 + (*due != crate::Due::Now) as usize, 0),
        }
    }

    fn left(&self, s: &Stack) -> Steps {
        match (s.len(), s.open) {
            (0, 0) => Ok(Vec::new()),
            (0, _) => Err("a return that leaves a transaction open".into()),
            (n, _) => Err(format!("a return that leaves {n} operand(s) on the stack")),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Unit;

    /// What reading a unit of one program and one subroutine gives:
    /// nothing, or why it is refused.
    fn refusal(program: &[&str], subroutine: &[&str]) -> Option<String> {
        let mut text = String::from("acode 1\nlanguage \"L\"\n\nmodule program \"P\" \"P\" 1:1\n");
        text += "slot \"n\" integer\n";
        for (n, i) in program.iter().enumerate() {
            text += &format!("{n} 1:1 {i}\n");
        }
        text += "\nmodule subroutine \"S\" \"S\" 1:1\nslot \"x\" any\nparam 0 in\n";
        for (n, i) in subroutine.iter().enumerate() {
            text += &format!("{n} 1:1 {i}\n");
        }
        Unit::read(&text).err().map(|e| e.0)
    }

    const SUB: &[&str] = &["bind 0 0", "return"];

    fn program(code: &[&str]) -> Option<String> {
        refusal(code, SUB).map(|e| {
            let e = e
                .strip_prefix("A-code module \"P\": ")
                .expect("the program's");
            e.to_string()
        })
    }

    #[test]
    fn what_the_compiler_writes_passes() {
        let code = [
            "stmt",
            "const (i 1)",
            "store 0",
            "load 0",
            "const (i 3)",
            "op lt",
            "jump.false 11",
            "load 0",
            "call 1 1",
            "ref 0",
            "call 1 1",
            "begin",
            "commit",
            "return",
        ];
        assert_eq!(program(&code), None);
    }

    #[test]
    fn the_reviews_four() {
        let e = program(&["load 7", "return"]).unwrap();
        assert_eq!(e, "instruction 0: slot 7, and the module has 1");
        let e = program(&["jump 9", "return"]).unwrap();
        assert_eq!(
            e,
            "instruction 0: a jump to 9, and the module has 2 instructions"
        );
        let e = program(&["call 5 0", "return"]).unwrap();
        assert_eq!(
            e,
            "instruction 0: module 5, and the unit has no such module"
        );
        let e = program(&["pop", "return"]).unwrap();
        assert_eq!(
            e,
            "instruction 0: 1 operand(s) popped, and the stack holds 0"
        );
    }

    #[test]
    fn paths_agree_and_end() {
        let e = program(&["const (i 1)", "return"]).unwrap();
        assert_eq!(
            e,
            "instruction 1: a return that leaves 1 operand(s) on the stack"
        );
        // One path leaves an operand the other does not.
        let e = program(&["const (b true)", "jump.false 3", "const (i 1)", "return"]);
        assert_eq!(
            e.unwrap(),
            "instruction 2: instruction 3 is reached with 0 operand(s) and 0 \
             transaction(s) open, and with 1 and 0"
        );
        let e = program(&["stmt"]).unwrap();
        assert_eq!(e, "instruction 0: the module ends without a return");
        let e = program(&["commit", "return"]).unwrap();
        assert_eq!(e, "instruction 0: a commit, and no transaction is begun");
        let e = program(&["begin", "return"]).unwrap();
        assert_eq!(e, "instruction 1: a return that leaves a transaction open");
    }

    #[test]
    fn calls_and_references() {
        let e = program(&["call 1 0", "return"]).unwrap();
        assert_eq!(
            e,
            "instruction 0: a call with 0 argument(s) to \"S\", which takes 1"
        );
        let e = program(&["ref 0", "print 1", "return"]).unwrap();
        assert_eq!(e, "instruction 1: a reference where a value is expected");
        let e = refusal(&["return"], &["bind 1 0", "return"]).unwrap();
        assert!(e.ends_with("argument 1, and the module takes 1"), "{e}");
        // What no path leads to is checked all the same.
        let e = program(&["return", "load 7", "return"]).unwrap();
        assert_eq!(e, "instruction 1: slot 7, and the module has 1");
    }
}
