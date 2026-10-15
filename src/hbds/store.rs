//! The HBDS model, what the print calls GTAD (pages 45 to 48): classes,
//! their attributes and links, objects, their values and links, kept in
//! memory, every mutation one [`Change`] so that a journal can record and
//! replay it ([`journal`](super::journal)). docs/hbds.md gives the rules.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::journal::Journal;
use crate::adt81::value::Value;

pub type Id = u64;

/// An a.d.t., by its id: the six basic ones (page 46).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AdtRef {
    Class(Id),
    Object(Id),
    ClassAttribute(Id),
    /// An object, and its class's attribute.
    ObjectAttribute(Id, Id),
    ClassLink(Id),
    ObjectLink(Id),
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
/// LET sets (COND, ACTION, pages 156 to 158).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Props {
    pub spec: Option<String>,
    pub unit: Option<String>,
    pub computed: bool,
    pub properties: BTreeMap<String, String>,
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

/// The struct, where `AdtRef::ObjectLink` is in scope.
type LinkOfObjects = ObjectLink;

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
        spec: Option<String>,
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

fn error<T>(message: impl Into<String>) -> Stored<T> {
    Err(Error(message.into()))
}

#[derive(Default)]
pub struct Store {
    next: Id,
    pub classes: BTreeMap<Id, Class>,
    pub attributes: BTreeMap<Id, Attribute>,
    pub objects: BTreeMap<Id, Object>,
    pub class_links: BTreeMap<Id, ClassLink>,
    pub object_links: BTreeMap<Id, ObjectLink>,
    pub associations: Vec<(AdtRef, AdtRef)>,
    journal: Option<Journal>,
}

