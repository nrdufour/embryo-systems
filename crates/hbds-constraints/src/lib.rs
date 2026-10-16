//! The constraint engine (pages 157 to 160): a constraint is an object of
//! a class in the system hyperclass CONSTRAINT, carrying a FORMULA, "an
//! algorithm" as a process's body is (page 159, figure 79). Its
//! components are the a.d.t. associated with it, `ASSOC "water : arc"
//! WITH c ["crossing"]` (page 159: "any a.d.t. may be associated with
//! another"). The engine tells, of each change, the constraints whose
//! components it concerns; running their FORMULAs at the end of the
//! statement, which they may refuse, is a language's. docs/hbds.md,
//! "Constraints", gives the rules.

use std::collections::BTreeSet;

use hbds_core::{AdtRef, Id, same};
use hbds_gtad::{Change, ClassKind, State, Store, Stored};

/// The system hyperclass of constraint classes, and its code.
pub const CONSTRAINT: &str = "CONSTRAINT";
pub const FORMULA: &str = "FORMULA";

/// The name, in a FORMULA, of its verdict: true unless it says otherwise.
pub const VERDICT: &str = "formula";

/// How many FORMULAs one statement may run, what they change running
/// others: past it, a cycle (as CHAV's limit, docs/hbds.md).
pub const LIMIT: usize = 10_000;

pub fn is_system(name: &str) -> bool {
    same(name, CONSTRAINT)
}

/// A constraint's code, kept as a property.
pub fn is_property(key: &str) -> bool {
    key == FORMULA
}

/// Creates CONSTRAINT if it is missing.
pub fn install(store: &mut Store) -> Stored<()> {
    if store.find_class(CONSTRAINT).is_none() {
        store.create_class(CONSTRAINT, ClassKind::Class)?;
    }
    store.check_with(touched);
    Ok(())
}

/// Whether the store has CONSTRAINT, read without a lookup counted (the
/// benchmarks' work stays the same where there are no constraints).
pub fn installed(store: &Store) -> bool {
    store
        .classes()
        .iter()
        .any(|(_, c)| same(&c.name, CONSTRAINT))
}

/// The a.d.t. a change concerns, as components: an object created,
/// destroyed or set concerns itself and its classes, a value set its
/// class attribute too; a link asserted or retracted its class link and
/// its two objects. Read before the change is made.
fn concerned(store: &Store, change: &Change) -> Vec<AdtRef> {
    let classes = |o: Id, class: Option<Id>| {
        let class = class.or_else(|| store.objects().get(&o).map(|o| o.class));
        let mut out: Vec<AdtRef> = (class.into_iter())
            .flat_map(|c| store.lineage(c))
            .map(AdtRef::Class)
            .collect();
        out.push(AdtRef::Object(o));
        out
    };
    match change {
        Change::Object { id, class, .. } => classes(*id, Some(*class)),
        Change::State {
            target: AdtRef::Object(o),
            state: State::Destroyed,
        } => classes(*o, None),
        Change::Set {
            object, attribute, ..
        } => {
            let mut out = classes(*object, None);
            out.push(AdtRef::ClassAttribute(*attribute));
            out
        }
        Change::ObjectLink {
            class_link,
            from,
            to,
            ..
        } => vec![
            AdtRef::ClassLink(*class_link),
            AdtRef::Object(*from),
            AdtRef::Object(*to),
        ],
        Change::Unlink { id } => match store.object_links().get(id) {
            Some(l) => vec![
                AdtRef::ClassLink(l.class_link),
                AdtRef::Object(l.from),
                AdtRef::Object(l.to),
            ],
            None => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// The store's checker: the constraints `change` concerns. A constraint
/// is concerned if it is active, has a FORMULA, and one of its components
/// is an a.d.t. the change concerns.
pub fn touched(store: &Store, change: &Change, out: &mut BTreeSet<Id>) {
    let concerned = concerned(store, change);
    if concerned.is_empty() {
        return;
    }
    let Some(constraint) = store.find_class(CONSTRAINT) else {
        return;
    };
    let mut classes = vec![constraint];
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
        for k in store.objects_of(c) {
            let o = &store.objects()[&k];
            let concerns = (store.associations().iter())
                .any(|(a, b)| *b == AdtRef::Object(k) && concerned.contains(a));
            if o.state == State::Active && concerns && o.props.properties.contains_key(FORMULA) {
                out.insert(k);
            }
        }
    }
}
