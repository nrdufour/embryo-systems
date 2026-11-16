//! The VM on A-code written as text, with no language behind it: a unit
//! run whole, in turns, past its budget, and as a client's session held
//! in a kernel's time. What a language's programs do is `tests/vm.rs`'s,
//! at the root.

use hbds_acode::{BUDGET_SPENT, Module, Unit};
use hbds_gtad::{Code, Store};
use hbds_rules::Uses;
use hbds_values::{Computed, Fault, Fz, Scalar, Value};
use hbds_vm::{Bound, Io, Sequencing, Session, Turn};

/// READ n; the sum of 1 to n; HOLD FOR 2; 10 / 0.
const SUM: &str = r#"acode 2
language "none"

module program "t" "T" 1:1
slot "n" integer
slot "s" integer
   0 5:9      stmt
   1 5:14     read value
   2 5:14     check.store 0
   3 5:14     store 0
   4 6:9      stmt
   5 6:14     const (i 0)
   6 6:9      check.store 1
   7 6:14     store 1
   8 7:9      stmt
   9 7:20     load 0
  10 7:24     const (i 0)
  11 7:20     op gt
  12 7:20     jump.false 26
  13 8:17     stmt
  14 8:22     load 1
  15 8:26     load 0
  16 8:22     op add
  17 8:17     check.store 1
  18 8:22     store 1
  19 9:17     stmt
  20 9:22     load 0
  21 9:26     const (i 1)
  22 9:22     op sub
  23 9:17     check.store 0
  24 9:22     store 0
  25 -        jump 9
  26 11:9     stmt
  27 11:15    const (s "sum")
  28 11:22    load 1
  29 11:9     print 2
  30 12:9     stmt
  31 12:9     running "HOLD"
  32 12:18    const (i 2)
  33 12:18    as.time
  34 12:9     hold
  35 13:9     stmt
  36 13:15    const (i 10)
  37 13:20    load 0
  38 13:15    op div
  39 13:9     print 1
  40 1:1      return
"#;

/// No language: lines read as integers, no function, no code to keep.
struct Bare;

impl hbds_acode::Language for Bare {
    fn name(&self) -> &str {
        "none"
    }

    fn native(&self, name: &str, _: &Fz) -> Computed<Fz> {
        Err(Fault(format!("no function {name}")))
    }

    fn read(&self, line: &str) -> Result<Value, String> {
        let n = line.trim().parse().map_err(|_| "not a number")?;
        Ok(Value::exact(Scalar::Integer(n)))
    }

    fn compile(&self, _: &str, _: &str, _: &str, _: &Unit) -> Result<Module, String> {
        Err("no language".into())
    }

    fn check(&self, _: &str, _: &str) -> Result<(), String> {
        Err("no language".into())
    }
}

impl hbds_rules::Compiler for Bare {
    fn uses(&self, _: &str, _: &Code) -> Result<Uses, String> {
        Err("no language".into())
    }
}

/// Lines to read, given when `ready`; what was written.
struct Lines {
    input: Vec<&'static str>,
    ready: bool,
    output: String,
}

fn lines(input: &[&'static str]) -> Lines {
    Lines {
        input: input.iter().rev().copied().collect(),
        ready: true,
        output: String::new(),
    }
}

impl Io for Lines {
    fn read_line(&mut self) -> Option<String> {
        self.input.pop().map(String::from)
    }

    fn write(&mut self, text: &str) {
        self.output += text;
    }

    fn ready(&mut self) -> bool {
        self.ready
    }
}

fn unit() -> Unit {
    Unit::read(SUM).unwrap()
}

#[test]
fn a_unit_runs_to_its_error_and_says_where() {
    let (mut store, mut io) = (Store::memory(), lines(&["4"]));
    let e = hbds_vm::run(&unit(), &mut store, &mut io, &Bare, None).unwrap_err();
    assert_eq!(io.output, "sum 10\n");
    assert_eq!(e.to_string(), "13:15: runtime error: division by zero");
}

#[test]
fn a_budget_ends_a_run_that_goes_on() {
    let (mut store, mut io) = (Store::memory(), lines(&["1000000"]));
    let e = hbds_vm::run(&unit(), &mut store, &mut io, &Bare, Some(100)).unwrap_err();
    assert!(e.message.ends_with(BUDGET_SPENT), "{e}");
    assert_eq!(io.output, "");
}

#[test]
fn a_session_runs_in_turns_and_waits_for_its_input() {
    let (mut store, mut sim, mut io) = (Store::memory(), Sequencing::new(), lines(&["4"]));
    let mut session = Session::new(unit(), None);
    io.ready = false;
    let turn = session.turn(&mut store, &mut sim, &mut io, &Bare, Some(5));
    assert!(matches!(turn, Turn::Input), "{turn:?}");
    io.ready = true;
    let mut turns = 0;
    let e = loop {
        match session.turn(&mut store, &mut sim, &mut io, &Bare, Some(5)) {
            Turn::Yield => turns += 1,
            Turn::Failed(e) => break e,
            turn => panic!("{turn:?}"),
        }
    };
    // A turn ends at a statement: ten of them, of 4 or 5 instructions.
    assert!((5..=12).contains(&turns), "{turns} turns");
    assert_eq!(io.output, "sum 10\n");
    assert_eq!(e.to_string(), "13:15: runtime error: division by zero");
}

#[test]
fn a_clients_hold_waits_for_the_kernel() {
    let (mut store, mut sim, mut io) = (Store::memory(), Sequencing::kernel(), lines(&["2"]));
    let mut client = Session::client(unit(), 7);
    let turn = client.turn(&mut store, &mut sim, &mut io, &Bare, None);
    assert!(matches!(turn, Turn::Held), "{turn:?}");
    assert_eq!((io.output.as_str(), sim.now()), ("sum 3\n", 0.0));
    assert_eq!(sim.next_due(), Some(2.0));
    // Its time is the kernel's to reach.
    let mut kernel = Session::kernel();
    kernel.load(unit());
    let bound = |until| Bound {
        until,
        events: None,
    };
    let turn = kernel.kernel_turn(&mut store, &mut sim, &mut io, &Bare, None, bound(1.0));
    assert!(matches!(turn, Turn::Idle), "{turn:?}");
    let turn = kernel.kernel_turn(&mut store, &mut sim, &mut io, &Bare, None, bound(5.0));
    assert!(matches!(turn, Turn::Due(7)), "{turn:?}");
    assert_eq!(sim.now(), 2.0);
    let turn = client.turn(&mut store, &mut sim, &mut io, &Bare, None);
    assert!(matches!(turn, Turn::Failed(_)), "{turn:?}");
}
