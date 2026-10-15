//! GTAD, the a.d.t. engine (project 12, pages 45 to 48): classes, their
//! attributes and links, objects, their values and links, in memory.
//! Every mutation is one [`Change`], which a [`Recorder`] given to the
//! store records before it is applied, and which replaying applies again:
//! persistence is another engine's, behind that trait. docs/hbds.md gives
//! the rules.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub use hbds_core::spec::Spec;
use hbds_core::value::Value;
pub use hbds_core::{AdtRef, Id, fold, same};
pub use hbds_maps::PMap;

/// What makes a store durable: given every change before it is applied,
/// or a transaction's changes when it is kept.
pub trait Recorder: Send {
    fn record(&mut self, change: &Change) -> Result<(), String>;

    /// A kept transaction's changes, in order; written at once if the
    /// recorder can.
    fn record_all(&mut self, changes: &[Change]) -> Result<(), String> {
        changes.iter().try_for_each(|c| self.record(c))
    }
}

/// Properties of an a.d.t., set by a program: pages 156 to 158.
pub const PROPERTIES: &[(&str, u16)] = &[("COND", 156), ("ACTION", 157)];

pub fn is_property(key: &str) -> bool {
    PROPERTIES.iter().any(|(name, _)| *name == key)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Active,
    Hibernating,
    Destroyed,
}

/// What a class is: a class, or one of page 47's second extensions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassKind {
    Class,
    Embryo,
    /// Made of these embryos.
    Prototype(Vec<Id>),
    /// Built as this prototype.
    Structure(Id),
}

/// What an a.d.t. may carry: SPECIFY's specification and the properties
/// a program sets (COND, ACTION, pages 156 to 158).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Props {
    pub spec: Option<Spec>,
    pub unit: Option<String>,
    pub computed: bool,
    pub properties: BTreeMap<String, Code>,
}

/// Code an a.d.t. carries, and the language it is written in: the store
/// keeps it, a language runs it.
#[derive(Debug, Clone, PartialEq)]
pub struct Code {
    pub language: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Class {
    pub name: String,
    pub kind: ClassKind,
    pub state: State,
    /// The hyperclasses that include it.
    pub hyper: BTreeSet<Id>,
    pub props: Props,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub class: Id,
    pub name: String,
    pub state: State,
    pub props: Props,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    pub class: Id,
    pub name: String,
    pub state: State,
    pub values: BTreeMap<Id, Value>,
    pub props: Props,
}

/// One direction of a link between classes; the other is `reverse`, which
/// is itself for a reflexive link. An unnamed reverse has no relation.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassLink {
    pub from: Id,
    pub relation: Option<String>,
    pub to: Id,
    pub reverse: Id,
    pub state: State,
    pub props: Props,
}

/// One direction of a link between objects, realizing a class link.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectLink {
    pub class_link: Id,
    pub from: Id,
    pub to: Id,
    pub reverse: Id,
    pub probability: Option<f64>,
    pub state: State,
}

/// Every mutation of the store: what the journal records.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Class {
        id: Id,
        name: String,
        kind: ClassKind,
    },
    Object {
        id: Id,
        class: Id,
        name: String,
    },
    Attribute {
        id: Id,
        class: Id,
        name: String,
    },
    /// A link and its reverse; `reverse == id` for a reflexive link.
    ClassLink {
        id: Id,
        reverse: Id,
        from: Id,
        relation: String,
        to: Id,
        reverse_relation: Option<String>,
    },
    ObjectLink {
        id: Id,
        reverse: Id,
        class_link: Id,
        from: Id,
        to: Id,
        probability: Option<f64>,
    },
    /// Removes an object link and its reverse.
    Unlink {
        id: Id,
    },
    Set {
        object: Id,
        attribute: Id,
        value: Value,
    },
    Specify {
        target: AdtRef,
        spec: Option<Spec>,
        unit: Option<String>,
        computed: bool,
    },
    State {
        target: AdtRef,
        state: State,
    },
    Purge {
        target: AdtRef,
    },
    Include {
        class: Id,
        hyper: Id,
    },
    Exclude {
        class: Id,
        hyper: Id,
    },
    Assoc {
        a: AdtRef,
        b: AdtRef,
    },
    Dissoc {
        a: AdtRef,
        b: AdtRef,
    },
    Property {
        target: AdtRef,
        name: String,
        language: String,
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub type Stored<T> = Result<T, Error>;

pub fn error<T>(message: impl Into<String>) -> Stored<T> {
    Err(Error(message.into()))
}

#[derive(Default)]
pub struct Store {
    next: Id,
    pub classes: PMap<Id, Class>,
    pub attributes: PMap<Id, Attribute>,
    pub objects: PMap<Id, Object>,
    pub class_links: PMap<Id, ClassLink>,
    pub object_links: PMap<Id, ObjectLink>,
    pub associations: Vec<(AdtRef, AdtRef)>,
    recorder: Option<Box<dyn Recorder>>,
    transaction: Option<Transaction>,
    counted: Counted,
}

/// The work a store has done, counted for the benchmarks: changes made,
/// the scans its lookups ran and the items they walked, and the entries
/// its maps copied on write because a snapshot shared them. Neither
/// journalled nor rolled back.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Work {
    pub changes: u64,
    pub lookups: u64,
    pub scanned: u64,
    pub copied: u64,
}

#[derive(Default)]
struct Counted {
    changes: Cell<u64>,
    lookups: Cell<u64>,
    scanned: Cell<u64>,
}

fn add(c: &Cell<u64>, n: usize) {
    c.set(c.get() + n as u64);
}

/// The model as it was when a transaction began.
#[derive(Clone)]
struct Snapshot {
    next: Id,
    classes: PMap<Id, Class>,
    attributes: PMap<Id, Attribute>,
    objects: PMap<Id, Object>,
    class_links: PMap<Id, ClassLink>,
    object_links: PMap<Id, ObjectLink>,
    associations: Vec<(AdtRef, AdtRef)>,
}

