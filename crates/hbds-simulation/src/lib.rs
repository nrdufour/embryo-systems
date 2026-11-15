//! The simulation engine (project 17, pages 112 to 115 and 125):
//! processes are objects of classes included in the system hyperclass
//! PROCESS (figure 51), ordered in time by a sequencing set, SIMULA 67's,
//! and moved between their four states by the primitives of figure 52. The
//! engine keeps the time and the states; running a process's body, a
//! coroutine, is a language's. docs/hbds.md, "Simulation", gives the rules.

use std::collections::{BTreeMap, BTreeSet};

use hbds_core::terms::{Reader, Term, float, list, read_terms, word, write_term};
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
    let (Some(process), Some(object)) = (store.find_class(PROCESS), store.objects().get(&o)) else {
        return false;
    };
    store.lineage(object.class).contains(&process)
}

/// Who runs: the main program, itself a process (SIMULA's), or a process;
/// in the daemon, a client's main program once it holds (decision 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Agent {
    Main,
    Process(Id),
    Client(u64),
    /// A reflex launched with a delay, in time (project 18, page 126):
    /// the set knows which reflex, for which object.
    Reflex(u64),
}

impl Agent {
    /// A main program: standalone, or a client's.
    pub fn is_main(self) -> bool {
        matches!(self, Agent::Main | Agent::Client(_))
    }
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
#[derive(Clone)]
pub struct Sequencing {
    now: f64,
    /// Due times, in order; first in, first out among equal times.
    set: Vec<(f64, Agent)>,
    states: BTreeMap<Agent, (State, f64)>,
    current: Agent,
    /// Who activated each process first: in the daemon, the client it
    /// reports to.
    owners: BTreeMap<Agent, Agent>,
    /// The clients that have acted on time: each the active agent, as
    /// SIMULA's main program is, until it holds or ends.
    engaged: BTreeSet<Agent>,
    /// The reflexes launched with a delay: each activation's reflex and
    /// object, and the number the next one takes.
    reflexes: BTreeMap<u64, (Id, Id)>,
    next_reflex: u64,
    /// The set as it was before its first change after a snapshot of the
    /// store's, by the snapshot's number, in order: what it goes back to
    /// when the store does (decision 7). None outside a transaction.
    copies: Vec<(u64, Sequencing)>,
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
            owners: BTreeMap::new(),
            engaged: BTreeSet::new(),
            reflexes: BTreeMap::new(),
            next_reflex: 1,
            copies: Vec::new(),
        }
    }

    /// The daemon's: one per store, no main program of its own, the
    /// kernel driving it.
    pub fn kernel() -> Sequencing {
        Sequencing {
            now: 0.0,
            set: Vec::new(),
            states: BTreeMap::new(),
            current: Agent::Main,
            owners: BTreeMap::new(),
            engaged: BTreeSet::new(),
            reflexes: BTreeMap::new(),
            next_reflex: 1,
            copies: Vec::new(),
        }
    }

    /// Goes back where the store went: a statement refused, or a
    /// rollback, undoes what it did to the set with what it did to the
    /// model. Asked before the set is read or changed; costs nothing
    /// when the store undid nothing.
    pub fn follow(&mut self, store: &mut Store) {
        if let Some(stamp) = store.take_undone()
            && let Some(i) = self.copies.iter().position(|(t, _)| *t >= stamp)
        {
            let mut copies = std::mem::take(&mut self.copies);
            copies.truncate(i + 1);
            *self = copies.pop().expect("found").1;
            self.copies = copies;
        }
        if !self.copies.is_empty() {
            // A transaction ended takes its copies with it.
            let began = store.began().unwrap_or(u64::MAX);
            self.copies.retain(|(t, _)| *t >= began);
        }
    }

    /// Before a change to the set: inside a transaction, the set as it
    /// is now is kept, once for each snapshot of the store's, for
    /// [`follow`](Sequencing::follow) to go back to.
    pub fn changing(&mut self, store: &mut Store) {
        self.follow(store);
        let stamp = store.stamp();
        if store.began().is_some() && self.copies.last().is_none_or(|(t, _)| *t != stamp) {
            let copies = std::mem::take(&mut self.copies);
            let copy = self.clone();
            self.copies = copies;
            self.copies.push((stamp, copy));
        }
    }

    pub fn now(&self) -> f64 {
        self.now
    }

    /// Who runs from now on: in the daemon, whichever turn is running.
    pub fn run_as(&mut self, a: Agent) {
        self.current = a;
    }

    /// Whether a client has acted on time: the kernel waits while it runs.
    pub fn is_engaged(&self, a: Agent) -> bool {
        self.engaged.contains(&a)
    }

    /// The running agent acts on time: a client becomes part of it.
    fn engage(&mut self) {
        if let Agent::Client(_) = self.current {
            self.engaged.insert(self.current);
        }
    }

    /// A reflex launched with `delay`: due at now + delay, behind those
    /// due then, its activation's agent.
    pub fn launch(&mut self, reflex: Id, this: Id, delay: f64) -> Result<Agent, String> {
        let delay = Self::time(delay, "a reflex's DELAY")?;
        if delay < 0.0 {
            return Err(format!("a DELAY of {delay}: a delay is not negative"));
        }
        let a = Agent::Reflex(self.next_reflex);
        self.next_reflex += 1;
        self.reflexes.insert(self.next_reflex - 1, (reflex, this));
        let t = self.now + delay;
        self.insert(a, t);
        self.set_state(a, State::Hold, t);
        let owner = self.owner_of_current();
        self.owners.insert(a, owner);
        Ok(a)
    }

    /// An activation's reflex and object.
    pub fn reflex(&self, n: u64) -> Option<(Id, Id)> {
        self.reflexes.get(&n).copied()
    }

    /// A reflex's launches in time that are in one of `states`, in the
    /// order launched: what CANCEL, DELAY, REACTIVATE and KILL of a reflex
    /// act on (page 126).
    pub fn launches(&self, reflex: Id, states: &[State]) -> Vec<Agent> {
        (self.reflexes.iter())
            .filter(|(_, (r, _))| *r == reflex)
            .map(|(n, _)| Agent::Reflex(*n))
            .filter(|a| states.contains(&self.state(*a)))
            .collect()
    }

    /// A reflex's launch due first: what BEFORE and AFTER a reflex mean.
    pub fn first_due(&self, reflex: Id) -> Option<Agent> {
        (self.set.iter()).find_map(|(_, a)| match a {
            Agent::Reflex(n) if self.reflexes.get(n).is_some_and(|(r, _)| *r == reflex) => Some(*a),
            _ => None,
        })
    }

    /// Who what the running agent starts reports to: its own owner, if it
    /// has one, so that a chain reports to the client behind it.
    fn owner_of_current(&self) -> Agent {
        self.owners
            .get(&self.current)
            .copied()
            .unwrap_or(self.current)
    }

    /// Who activated `p` first.
    pub fn owner(&self, p: Agent) -> Option<Agent> {
        self.owners.get(&p).copied()
    }

    /// When the next agent is due, if one is.
    pub fn next_due(&self) -> Option<f64> {
        self.set.first().map(|(t, _)| *t)
    }

    /// An agent gone for good, a client that left: out of the set and
    /// forgotten.
    pub fn forget(&mut self, a: Agent) {
        self.remove(a);
        self.states.remove(&a);
        self.engaged.remove(&a);
        self.owners.remove(&a);
        if let Agent::Reflex(n) = a {
            self.reflexes.remove(&n);
        }
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
                Agent::Main | Agent::Client(_) | Agent::Reflex(_) => None,
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
        self.engage();
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
        let owner = self.owner_of_current();
        self.owners.entry(p).or_insert(owner);
        Ok(())
    }

    /// CANCEL: out of the set, passive (page 125).
    pub fn cancel(&mut self, p: Agent, d: Describe) -> Result<(), String> {
        self.engage();
        self.expect(p, &[State::Hold], d)?;
        self.remove(p);
        let clock = self.clock(p);
        self.set_state(p, State::Passive, clock);
        Ok(())
    }

    /// DELAY: due `dt` later, behind those already due then.
    pub fn delay(&mut self, p: Agent, dt: f64, d: Describe) -> Result<(), String> {
        self.engage();
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
        self.engage();
        self.expect(p, &[State::Hold, State::Passive], d)?;
        self.remove(p);
        let clock = self.clock(p);
        self.set_state(p, State::Terminated, clock);
        Ok(())
    }

    /// HOLD: the running agent due `dt` from now.
    pub fn hold(&mut self, dt: f64) -> Result<(), String> {
        self.engage();
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
        if self.current.is_main() {
            return Err("the main program is not passivated: nothing could reactivate it".into());
        }
        let a = self.current;
        self.set_state(a, State::Passive, self.now);
        Ok(())
    }

    /// TERMINATE, or the end of its body: the running process, for good.
    pub fn terminate(&mut self) -> Result<(), String> {
        if self.current.is_main() {
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

fn agent_term(a: Agent) -> Term {
    match a {
        Agent::Main => list(vec![word("main")]),
        Agent::Process(p) => list(vec![word("process"), word(p)]),
        Agent::Client(c) => list(vec![word("client"), word(c)]),
        Agent::Reflex(n) => list(vec![word("reflex"), word(n)]),
    }
}

fn read_agent(r: &mut Reader) -> Result<Agent, String> {
    let mut a = r.list()?;
    let agent = match a.word()? {
        "main" => Agent::Main,
        "process" => Agent::Process(a.id()?),
        "client" => Agent::Client(a.parse()?),
        "reflex" => Agent::Reflex(a.parse()?),
        w => return Err(format!("{w:?} is not an agent")),
    };
    a.end()?;
    Ok(agent)
}

const STATES: [(State, &str); 4] = [
    (State::Active, "active"),
    (State::Hold, "hold"),
    (State::Passive, "passive"),
    (State::Terminated, "terminated"),
];

impl Sequencing {
    /// The set as text, for the daemon's journal (docs/architecture.md,
    /// decision 5): the time, who runs, who is due when, every agent's
    /// state and clock, the owners and the clients in time.
    pub fn save(&self) -> String {
        let state = |s: State| word(STATES.iter().find(|(t, _)| *t == s).expect("a state").1);
        let terms = [
            float(self.now),
            agent_term(self.current),
            list(
                self.set
                    .iter()
                    .map(|(t, a)| list(vec![float(*t), agent_term(*a)]))
                    .collect(),
            ),
            list(
                (self.states.iter())
                    .map(|(a, (s, c))| list(vec![agent_term(*a), state(*s), float(*c)]))
                    .collect(),
            ),
            list(
                (self.owners.iter())
                    .map(|(p, o)| list(vec![agent_term(*p), agent_term(*o)]))
                    .collect(),
            ),
            list(self.engaged.iter().map(|a| agent_term(*a)).collect()),
            list(
                (self.reflexes.iter())
                    .map(|(n, (r, o))| list(vec![word(n), word(r), word(o)]))
                    .collect(),
            ),
            word(self.next_reflex),
        ];
        let mut out = String::new();
        for (i, t) in terms.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            write_term(t, &mut out);
        }
        out
    }

    /// The set `save` wrote, without the clients: they did not outlive
    /// the daemon that ran them.
    pub fn restore(text: &str) -> Result<Sequencing, String> {
        let terms = read_terms(text)?;
        let mut r = Reader::new(&terms);
        let now = r.parse::<f64>()?;
        let current = read_agent(&mut r)?;
        let mut set = Vec::new();
        let mut items = r.list()?;
        while items.more() {
            let mut i = items.list()?;
            set.push((i.parse::<f64>()?, read_agent(&mut i)?));
            i.end()?;
        }
        let mut states = BTreeMap::new();
        let mut items = r.list()?;
        while items.more() {
            let mut i = items.list()?;
            let a = read_agent(&mut i)?;
            let w = i.word()?;
            let s = STATES
                .iter()
                .find(|(_, n)| *n == w)
                .ok_or(format!("{w:?} is not a state"))?
                .0;
            states.insert(a, (s, i.parse::<f64>()?));
            i.end()?;
        }
        let mut owners = BTreeMap::new();
        let mut items = r.list()?;
        while items.more() {
            let mut i = items.list()?;
            owners.insert(read_agent(&mut i)?, read_agent(&mut i)?);
            i.end()?;
        }
        let mut engaged = BTreeSet::new();
        let mut items = r.list()?;
        while items.more() {
            engaged.insert(read_agent(&mut items)?);
        }
        // The reflexes in time, which a set saved before them lacks.
        let (mut reflexes, mut next_reflex) = (BTreeMap::new(), 1);
        if r.more() {
            let mut items = r.list()?;
            while items.more() {
                let mut i = items.list()?;
                reflexes.insert(i.parse()?, (i.id()?, i.id()?));
                i.end()?;
            }
            next_reflex = r.parse()?;
        }
        r.end()?;
        let client = |a: &Agent| matches!(a, Agent::Client(_));
        set.retain(|(_, a)| !client(a));
        states.retain(|a, _| !client(a));
        owners.retain(|_, o| !client(o));
        engaged.clear();
        Ok(Sequencing {
            now,
            set,
            states,
            current: if client(&current) {
                Agent::Main
            } else {
                current
            },
            owners,
            engaged,
            reflexes,
            next_reflex,
            copies: Vec::new(),
        })
    }
}

/// STATE and CLOCK as the store keeps them, at the end of a run: a
/// process not terminated is passive, the sequencing set gone with it.
pub fn record(store: &mut Store, p: Id, state: State, clock: f64) -> Stored<()> {
    let class = store.objects()[&p].class;
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
    let a = store.find_attribute(store.objects().get(&p)?.class, name)?;
    store.get(p, a).cloned()
}

/// Whether an attribute is PROCESS's STATE or CLOCK.
pub fn is_live(store: &Store, attribute: Id) -> bool {
    let Some(process) = store.find_class(PROCESS) else {
        return false;
    };
    let a = &store.attributes()[&attribute];
    a.class == process && (same(&a.name, STATE) || same(&a.name, CLOCK))
}

/// What an a.d.t. reference is, if it is a process.
pub fn process_of(store: &Store, r: AdtRef) -> Option<Id> {
    match r {
        AdtRef::Object(o) if is_process(store, o) => Some(o),
        _ => None,
    }
}
