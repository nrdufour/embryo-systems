//! The vocabulary the HBDS engines share: values, references to a.d.t.,
//! their types and kinds, specifications, functor signatures. Data only;
//! each engine is a crate of its own on top of it (docs/hbds.md,
//! "Engines").

use std::fmt;

pub mod sha1;
pub mod spec;
pub mod terms;
pub mod value;

pub type Id = u64;

/// How deep calls may nest in one flow of control: the main program, a
/// coroutine, a process's body. The same on every engine that runs
/// modules (docs/architecture.md, decision 2).
pub const CALLS: usize = 1000;

/// An a.d.t., by its id: the six basic ones (page 46).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AdtRef {
    Class(Id),
    Object(Id),
    ClassAttribute(Id),
    /// An object, and its class's attribute.
    ObjectAttribute(Id, Id),
    ClassLink(Id),
    ObjectLink(Id),
}

/// A functor as a catalogue describes it: its name, the page that lists
/// it, what it applies to and what it gives.
pub struct Functor {
    pub name: &'static str,
    pub page: u16,
    pub takes: Takes,
    pub gives: Gives,
}

impl Functor {
    pub const fn new(name: &'static str, page: u16, takes: Takes, gives: Gives) -> Functor {
        Functor {
            name,
            page,
            takes,
            gives,
        }
    }
}

/// What a functor applies to: one of these, or a list of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Takes {
    Classes,
    ClassAttributes,
    ClassLinks,
    Objects,
    ObjectAttributes,
    ObjectLinks,
    /// A list, of anything.
    Lists,
    Any,
}

/// What a functor gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gives {
    ListOf(AdtType),
    Integer,
    /// An element of the prefix list.
    Element,
    /// A list like the prefix.
    Same,
    Unknown,
}

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
