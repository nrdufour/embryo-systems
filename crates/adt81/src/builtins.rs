//! What ADT'81 adds to the names HBDS provides: functions of a real
//! number. The functors and properties are HBDS's ([`hbds::functors`]).

pub use hbds::functors::{
    FUNCTORS, Functor, Gives, PROPERTIES, Takes, functor, is_functor, is_property,
};

/// Functions of a real number: ours, not the print's, which lists none
/// (docs/adt81.md, "Running"). FLOOR, CEIL and ROUND give integers.
pub const FUNCTIONS: &[&str] = &[
    "SQRT", "ABS", "EXP", "LN", "SIN", "COS", "TAN", "ATAN", "FLOOR", "CEIL", "ROUND",
];

pub fn is_function(key: &str) -> bool {
    FUNCTIONS.contains(&key)
}