/// Results in order, without repeats.
fn push_new(out: &mut Vec<AdtRef>, r: AdtRef) {
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

    /// A store that records every change in `journal`.
    pub(super) fn journaled(journal: Journal) -> Store {
        Store {
            next: 1,
            journal: Some(journal),
            ..Store::default()
        }
    }

    /// Takes `other`'s model, keeping this store's journal: a store
    /// replayed, then journaled.
    pub(super) fn take(&mut self, other: Store) {
        let journal = self.journal.take();
        *self = Store { journal, ..other };
    }

    fn fresh(&mut self) -> Id {
        let id = self.next;
        self.next += 1;
        id
    }

    /// Records `change` in the journal, if there is one, then applies it.
    fn commit(&mut self, change: Change) -> Stored<()> {
        if let Some(journal) = &mut self.journal {
            journal.record(&change)?;
        }
        self.apply(change);
        Ok(())
    }

    /// Applies a change already checked: the one path for every mutation,
    /// live or replayed.
    pub(super) fn apply(&mut self, change: Change) {
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
            Change::Property { target, name, text } => {
                if let Some(p) = self.props_mut(target) {
                    p.properties.insert(name, text);
                }
            }
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
                for o in self.objects.values_mut() {
                    o.values.remove(&id);
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
    fn active(&self, r: AdtRef) -> bool {
        let on = |r| self.state(r) == Some(State::Active);
        match r {
            AdtRef::Class(_) => on(r),
            AdtRef::Object(id) => on(r) && on(AdtRef::Class(self.objects[&id].class)),
            AdtRef::ClassAttribute(id) => on(r) && on(AdtRef::Class(self.attributes[&id].class)),
            AdtRef::ObjectAttribute(o, a) => {
                self.active(AdtRef::Object(o)) && self.active(AdtRef::ClassAttribute(a))
            }
            AdtRef::ClassLink(id) => {
                let l = &self.class_links[&id];
                on(r) && on(AdtRef::Class(l.from)) && on(AdtRef::Class(l.to))
            }
            AdtRef::ObjectLink(id) => {
                let l = &self.object_links[&id];
                on(r) && self.active(AdtRef::Object(l.from)) && self.active(AdtRef::Object(l.to))
            }
        }
    }

    /// A class by name, whatever its state.
    pub fn find_class(&self, name: &str) -> Option<Id> {
        self.classes
            .iter()
            .find(|(_, c)| c.name == name)
            .map(|(i, _)| *i)
    }

    /// An active class by name.
    pub fn class(&self, name: &str) -> Stored<Id> {
        match self.find_class(name) {
            Some(id) if self.active(AdtRef::Class(id)) => Ok(id),
            Some(_) => error(format!("the class \"{name}\" is not active")),
            None => error(format!("there is no class \"{name}\"")),
        }
    }

    pub fn find_object(&self, class: Id, name: &str) -> Option<Id> {
        (self.objects.iter())
            .find(|(_, o)| o.class == class && o.name == name)
            .map(|(i, _)| *i)
    }

    pub fn object(&self, class: Id, name: &str) -> Stored<Id> {
        match self.find_object(class, name) {
            Some(id) if self.active(AdtRef::Object(id)) => Ok(id),
            Some(_) => error(format!("the object \"{name}\" is not active")),
            None => error(format!(
                "there is no object \"{name}\" in \"{}\"",
                self.classes[&class].name
            )),
        }
    }

    pub fn find_attribute(&self, class: Id, name: &str) -> Option<Id> {
        (self.attributes.iter())
            .find(|(_, a)| a.class == class && a.name == name)
            .map(|(i, _)| *i)
    }

    pub fn attribute(&self, class: Id, name: &str) -> Stored<Id> {
        match self.find_attribute(class, name) {
            Some(id) if self.active(AdtRef::ClassAttribute(id)) => Ok(id),
            _ => error(format!(
                "the class \"{}\" has no attribute \"{name}\"",
                self.classes[&class].name
            )),
        }
    }

    pub fn find_class_link(&self, from: Id, relation: &str, to: Id) -> Option<Id> {
        (self.class_links.iter())
            .find(|(_, l)| l.from == from && l.to == to && l.relation.as_deref() == Some(relation))
            .map(|(i, _)| *i)
    }

    pub fn class_link(&self, from: Id, relation: &str, to: Id) -> Stored<Id> {
        match self.find_class_link(from, relation, to) {
            Some(id) if self.active(AdtRef::ClassLink(id)) => Ok(id),
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
        (self.object_links.iter())
            .find(|(_, l)| l.class_link == class_link && l.from == from && l.to == to)
            .filter(|(i, _)| self.active(AdtRef::ObjectLink(**i)))
            .map(|(i, _)| *i)
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
        if self.find_attribute(class, name).is_some() {
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
        if self.find_class_link(from, relation, to).is_some() {
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

    pub fn set(&mut self, object: Id, attribute: Id, value: Value) -> Stored<()> {
        self.commit(Change::Set {
            object,
            attribute,
            value,
        })
    }

    pub fn specify(
        &mut self,
        target: AdtRef,
        spec: Option<String>,
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

    pub fn set_property(&mut self, target: AdtRef, name: &str, text: &str) -> Stored<()> {
        if matches!(target, AdtRef::ObjectLink(_)) {
            return error("an object link carries no property; its class link does");
        }
        self.commit(Change::Property {
            target,
            name: name.to_string(),
            text: text.to_string(),
        })
    }

    // Functors

    /// A functor of the catalogue (pages 76 to 80) applied to each prefix,
    /// the results united in order. `args` are a multifunctor's strings.
    pub fn functor(&self, name: &str, prefix: &[AdtRef], args: &[String]) -> Stored<Vec<AdtRef>> {
        let mut out = Vec::new();
        for &p in prefix {
            if !self.active(p) {
                continue;
            }
            for r in self.functor_one(name, p, args)? {
                if self.active(r) {
                    push_new(&mut out, r);
                }
            }
        }
        Ok(out)
    }

    fn functor_one(&self, name: &str, p: AdtRef, args: &[String]) -> Stored<Vec<AdtRef>> {
        use AdtRef::*;
        let wrong = || error(format!("`{name}` does not apply to {}", self.describe(p)));
        let arg = |i: usize| -> Stored<&str> {
            args.get(i)
                .map(String::as_str)
                .ok_or_else(|| Error(format!("`{name}` takes {} argument(s)", i + 1)))
        };
        let objects_of = |c: Id| {
            (self.objects.iter())
                .filter(move |(_, o)| o.class == c)
                .map(|(i, _)| Object(*i))
        };
        let attributes_of = |c: Id| {
            (self.attributes.iter())
                .filter(move |(_, a)| a.class == c)
                .map(|(i, _)| *i)
        };
        let links = |f: &dyn Fn(&LinkOfObjects) -> bool| -> Vec<AdtRef> {
            (self.object_links.iter())
                .filter(|(_, l)| f(l))
                .map(|(i, _)| ObjectLink(*i))
                .collect()
        };
        let relation = |l: &LinkOfObjects| {
            self.class_links
                .get(&l.class_link)
                .and_then(|c| c.relation.clone())
        };
        let class_of = |o: Id| {
            self.objects
                .get(&o)
                .map(|o| self.classes[&o.class].name.clone())
        };
        Ok(match (name, p) {
            ("ATT", Class(c)) => attributes_of(c).map(ClassAttribute).collect(),
            ("VATT", ClassAttribute(a)) => {
                let c = self.attributes[&a].class;
                objects_of(c)
                    .map(|o| match o {
                        Object(o) => ObjectAttribute(o, a),
                        _ => unreachable!(),
                    })
                    .collect()
            }
            ("OBJ", Class(c)) => objects_of(c).collect(),
            ("PSC" | "NSC", Class(c)) => (self.class_links.iter())
                .filter(|(_, l)| {
                    if name == "PSC" {
                        l.from == c
                    } else {
                        l.to == c
                    }
                })
                .map(|(i, _)| ClassLink(*i))
                .collect(),
            ("EFF", ClassLink(l)) => links(&|o| o.class_link == l),
            ("OATT", Object(o)) => attributes_of(self.objects[&o].class)
                .map(|a| ObjectAttribute(o, a))
                .collect(),
            ("OPSC", Object(o)) => links(&|l| l.from == o),
            ("ONSC", Object(o)) => links(&|l| l.to == o),
            ("ATTOF", ClassAttribute(a)) => vec![Class(self.attributes[&a].class)],
            ("VATTOF", ObjectAttribute(_, a)) => vec![ClassAttribute(a)],
            ("OBJOF", Object(o)) => vec![Class(self.objects[&o].class)],
            ("INI", ClassLink(l)) => vec![Class(self.class_links[&l].from)],
            ("FIN", ClassLink(l)) => vec![Class(self.class_links[&l].to)],
            ("EFFOF", ObjectLink(l)) => vec![ClassLink(self.object_links[&l].class_link)],
            ("OATTOF", ObjectAttribute(o, _)) => vec![Object(o)],
            ("OINI", ObjectLink(l)) => vec![Object(self.object_links[&l].from)],
            ("OFIN", ObjectLink(l)) => vec![Object(self.object_links[&l].to)],
            ("CONT", Class(h)) => (self.classes.iter())
                .filter(|(_, c)| c.hyper.contains(&h))
                .map(|(i, _)| Class(*i))
                .collect(),
            ("CONTOF", Class(c)) => self.classes[&c].hyper.iter().map(|h| Class(*h)).collect(),
            ("INV", ClassLink(l)) => vec![ClassLink(self.class_links[&l].reverse)],
            ("OINV", ObjectLink(l)) => vec![ObjectLink(self.object_links[&l].reverse)],
            ("OPSCR", Object(o)) => {
                let r = arg(0)?;
                links(&|l| l.from == o && relation(l).as_deref() == Some(r))
            }
            ("OPSCRC", Object(o)) => {
                let (r, c) = (arg(0)?, arg(1)?);
                links(&|l| {
                    l.from == o
                        && relation(l).as_deref() == Some(r)
                        && class_of(l.to).as_deref() == Some(c)
                })
            }
            ("ONSCRC", Object(o)) => {
                let (r, c) = (arg(0)?, arg(1)?);
                links(&|l| {
                    l.to == o
                        && relation(l).as_deref() == Some(r)
                        && class_of(l.from).as_deref() == Some(c)
                })
            }
            ("OATTN", Object(o)) => {
                let a = arg(0)?;
                match self.find_attribute(self.objects[&o].class, a) {
                    Some(a) => vec![ObjectAttribute(o, a)],
                    None => Vec::new(),
                }
            }
            _ => return wrong(),
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
            for (k, v) in &p.properties {
                t += &format!(" {k} = \"{v}\"");
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

    fn names(s: &Store, refs: &[AdtRef]) -> Vec<String> {
        refs.iter().map(|r| s.describe(*r)).collect()
    }

    fn apply(s: &Store, name: &str, prefix: &[AdtRef], args: &[&str]) -> Vec<String> {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        names(s, &s.functor(name, prefix, &args).unwrap())
    }

    #[test]
    fn functors() {
        let (s, i) = world();
        assert_eq!(
            apply(&s, "OBJ", &[Class(i["TOWN"])], &[]),
            ["Lyon[TOWN]", "Arles[TOWN]"]
        );
        assert_eq!(apply(&s, "ATT", &[Class(i["TOWN"])], &[]), ["TOWN.AREA"]);
        assert_eq!(
            apply(&s, "VATT", &[ClassAttribute(i["AREA"])], &[]),
            ["Lyon[TOWN].AREA", "Arles[TOWN].AREA"]
        );
        assert_eq!(
            apply(&s, "PSC", &[Class(i["RIVER"])], &[]),
            ["RIVER \"crosses\" TOWN", "RIVER (unnamed) DAM"]
        );
        assert_eq!(
            apply(&s, "NSC", &[Class(i["RIVER"])], &[]),
            ["TOWN \"crossed by\" RIVER", "DAM \"feeds\" RIVER"]
        );
        assert_eq!(apply(&s, "EFF", &[ClassLink(i["crosses"])], &[]).len(), 2);
        assert_eq!(
            apply(&s, "OPSCRC", &[Object(i["Rhone"])], &["crosses", "TOWN"]),
            [
                "Rhone[RIVER] \"crosses\" Lyon[TOWN]",
                "Rhone[RIVER] \"crosses\" Arles[TOWN]"
            ]
        );
        let lyon = [Object(i["Lyon"])];
        let crossed = s
            .functor("OPSCRC", &lyon, &["crossed by".into(), "RIVER".into()])
            .unwrap();
        assert_eq!(apply(&s, "OFIN", &crossed, &[]), ["Rhone[RIVER]"]);
        assert_eq!(apply(&s, "OINI", &crossed, &[]), ["Lyon[TOWN]"]);
        let rhone = [Object(i["Rhone"])];
        let fed = s
            .functor("ONSCRC", &rhone, &["feeds".into(), "DAM".into()])
            .unwrap();
        assert_eq!(apply(&s, "OINI", &fed, &[]), ["Vouglans[DAM]"]);
        assert_eq!(
            apply(&s, "OATTN", &[Object(i["Vouglans"])], &["VOL"]),
            ["Vouglans[DAM].VOL"]
        );
        assert_eq!(apply(&s, "OBJOF", &rhone, &[]), ["RIVER"]);
        assert_eq!(
            apply(&s, "INV", &[ClassLink(i["crosses"])], &[]),
            ["TOWN \"crossed by\" RIVER"]
        );
        assert_eq!(
            apply(&s, "CONT", &[Class(i["HYDRO"])], &[]),
            ["TOWN", "RIVER"]
        );
        assert_eq!(apply(&s, "CONTOF", &[Class(i["TOWN"])], &[]), ["HYDRO"]);
        // A list prefix: results united, in order, without repeats.
        assert_eq!(
            apply(
                &s,
                "OBJOF",
                &[Object(i["Lyon"]), Object(i["Arles"]), Object(i["Rhone"])],
                &[]
            ),
            ["TOWN", "RIVER"]
        );
        assert!(s.functor("OBJ", &rhone, &[]).is_err());
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
    fn states() {
        let (mut s, i) = world();
        let lyon = Object(i["Lyon"]);
        s.change_state(lyon, State::Active, State::Hibernating)
            .unwrap();
        assert_eq!(apply(&s, "OBJ", &[Class(i["TOWN"])], &[]), ["Arles[TOWN]"]);
        assert!(s.purge_adt(lyon).is_err());
        s.change_state(lyon, State::Hibernating, State::Active)
            .unwrap();
        s.change_state(lyon, State::Active, State::Destroyed)
            .unwrap();
        assert!(
            s.change_state(lyon, State::Hibernating, State::Active)
                .is_err()
        );
        s.purge_adt(lyon).unwrap();
        assert_eq!(apply(&s, "OBJ", &[Class(i["TOWN"])], &[]), ["Arles[TOWN]"]);
        // Its links went with it.
        assert_eq!(
            apply(&s, "OPSC", &[Object(i["Rhone"])], &[]),
            [
                "Rhone[RIVER] \"crosses\" Arles[TOWN]",
                "Rhone[RIVER] (unnamed) Vouglans[DAM]"
            ]
        );
        assert!(s.object(i["TOWN"], "Lyon").is_err());
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
}
