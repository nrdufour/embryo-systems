//! The reflex engine (project 18, pages 116 to 126): a reflex is the
//! reaction of an a.d.t., its launcher, to an event; the reflexes of an
//! a.d.t. are its behavior (figure 53). A reflex is an object of a class
//! in the system hyperclass REFLEX, as a rule is in RULE: its EVENT, its
//! DELAY, and its code, the property REACTION, THIS the object concerned.
//! Its launchers are the a.d.t. associated with it, `ASSOC "point" . "x"
//! WITH "w" ["watch"]`, figure 53's dotted lines, which DISSOC takes back
//! (page 122: launchers change along a life). The engine tells, of each
//! change, the reflexes it launches; running them, queued after the
//! launching statement, is a language's. docs/hbds.md, "Reflexes", gives
//! the rules.

use hbds_core::value::{Scalar, Value};
use hbds_core::{AdtRef, Id, same};
use hbds_gtad::{Change, ClassKind, Launch, State, Store, Stored};

/// The system hyperclass of reflex classes, its attributes, its code.
pub const REFLEX: &str = "REFLEX";
pub const EVENT: &str = "EVENT";
pub const DELAY: &str = "DELAY";
pub const REACTION: &str = "REACTION";

/// What launches a reflex: a launcher class's objects created or
/// destroyed, a class attribute's values set, a class link's links
/// asserted or retracted.
pub const EVENTS: &[&str] = &["create", "destroy", "set", "assert", "retract"];

/// How many reflexes one statement may launch, all generations counted:
/// past it, a cycle (as CHAV's limit, docs/hbds.md).
pub const LIMIT: usize = 10_000;

pub fn is_system(name: &str) -> bool {
    same(name, REFLEX)
}

/// A reflex's code, kept as a property.
pub fn is_property(key: &str) -> bool {
    key == REACTION
}

/// Creates what is missing of REFLEX and its attributes.
pub fn install(store: &mut Store) -> Stored<()> {
    let reflex = match store.find_class(REFLEX) {
        Some(c) => c,
        None => store.create_class(REFLEX, ClassKind::Class)?,
    };
    for a in [EVENT, DELAY] {
        if store.find_attribute(reflex, a).is_none() {
            store.create_attribute(reflex, a)?;
        }
    }
    store.watch(launches);
    Ok(())
}

/// Whether an object is a reflex: its class is in REFLEX's lineage.
pub fn is_reflex(store: &Store, o: Id) -> bool {
    let (Some(reflex), Some(object)) = (store.find_class(REFLEX), store.objects().get(&o)) else {
        return false;
    };
    object.class != reflex && store.lineage(object.class).contains(&reflex)
}

/// Whether the store has REFLEX, read without a lookup counted (the
/// benchmarks' work stays the same where there are no reflexes).
pub fn installed(store: &Store) -> bool {
    store.classes().iter().any(|(_, c)| same(&c.name, REFLEX))
}

/// What a change is to the reflexes: its event, the launchers it may
/// wake, the object concerned, the value set. Read before the change is
/// made.
type Event = (&'static str, Vec<AdtRef>, Id, Option<Value>);

fn event(store: &Store, change: &Change) -> Option<Event> {
    let classes = |c: Id| store.lineage(c).into_iter().map(AdtRef::Class).collect();
    match change {
        Change::Object { id, class, .. } => Some(("create", classes(*class), *id, None)),
        Change::State {
            target: AdtRef::Object(o),
            state: State::Destroyed,
        } => Some(("destroy", classes(store.objects().get(o)?.class), *o, None)),
        Change::Set {
            object,
            attribute,
            value,
        } => Some((
            "set",
            vec![AdtRef::ClassAttribute(*attribute)],
            *object,
            Some(value.clone()),
        )),
        Change::ObjectLink {
            class_link, from, ..
        } => Some(("assert", vec![AdtRef::ClassLink(*class_link)], *from, None)),
        Change::Unlink { id } => {
            let l = store.object_links().get(id)?;
            Some((
                "retract",
                vec![AdtRef::ClassLink(l.class_link)],
                l.from,
                None,
            ))
        }
        _ => None,
    }
}

/// The store's watcher: the reflexes `change` launches, oldest first,
/// each with the object it concerns and the value set. A reflex launches if it is active,
/// its EVENT is the change's, one of its launchers is an a.d.t. the
/// change wakes, and it has a REACTION.
pub fn launches(store: &Store, change: &Change, out: &mut Vec<Launch>) {
    let Some((event, launchers, this, value)) = event(store, change) else {
        return;
    };
    let Some(reflex) = store.find_class(REFLEX) else {
        return;
    };
    let mut found = Vec::new();
    let mut classes = vec![reflex];
    let mut i = 0;
    while i < classes.len() {
        for c in store.members(classes[i]) {
            if !classes.contains(&c) {
                classes.push(c);
            }
        }
        i += 1;
    }
    for c in classes {
        for r in store.objects_of(c) {
            let o = &store.objects()[&r];
            if o.state != State::Active {
                continue;
            }
            let value = |name| {
                let a = store.find_attribute(o.class, name)?;
                store.get(r, a).cloned()
            };
            let wakes = (store.associations().iter())
                .any(|(l, b)| *b == AdtRef::Object(r) && launchers.contains(l));
            let fits = matches!(
                value(EVENT).as_ref().and_then(Value::scalar),
                Some(Scalar::String(e)) if same(e, event)
            );
            let code = o.props.properties.contains_key(REACTION);
            if wakes && fits && code {
                found.push(r);
            }
        }
    }
    found.sort();
    found.dedup();
    out.extend(found.into_iter().map(|reflex| Launch {
        reflex,
        this,
        value: value.clone(),
    }));
}

/// A reflex's DELAY: 0 if it has none.
pub fn delay(store: &Store, reflex: Id) -> f64 {
    let Some(o) = store.objects().get(&reflex) else {
        return 0.0;
    };
    let a = store.find_attribute(o.class, DELAY);
    match a.and_then(|a| store.get(reflex, a)).and_then(Value::scalar) {
        Some(Scalar::Integer(i)) => *i as f64,
        Some(Scalar::Float(x)) => *x,
        Some(Scalar::Rational(n, d)) => *n as f64 / *d as f64,
        _ => 0.0,
    }
}
