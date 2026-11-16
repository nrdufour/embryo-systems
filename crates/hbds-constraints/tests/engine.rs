//! The constraints a change concerns, on a bare store.

use hbds_constraints::*;
use hbds_core::value::{Scalar, Value};
use hbds_core::{AdtRef, Id};
use hbds_gtad::{ClassKind, State, Store};

/// A class "gas", its attributes "p" and "t", its object "g1"; a class
/// "law" INCLUDEd INTO CONSTRAINT.
fn gases() -> (Store, [Id; 2], Id, Id) {
    let mut store = Store::memory();
    install(&mut store).unwrap();
    let gas = store.create_class("gas", ClassKind::Class).unwrap();
    let p = store.create_attribute(gas, "p").unwrap();
    let t = store.create_attribute(gas, "t").unwrap();
    let g1 = store.create_object(gas, "g1").unwrap();
    let law = store.create_class("law", ClassKind::Class).unwrap();
    let constraint = store.find_class(CONSTRAINT).unwrap();
    store.include(law, constraint).unwrap();
    store.take_touched();
    (store, [p, t], g1, law)
}

fn formula(store: &mut Store, k: Id) {
    (store.set_property(AdtRef::Object(k), FORMULA, "fake", "holds")).unwrap();
}

#[test]
fn a_change_concerns_the_constraints_its_adt_are_components_of() {
    let (mut store, [p, t], g1, law) = gases();
    let on_t = store.create_object(law, "on t").unwrap();
    let on_g1 = store.create_object(law, "on g1").unwrap();
    for k in [on_t, on_g1] {
        formula(&mut store, k);
    }
    (store.assoc(AdtRef::ClassAttribute(t), AdtRef::Object(on_t))).unwrap();
    (store.assoc(AdtRef::Object(g1), AdtRef::Object(on_g1))).unwrap();
    store.take_touched();
    // A value of p concerns the object, not t's constraint.
    store.set(g1, p, Value::exact(Scalar::Integer(2))).unwrap();
    assert_eq!(store.take_touched(), [on_g1]);
    store.set(g1, t, Value::exact(Scalar::Integer(6))).unwrap();
    assert_eq!(store.take_touched(), [on_t, on_g1]);
}

#[test]
fn without_a_formula_or_hibernating_a_constraint_is_not_concerned() {
    let (mut store, [_, t], g1, law) = gases();
    let mute = store.create_object(law, "mute").unwrap();
    let asleep = store.create_object(law, "asleep").unwrap();
    formula(&mut store, asleep);
    for k in [mute, asleep] {
        (store.assoc(AdtRef::ClassAttribute(t), AdtRef::Object(k))).unwrap();
    }
    (store.change_state(AdtRef::Object(asleep), State::Active, State::Hibernating)).unwrap();
    store.take_touched();
    store.set(g1, t, Value::exact(Scalar::Integer(6))).unwrap();
    assert_eq!(store.take_touched(), []);
}
