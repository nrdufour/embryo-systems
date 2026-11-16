//! What a change launches, on a bare store.

use hbds_core::value::{Scalar, Value};
use hbds_core::{AdtRef, Id};
use hbds_gtad::{ClassKind, Store};
use hbds_reflexes::*;

/// A class "valve", its attribute "state", its object "v1".
fn valves() -> (Store, Id, Id) {
    let mut store = Store::memory();
    install(&mut store).unwrap();
    let valve = store.create_class("valve", ClassKind::Class).unwrap();
    let state = store.create_attribute(valve, "state").unwrap();
    let v1 = store.create_object(valve, "v1").unwrap();
    (store, state, v1)
}

fn set(store: &mut Store, o: Id, name: &str, v: Scalar) {
    let class = store.objects()[&o].class;
    let a = store.find_attribute(class, name).unwrap();
    store.set(o, a, Value::exact(v)).unwrap();
}

/// A reflex of `class` on `event`, with a REACTION.
fn reflex(store: &mut Store, class: Id, name: &str, event: &str) -> Id {
    let r = store.create_object(class, name).unwrap();
    set(store, r, EVENT, Scalar::String(event.into()));
    (store.set_property(AdtRef::Object(r), REACTION, "fake", "ring")).unwrap();
    r
}

/// A class "watch" INCLUDEd INTO REFLEX.
fn watch(store: &mut Store) -> Id {
    let watch = store.create_class("watch", ClassKind::Class).unwrap();
    let reflex = store.find_class(REFLEX).unwrap();
    store.include(watch, reflex).unwrap();
    watch
}

fn launched(store: &mut Store) -> Vec<(Id, Id)> {
    let launched = store.take_launched();
    launched.iter().map(|l| (l.reflex, l.this)).collect()
}

#[test]
fn a_set_launches_the_reflexes_its_attribute_is_associated_with() {
    let (mut store, state, v1) = valves();
    let watch = watch(&mut store);
    let alarm = reflex(&mut store, watch, "alarm", "set");
    let other = reflex(&mut store, watch, "other", "create");
    for r in [alarm, other] {
        (store.assoc(AdtRef::ClassAttribute(state), AdtRef::Object(r))).unwrap();
    }
    store.take_launched();
    set(&mut store, v1, "state", Scalar::Integer(1));
    assert_eq!(launched(&mut store), [(alarm, v1)]);
    // Its launcher taken back, it is launched no more (page 122).
    (store.dissoc(AdtRef::ClassAttribute(state), AdtRef::Object(alarm))).unwrap();
    set(&mut store, v1, "state", Scalar::Integer(2));
    assert_eq!(launched(&mut store), []);
}

#[test]
fn without_a_reaction_nothing_is_launched() {
    let (mut store, state, v1) = valves();
    let watch = watch(&mut store);
    let mute = store.create_object(watch, "mute").unwrap();
    set(&mut store, mute, EVENT, Scalar::String("set".into()));
    (store.assoc(AdtRef::ClassAttribute(state), AdtRef::Object(mute))).unwrap();
    set(&mut store, v1, "state", Scalar::Integer(1));
    assert_eq!(launched(&mut store), []);
    assert!(is_reflex(&store, mute));
    assert!(!is_reflex(&store, v1));
}

/// What is launched is a reflex, so that what is in time can be named,
/// to cancel it: an object of REFLEX itself was launched and was none.
#[test]
fn an_object_of_reflex_itself_is_a_reflex() {
    let (mut store, state, v1) = valves();
    let class = store.find_class(REFLEX).unwrap();
    let alarm = reflex(&mut store, class, "alarm", "set");
    (store.assoc(AdtRef::ClassAttribute(state), AdtRef::Object(alarm))).unwrap();
    store.take_launched();
    set(&mut store, v1, "state", Scalar::Integer(1));
    assert_eq!(launched(&mut store), [(alarm, v1)]);
    assert!(is_reflex(&store, alarm));
}

#[test]
fn a_delay_is_a_number_or_none() {
    let (mut store, _, _) = valves();
    let watch = watch(&mut store);
    let alarm = reflex(&mut store, watch, "alarm", "set");
    assert_eq!(delay(&store, alarm), 0.0);
    set(&mut store, alarm, DELAY, Scalar::Rational(5, 2));
    assert_eq!(delay(&store, alarm), 2.5);
}