/// A point inside a transaction to come back to: the model then, and how
/// many changes the transaction held.
pub struct Savepoint {
    snapshot: Snapshot,
    changes: usize,
}

/// Changes applied but not yet recorded: kept or refused together.
struct Transaction {
    depth: usize,
    snapshot: Snapshot,
    changes: Vec<Change>,
}

/// Results in order, without repeats.
pub fn push_new(out: &mut Vec<AdtRef>, r: AdtRef) {
    if !out.contains(&r) {
        out.push(r);
    }
}

impl Store {
    /// A store that lives as long as the run.
    pub fn memory() -> Store {
        Store {
            next: 1,
            ..Store::default()
        }
    }

    /// From now on, every change goes to `recorder` before it is applied.
    pub fn record_to(&mut self, recorder: Box<dyn Recorder>) {
        self.recorder = Some(recorder);
    }

    /// Applies a change recorded before: what opening a durable store does.
    pub fn replay(&mut self, change: Change) {
        self.apply(change);
    }

    fn fresh(&mut self) -> Id {
        let id = self.next;
        self.next += 1;
        id
    }

    /// The work done so far.
    pub fn work(&self) -> Work {
        let c = &self.counted;
        Work {
            changes: c.changes.get(),
            lookups: c.lookups.get(),
            scanned: c.scanned.get(),
            copied: self.classes.copied()
                + self.attributes.copied()
                + self.objects.copied()
                + self.class_links.copied()
                + self.object_links.copied(),
        }
    }

    /// Counts a scan that walked `n` items.
    fn scanned(&self, n: usize) {
        add(&self.counted.lookups, 1);
        add(&self.counted.scanned, n);
    }

    /// Records `change` in the journal, if there is one, then applies it.
    fn commit(&mut self, change: Change) -> Stored<()> {
        add(&self.counted.changes, 1);
        if let Some(t) = &mut self.transaction {
            t.changes.push(change.clone());
        } else if let Some(recorder) = &mut self.recorder {
            recorder.record(&change).map_err(Error)?;
        }
        self.apply(change);
        Ok(())
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            next: self.next,
            classes: self.classes.clone(),
            attributes: self.attributes.clone(),
            objects: self.objects.clone(),
            class_links: self.class_links.clone(),
            object_links: self.object_links.clone(),
            associations: self.associations.clone(),
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.next = s.next;
        self.classes.restore(s.classes);
        self.attributes.restore(s.attributes);
        self.objects.restore(s.objects);
        self.class_links.restore(s.class_links);
        self.object_links.restore(s.object_links);
        self.associations = s.associations;
    }

    /// Begins a transaction: the changes that follow apply at once, but
    /// are recorded only when [`end`](Store::end) keeps them. Inside one,
    /// `begin` nests, and only the outermost `end` decides.
    pub fn begin(&mut self) {
        match &mut self.transaction {
            Some(t) => t.depth += 1,
            None => {
                self.transaction = Some(Transaction {
                    depth: 1,
                    snapshot: self.snapshot(),
                    changes: Vec::new(),
                });
            }
        }
    }

    /// Ends a transaction: keeps its changes, recording them, or refuses
    /// them, the model back as it was at `begin`. A recorder that fails
    /// refuses them too.
    pub fn end(&mut self, keep: bool) -> Stored<()> {
        let t = self.transaction.as_mut().expect("end without begin");
        if t.depth > 1 {
            t.depth -= 1;
            return Ok(());
        }
        let t = self.transaction.take().expect("a transaction");
        if !keep {
            self.restore(t.snapshot);
            return Ok(());
        }
        if let Some(recorder) = &mut self.recorder
            && let Err(e) = recorder.record_all(&t.changes)
        {
            self.restore(t.snapshot);
            return Err(Error(e));
        }
        Ok(())
    }

    /// The open transaction's changes so far, in order; none outside one.
    pub fn pending(&self) -> &[Change] {
        self.transaction.as_ref().map_or(&[], |t| &t.changes)
    }

    /// A point of the open transaction to [`rollback`](Store::rollback)
    /// to; `None` outside one.
    pub fn savepoint(&self) -> Option<Savepoint> {
        let t = self.transaction.as_ref()?;
        Some(Savepoint {
            snapshot: self.snapshot(),
            changes: t.changes.len(),
        })
    }

    /// Undoes what the open transaction did since `point`.
    pub fn rollback(&mut self, point: Savepoint) {
        self.restore(point.snapshot);
        if let Some(t) = &mut self.transaction {
            t.changes.truncate(point.changes);
        }
    }

