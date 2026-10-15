//! The names ADT'81 knows: the functors of the engines, on a.d.t.
//! (hbds-functors) and on lists (hbds-lists), the properties GTAD keeps,
//! and the functions of a real number, which are ADT'81's own.

pub use hbds_core::{Functor, Gives, Takes};
pub use hbds_gtad::{PROPERTIES, is_property};

/// A functor of either engine's catalogue.
pub fn functor(key: &str) -> Option<&'static Functor> {
    hbds_functors::functor(key).or_else(|| hbds_lists::functor(key))
}

pub fn is_functor(key: &str) -> bool {
    functor(key).is_some()
}

/// Functions of a real number: ours, not the print's, which lists none
/// (docs/adt81.md, "Running"). FLOOR, CEIL and ROUND give integers.
pub const FUNCTIONS: &[&str] = &[
    "SQRT", "ABS", "EXP", "LN", "SIN", "COS", "TAN", "ATAN", "FLOOR", "CEIL", "ROUND",
];

pub fn is_function(key: &str) -> bool {
    FUNCTIONS.contains(&key)
}
