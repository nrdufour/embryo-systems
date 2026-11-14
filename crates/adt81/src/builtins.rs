//! The names ADT'81 knows: the functors of the engines, on a.d.t.
//! (hbds-functors), on lists (hbds-lists) and on rules and facts
//! (hbds-rules), the properties GTAD and SES keep, the expert-system
//! engines, and the functions of a real number, which are ADT'81's own.

pub use hbds_core::{Functor, Gives, Takes};
pub use hbds_gtad::PROPERTIES;

/// A functor of an engine's catalogue.
pub fn functor(key: &str) -> Option<&'static Functor> {
    (hbds_functors::functor(key))
        .or_else(|| hbds_lists::functor(key))
        .or_else(|| hbds_rules::functor(key))
}

/// COND and ACTION (GTAD's), a rule's parts and a fact's parameters
/// (SES's).
pub fn is_property(key: &str) -> bool {
    hbds_gtad::is_property(key)
        || hbds_rules::is_property(key)
        || hbds_reflexes::is_property(key)
        || hbds_constraints::is_property(key)
        || hbds_dialog::is_property(key)
}

/// The expert-system engines (page 106) and the dialog engine (figure
/// 72), CALLed as subroutines.
pub const SUBROUTINES: &[&str] = &["CHAV", "HYPOCHAV", "DIAL"];

pub fn is_subroutine(key: &str) -> bool {
    SUBROUTINES.contains(&key)
}

pub fn is_functor(key: &str) -> bool {
    functor(key).is_some()
}

/// Functions of a real number: ours, not the print's, which lists none
/// (docs/adt81.md, "Running"). FLOOR, CEIL and ROUND give integers.
pub const FUNCTIONS: &[&str] = &[
    "SQRT", "ABS", "EXP", "LN", "SIN", "COS", "TAN", "ATAN", "FLOOR", "CEIL", "ROUND",
];

/// Names of values ADT'81 knows: TIME, the simulation's (SIMULA's
/// `time`), which a variable of that name hides. Ours, not the print's.
pub const VALUES: &[&str] = &["TIME"];

pub fn is_value(key: &str) -> bool {
    VALUES.contains(&key)
}

pub fn is_function(key: &str) -> bool {
    FUNCTIONS.contains(&key)
}
