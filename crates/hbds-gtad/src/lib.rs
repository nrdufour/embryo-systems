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

    /// From now on, what is recorded waits, in order, for [`release`](
    /// Recorder::release): the daemon's kernel, between a process's two
    /// pauses (docs/architecture.md, decision 5).
    fn hold(&mut self) {}

    /// What waited, then `extra`, as one transaction.
    fn release(&mut self, extra: &[Change]) -> Result<(), String> {
        self.record_all(extra)
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

static NO_PROPS: Props = Props {
    spec: None,
    unit: None,
    computed: false,
    properties: BTreeMap::new(),
};

/// An a.d.t.'s `Props`, boxed: nearly all carry none, and then cost a
/// pointer. Reads see an empty `Props`.
#[derive(Debug, Clone, Default)]
pub struct PropsBox(Option<Box<Props>>);

impl PropsBox {
    fn get_mut(&mut self) -> &mut Props {
        self.0.get_or_insert_default()
    }
}

impl std::ops::Deref for PropsBox {
    type Target = Props;
    fn deref(&self) -> &Props {
        self.0.as_deref().unwrap_or(&NO_PROPS)
    }
}

impl PartialEq for PropsBox {
    fn eq(&self, other: &PropsBox) -> bool {
        **self == **other
    }
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
    pub props: PropsBox,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub class: Id,
    pub name: String,
    pub state: State,
    pub props: PropsBox,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    pub class: Id,
    pub name: String,
    pub state: State,
    pub props: PropsBox,
}

/// An object's attribute (page 46, figure 6): its value, and what it
/// carries of its own, COND and ACTION among them (pages 156 and 157). It
/// has no state of its own (page 23 changes none): it is seen while its
/// object and its class attribute are. Kept by (object, attribute) only
/// while it holds something.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObjectAttribute {
    pub value: Option<Value>,
    pub props: PropsBox,
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
    pub props: PropsBox,
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
    pub props: PropsBox,
}

/// What a pair of objects is held to, by a link of their classes (page
/// 159): MUST keeps the link, CANNOT keeps it out. MAY, the default, is
/// no entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modality {
    Must,
    Cannot,
}

