//! The neural engine (project 18, pages 122 to 126, figure 58): a neuron
//! is an object of a class INCLUDEd INTO the system hyperclass NEURON,
//! itself INCLUDEd INTO REFLEX, so that a neuron is a reflex, launched as
//! one (page 126: the neural engine is "essentialy an extension" of the
//! reflex engine). Figure 58 adds an intensity: a stimulus below the
//! detection minimum is ignored, stored up to the maximum storage, passed
//! on capped at the saturation maximum, after the delays for acquisition,
//! processing and transmission. The print names "automated learning"
//! without describing it; ours is the code property LEARNING, run after
//! each firing, THIS the neuron. docs/hbds.md, "Neurons", gives the rules.

use hbds_core::Id;
use hbds_core::same;
use hbds_core::value::{Scalar, Value};
use hbds_gtad::{ClassKind, Store, Stored};

/// The system hyperclass of neuron classes and its attributes: figure
/// 58's three intensities and three delays, what is stored, what the
/// last firing passed on.
pub const NEURON: &str = "NEURON";
pub const DETECTION: &str = "DETECTION";
pub const SATURATION: &str = "SATURATION";
pub const STORAGE: &str = "STORAGE";
pub const ACQUISITION: &str = "ACQUISITION";
pub const PROCESSING: &str = "PROCESSING";
pub const TRANSMISSION: &str = "TRANSMISSION";
pub const LEVEL: &str = "LEVEL";
pub const INPUT: &str = "INPUT";
pub const LEARNING: &str = "LEARNING";

const ATTRIBUTES: [&str; 8] = [
    DETECTION,
    SATURATION,
    STORAGE,
    ACQUISITION,
    PROCESSING,
    TRANSMISSION,
    LEVEL,
    INPUT,
];

pub fn is_system(name: &str) -> bool {
    same(name, NEURON)
}

/// A neuron's learning, kept as a property.
pub fn is_property(key: &str) -> bool {
    key == LEARNING
}

/// Creates what is missing of REFLEX, of NEURON included into it, and of
/// NEURON's attributes.
pub fn install(store: &mut Store) -> Stored<()> {
    hbds_reflexes::install(store)?;
    let reflex = store.find_class(hbds_reflexes::REFLEX).expect("installed");
    let neuron = match store.find_class(NEURON) {
        Some(c) => c,
        None => store.create_class(NEURON, ClassKind::Class)?,
    };
    if !store.members(reflex).contains(&neuron) {
        store.include(neuron, reflex)?;
    }
    for a in ATTRIBUTES {
        if store.find_attribute(neuron, a).is_none() {
            store.create_attribute(neuron, a)?;
        }
    }
    Ok(())
}

/// Whether a reflex is a neuron: its class is in NEURON's lineage.
pub fn is_neuron(store: &Store, reflex: Id) -> bool {
    let Some(neuron) = store.find_class(NEURON) else {
        return false;
    };
    let Some(o) = store.objects().get(&reflex) else {
        return false;
    };
    o.class != neuron && store.lineage(o.class).contains(&neuron)
}

fn number(v: Option<&Value>) -> Option<f64> {
    match v.and_then(Value::scalar)? {
        Scalar::Integer(i) => Some(*i as f64),
        Scalar::Float(x) => Some(*x),
        Scalar::Rational(n, d) => Some(*n as f64 / *d as f64),
        _ => None,
    }
}

/// A neuron's numeric attribute, if it has one.
fn read(store: &Store, neuron: Id, name: &str) -> Option<f64> {
    let class = store.objects().get(&neuron)?.class;
    let a = store.find_attribute(class, name)?;
    number(store.get(neuron, a))
}

fn write(store: &mut Store, neuron: Id, name: &str, x: f64) -> Stored<()> {
    let class = store.objects()[&neuron].class;
    let a = store.find_attribute(class, name).expect("installed");
    store.set(neuron, a, Value::exact(Scalar::Float(x)))
}

/// A stimulus's intensity: the value a `set` gave, if it is a number; 1
/// for any other event.
pub fn intensity(value: Option<&Value>) -> f64 {
    number(value).unwrap_or(1.0)
}

/// A stimulus received: ignored below DETECTION (false), else stored,
/// LEVEL capped at STORAGE.
pub fn stimulate(store: &mut Store, neuron: Id, value: Option<&Value>) -> Stored<bool> {
    let i = intensity(value);
    if read(store, neuron, DETECTION).is_some_and(|d| i < d) {
        return Ok(false);
    }
    let mut level = read(store, neuron, LEVEL).unwrap_or(0.0) + i;
    if let Some(max) = read(store, neuron, STORAGE) {
        level = level.min(max);
    }
    write(store, neuron, LEVEL, level)?;
    Ok(true)
}

/// When a neuron fires after its stimulus: its DELAY and figure 58's
/// three delays, summed.
pub fn delay(store: &Store, neuron: Id) -> f64 {
    let own = [ACQUISITION, PROCESSING, TRANSMISSION]
        .iter()
        .filter_map(|a| read(store, neuron, a))
        .sum::<f64>();
    hbds_reflexes::delay(store, neuron) + own
}

/// A firing: INPUT is what is stored, capped at SATURATION, taken from
/// LEVEL. Nothing stored, it does not fire (false): stimuli summed by an
/// earlier firing.
pub fn discharge(store: &mut Store, neuron: Id) -> Stored<bool> {
    let level = read(store, neuron, LEVEL).unwrap_or(0.0);
    if level <= 0.0 {
        return Ok(false);
    }
    let mut input = level;
    if let Some(max) = read(store, neuron, SATURATION) {
        input = input.min(max);
    }
    write(store, neuron, INPUT, input)?;
    write(store, neuron, LEVEL, level - input)?;
    Ok(true)
}
