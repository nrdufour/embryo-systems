//! The simulation engine (project 17, pages 112 to 115 and 125):
//! processes are objects of classes included in the system hyperclass
//! PROCESS (figure 51), ordered in time by a sequencing set, SIMULA 67's,
//! and moved between their four states by the primitives of figure 52. The
//! engine keeps the time and the states; running a process's body, a
//! coroutine, is a language's. docs/hbds.md, "Simulation", gives the rules.

use std::collections::BTreeMap;

use hbds_core::value::{Scalar, Value};
use hbds_core::{AdtRef, Id, same};
use hbds_gtad::{ClassKind, Store, Stored};

/// The system hyperclass of process classes, and its attributes.
pub const PROCESS: &str = "PROCESS";
pub const STATE: &str = "STATE";
pub const CLOCK: &str = "CLOCK";

pub fn is_system(name: &str) -> bool {
    same(name, PROCESS)
}

/// Creates what is missing of PROCESS and its attributes.
pub fn install(store: &mut Store) -> Stored<()> {
    let process = match store.find_class(PROCESS) {
        Some(c) => c,
        None => store.create_class(PROCESS, ClassKind::Class)?,
    };
    for a in [STATE, CLOCK] {
        if store.find_attribute(process, a).is_none() {
            store.create_attribute(process, a)?;
        }
    }
    Ok(())
}

/// Whether an object is a process: of a class in PROCESS, at any depth.
pub fn is_process(store: &Store, o: Id) -> bool {
    let (Some(process), Some(object)) = (store.find_class(PROCESS), store.objects.get(&o)) else {
        return false;
    };
    store.lineage(object.class).contains(&process)
}

/// Who runs: the main program, itself a process (SIMULA's), or a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Agent {
    Main,
    Process(Id),
}

/// Figure 51's four states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Active,
    Hold,
    Passive,
    Terminated,
}

pub fn state_name(s: State) -> &'static str {
    match s {
        State::Active => "ACTIVE",
        State::Hold => "HOLD",
        State::Passive => "PASSIVE",
        State::Terminated => "TERMINATED",
    }
}

/// Where REACTIVATE puts a process.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum When {
    Now,
    At(f64),
    Before(Agent),
    After(Agent),
}

/// The time, the sequencing set, and each agent's state and clock.
pub struct Sequencing {
    now: f64,
    /// Due times, in order; first in, first out among equal times.
    set: Vec<(f64, Agent)>,
    states: BTreeMap<Agent, (State, f64)>,
    current: Agent,
}

impl Default for Sequencing {
    fn default() -> Self {
        Self::new()
    }
}

/// Names an agent in an error: the language's way of showing it.
pub type Describe<'a> = &'a dyn Fn(Agent) -> String;

impl Sequencing {
    /// Time 0, the main program running.
    pub fn new() -> Sequencing {
        Sequencing {
            now: 0.0,
            set: Vec::new(),
            states: BTreeMap::from([(Agent::Main, (State::Active, 0.0))]),
            current: Agent::Main,
        }
    }

    pub fn now(&self) -> f64 {
        self.now
    }

    pub fn current(&self) -> Agent {
        self.current
    }

    /// Whether the engine has seen this agent in this run.
    pub fn knows(&self, a: Agent) -> bool {
        self.states.contains_key(&a)
    }

    /// An agent's state; a process never activated is passive.
    pub fn state(&self, a: Agent) -> State {
        self.states.get(&a).map_or(State::Passive, |s| s.0)
    }

    /// When it is due while it holds, when it last ran otherwise.
    pub fn clock(&self, a: Agent) -> f64 {
        self.states.get(&a).map_or(0.0, |s| s.1)
    }

    /// The processes this run has seen, and their states and clocks.
    pub fn processes(&self) -> Vec<(Id, State, f64)> {
        (self.states.iter())
            .filter_map(|(a, (s, c))| match a {
                Agent::Process(id) => Some((*id, *s, *c)),
                Agent::Main => None,
            })
            .collect()
    }

    /// The agents in the set, in the order they are due.
    pub fn due(&self) -> Vec<(f64, Agent)> {
        self.set.clone()
    }

    fn set_state(&mut self, a: Agent, s: State, clock: f64) {
        self.states.insert(a, (s, clock));
    }

    /// Into the set at `t`, behind those already due then.
    fn insert(&mut self, a: Agent, t: f64) {
        let i = self.set.partition_point(|(u, _)| *u <= t);
        self.set.insert(i, (t, a));
    }

    fn remove(&mut self, a: Agent) {
        self.set.retain(|(_, b)| *b != a);
    }

    fn expect(&self, a: Agent, wanted: &[State], d: Describe) -> Result<(), String> {
        let s = self.state(a);
        if wanted.contains(&s) {
            return Ok(());
        }
        let wanted: Vec<&str> = wanted.iter().map(|w| state_name(*w)).collect();
        Err(format!(
            "{} is {}, not {}",
            d(a),
            state_name(s),
            wanted.join(" or ")
        ))
    }

    fn time(t: f64, what: &str) -> Result<f64, String> {
        if t.is_finite() {
            Ok(t)
        } else {
            Err(format!("{what} is not a time"))
        }
    }

