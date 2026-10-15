//! HBDS, the engine: the a.d.t. a program creates and queries, kept in
//! memory and, given a directory, made durable by a journal; their
//! values, specifications and functors. It names no language: languages
//! (ADT'81 first) depend on it, never the other way round. docs/hbds.md
//! tells how, and what is decided.

use std::fmt;

pub mod functors;
pub mod journal;
pub mod spec;
pub mod store;
pub mod value;

pub use store::{AdtRef, Store};

/// The a.d.t. of HBDS: the six basic ones (page 46) and their extensions
/// (page 47).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdtType {
    Class,
    ClassAttribute,
    ClassLink,
    Object,
    ObjectAttribute,
    ObjectLink,
    HyperClass,
    HyperAttribute,
    HyperLink,
    Embryo,
    Prototype,
    Structure,
}

/// What an a.d.t. is, whatever its exact type: hyperclasses, embryos and
/// prototypes are classes, and so on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdtKind {
    Class,
    Object,
    ClassAttribute,
    ObjectAttribute,
    ClassLink,
    ObjectLink,
}

impl AdtType {
    pub fn kind(self) -> AdtKind {
        match self {
            AdtType::Class
            | AdtType::HyperClass
            | AdtType::Embryo
            | AdtType::Prototype
            | AdtType::Structure => AdtKind::Class,
            AdtType::Object => AdtKind::Object,
            AdtType::ClassAttribute | AdtType::HyperAttribute => AdtKind::ClassAttribute,
            AdtType::ObjectAttribute => AdtKind::ObjectAttribute,
            AdtType::ClassLink | AdtType::HyperLink => AdtKind::ClassLink,
            AdtType::ObjectLink => AdtKind::ObjectLink,
        }
    }
}

impl AdtKind {
    /// With its article: "a class", "an object".
    pub fn a(&self) -> String {
        match self {
            AdtKind::Object | AdtKind::ObjectAttribute | AdtKind::ObjectLink => {
                format!("an {self}")
            }
            _ => format!("a {self}"),
        }
    }

    pub fn plural(&self) -> String {
        match self {
            AdtKind::Class => "classes".into(),
            _ => format!("{self}s"),
        }
    }
}

impl fmt::Display for AdtKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            AdtKind::Class => "class",
            AdtKind::Object => "object",
            AdtKind::ClassAttribute => "class attribute",
            AdtKind::ObjectAttribute => "object attribute",
            AdtKind::ClassLink => "class link",
            AdtKind::ObjectLink => "object link",
        })
    }
}

/// Whether two a.d.t. names are one: they compare folded, as the
/// polycopié's own code needs (`"arc"` reaching `FUNCTION "ARC" .
/// "LENGTH"`, page 127), and are kept as written.
pub fn same(a: &str, b: &str) -> bool {
    a == b || a.to_uppercase() == b.to_uppercase()
}

/// A name folded, for keys.
pub fn fold(name: &str) -> String {
    name.to_uppercase()
}