    /// Applies a change already checked: the one path for every mutation,
    /// live or replayed.
    fn apply(&mut self, change: Change) {
        let mut seen = |id: Id| self.next = self.next.max(id + 1);
        match &change {
            Change::Class { id, .. } | Change::Object { id, .. } | Change::Attribute { id, .. } => {
                seen(*id)
            }
            Change::ClassLink { id, reverse, .. } | Change::ObjectLink { id, reverse, .. } => {
                seen(*id);
                seen(*reverse);
            }
            _ => {}
        }
        match change {
            Change::Class { id, name, kind } => {
                self.classes.insert(
                    id,
                    Class {
                        name,
                        kind,
                        state: State::Active,
                        hyper: BTreeSet::new(),
                        props: Props::default(),
                    },
                );
            }
            Change::Object { id, class, name } => {
                self.objects.insert(
                    id,
                    Object {
                        class,
                        name,
                        state: State::Active,
                        values: BTreeMap::new(),
                        props: Props::default(),
                    },
                );
            }
            Change::Attribute { id, class, name } => {
                self.attributes.insert(
                    id,
                    Attribute {
                        class,
                        name,
                        state: State::Active,
                        props: Props::default(),
                    },
                );
            }
            Change::ClassLink {
                id,
                reverse,
                from,
                relation,
                to,
                reverse_relation,
            } => {
                let link = |from, relation, to, reverse| ClassLink {
                    from,
                    relation,
                    to,
                    reverse,
                    state: State::Active,
                    props: Props::default(),
                };
                self.class_links
                    .insert(id, link(from, Some(relation), to, reverse));
                if reverse != id {
                    self.class_links
                        .insert(reverse, link(to, reverse_relation, from, id));
                }
            }
            Change::ObjectLink {
                id,
                reverse,
                class_link,
                from,
                to,
                probability,
            } => {
                let back = self
                    .class_links
                    .get(&class_link)
                    .map_or(class_link, |l| l.reverse);
                let link = |class_link, from, to, reverse| ObjectLink {
                    class_link,
                    from,
                    to,
                    reverse,
                    probability,
                    state: State::Active,
                };
                self.object_links
                    .insert(id, link(class_link, from, to, reverse));
                self.object_links.insert(reverse, link(back, to, from, id));
            }
            Change::Unlink { id } => {
                if let Some(l) = self.object_links.remove(&id) {
                    self.object_links.remove(&l.reverse);
                }
            }
            Change::Set {
                object,
                attribute,
                value,
            } => {
                if let Some(o) = self.objects.get_mut(&object) {
                    o.values.insert(attribute, value);
                }
            }
            Change::Specify {
                target,
                spec,
                unit,
                computed,
            } => {
                if let Some(p) = self.props_mut(target) {
                    p.spec = spec;
                    p.unit = unit;
                    p.computed = computed;
                }
            }
            Change::State { target, state } => self.set_state(target, state),
            Change::Purge { target } => self.purge(target),
            Change::Include { class, hyper } => {
                if let Some(c) = self.classes.get_mut(&class) {
                    c.hyper.insert(hyper);
                }
            }
            Change::Exclude { class, hyper } => {
                if let Some(c) = self.classes.get_mut(&class) {
                    c.hyper.remove(&hyper);
                }
            }
            Change::Assoc { a, b } => {
                if !self.associations.contains(&(a, b)) {
                    self.associations.push((a, b));
                }
            }
            Change::Dissoc { a, b } => self.associations.retain(|p| *p != (a, b)),
            Change::Property {
                target,
                name,
                language,
                text,
            } => {
                if let Some(p) = self.props_mut(target) {
                    p.properties.insert(name, Code { language, text });
                }
            }
        }
    }

    /// What an a.d.t. carries: its specification, its COND and ACTION.
    pub fn props(&self, r: AdtRef) -> Option<&Props> {
        match r {
            AdtRef::Class(id) => self.classes.get(&id).map(|c| &c.props),
            AdtRef::Object(id) => self.objects.get(&id).map(|o| &o.props),
            AdtRef::ClassAttribute(id) | AdtRef::ObjectAttribute(_, id) => {
                self.attributes.get(&id).map(|a| &a.props)
            }
            AdtRef::ClassLink(id) => self.class_links.get(&id).map(|l| &l.props),
            AdtRef::ObjectLink(_) => None,
        }
    }

    fn props_mut(&mut self, r: AdtRef) -> Option<&mut Props> {
        match r {
            AdtRef::Class(id) => self.classes.get_mut(&id).map(|c| &mut c.props),
            AdtRef::Object(id) => self.objects.get_mut(&id).map(|o| &mut o.props),
            AdtRef::ClassAttribute(id) | AdtRef::ObjectAttribute(_, id) => {
                self.attributes.get_mut(&id).map(|a| &mut a.props)
            }
            AdtRef::ClassLink(id) => self.class_links.get_mut(&id).map(|l| &mut l.props),
            AdtRef::ObjectLink(_) => None,
        }
    }

    fn set_state(&mut self, r: AdtRef, state: State) {
        match r {
            AdtRef::Class(id) => {
                if let Some(c) = self.classes.get_mut(&id) {
                    c.state = state;
                }
            }
            AdtRef::Object(id) => {
                if let Some(o) = self.objects.get_mut(&id) {
                    o.state = state;
                }
            }
            AdtRef::ClassAttribute(id) | AdtRef::ObjectAttribute(_, id) => {
                if let Some(a) = self.attributes.get_mut(&id) {
                    a.state = state;
                }
            }
            AdtRef::ClassLink(id) => {
                if let Some(l) = self.class_links.get_mut(&id) {
                    l.state = state;
                    let reverse = l.reverse;
                    if let Some(r) = self.class_links.get_mut(&reverse) {
                        r.state = state;
                    }
                }
            }
            AdtRef::ObjectLink(id) => {
                if let Some(l) = self.object_links.get_mut(&id) {
                    l.state = state;
                    let reverse = l.reverse;
                    if let Some(r) = self.object_links.get_mut(&reverse) {
                        r.state = state;
                    }
                }
            }
        }
    }

    fn purge(&mut self, r: AdtRef) {
        match r {
            AdtRef::Class(id) => {
                self.classes.remove(&id);
                let objects: Vec<Id> = (self.objects.iter())
                    .filter(|(_, o)| o.class == id)
                    .map(|(i, _)| *i)
                    .collect();
                for o in objects {
                    self.purge(AdtRef::Object(o));
                }
                self.attributes.retain(|_, a| a.class != id);
                let links: Vec<Id> = (self.class_links.iter())
                    .filter(|(_, l)| l.from == id || l.to == id)
                    .map(|(i, _)| *i)
                    .collect();
                for l in links {
                    self.purge(AdtRef::ClassLink(l));
                }
            }
            AdtRef::Object(id) => {
                self.objects.remove(&id);
                self.object_links.retain(|_, l| l.from != id && l.to != id);
            }
            AdtRef::ClassAttribute(id) | AdtRef::ObjectAttribute(_, id) => {
                self.attributes.remove(&id);
                let holding: Vec<Id> = (self.objects.iter())
                    .filter(|(_, o)| o.values.contains_key(&id))
                    .map(|(i, _)| *i)
                    .collect();
                for o in holding {
                    self.objects.get_mut(&o).unwrap().values.remove(&id);
                }
            }
            AdtRef::ClassLink(id) => {
                if let Some(l) = self.class_links.remove(&id) {
                    self.class_links.remove(&l.reverse);
                    let reverse = l.reverse;
                    self.object_links
                        .retain(|_, o| o.class_link != id && o.class_link != reverse);
                }
            }
            AdtRef::ObjectLink(id) => self.apply(Change::Unlink { id }),
        }
        self.associations
            .retain(|(a, b)| !involves(*a, r) && !involves(*b, r));
    }