    /// ACTIVATE and REACTIVATE: a passive process into the set.
    pub fn reactivate(&mut self, p: Agent, when: When, d: Describe) -> Result<(), String> {
        self.expect(p, &[State::Passive], d)?;
        let (t, i) = match when {
            When::Now => (self.now, None),
            When::At(t) => {
                let t = Self::time(t, "AT's")?;
                if t < self.now {
                    return Err(format!("{t} is past: the time is {}", self.now));
                }
                (t, None)
            }
            // Due when q is, just before or just after it.
            When::Before(q) | When::After(q) => {
                self.expect(q, &[State::Hold], d)?;
                let i = self.set.iter().position(|(_, b)| *b == q).expect("held");
                let after = matches!(when, When::After(_));
                (self.set[i].0, Some(if after { i + 1 } else { i }))
            }
        };
        match i {
            Some(i) => self.set.insert(i, (t, p)),
            None => self.insert(p, t),
        }
        self.set_state(p, State::Hold, t);
        Ok(())
    }

    /// CANCEL: out of the set, passive (page 125).
    pub fn cancel(&mut self, p: Agent, d: Describe) -> Result<(), String> {
        self.expect(p, &[State::Hold], d)?;
        self.remove(p);
        let clock = self.clock(p);
        self.set_state(p, State::Passive, clock);
        Ok(())
    }

    /// DELAY: due `dt` later, behind those already due then.
    pub fn delay(&mut self, p: Agent, dt: f64, d: Describe) -> Result<(), String> {
        self.expect(p, &[State::Hold], d)?;
        let t = Self::time(self.clock(p) + dt, "DELAY's")?;
        if t < self.now {
            return Err(format!("{} would be due at {t}, which is past", d(p)));
        }
        self.remove(p);
        self.insert(p, t);
        self.set_state(p, State::Hold, t);
        Ok(())
    }

    /// KILL: terminated, for good.
    pub fn kill(&mut self, p: Agent, d: Describe) -> Result<(), String> {
        self.expect(p, &[State::Hold, State::Passive], d)?;
        self.remove(p);
        let clock = self.clock(p);
        self.set_state(p, State::Terminated, clock);
        Ok(())
    }

    /// HOLD: the running agent due `dt` from now.
    pub fn hold(&mut self, dt: f64) -> Result<(), String> {
        let dt = Self::time(dt, "HOLD's duration")?;
        if dt < 0.0 {
            return Err(format!("HOLD for {dt}: a duration is not negative"));
        }
        let (a, t) = (self.current, self.now + dt);
        self.insert(a, t);
        self.set_state(a, State::Hold, t);
        Ok(())
    }

    /// PASSIVATE: the running process out of time, until reactivated.
    pub fn passivate(&mut self) -> Result<(), String> {
        if self.current == Agent::Main {
            return Err("the main program is not passivated: nothing could reactivate it".into());
        }
        let a = self.current;
        self.set_state(a, State::Passive, self.now);
        Ok(())
    }

    /// TERMINATE, or the end of its body: the running process, for good.
    pub fn terminate(&mut self) -> Result<(), String> {
        if self.current == Agent::Main {
            return Err("the main program is not terminated: it ends at its END".into());
        }
        let a = self.current;
        self.set_state(a, State::Terminated, self.now);
        Ok(())
    }

    /// The next agent due: out of the set, active, the time its.
    pub fn advance(&mut self) -> Option<Agent> {
        if self.set.is_empty() {
            return None;
        }
        let (t, a) = self.set.remove(0);
        self.now = t;
        self.current = a;
        self.set_state(a, State::Active, t);
        Some(a)
    }
}

/// STATE and CLOCK as the store keeps them, at the end of a run: a
/// process not terminated is passive, the sequencing set gone with it.
pub fn record(store: &mut Store, p: Id, state: State, clock: f64) -> Stored<()> {
    let class = store.objects[&p].class;
    let state = match state {
        State::Terminated => State::Terminated,
        _ => State::Passive,
    };
    let values = [
        (
            STATE,
            Value::exact(Scalar::String(state_name(state).into())),
        ),
        (CLOCK, Value::exact(Scalar::Float(clock))),
    ];
    for (name, v) in values {
        if let Some(a) = store.find_attribute(class, name) {
            store.set(p, a, v)?;
        }
    }
    Ok(())
}

/// A process's STATE or CLOCK as the store kept it, if it did.
pub fn stored(store: &Store, p: Id, name: &str) -> Option<Value> {
    let a = store.find_attribute(store.objects.get(&p)?.class, name)?;
    store.get(p, a).cloned()
}

/// Whether an attribute is PROCESS's STATE or CLOCK.
pub fn is_live(store: &Store, attribute: Id) -> bool {
    let Some(process) = store.find_class(PROCESS) else {
        return false;
    };
    let a = &store.attributes[&attribute];
    a.class == process && (same(&a.name, STATE) || same(&a.name, CLOCK))
}

/// What an a.d.t. reference is, if it is a process.
pub fn process_of(store: &Store, r: AdtRef) -> Option<Id> {
    match r {
        AdtRef::Object(o) if is_process(store, o) => Some(o),
        _ => None,
    }
}
