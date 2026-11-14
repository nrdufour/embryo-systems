//! Figure 58's intensities on a bare store.

use hbds_core::Id;
use hbds_core::value::{Scalar, Value};
use hbds_gtad::Store;
use hbds_neurons::*;

fn neuron() -> (Store, Id) {
    let mut store = Store::memory();
    install(&mut store).unwrap();
    let c = store
        .create_class("cell", hbds_gtad::ClassKind::Class)
        .unwrap();
    let neuron = store.find_class(NEURON).unwrap();
    store.include(c, neuron).unwrap();
    let n = store.create_object(c, "n").unwrap();
    (store, n)
}

fn set(store: &mut Store, n: Id, name: &str, x: f64) {
    let class = store.objects()[&n].class;
    let a = store.find_attribute(class, name).unwrap();
    store.set(n, a, Value::exact(Scalar::Float(x))).unwrap();
}

fn get(store: &Store, n: Id, name: &str) -> String {
    let class = store.objects()[&n].class;
    let a = store.find_attribute(class, name).unwrap();
    store.get(n, a).map(|v| store.show(v)).unwrap_or_default()
}

fn x(i: f64) -> Value {
    Value::exact(Scalar::Float(i))
}

#[test]
fn a_neuron_is_a_reflex() {
    let (store, n) = neuron();
    assert!(is_neuron(&store, n));
    let reflex = store.find_class(hbds_reflexes::REFLEX).unwrap();
    let c = store.objects()[&n].class;
    assert!(store.lineage(c).contains(&reflex));
}

#[test]
fn below_detection_a_stimulus_is_ignored() {
    let (mut store, n) = neuron();
    set(&mut store, n, DETECTION, 2.0);
    assert!(!stimulate(&mut store, n, Some(&x(1.5))).unwrap());
    assert!(!discharge(&mut store, n).unwrap());
    assert!(stimulate(&mut store, n, Some(&x(2.0))).unwrap());
    assert!(discharge(&mut store, n).unwrap());
    assert_eq!(get(&store, n, INPUT), "2.0");
}

#[test]
fn storage_caps_the_level_and_saturation_the_input() {
    let (mut store, n) = neuron();
    set(&mut store, n, STORAGE, 10.0);
    set(&mut store, n, SATURATION, 4.0);
    stimulate(&mut store, n, Some(&x(7.0))).unwrap();
    stimulate(&mut store, n, Some(&x(7.0))).unwrap();
    assert_eq!(get(&store, n, LEVEL), "10.0");
    assert!(discharge(&mut store, n).unwrap());
    assert_eq!(get(&store, n, INPUT), "4.0");
    assert_eq!(get(&store, n, LEVEL), "6.0");
}

#[test]
fn stimuli_sum_into_one_firing() {
    let (mut store, n) = neuron();
    stimulate(&mut store, n, Some(&x(1.0))).unwrap();
    stimulate(&mut store, n, None).unwrap();
    assert!(discharge(&mut store, n).unwrap());
    assert_eq!(get(&store, n, INPUT), "2.0");
    assert!(!discharge(&mut store, n).unwrap());
}

#[test]
fn the_delays_sum() {
    let (mut store, n) = neuron();
    assert_eq!(delay(&store, n), 0.0);
    set(&mut store, n, ACQUISITION, 1.0);
    set(&mut store, n, PROCESSING, 2.0);
    set(&mut store, n, TRANSMISSION, 0.5);
    let class = store.objects()[&n].class;
    let d = store.find_attribute(class, hbds_reflexes::DELAY).unwrap();
    store.set(n, d, Value::exact(Scalar::Integer(1))).unwrap();
    assert_eq!(delay(&store, n), 4.5);
}

#[test]
fn a_non_number_counts_as_one() {
    assert_eq!(
        intensity(Some(&Value::exact(Scalar::String("x".into())))),
        1.0
    );
    assert_eq!(intensity(None), 1.0);
}