    // Looking up

    /// An a.d.t.'s own state: what HIBERN, AWAKE, DESTROY, RESUR and PURGE
    /// change.
    pub fn state(&self, r: AdtRef) -> Option<State> {
        Some(match r {
            AdtRef::Class(id) => self.classes.get(&id)?.state,
            AdtRef::Object(id) => self.objects.get(&id)?.state,
            AdtRef::ClassAttribute(id) | AdtRef::ObjectAttribute(_, id) => {
                self.attributes.get(&id)?.state
            }
            AdtRef::ClassLink(id) => self.class_links.get(&id)?.state,
            AdtRef::ObjectLink(id) => self.object_links.get(&id)?.state,
        })
    }

    /// Whether an a.d.t. is seen: it is active, and so is what it depends
    /// on (an object's class, a link's two ends, an attribute's carrier).
    pub fn visible(&self, r: AdtRef) -> bool {
        let on = |r| self.state(r) == Some(State::Active);
        match r {
            AdtRef::Class(_) => on(r),
            AdtRef::Object(id) => on(r) && on(AdtRef::Class(self.objects[&id].class)),
            AdtRef::ClassAttribute(id) => on(r) && on(AdtRef::Class(self.attributes[&id].class)),
            AdtRef::ObjectAttribute(o, a) => {
                self.visible(AdtRef::Object(o)) && self.visible(AdtRef::ClassAttribute(a))
            }
            AdtRef::ClassLink(id) => {
                let l = &self.class_links[&id];
                on(r) && on(AdtRef::Class(l.from)) && on(AdtRef::Class(l.to))
            }
            AdtRef::ObjectLink(id) => {
                let l = &self.object_links[&id];
                on(r) && self.visible(AdtRef::Object(l.from)) && self.visible(AdtRef::Object(l.to))
            }
        }
    }

    /// A class by name, whatever its state.
    pub fn find_class(&self, name: &str) -> Option<Id> {
        let mut n = 0;
        let found = (self.classes.iter())
            .inspect(|_| n += 1)
            .find(|(_, c)| same(&c.name, name))
            .map(|(i, _)| *i);
        self.scanned(n);
        found
    }

    /// An active class by name.
    pub fn class(&self, name: &str) -> Stored<Id> {
        match self.find_class(name) {
            Some(id) if self.visible(AdtRef::Class(id)) => Ok(id),
            Some(_) => error(format!("the class \"{name}\" is not active")),
            None => error(format!("there is no class \"{name}\"")),
        }
    }

    pub fn find_object(&self, class: Id, name: &str) -> Option<Id> {
        let mut n = 0;
        let found = (self.objects.iter())
            .inspect(|_| n += 1)
            .find(|(_, o)| o.class == class && same(&o.name, name))
            .map(|(i, _)| *i);
        self.scanned(n);
        found
    }

    pub fn object(&self, class: Id, name: &str) -> Stored<Id> {
        match self.find_object(class, name) {
            Some(id) if self.visible(AdtRef::Object(id)) => Ok(id),
            Some(_) => error(format!("the object \"{name}\" is not active")),
            None => error(format!(
                "there is no object \"{name}\" in \"{}\"",
                self.classes[&class].name
            )),
        }
    }

    /// A class, then the hyperclasses that include it, and theirs, each
    /// once: what it inherits from (docs/hbds.md, "Inheritance").
    pub fn lineage(&self, class: Id) -> Vec<Id> {
        let mut out = vec![class];
        let mut i = 0;
        while i < out.len() {
            if let Some(c) = self.classes.get(&out[i]) {
                for h in &c.hyper {
                    if !out.contains(h) {
                        out.push(*h);
                    }
                }
            }
            i += 1;
        }
        out
    }

    /// A class's attribute, its own or inherited from its hyperclasses.
    pub fn find_attribute(&self, class: Id, name: &str) -> Option<Id> {
        let lineage = self.lineage(class);
        lineage.iter().find_map(|c| self.own_attribute(*c, name))
    }

    fn own_attribute(&self, class: Id, name: &str) -> Option<Id> {
        let mut n = 0;
        let found = (self.attributes.iter())
            .inspect(|_| n += 1)
            .find(|(_, a)| a.class == class && same(&a.name, name))
            .map(|(i, _)| *i);
        self.scanned(n);
        found
    }

    pub fn attribute(&self, class: Id, name: &str) -> Stored<Id> {
        match self.find_attribute(class, name) {
            Some(id) if self.visible(AdtRef::ClassAttribute(id)) => Ok(id),
            _ => error(format!(
                "the class \"{}\" has no attribute \"{name}\"",
                self.classes[&class].name
            )),
        }
    }

    /// A link between two classes, their own or inherited from their
    /// hyperclasses.
    pub fn find_class_link(&self, from: Id, relation: &str, to: Id) -> Option<Id> {
        let (froms, tos) = (self.lineage(from), self.lineage(to));
        (froms.iter())
            .flat_map(|a| tos.iter().map(move |b| (*a, *b)))
            .find_map(|(a, b)| self.own_class_link(a, relation, b))
    }