impl Modality {
    pub fn word(self) -> &'static str {
        match self {
            Modality::Must => "MUST",
            Modality::Cannot => "CANNOT",
        }
    }
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
    /// MUST, CANNOT, or MAY (none) for a pair, by a class link.
    Modality {
        class_link: Id,
        from: Id,
        to: Id,
        modality: Option<Modality>,
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
    /// An A-code unit the store keeps, named by its text's hash
    /// (docs/architecture.md, decisions 1 and 10).
    Module {
        hash: String,
        text: String,
    },
    /// The daemon's kernel's fibers, and its sequencing set, as their
    /// engines write them: the last one kept (decision 5).
    Fibers {
        text: String,
    },
    Sequencing {
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

/// A reflex launched by a change: the reflex, the object concerned, and
/// the value the change set, if it set one (a neuron's intensity).
#[derive(Clone, Debug, PartialEq)]
pub struct Launch {
    pub reflex: Id,
    pub this: Id,
    pub value: Option<Value>,
}

/// Of a change about to be made, the launches it makes.
pub type Watcher = fn(&Store, &Change, &mut Vec<Launch>);

/// What tells, of a change, the constraints it concerns (pages 157 to
/// 160): their FORMULAs run at the end of the statement.
pub type Checker = fn(&Store, &Change, &mut BTreeSet<Id>);

pub fn error<T>(message: impl Into<String>) -> Stored<T> {
    Err(Error(message.into()))
}

#[derive(Default)]
pub struct Store {
    next: Id,
    classes: PMap<Id, Class>,
    attributes: PMap<Id, Attribute>,
    objects: PMap<Id, Object>,
    object_attributes: PMap<(Id, Id), ObjectAttribute>,
    class_links: PMap<Id, ClassLink>,
    object_links: PMap<Id, ObjectLink>,
    associations: Vec<(AdtRef, AdtRef)>,
    /// MUST and CANNOT, by (class link, from, to), as written.
    modalities: BTreeMap<(Id, Id, Id), Modality>,
    /// The modules kept, by hash, in the order they came.
    modules: Vec<(String, String)>,
    /// The kernel's fibers and sequencing set, last kept.
    fibers: Option<String>,
    sequencing: Option<String>,
    ix: Indexes,
    recorder: Option<Box<dyn Recorder>>,
    /// The changes kept since last taken, when someone watches.
    kept: Option<Vec<Change>>,
    /// What tells, of each change, the reflexes it launches: the reflex
    /// engine's, once REFLEX exists (project 18).
    watcher: Option<Watcher>,
    /// The launches not yet run, in order: in the transaction's
    /// snapshot, so that a refused change launches nothing.
    launched: Vec<Launch>,
    /// What tells, of each change, the constraints it concerns: the
    /// constraint engine's, once CONSTRAINT exists.
    checker: Option<Checker>,
    /// The constraints concerned since their FORMULAs last ran: in the
    /// snapshot, as `launched` is.
    touched: BTreeSet<Id>,
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
    object_attributes: PMap<(Id, Id), ObjectAttribute>,
    class_links: PMap<Id, ClassLink>,
    object_links: PMap<Id, ObjectLink>,
    associations: Vec<(AdtRef, AdtRef)>,
    modalities: BTreeMap<(Id, Id, Id), Modality>,
    modules: Vec<(String, String)>,
    fibers: Option<String>,
    sequencing: Option<String>,
    launched: Vec<Launch>,
    touched: BTreeSet<Id>,
    ix: Indexes,
}

/// The world as a store held it at one moment, apart from the store: its
/// maps shared, so cheap to take, and sent to another thread to build
/// [`changes`](Image::changes) while the store goes on.
pub struct Image {
    classes: PMap<Id, Class>,
    attributes: PMap<Id, Attribute>,
    objects: PMap<Id, Object>,
    object_attributes: PMap<(Id, Id), ObjectAttribute>,
    class_links: PMap<Id, ClassLink>,
    object_links: PMap<Id, ObjectLink>,
    associations: Vec<(AdtRef, AdtRef)>,
    modalities: BTreeMap<(Id, Id, Id), Modality>,
    modules: Vec<(String, String)>,
    fibers: Option<String>,
    sequencing: Option<String>,
}

/// What the model is looked up by (docs/architecture.md, decision 11),
/// kept as it changes in `apply` and `purge` only: so a replay rebuilds
/// it, and nothing journals it. Names are folded. A map of pairs to `()`
/// is a set, read by ranging over its first element, the second in Id
/// order, as the scans it replaces gave.
#[derive(Clone, Default, PartialEq)]
struct Indexes {
    class_names: PMap<String, Id>,
    object_names: PMap<(Id, String), Id>,
    /// (class, object)
    class_objects: PMap<(Id, Id), ()>,
    attribute_names: PMap<(Id, String), Id>,
    /// (class, attribute)
    class_attributes: PMap<(Id, Id), ()>,
    /// (folded name, attribute), whatever the class.
    attributes_named: PMap<(String, Id), ()>,
    /// (class, class link), by the link's two ends.
    class_links_from: PMap<(Id, Id), ()>,
    class_links_to: PMap<(Id, Id), ()>,
    /// (object, object link), by the link's two ends, and (class link,
    /// object link).
    links_from: PMap<(Id, Id), ()>,
    links_to: PMap<(Id, Id), ()>,
    links_of: PMap<(Id, Id), ()>,
    /// (hyperclass, class it includes)
    members: PMap<(Id, Id), ()>,
}

impl Indexes {
    fn class(&mut self, id: Id, c: &Class, on: bool) {
        put(&mut self.class_names, fold(&c.name), id, on);
        for h in &c.hyper {
            put(&mut self.members, (*h, id), (), on);
        }
    }

    fn object(&mut self, id: Id, o: &Object, on: bool) {
        put(&mut self.object_names, (o.class, fold(&o.name)), id, on);
        put(&mut self.class_objects, (o.class, id), (), on);
    }

    fn attribute(&mut self, id: Id, a: &Attribute, on: bool) {
        put(&mut self.attribute_names, (a.class, fold(&a.name)), id, on);
        put(&mut self.class_attributes, (a.class, id), (), on);
        put(&mut self.attributes_named, (fold(&a.name), id), (), on);
    }

    fn class_link(&mut self, id: Id, l: &ClassLink, on: bool) {
        put(&mut self.class_links_from, (l.from, id), (), on);
        put(&mut self.class_links_to, (l.to, id), (), on);
    }

    fn object_link(&mut self, id: Id, l: &ObjectLink, on: bool) {
        put(&mut self.links_from, (l.from, id), (), on);
        put(&mut self.links_to, (l.to, id), (), on);
        put(&mut self.links_of, (l.class_link, id), (), on);
    }

    fn copied(&self) -> u64 {
        self.maps().iter().map(|m| m.copied()).sum::<u64>()
            + self.class_names.copied()
            + self.object_names.copied()
            + self.attribute_names.copied()
            + self.attributes_named.copied()
    }

    fn sets(&mut self) -> [&mut PMap<(Id, Id), ()>; 8] {
        [
            &mut self.class_objects,
            &mut self.class_attributes,
            &mut self.class_links_from,
            &mut self.class_links_to,
            &mut self.links_from,
            &mut self.links_to,
            &mut self.links_of,
            &mut self.members,
        ]
    }

    fn maps(&self) -> [&PMap<(Id, Id), ()>; 8] {
        [
            &self.class_objects,
            &self.class_attributes,
            &self.class_links_from,
            &self.class_links_to,
            &self.links_from,
            &self.links_to,
            &self.links_of,
            &self.members,
        ]
    }

    /// Back to `older`, keeping the counts of what was copied.
    fn restore(&mut self, mut older: Indexes) {
        self.class_names.restore(older.class_names.clone());
        self.object_names.restore(older.object_names.clone());
        self.attribute_names.restore(older.attribute_names.clone());
        self.attributes_named
            .restore(older.attributes_named.clone());
        for (now, then) in self.sets().into_iter().zip(older.sets()) {
            now.restore(then.clone());
        }
    }

    /// The differences from `other`, by index name.
    fn differences(&self, other: &Indexes) -> Vec<&'static str> {
        let mut out = Vec::new();
        let mut check = |name, same: bool| {
            if !same {
                out.push(name);
            }
        };
        check("class names", self.class_names == other.class_names);
        check("object names", self.object_names == other.object_names);
        check("class objects", self.class_objects == other.class_objects);
        check(
            "attribute names",
            self.attribute_names == other.attribute_names,
        );
        check(
            "class attributes",
            self.class_attributes == other.class_attributes,
        );
        check(
            "attributes named",
            self.attributes_named == other.attributes_named,
        );
        check(
            "class links from",
            self.class_links_from == other.class_links_from,
        );
        check(
            "class links to",
            self.class_links_to == other.class_links_to,
        );
        check("links from", self.links_from == other.links_from);
        check("links to", self.links_to == other.links_to);
        check("links of", self.links_of == other.links_of);
        check("members", self.members == other.members);
        out
    }
}

/// Adds the entry, or removes it.
fn put<K: Ord + Clone, V: Clone>(map: &mut PMap<K, V>, key: K, val: V, on: bool) {
    if on {
        map.insert(key, val);
    } else {
        map.remove(&key);
    }
}

/// The seconds of the pairs whose first is `first`, in order.
fn seconds(set: &PMap<(Id, Id), ()>, first: Id) -> Vec<Id> {
    (set.range_from(&(first, 0)))
        .take_while(|((a, _), _)| *a == first)
        .map(|((_, b), _)| *b)
        .collect()
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
                + self.object_attributes.copied()
                + self.class_links.copied()
                + self.object_links.copied()
                + self.ix.copied(),
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
        if let Some(watcher) = self.watcher {
            let mut launches = Vec::new();
            watcher(self, &change, &mut launches);
            self.launched.extend(launches);
        }
        if let Some(checker) = self.checker {
            let mut touched = std::mem::take(&mut self.touched);
            checker(self, &change, &mut touched);
            self.touched = touched;
        }
        if let Some(t) = &mut self.transaction {
            t.changes.push(change.clone());
        } else {
            if let Some(recorder) = &mut self.recorder {
                recorder.record(&change).map_err(Error)?;
            }
            if let Some(kept) = &mut self.kept {
                kept.push(change.clone());
            }
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
            object_attributes: self.object_attributes.clone(),
            class_links: self.class_links.clone(),
            object_links: self.object_links.clone(),
            associations: self.associations.clone(),
            modalities: self.modalities.clone(),
            modules: self.modules.clone(),
            fibers: self.fibers.clone(),
            sequencing: self.sequencing.clone(),
            launched: self.launched.clone(),
            touched: self.touched.clone(),
            ix: self.ix.clone(),
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.next = s.next;
        self.classes.restore(s.classes);
        self.attributes.restore(s.attributes);
        self.objects.restore(s.objects);
        self.object_attributes.restore(s.object_attributes);
        self.class_links.restore(s.class_links);
        self.object_links.restore(s.object_links);
        self.associations = s.associations;
        self.modalities = s.modalities;
        self.modules = s.modules;
        self.fibers = s.fibers;
        self.sequencing = s.sequencing;
        self.launched = s.launched;
        self.touched = s.touched;
        self.ix.restore(s.ix);
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
        if let Some(kept) = &mut self.kept {
            kept.extend(t.changes);
        }
        Ok(())
    }

    /// From now on, `checker` tells the constraints each change concerns.
    pub fn check_with(&mut self, checker: Checker) {
        self.checker = Some(checker);
    }

    /// Whether a checker tells this store's constraints concerned.
    pub fn checking(&self) -> bool {
        self.checker.is_some()
    }

    /// The constraints concerned since last taken, in id order.
    pub fn take_touched(&mut self) -> Vec<Id> {
        std::mem::take(&mut self.touched).into_iter().collect()
    }

    pub fn has_touched(&self) -> bool {
        !self.touched.is_empty()
    }

    /// Whether the transaction open is the outermost: the one whose end
    /// decides, at the end of a statement.
    pub fn outermost(&self) -> bool {
        self.transaction.as_ref().is_some_and(|t| t.depth == 1)
    }

    /// From now on, `watcher` tells what each change launches.
    pub fn watch(&mut self, watcher: Watcher) {
        self.watcher = Some(watcher);
    }

    /// The launches not yet run, taken, in order.
    pub fn take_launched(&mut self) -> Vec<Launch> {
        std::mem::take(&mut self.launched)
    }

    /// Whether a watcher tells this store's launches.
    pub fn watching(&self) -> bool {
        self.watcher.is_some()
    }

    pub fn has_launched(&self) -> bool {
        !self.launched.is_empty()
    }

    /// From now on, the changes kept are gathered for [`take_kept`](
    /// Store::take_kept): the daemon's feed.
    pub fn observe(&mut self) {
        self.kept.get_or_insert_with(Vec::new);
    }

    /// The changes kept since last taken, in order: a transaction kept,
    /// or a change made outside one.
    pub fn take_kept(&mut self) -> Vec<Change> {
        self.kept.as_mut().map(std::mem::take).unwrap_or_default()
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
                self.put_class(
                    id,
                    Class {
                        name,
                        kind,
                        state: State::Active,
                        hyper: BTreeSet::new(),
                        props: PropsBox::default(),
                    },
                );
            }
            Change::Object { id, class, name } => {
                self.put_object(
                    id,
                    Object {
                        class,
                        name,
                        state: State::Active,
                        props: PropsBox::default(),
                    },
                );
            }
            Change::Attribute { id, class, name } => {
                self.put_attribute(
                    id,
                    Attribute {
                        class,
                        name,
                        state: State::Active,
                        props: PropsBox::default(),
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
                    props: PropsBox::default(),
                };
                self.put_class_link(id, link(from, Some(relation), to, reverse));
                if reverse != id {
                    self.put_class_link(reverse, link(to, reverse_relation, from, id));
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
                    props: PropsBox::default(),
                };
                self.put_object_link(id, link(class_link, from, to, reverse));
                self.put_object_link(reverse, link(back, to, from, id));
            }
            Change::Unlink { id } => {
                if let Some(l) = self.drop_object_link(id) {
                    self.drop_object_link(l.reverse);
                }
            }
            Change::Set {
                object,
                attribute,
                value,
            } => {
                if self.objects.contains_key(&object) {
                    self.object_attribute_mut(object, attribute).value = Some(value);
                }
            }
            Change::Specify {
                target,
                spec,
                unit,
                computed,
            } => {
                // An object attribute is specified by its class attribute
                // (page 24: SPECIFY takes class attributes).
                let target = match target {
                    AdtRef::ObjectAttribute(_, a) => AdtRef::ClassAttribute(a),
                    t => t,
                };
                if let Some(p) = self.props_mut(target) {
                    p.spec = spec;
                    p.unit = unit;
                    p.computed = computed;
                }
            }
            Change::State { target, state } => self.set_state(target, state),
            Change::Purge { target } => {
                self.purge(target);
                // What held gone objects or links goes with them.
                let (objects, class_links) = (&self.objects, &self.class_links);
                self.modalities.retain(|(l, a, b), _| {
                    class_links.contains_key(l)
                        && objects.contains_key(a)
                        && objects.contains_key(b)
                });
            }
            Change::Include { class, hyper } => {
                if let Some(c) = self.classes.get_mut(&class) {
                    c.hyper.insert(hyper);
                    self.ix.members.insert((hyper, class), ());
                }
            }
            Change::Exclude { class, hyper } => {
                if let Some(c) = self.classes.get_mut(&class) {
                    c.hyper.remove(&hyper);
                    self.ix.members.remove(&(hyper, class));
                }
            }
            Change::Module { hash, text } => {
                if !self.modules.iter().any(|(h, _)| *h == hash) {
                    self.modules.push((hash, text));
                }
            }
            Change::Fibers { text } => self.fibers = Some(text),
            Change::Sequencing { text } => self.sequencing = Some(text),
            Change::Assoc { a, b } => {
                if !self.associations.contains(&(a, b)) {
                    self.associations.push((a, b));
                }
            }
            Change::Dissoc { a, b } => self.associations.retain(|p| *p != (a, b)),
            Change::Modality {
                class_link,
                from,
                to,
                modality,
            } => match modality {
                Some(m) => {
                    self.modalities.insert((class_link, from, to), m);
                }
                None => {
                    self.modalities.remove(&(class_link, from, to));
                }
            },
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
            AdtRef::Class(id) => self.classes.get(&id).map(|c| &*c.props),
            AdtRef::Object(id) => self.objects.get(&id).map(|o| &*o.props),
            AdtRef::ClassAttribute(id) => self.attributes.get(&id).map(|a| &*a.props),
            AdtRef::ObjectAttribute(o, a) => {
                self.object_attributes.get(&(o, a)).map(|oa| &*oa.props)
            }
            AdtRef::ClassLink(id) => self.class_links.get(&id).map(|l| &*l.props),
            AdtRef::ObjectLink(id) => self.object_links.get(&id).map(|l| &*l.props),
        }
    }

    fn props_mut(&mut self, r: AdtRef) -> Option<&mut Props> {
        match r {
            AdtRef::Class(id) => self.classes.get_mut(&id).map(|c| c.props.get_mut()),
            AdtRef::Object(id) => self.objects.get_mut(&id).map(|o| o.props.get_mut()),
            AdtRef::ClassAttribute(id) => self.attributes.get_mut(&id).map(|a| a.props.get_mut()),
            AdtRef::ObjectAttribute(o, a) => {
                if !self.objects.contains_key(&o) || !self.attributes.contains_key(&a) {
                    return None;
                }
                Some(self.object_attribute_mut(o, a).props.get_mut())
            }
            AdtRef::ClassLink(id) => self.class_links.get_mut(&id).map(|l| l.props.get_mut()),
            AdtRef::ObjectLink(id) => self.object_links.get_mut(&id).map(|l| l.props.get_mut()),
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
            AdtRef::ClassAttribute(id) => {
                if let Some(a) = self.attributes.get_mut(&id) {
                    a.state = state;
                }
            }
            // No state of its own: page 23 changes none.
            AdtRef::ObjectAttribute(..) => {}
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
                self.drop_class(id);
                for o in seconds(&self.ix.class_objects, id) {
                    self.purge(AdtRef::Object(o));
                }
                for a in seconds(&self.ix.class_attributes, id) {
                    self.drop_attribute(a);
                }
                let mut links = seconds(&self.ix.class_links_from, id);
                links.extend(seconds(&self.ix.class_links_to, id));
                for l in links {
                    self.purge(AdtRef::ClassLink(l));
                }
            }
            AdtRef::Object(id) => {
                self.drop_object(id);
                for a in self.attributes_held(id) {
                    self.object_attributes.remove(&(id, a));
                }
                let mut links = seconds(&self.ix.links_from, id);
                links.extend(seconds(&self.ix.links_to, id));
                for l in links {
                    self.drop_object_link(l);
                }
            }
            AdtRef::ClassAttribute(id) => {
                self.drop_attribute(id);
                let held: Vec<(Id, Id)> = (self.object_attributes.keys())
                    .filter(|(_, a)| *a == id)
                    .copied()
                    .collect();
                for k in held {
                    self.object_attributes.remove(&k);
                }
            }
            AdtRef::ObjectAttribute(o, a) => {
                self.object_attributes.remove(&(o, a));
            }
            AdtRef::ClassLink(id) => {
                if let Some(l) = self.drop_class_link(id) {
                    self.drop_class_link(l.reverse);
                    let mut links = seconds(&self.ix.links_of, id);
                    links.extend(seconds(&self.ix.links_of, l.reverse));
                    for o in links {
                        self.drop_object_link(o);
                    }
                }
            }
            AdtRef::ObjectLink(id) => self.apply(Change::Unlink { id }),
        }
        self.associations
            .retain(|(a, b)| !involves(*a, r) && !involves(*b, r));
    }

    // The model's entries, with their indexes: the only way in or out.

    fn put_class(&mut self, id: Id, c: Class) {
        self.ix.class(id, &c, true);
        self.classes.insert(id, c);
    }

    fn drop_class(&mut self, id: Id) -> Option<Class> {
        let c = self.classes.remove(&id)?;
        self.ix.class(id, &c, false);
        Some(c)
    }

    fn put_object(&mut self, id: Id, o: Object) {
        self.ix.object(id, &o, true);
        self.objects.insert(id, o);
    }

    fn drop_object(&mut self, id: Id) -> Option<Object> {
        let o = self.objects.remove(&id)?;
        self.ix.object(id, &o, false);
        Some(o)
    }

    fn put_attribute(&mut self, id: Id, a: Attribute) {
        self.ix.attribute(id, &a, true);
        self.attributes.insert(id, a);
    }

    fn drop_attribute(&mut self, id: Id) -> Option<Attribute> {
        let a = self.attributes.remove(&id)?;
        self.ix.attribute(id, &a, false);
        Some(a)
    }

    fn put_class_link(&mut self, id: Id, l: ClassLink) {
        self.ix.class_link(id, &l, true);
        self.class_links.insert(id, l);
    }

    fn drop_class_link(&mut self, id: Id) -> Option<ClassLink> {
        let l = self.class_links.remove(&id)?;
        self.ix.class_link(id, &l, false);
        Some(l)
    }

    fn put_object_link(&mut self, id: Id, l: ObjectLink) {
        self.ix.object_link(id, &l, true);
        self.object_links.insert(id, l);
    }

    fn drop_object_link(&mut self, id: Id) -> Option<ObjectLink> {
        let l = self.object_links.remove(&id)?;
        self.ix.object_link(id, &l, false);
        Some(l)
    }

    /// The model holds together, every reference to something there,
    /// and every index is what the model rebuilds. For tests.
    pub fn verify(&self) -> Result<(), String> {
        let class = |c: &Id| self.classes.contains_key(c);
        let object = |o: &Id| self.objects.contains_key(o);
        let mut dangling = Vec::new();
        for (id, o) in &self.objects {
            if !class(&o.class) {
                dangling.push(format!("object {id}'s class"));
            }
        }
        for (id, a) in &self.attributes {
            if !class(&a.class) {
                dangling.push(format!("attribute {id}'s class"));
            }
        }
        for (id, l) in &self.class_links {
            if !class(&l.from) || !class(&l.to) || !self.class_links.contains_key(&l.reverse) {
                dangling.push(format!("class link {id}'s ends or reverse"));
            }
        }
        for (id, l) in &self.object_links {
            if !object(&l.from)
                || !object(&l.to)
                || !self.class_links.contains_key(&l.class_link)
                || !self.object_links.contains_key(&l.reverse)
            {
                dangling.push(format!("object link {id}'s ends, class link or reverse"));
            }
        }
        for ((o, a), _) in &self.object_attributes {
            if !object(o) || !self.attributes.contains_key(a) {
                dangling.push(format!("object attribute {o}.{a}'s object or attribute"));
            }
        }
        if !dangling.is_empty() {
            return Err(format!("dangling: {}", dangling.join(", ")));
        }
        let mut ix = Indexes::default();
        for (id, c) in &self.classes {
            ix.class(*id, c, true);
        }
        for (id, o) in &self.objects {
            ix.object(*id, o, true);
        }
        for (id, a) in &self.attributes {
            ix.attribute(*id, a, true);
        }
        for (id, l) in &self.class_links {
            ix.class_link(*id, l, true);
        }
        for (id, l) in &self.object_links {
            ix.object_link(*id, l, true);
        }
        match self.ix.differences(&ix)[..] {
            [] => Ok(()),
            ref wrong => Err(format!("indexes out of date: {}", wrong.join(", "))),
        }
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
        self.scanned(1);
        self.ix.class_names.get(&fold(name)).copied()
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
        self.scanned(1);
        self.ix.object_names.get(&(class, fold(name))).copied()
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
        self.scanned(1);
        self.ix.attribute_names.get(&(class, fold(name))).copied()
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
        self.class_links_from(from).into_iter().find(|i| {
            let l = &self.class_links[i];
            l.to == to && l.relation.as_deref().is_some_and(|r| same(r, relation))
        })
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
        (self.links_from(from).into_iter())
            .find(|i| {
                let l = &self.object_links[i];
                l.class_link == class_link && l.to == to
            })
            .filter(|i| self.visible(AdtRef::ObjectLink(*i)))
    }

    /// The pairs of an index whose first is `first`, counted.
    fn range(&self, set: &PMap<(Id, Id), ()>, first: Id) -> Vec<Id> {
        let found = seconds(set, first);
        self.scanned(found.len());
        found
    }

    /// A class's objects, oldest first, whatever their state.
    pub fn objects_of(&self, class: Id) -> Vec<Id> {
        self.range(&self.ix.class_objects, class)
    }

    /// A class's own attributes, oldest first.
    pub fn attributes_of(&self, class: Id) -> Vec<Id> {
        self.range(&self.ix.class_attributes, class)
    }

    /// The attributes of that name, in every class, oldest first.
    pub fn attributes_named(&self, name: &str) -> Vec<Id> {
        let name = fold(name);
        let found: Vec<Id> = (self.ix.attributes_named.range_from(&(name.clone(), 0)))
            .take_while(|((n, _), _)| *n == name)
            .map(|((_, a), _)| *a)
            .collect();
        self.scanned(found.len());
        found
    }

    /// The class links from a class, each direction a link, oldest first.
    pub fn class_links_from(&self, class: Id) -> Vec<Id> {
        self.range(&self.ix.class_links_from, class)
    }

    pub fn class_links_to(&self, class: Id) -> Vec<Id> {
        self.range(&self.ix.class_links_to, class)
    }

    /// The object links from an object, oldest first.
    pub fn links_from(&self, object: Id) -> Vec<Id> {
        self.range(&self.ix.links_from, object)
    }

    pub fn links_to(&self, object: Id) -> Vec<Id> {
        self.range(&self.ix.links_to, object)
    }

    /// The object links realizing a class link, oldest first.
    pub fn links_of(&self, class_link: Id) -> Vec<Id> {
        self.range(&self.ix.links_of, class_link)
    }

    /// The classes a hyperclass includes directly, oldest first.
    pub fn members(&self, hyper: Id) -> Vec<Id> {
        self.range(&self.ix.members, hyper)
    }

    // The model, to read; it changes only through changes.

    pub fn classes(&self) -> &PMap<Id, Class> {
        &self.classes
    }

    pub fn attributes(&self) -> &PMap<Id, Attribute> {
        &self.attributes
    }

    pub fn objects(&self) -> &PMap<Id, Object> {
        &self.objects
    }

    pub fn class_links(&self) -> &PMap<Id, ClassLink> {
        &self.class_links
    }

    pub fn object_links(&self) -> &PMap<Id, ObjectLink> {
        &self.object_links
    }

    pub fn associations(&self) -> &[(AdtRef, AdtRef)] {
        &self.associations
    }

    pub fn get(&self, object: Id, attribute: Id) -> Option<&Value> {
        self.object_attributes
            .get(&(object, attribute))?
            .value
            .as_ref()
    }

    /// An object's values, by attribute, in Id order.
    pub fn values_of(&self, object: Id) -> impl Iterator<Item = (Id, &Value)> {
        (self.object_attributes.range_from(&(object, 0)))
            .take_while(move |((o, _), _)| *o == object)
            .filter_map(|((_, a), oa)| Some((*a, oa.value.as_ref()?)))
    }

    /// The attributes an object holds something for, in Id order.
    fn attributes_held(&self, object: Id) -> Vec<Id> {
        (self.object_attributes.range_from(&(object, 0)))
            .take_while(|((o, _), _)| *o == object)
            .map(|((_, a), _)| *a)
            .collect()
    }

    /// An object's attribute, to change: made if it held nothing.
    fn object_attribute_mut(&mut self, object: Id, attribute: Id) -> &mut ObjectAttribute {
        if !self.object_attributes.contains_key(&(object, attribute)) {
            self.object_attributes
                .insert((object, attribute), ObjectAttribute::default());
        }
        self.object_attributes
            .get_mut(&(object, attribute))
            .unwrap()
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
        if let Some((key, Modality::Cannot)) = self.held(class_link, from, to) {
            return error(format!(
                "{}: the link is not asserted",
                self.pair(key, Modality::Cannot)
            ));
        }
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
        let Some(id) = self.object_link(from, relation, to) else {
            return Ok(());
        };
        let class_link = self.object_links[&id].class_link;
        if let Some((key, Modality::Must)) = self.held(class_link, from, to) {
            return error(format!(
                "{}: the link is not retracted",
                self.pair(key, Modality::Must)
            ));
        }
        self.commit(Change::Unlink { id })
    }

    /// MUST, CANNOT, or MAY (`None`): what holds `from` and `to` by the
    /// link of their classes named `relation` from then on (page 159).
    pub fn hold_to(
        &mut self,
        from: Id,
        relation: &str,
        to: Id,
        modality: Option<Modality>,
    ) -> Stored<()> {
        let (a, b) = (self.objects[&from].class, self.objects[&to].class);
        let class_link = self.class_link(a, relation, b)?;
        // Written either way, it is one pair: the other way is cleared.
        let reverse = self.class_links[&class_link].reverse;
        if self.modalities.contains_key(&(reverse, to, from))
            && (reverse, to, from) != (class_link, from, to)
        {
            self.commit(Change::Modality {
                class_link: reverse,
                from: to,
                to: from,
                modality: None,
            })?;
        }
        if self.modalities.get(&(class_link, from, to)).copied() == modality {
            return Ok(());
        }
        self.commit(Change::Modality {
            class_link,
            from,
            to,
            modality,
        })
    }

    /// What holds a pair by a class link, written either way.
    pub fn modality(&self, class_link: Id, from: Id, to: Id) -> Option<Modality> {
        self.held(class_link, from, to).map(|(_, m)| m)
    }

    /// What holds a pair, and the pair as it was written.
    fn held(&self, class_link: Id, from: Id, to: Id) -> Option<((Id, Id, Id), Modality)> {
        let reverse = self.class_links.get(&class_link).map(|l| l.reverse);
        let mut keys = [Some((class_link, from, to)), reverse.map(|r| (r, to, from))].into_iter();
        keys.find_map(|k| k.and_then(|k| self.modalities.get(&k).map(|m| (k, *m))))
    }

    /// A pair held, as it was written: a[c] MUST "follow" b[c].
    fn pair(&self, (l, a, b): (Id, Id, Id), m: Modality) -> String {
        let relation = self.class_links[&l].relation.clone().unwrap_or_default();
        format!(
            "{} {} \"{relation}\" {}",
            self.describe(AdtRef::Object(a)),
            m.word(),
            self.describe(AdtRef::Object(b))
        )
    }

    /// The MUST and CANNOT standing, by (class link, from, to).
    pub fn modalities(&self) -> &BTreeMap<(Id, Id, Id), Modality> {
        &self.modalities
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
        if matches!(target, AdtRef::ObjectAttribute(..)) {
            return error(
                "an object attribute has no state of its own; its object and class attribute do",
            );
        }
        if let (AdtRef::Object(o), State::Destroyed) = (target, to)
            && let Some((&key, _)) = (self.modalities.iter())
                .find(|((_, a, b), m)| **m == Modality::Must && (*a == o || *b == o))
        {
            return error(format!(
                "{}: {} is not destroyed",
                self.pair(key, Modality::Must),
                self.describe(target)
            ));
        }
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
    /// Keeps an A-code unit's text, once: its hash.
    pub fn keep_module(&mut self, text: &str) -> Stored<String> {
        let hash = hbds_core::sha1::hex(text.as_bytes());
        if !self.modules.iter().any(|(h, _)| *h == hash) {
            self.commit(Change::Module {
                hash: hash.clone(),
                text: text.to_string(),
            })?;
        }
        Ok(hash)
    }

    /// The modules kept, by hash, in the order they came.
    pub fn modules(&self) -> &[(String, String)] {
        &self.modules
    }

    /// The kernel's fibers and sequencing set, as last kept.
    pub fn fibers(&self) -> Option<&str> {
        self.fibers.as_deref()
    }

    pub fn sequencing(&self) -> Option<&str> {
        self.sequencing.as_deref()
    }

    /// Keeps the sequencing set as a client changed it.
    pub fn keep_sequencing(&mut self, text: String) -> Stored<()> {
        self.commit(Change::Sequencing { text })
    }

    /// From now on, what is recorded waits for [`release_records`](
    /// Store::release_records), in order.
    pub fn hold_records(&mut self) {
        if let Some(r) = &mut self.recorder {
            r.hold();
        }
    }

    /// Applies `extra`, then records what waited and `extra` as one
    /// transaction.
    pub fn release_records(&mut self, extra: Vec<Change>) -> Stored<()> {
        for c in &extra {
            self.apply(c.clone());
        }
        match &mut self.recorder {
            Some(r) => r.release(&extra).map_err(Error),
            None => Ok(()),
        }
    }

    /// The world as it is now, to build its changes elsewhere.
    pub fn image(&self) -> Image {
        Image {
            classes: self.classes.clone(),
            attributes: self.attributes.clone(),
            objects: self.objects.clone(),
            object_attributes: self.object_attributes.clone(),
            class_links: self.class_links.clone(),
            object_links: self.object_links.clone(),
            associations: self.associations.clone(),
            modalities: self.modalities.clone(),
            modules: self.modules.clone(),
            fibers: self.fibers.clone(),
            sequencing: self.sequencing.clone(),
        }
    }

    /// The shortest sequence of changes that rebuilds this store, ids
    /// kept: what compaction writes (docs/architecture.md, decision 5).
    pub fn changes(&self) -> Vec<Change> {
        self.image().changes()
    }

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
            for aid in self.attributes_of(*id) {
                let a = &self.attributes[&aid];
                out += &format!(
                    "  attribute {}{}{}\n",
                    a.name,
                    state(a.state),
                    props(&a.props)
                );
            }
            for lid in self.class_links_from(*id) {
                let l = &self.class_links[&lid];
                out += &format!(
                    "  link {}{}{}\n",
                    self.describe(AdtRef::ClassLink(lid)),
                    state(l.state),
                    props(&l.props)
                );
            }
            for oid in self.objects_of(*id) {
                let o = &self.objects[&oid];
                out += &format!("  object {}{}{}\n", o.name, state(o.state), props(&o.props));
                for a in self.attributes_held(oid) {
                    let oa = &self.object_attributes[&(oid, a)];
                    let name = self.attributes.get(&a).map_or("?", |a| a.name.as_str());
                    let value = oa
                        .value
                        .as_ref()
                        .map_or(String::new(), |v| format!(" = {}", self.show(v)));
                    out += &format!("    {name}{value}{}\n", props(&oa.props));
                }
                for lid in self.links_from(oid) {
                    let l = &self.object_links[&lid];
                    let p = l.probability.map_or(String::new(), |p| format!(" % {p}"));
                    out += &format!(
                        "    link {}{p}{}{}\n",
                        self.describe(AdtRef::ObjectLink(lid)),
                        state(l.state),
                        props(&l.props)
                    );
                }
            }
        }
        for (hash, _) in &self.modules {
            out += &format!("module {hash}\n");
        }
        for (a, b) in &self.associations {
            out += &format!("assoc {} WITH {}\n", self.describe(*a), self.describe(*b));
        }
        for (&key, &m) in &self.modalities {
            out += &format!("{}\n", self.pair(key, m));
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

impl Image {
    /// The shortest sequence of changes that rebuilds the store it was
    /// taken from, ids kept.
    pub fn changes(&self) -> Vec<Change> {
        let mut out = Vec::new();
        self.each_change(&mut |c| out.push(c));
        out
    }

    /// [`changes`](Image::changes), one at a time: what compaction writes
    /// as it goes, holding none of them.
    pub fn each_change(&self, f: &mut dyn FnMut(Change)) {
        for (&id, c) in self.classes.iter() {
            f(Change::Class {
                id,
                name: c.name.clone(),
                kind: c.kind.clone(),
            });
        }
        for (&id, c) in self.classes.iter() {
            for &hyper in &c.hyper {
                f(Change::Include { class: id, hyper });
            }
        }
        for (&id, a) in self.attributes.iter() {
            f(Change::Attribute {
                id,
                class: a.class,
                name: a.name.clone(),
            });
        }
        for (&id, l) in self.class_links.iter() {
            // Once a pair, from the direction that names its relation (the
            // first, if both do).
            let back = self.class_links.get(&l.reverse);
            let named_back = back.is_some_and(|b| b.relation.is_some());
            let first = match &l.relation {
                None => false,
                Some(_) => id <= l.reverse || !named_back,
            };
            if !first {
                continue;
            }
            f(Change::ClassLink {
                id,
                reverse: l.reverse,
                from: l.from,
                relation: l.relation.clone().expect("named"),
                to: l.to,
                reverse_relation: back.and_then(|b| b.relation.clone()),
            });
        }
        for (&id, o) in self.objects.iter() {
            f(Change::Object {
                id,
                class: o.class,
                name: o.name.clone(),
            });
        }
        for (&(object, attribute), oa) in self.object_attributes.iter() {
            if let Some(value) = &oa.value {
                f(Change::Set {
                    object,
                    attribute,
                    value: value.clone(),
                });
            }
        }
        for (&id, l) in self.object_links.iter() {
            if id <= l.reverse {
                f(Change::ObjectLink {
                    id,
                    reverse: l.reverse,
                    class_link: l.class_link,
                    from: l.from,
                    to: l.to,
                    probability: l.probability,
                });
            }
        }
        let carriers = (self
            .classes
            .iter()
            .map(|(&id, c)| (AdtRef::Class(id), &c.props)))
        .chain(
            self.attributes
                .iter()
                .map(|(&id, a)| (AdtRef::ClassAttribute(id), &a.props)),
        )
        .chain(
            self.class_links
                .iter()
                .map(|(&id, l)| (AdtRef::ClassLink(id), &l.props)),
        )
        .chain(
            self.objects
                .iter()
                .map(|(&id, o)| (AdtRef::Object(id), &o.props)),
        )
        .chain(
            self.object_attributes
                .iter()
                .map(|(&(o, a), oa)| (AdtRef::ObjectAttribute(o, a), &oa.props)),
        )
        .chain(
            self.object_links
                .iter()
                .map(|(&id, l)| (AdtRef::ObjectLink(id), &l.props)),
        );
        for (target, p) in carriers {
            if p.spec.is_some() || p.unit.is_some() || p.computed {
                f(Change::Specify {
                    target,
                    spec: p.spec,
                    unit: p.unit.clone(),
                    computed: p.computed,
                });
            }
            for (name, code) in &p.properties {
                f(Change::Property {
                    target,
                    name: name.clone(),
                    language: code.language.clone(),
                    text: code.text.clone(),
                });
            }
        }
        for &(a, b) in &self.associations {
            f(Change::Assoc { a, b });
        }
        for (&(class_link, from, to), &m) in &self.modalities {
            f(Change::Modality {
                class_link,
                from,
                to,
                modality: Some(m),
            });
        }
        for (hash, text) in &self.modules {
            f(Change::Module {
                hash: hash.clone(),
                text: text.clone(),
            });
        }
        if let Some(text) = &self.fibers {
            f(Change::Fibers { text: text.clone() });
        }
        if let Some(text) = &self.sequencing {
            f(Change::Sequencing { text: text.clone() });
        }
        let states = (self
            .classes
            .iter()
            .map(|(&id, c)| (AdtRef::Class(id), c.state)))
        .chain(
            self.attributes
                .iter()
                .map(|(&id, a)| (AdtRef::ClassAttribute(id), a.state)),
        )
        .chain(
            self.class_links
                .iter()
                .filter(|(id, l)| **id <= l.reverse)
                .map(|(&id, l)| (AdtRef::ClassLink(id), l.state)),
        )
        .chain(
            self.objects
                .iter()
                .map(|(&id, o)| (AdtRef::Object(id), o.state)),
        )
        .chain(
            self.object_links
                .iter()
                .filter(|(id, l)| **id <= l.reverse)
                .map(|(&id, l)| (AdtRef::ObjectLink(id), l.state)),
        );
        for (target, state) in states {
            if state != State::Active {
                f(Change::State { target, state });
            }
        }
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

    /// Random changes of every kind, some in transactions kept or refused:
    /// after each, the indexes are what the model rebuilds.
    #[test]
    fn indexes_follow_every_change() {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        let mut s = Store::memory();
        let pick = |ids: Vec<Id>, n: u64| ids.get(n as usize % ids.len().max(1)).copied();
        let states = [State::Active, State::Hibernating, State::Destroyed];
        for step in 0..3000 {
            let classes: Vec<Id> = s.classes().keys().copied().collect();
            let objects: Vec<Id> = s.objects().keys().copied().collect();
            let attributes: Vec<Id> = s.attributes().keys().copied().collect();
            let class_links: Vec<Id> = s.class_links().keys().copied().collect();
            let object_links: Vec<Id> = s.object_links().keys().copied().collect();
            let (c, d) = (pick(classes.clone(), next(97)), pick(classes, next(97)));
            let (o, p) = (pick(objects.clone(), next(97)), pick(objects, next(97)));
            let name = format!("n{}", next(12));
            let target = match next(5) {
                0 => c.map(AdtRef::Class),
                1 => o.map(AdtRef::Object),
                2 => pick(attributes, next(97)).map(AdtRef::ClassAttribute),
                3 => pick(class_links, next(97)).map(AdtRef::ClassLink),
                _ => pick(object_links, next(97)).map(AdtRef::ObjectLink),
            };
            let transaction = next(4) == 0;
            if transaction {
                s.begin();
            }
            let _ = match next(12) {
                0 | 1 => s.create_class(&name, ClassKind::Class).map(|_| ()),
                2 | 3 => c.map_or(Ok(()), |c| s.create_object(c, &name).map(|_| ())),
                4 => c.map_or(Ok(()), |c| s.create_attribute(c, &name).map(|_| ())),
                5 => match (c, d) {
                    (Some(c), Some(d)) => {
                        let back = next(2).eq(&0).then_some("back");
                        s.create_class_link(c, &name, d, back, c == d && next(2) == 0)
                            .map(|_| ())
                    }
                    _ => Ok(()),
                },
                6 | 7 => match (o, p) {
                    (Some(o), Some(p)) => {
                        let relation = s
                            .class_links_from(s.objects()[&o].class)
                            .into_iter()
                            .find_map(|l| s.class_links()[&l].relation.clone());
                        relation.map_or(Ok(()), |r| s.assert_link(o, &r, p, None).map(|_| ()))
                    }
                    _ => Ok(()),
                },
                8 => target.map_or(Ok(()), |t| {
                    let to = states[next(3) as usize];
                    match s.state(t) {
                        Some(from) => s.change_state(t, from, to),
                        None => Ok(()),
                    }
                }),
                9 => target.map_or(Ok(()), |t| s.purge_adt(t)),
                10 => match (c, d) {
                    (Some(c), Some(d)) => s.include(c, d),
                    _ => Ok(()),
                },
                _ => match (c, d) {
                    (Some(c), Some(d)) => s.exclude(c, d),
                    _ => Ok(()),
                },
            };
            if transaction {
                let _ = s.end(next(2) == 0);
            }
            if let Err(e) = s.verify() {
                panic!("step {step}: {e}");
            }
            if step % 100 == 0 && s.transaction.is_none() {
                let mut rebuilt = Store::memory();
                for c in s.changes() {
                    rebuilt.replay(c);
                }
                rebuilt
                    .verify()
                    .unwrap_or_else(|e| panic!("step {step}, rebuilt: {e}"));
                assert_eq!(rebuilt.dump(), s.dump(), "step {step}: rebuilt");
                assert_eq!(rebuilt.changes(), s.changes(), "step {step}: rebuilt");
            }
        }
        assert!(
            s.objects().len() > 50 && s.object_links().len() > 50,
            "too little built"
        );
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

    #[test]
    fn an_object_attribute_carries_its_own_and_goes_with_what_it_depends_on() {
        let (mut s, ids) = world();
        let (town, area) = (ids["TOWN"], ids["AREA"]);
        let lyon = s.find_object(town, "Lyon").unwrap();
        let arles = s.find_object(town, "Arles").unwrap();
        let own = AdtRef::ObjectAttribute(lyon, area);
        s.set_property(own, "ACTION", "ADT81", "PRINT 1").unwrap();
        s.set(arles, area, Value::integer(3)).unwrap();
        assert!(s.props(own).unwrap().properties.contains_key("ACTION"));
        assert!(
            s.props(AdtRef::ClassAttribute(area))
                .unwrap()
                .properties
                .is_empty()
        );
        assert!(
            s.props(AdtRef::ObjectAttribute(arles, area))
                .unwrap()
                .properties
                .is_empty()
        );
        assert!(
            s.change_state(own, State::Active, State::Hibernating)
                .is_err()
        );
        let mut replayed = Store::memory();
        for c in s.changes() {
            replayed.replay(c);
        }
        assert_eq!(replayed.dump(), s.dump());
        s.change_state(AdtRef::Object(lyon), State::Active, State::Destroyed)
            .unwrap();
        s.purge_adt(AdtRef::Object(lyon)).unwrap();
        assert!(s.props(own).is_none());
        s.change_state(
            AdtRef::ClassAttribute(area),
            State::Active,
            State::Destroyed,
        )
        .unwrap();
        s.purge_adt(AdtRef::ClassAttribute(area)).unwrap();
        assert_eq!(s.get(arles, area), None);
        s.verify().unwrap();
    }
}