    fn own_class_link(&self, from: Id, relation: &str, to: Id) -> Option<Id> {
        let mut n = 0;
        let found = (self.class_links.iter())
            .inspect(|_| n += 1)
            .find(|(_, l)| {
                l.from == from
                    && l.to == to
                    && l.relation.as_deref().is_some_and(|r| same(r, relation))
            })
            .map(|(i, _)| *i);
        self.scanned(n);
        found
    }

    pub fn class_link(&self, from: Id, relation: &str, to: Id) -> Stored<Id> {
        match self.find_class_link(from, relation, to) {
            Some(id) if self.visible(AdtRef::ClassLink(id)) => Ok(id),
            _ => error(format!(
                "there is no link \"{}\" \"{relation}\" \"{}\"",
                self.classes[&from].name, self.classes[&to].name
            )),
        }
    }

    /// The link from `from` to `to` realizing `relation`, if any.
    pub fn object_link(&self, from: Id, relation: &str, to: Id) -> Option<Id> {
        let (Some(a), Some(b)) = (self.objects.get(&from), self.objects.get(&to)) else {
            return None;
        };
        let class_link = self.find_class_link(a.class, relation, b.class)?;
        let mut n = 0;
        let found = (self.object_links.iter())
            .inspect(|_| n += 1)
            .find(|(_, l)| l.class_link == class_link && l.from == from && l.to == to)
            .filter(|(i, _)| self.visible(AdtRef::ObjectLink(**i)))
            .map(|(i, _)| *i);
        self.scanned(n);
        found
    }

    pub fn get(&self, object: Id, attribute: Id) -> Option<&Value> {
        self.objects.get(&object)?.values.get(&attribute)
    }

    // Changing

    pub fn create_class(&mut self, name: &str, kind: ClassKind) -> Stored<Id> {
        if self.find_class(name).is_some() {
            return error(format!("the class \"{name}\" exists"));
        }
        let id = self.fresh();
        let name = name.to_string();
        self.commit(Change::Class { id, name, kind })?;
        Ok(id)
    }

    pub fn create_object(&mut self, class: Id, name: &str) -> Stored<Id> {
        if self.find_object(class, name).is_some() {
            return error(format!(
                "the object \"{name}\" of \"{}\" exists",
                self.classes[&class].name
            ));
        }
        let id = self.fresh();
        let name = name.to_string();
        self.commit(Change::Object { id, class, name })?;
        Ok(id)
    }

    pub fn create_attribute(&mut self, class: Id, name: &str) -> Stored<Id> {
        if self.own_attribute(class, name).is_some() {
            return error(format!(
                "the attribute \"{}\" . \"{name}\" exists",
                self.classes[&class].name
            ));
        }
        let id = self.fresh();
        let name = name.to_string();
        self.commit(Change::Attribute { id, class, name })?;
        Ok(id)
    }

    /// A link between classes and its reverse, named or not; a reflexive
    /// link is its own reverse (page 74).
    pub fn create_class_link(
        &mut self,
        from: Id,
        relation: &str,
        to: Id,
        reverse: Option<&str>,
        reflexive: bool,
    ) -> Stored<Id> {
        if self.own_class_link(from, relation, to).is_some() {
            return error(format!("the link \"{relation}\" exists"));
        }
        if reflexive && from != to {
            return error("a REFLEXIVE link joins a class to itself");
        }
        let id = self.fresh();
        let back = if reflexive { id } else { self.fresh() };
        self.commit(Change::ClassLink {
            id,
            reverse: back,
            from,
            relation: relation.to_string(),
            to,
            reverse_relation: reverse.map(String::from),
        })?;
        Ok(id)
    }

    /// ASSERT: links two objects by a link their classes have.
    pub fn assert_link(
        &mut self,
        from: Id,
        relation: &str,
        to: Id,
        probability: Option<f64>,
    ) -> Stored<Id> {
        if let Some(id) = self.object_link(from, relation, to) {
            return Ok(id);
        }
        let (a, b) = (self.objects[&from].class, self.objects[&to].class);
        let class_link = self.class_link(a, relation, b)?;
        let id = self.fresh();
        let reverse = self.fresh();
        self.commit(Change::ObjectLink {
            id,
            reverse,
            class_link,
            from,
            to,
            probability,
        })?;
        Ok(id)
    }

    pub fn retract_link(&mut self, from: Id, relation: &str, to: Id) -> Stored<()> {
        match self.object_link(from, relation, to) {
            Some(id) => self.commit(Change::Unlink { id }),
            None => Ok(()),
        }
    }

    /// Sets a value; setting the one it has changes nothing.
    pub fn set(&mut self, object: Id, attribute: Id, value: Value) -> Stored<()> {
        if self.get(object, attribute) == Some(&value) {
            return Ok(());
        }
        self.commit(Change::Set {
            object,
            attribute,
            value,
        })
    }

    pub fn specify(
        &mut self,
        target: AdtRef,
        spec: Option<Spec>,
        unit: Option<String>,
        computed: bool,
    ) -> Stored<()> {
        if matches!(target, AdtRef::ObjectLink(_)) {
            return error("an object link takes no specification");
        }
        self.commit(Change::Specify {
            target,
            spec,
            unit,
            computed,
        })
    }

    /// HIBERN, AWAKE, DESTROY, RESUR: each from the state it expects.
    pub fn change_state(&mut self, target: AdtRef, from: State, to: State) -> Stored<()> {
        match self.state(target) {
            Some(s) if s == from => self.commit(Change::State { target, state: to }),
            Some(s) => error(format!(
                "{} is {}, not {}",
                self.describe(target),
                state_name(s),
                state_name(from)
            )),
            None => error("no such a.d.t."),
        }
    }

    /// PURGE: removes a destroyed a.d.t., and what depends on it.
    pub fn purge_adt(&mut self, target: AdtRef) -> Stored<()> {
        match self.state(target) {
            Some(State::Destroyed) => self.commit(Change::Purge { target }),
            Some(s) => error(format!(
                "{} is {}: only a destroyed a.d.t. is purged",
                self.describe(target),
                state_name(s)
            )),
            None => error("no such a.d.t."),
        }
    }

    pub fn include(&mut self, class: Id, hyper: Id) -> Stored<()> {
        if class == hyper {
            return error("a class does not include itself");
        }
        self.commit(Change::Include { class, hyper })
    }

    pub fn exclude(&mut self, class: Id, hyper: Id) -> Stored<()> {
        self.commit(Change::Exclude { class, hyper })
    }

    pub fn assoc(&mut self, a: AdtRef, b: AdtRef) -> Stored<()> {
        self.commit(Change::Assoc { a, b })
    }

    pub fn dissoc(&mut self, a: AdtRef, b: AdtRef) -> Stored<()> {
        self.commit(Change::Dissoc { a, b })
    }

    /// Code for an a.d.t. to carry, in `language`.
    pub fn set_property(
        &mut self,
        target: AdtRef,
        name: &str,
        language: &str,
        text: &str,
    ) -> Stored<()> {
        if matches!(target, AdtRef::ObjectLink(_)) {
            return error("an object link carries no property; its class link does");
        }
        self.commit(Change::Property {
            target,
            name: name.to_string(),
            language: language.to_string(),
            text: text.to_string(),
        })
    }

    // Showing

    /// An a.d.t. as PRINT shows it: `TOWN`, `Paris[TOWN]`, `TOWN.AREA`,
    /// `RIVER "crosses" TOWN`.
    pub fn describe(&self, r: AdtRef) -> String {
        let class = |id: Id| {
            self.classes
                .get(&id)
                .map_or_else(|| format!("#{id}"), |c| c.name.clone())
        };
        let object = |id: Id| match self.objects.get(&id) {
            Some(o) => format!("{}[{}]", o.name, class(o.class)),
            None => format!("#{id}"),
        };
        let attribute = |id: Id| {
            self.attributes
                .get(&id)
                .map_or_else(|| format!("#{id}"), |a| a.name.clone())
        };
        let relation = |l: &ClassLink| match &l.relation {
            Some(r) => format!("\"{r}\""),
            None => "(unnamed)".into(),
        };
        match r {
            AdtRef::Class(id) => class(id),
            AdtRef::Object(id) => object(id),
            AdtRef::ClassAttribute(id) => match self.attributes.get(&id) {
                Some(a) => format!("{}.{}", class(a.class), a.name),
                None => format!("#{id}"),
            },
            AdtRef::ObjectAttribute(o, a) => format!("{}.{}", object(o), attribute(a)),
            AdtRef::ClassLink(id) => match self.class_links.get(&id) {
                Some(l) => format!("{} {} {}", class(l.from), relation(l), class(l.to)),
                None => format!("#{id}"),
            },
            AdtRef::ObjectLink(id) => match self.object_links.get(&id) {
                Some(l) => {
                    let r = self
                        .class_links
                        .get(&l.class_link)
                        .map_or("?".into(), relation);
                    format!("{} {r} {}", object(l.from), object(l.to))
                }
                None => format!("#{id}"),
            },
        }
    }

    /// Everything the store holds, in a stable order: what `embryo-systems
    /// store DIR` prints.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        let state = |s: State| match s {
            State::Active => String::new(),
            s => format!(" ({})", state_name(s)),
        };
        let props = |p: &Props| {
            let mut t = String::new();
            if let Some(s) = &p.spec {
                t += &format!(" : {s}");
            }
            if let Some(u) = &p.unit {
                t += &format!(" UNIT ({u})");
            }
            if p.computed {
                t += " COMPUTED";
            }
            for (k, code) in &p.properties {
                t += &format!(" {k} ({}) = \"{}\"", code.language, code.text);
            }
            t
        };
        for (id, c) in &self.classes {
            let kind = match &c.kind {
                ClassKind::Class => String::new(),
                ClassKind::Embryo => " embryo".into(),
                ClassKind::Prototype(e) => format!(
                    " prototype of {}",
                    e.iter()
                        .map(|e| self.describe(AdtRef::Class(*e)))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                ClassKind::Structure(p) => {
                    format!(" structure as {}", self.describe(AdtRef::Class(*p)))
                }
            };
            out += &format!(
                "class {}{kind}{}{}\n",
                c.name,
                state(c.state),
                props(&c.props)
            );
            for h in &c.hyper {
                out += &format!("  in {}\n", self.describe(AdtRef::Class(*h)));
            }
            for (aid, a) in self.attributes.iter().filter(|(_, a)| a.class == *id) {
                let _ = aid;
                out += &format!(
                    "  attribute {}{}{}\n",
                    a.name,
                    state(a.state),
                    props(&a.props)
                );
            }
            for (lid, l) in self.class_links.iter().filter(|(_, l)| l.from == *id) {
                out += &format!(
                    "  link {}{}{}\n",
                    self.describe(AdtRef::ClassLink(*lid)),
                    state(l.state),
                    props(&l.props)
                );
            }
            for (oid, o) in self.objects.iter().filter(|(_, o)| o.class == *id) {
                out += &format!("  object {}{}{}\n", o.name, state(o.state), props(&o.props));
                for (a, v) in &o.values {
                    let name = self.attributes.get(a).map_or("?", |a| a.name.as_str());
                    out += &format!("    {name} = {}\n", self.show(v));
                }
                for (lid, l) in self.object_links.iter().filter(|(_, l)| l.from == *oid) {
                    let p = l.probability.map_or(String::new(), |p| format!(" % {p}"));
                    out += &format!(
                        "    link {}{p}{}\n",
                        self.describe(AdtRef::ObjectLink(*lid)),
                        state(l.state)
                    );
                }
            }
        }
        for (a, b) in &self.associations {
            out += &format!("assoc {} WITH {}\n", self.describe(*a), self.describe(*b));
        }
        out
    }

    /// A value, a.d.t. by their names.
    pub fn show(&self, v: &Value) -> String {
        match v {
            Value::Adt(r) => self.describe(*r),
            Value::List(l) => format!(
                "[{}]",
                l.items
                    .iter()
                    .map(|v| self.show(v))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            v => v.to_string(),
        }
    }
}

fn involves(a: AdtRef, r: AdtRef) -> bool {
    a == r
        || match (a, r) {
            (AdtRef::ObjectAttribute(o, _), AdtRef::Object(x)) => o == x,
            (AdtRef::ObjectAttribute(_, at), AdtRef::ClassAttribute(x)) => at == x,
            _ => false,
        }
}

pub fn state_name(s: State) -> &'static str {
    match s {
        State::Active => "active",
        State::Hibernating => "hibernating",
        State::Destroyed => "destroyed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use AdtRef::*;
    use hbds_core::value::Value;

    /// TOWN, RIVER, DAM; RIVER "crosses" TOWN, reversed "crossed by";
    /// DAM "feeds" RIVER; TOWN "neighbor" TOWN, reflexive; objects.
    fn world() -> (Store, BTreeMap<&'static str, Id>) {
        let mut s = Store::memory();
        let mut ids = BTreeMap::new();
        for c in ["TOWN", "RIVER", "DAM", "HYDRO"] {
            ids.insert(c, s.create_class(c, ClassKind::Class).unwrap());
        }
        ids.insert("AREA", s.create_attribute(ids["TOWN"], "AREA").unwrap());
        ids.insert("VOL", s.create_attribute(ids["DAM"], "VOL").unwrap());
        ids.insert(
            "crosses",
            s.create_class_link(
                ids["RIVER"],
                "crosses",
                ids["TOWN"],
                Some("crossed by"),
                false,
            )
            .unwrap(),
        );
        s.create_class_link(ids["DAM"], "feeds", ids["RIVER"], None, false)
            .unwrap();
        ids.insert(
            "neighbor",
            s.create_class_link(ids["TOWN"], "neighbor", ids["TOWN"], None, true)
                .unwrap(),
        );
        for (o, c) in [
            ("Lyon", "TOWN"),
            ("Arles", "TOWN"),
            ("Rhone", "RIVER"),
            ("Vouglans", "DAM"),
        ] {
            ids.insert(o, s.create_object(ids[c], o).unwrap());
        }
        s.assert_link(ids["Rhone"], "crosses", ids["Lyon"], None)
            .unwrap();
        s.assert_link(ids["Rhone"], "crosses", ids["Arles"], Some(0.9))
            .unwrap();
        s.assert_link(ids["Vouglans"], "feeds", ids["Rhone"], None)
            .unwrap();
        s.assert_link(ids["Lyon"], "neighbor", ids["Arles"], None)
            .unwrap();
        s.set(ids["Vouglans"], ids["VOL"], Value::integer(605))
            .unwrap();
        s.include(ids["TOWN"], ids["HYDRO"]).unwrap();
        s.include(ids["RIVER"], ids["HYDRO"]).unwrap();
        (s, ids)
    }

    #[test]
    fn reflexive_links_are_their_own_reverse() {
        let (s, i) = world();
        let l = &s.class_links[&i["neighbor"]];
        assert_eq!(l.reverse, i["neighbor"]);
        // Lyon neighbor Arles, so Arles neighbor Lyon.
        assert!(s.object_link(i["Arles"], "neighbor", i["Lyon"]).is_some());
        assert!(s.object_link(i["Lyon"], "crosses", i["Rhone"]).is_none());
        assert!(s.object_link(i["Lyon"], "crossed by", i["Rhone"]).is_some());
    }

    #[test]
    fn errors() {
        let (mut s, i) = world();
        assert_eq!(
            s.class("PORT"),
            Err(Error("there is no class \"PORT\"".into()))
        );
        assert!(s.create_class("TOWN", ClassKind::Class).is_err());
        assert!(s.create_object(i["TOWN"], "Lyon").is_err());
        assert_eq!(
            s.assert_link(i["Lyon"], "crosses", i["Rhone"], None),
            Err(Error(
                "there is no link \"TOWN\" \"crosses\" \"RIVER\"".into()
            ))
        );
        assert!(
            s.create_class_link(i["TOWN"], "x", i["RIVER"], None, true)
                .is_err()
        );
        assert!(s.attribute(i["RIVER"], "AREA").is_err());
    }

    #[test]
    fn names_compare_folded() {
        let (mut s, i) = world();
        assert_eq!(s.class("town"), Ok(i["TOWN"]));
        assert_eq!(s.object(i["TOWN"], "LYON"), Ok(i["Lyon"]));
        assert_eq!(s.attribute(i["TOWN"], "Area"), Ok(i["AREA"]));
        assert!(s.object_link(i["Rhone"], "CROSSES", i["Lyon"]).is_some());
        assert_eq!(s.describe(Class(i["TOWN"])), "TOWN");
        assert!(s.create_class("Town", ClassKind::Class).is_err());
    }

    /// Records into a shared list, or refuses everything.
    struct Log(std::sync::Arc<std::sync::Mutex<Vec<String>>>, bool);

    impl Recorder for Log {
        fn record(&mut self, change: &Change) -> Result<(), String> {
            if self.1 {
                return Err("the disk is full".into());
            }
            self.0.lock().unwrap().push(format!("{change:?}"));
            Ok(())
        }
    }

    #[test]
    fn transactions() {
        let (mut s, i) = world();
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        s.record_to(Box::new(Log(log.clone(), false)));
        let before = s.dump();
        // Refused: everything undone, nothing recorded.
        s.begin();
        let c = s.create_class("PORT", ClassKind::Class).unwrap();
        s.create_object(c, "Marseille").unwrap();
        s.set(i["Vouglans"], i["VOL"], Value::integer(1)).unwrap();
        s.end(false).unwrap();
        assert_eq!(s.dump(), before);
        assert!(log.lock().unwrap().is_empty());
        // Kept, nested: recorded once, in order, by the outermost end.
        s.begin();
        s.create_class("PORT", ClassKind::Class).unwrap();
        s.begin();
        s.create_class("SEA", ClassKind::Class).unwrap();
        s.end(true).unwrap();
        assert!(log.lock().unwrap().is_empty());
        s.end(true).unwrap();
        let recorded = log.lock().unwrap().clone();
        assert_eq!(recorded.len(), 2);
        assert!(recorded[0].contains("PORT") && recorded[1].contains("SEA"));
        // A recorder that fails refuses the transaction.
        s.record_to(Box::new(Log(log.clone(), true)));
        let before = s.dump();
        s.begin();
        s.create_class("LAKE", ClassKind::Class).unwrap();
        assert!(s.end(true).is_err());
        assert_eq!(s.dump(), before);
    }

    #[test]
    fn a_recorder_sees_every_change_first() {
        struct Count(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
        impl Recorder for Count {
            fn record(&mut self, change: &Change) -> Result<(), String> {
                self.0.lock().unwrap().push(format!("{change:?}"));
                Ok(())
            }
        }
        struct Refuse;
        impl Recorder for Refuse {
            fn record(&mut self, _: &Change) -> Result<(), String> {
                Err("the disk is full".into())
            }
        }
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut s = Store::memory();
        s.record_to(Box::new(Count(seen.clone())));
        let c = s.create_class("TOWN", ClassKind::Class).unwrap();
        s.create_object(c, "Lyon").unwrap();
        assert_eq!(seen.lock().unwrap().len(), 2);
        // A change the recorder refuses is not applied.
        s.record_to(Box::new(Refuse));
        assert_eq!(
            s.create_class("RIVER", ClassKind::Class),
            Err(Error("the disk is full".into()))
        );
        assert!(s.find_class("RIVER").is_none());
    }

    #[test]
    fn classes_inherit_from_their_hyperclasses() {
        let (mut s, ids) = world();
        // HYDRO includes DAM; NATURE includes HYDRO: two levels up.
        let nature = s.create_class("NATURE", ClassKind::Class).unwrap();
        s.include(ids["DAM"], ids["HYDRO"]).unwrap();
        s.include(ids["HYDRO"], nature).unwrap();
        let age = s.create_attribute(nature, "AGE").unwrap();
        let flows = s
            .create_class_link(nature, "flows", nature, None, false)
            .unwrap();
        assert_eq!(s.lineage(ids["DAM"]), [ids["DAM"], ids["HYDRO"], nature]);
        assert_eq!(s.find_attribute(ids["DAM"], "age"), Some(age));
        assert_eq!(s.attribute(ids["DAM"], "AGE"), Ok(age));
        assert_eq!(
            s.find_class_link(ids["DAM"], "flows", ids["DAM"]),
            Some(flows)
        );
        let lake = s.create_class("LAKE", ClassKind::Class).unwrap();
        assert_eq!(s.find_attribute(lake, "AGE"), None);
        // An object carries an inherited attribute's value, and realizes
        // an inherited link.
        let v = s.find_object(ids["DAM"], "Vouglans").unwrap();
        s.set(v, age, Value::integer(50)).unwrap();
        assert_eq!(s.get(v, age), Some(&Value::integer(50)));
        let w = s.create_object(ids["DAM"], "Tignes").unwrap();
        let l = s.assert_link(v, "flows", w, None).unwrap();
        assert_eq!(s.object_links[&l].class_link, flows);
        // A class's own attribute of the same name is created, and found
        // first.
        let own = s.create_attribute(ids["DAM"], "AGE").unwrap();
        assert_eq!(s.find_attribute(ids["DAM"], "AGE"), Some(own));
        // A cycle of inclusions ends.
        s.include(nature, ids["DAM"]).unwrap();
        assert_eq!(s.lineage(nature), [nature, ids["DAM"], ids["HYDRO"]]);
        assert_eq!(s.find_attribute(nature, "VOL"), Some(ids["VOL"]));
    }

    #[test]
    fn setting_the_value_an_attribute_has_changes_nothing() {
        let (mut s, ids) = world();
        let lyon = s.find_object(ids["TOWN"], "Lyon").unwrap();
        s.begin();
        s.set(lyon, ids["AREA"], Value::integer(48)).unwrap();
        s.set(lyon, ids["AREA"], Value::integer(48)).unwrap();
        assert_eq!(s.pending().len(), 1);
        s.end(true).unwrap();
    }

    #[test]
    fn pending_changes_and_savepoints() {
        let (mut s, ids) = world();
        assert!(s.pending().is_empty());
        assert!(s.savepoint().is_none());
        let lyon = s.find_object(ids["TOWN"], "Lyon").unwrap();
        s.begin();
        s.set(lyon, ids["AREA"], Value::integer(48)).unwrap();
        s.begin();
        let point = s.savepoint().unwrap();
        let dump = s.dump();
        s.create_class("LAKE", ClassKind::Class).unwrap();
        s.set(lyon, ids["AREA"], Value::integer(50)).unwrap();
        assert_eq!(s.pending().len(), 3);
        s.rollback(point);
        assert_eq!(s.pending().len(), 1);
        assert_eq!(s.dump(), dump);
        s.end(true).unwrap();
        assert_eq!(s.pending().len(), 1);
        s.end(true).unwrap();
        assert!(s.pending().is_empty());
        assert_eq!(s.get(lyon, ids["AREA"]), Some(&Value::integer(48)));
    }
}
