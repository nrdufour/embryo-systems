//! The names the system provides, as the polycopié lists them: each with
//! the page that does. The print calls its lists partial ("there are many
//! other multifunctors", page 77; "many other list functors", page 80), so
//! a name missing here is only warned about.

use super::ast::AdtType;

pub struct Functor {
    pub name: &'static str,
    pub page: u16,
    pub takes: Takes,
    pub gives: Gives,
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

use Gives::*;
use Takes::*;

const fn f(name: &'static str, page: u16, takes: Takes, gives: Gives) -> Functor {
    Functor {
        name,
        page,
        takes,
        gives,
    }
}

/// Built-in functors, upper case, with their page, the prefix each takes
/// (that a.d.t. or a list of them) and what each gives (pages 76 and 80).
/// Multifunctors' parameters are not described.
pub const FUNCTORS: &[Functor] = &[
    // Direct functors, page 76.
    f("ATT", 76, Classes, ListOf(AdtType::CAttribute)),
    f("VATT", 76, ClassAttributes, ListOf(AdtType::OAttribute)),
    f("OBJ", 76, Classes, ListOf(AdtType::Object)),
    f("PSC", 76, Classes, ListOf(AdtType::CLink)),
    f("NSC", 76, Classes, ListOf(AdtType::CLink)),
    f("EFF", 76, ClassLinks, ListOf(AdtType::OLink)),
    f("OATT", 76, Objects, ListOf(AdtType::OAttribute)),
    f("OPSC", 76, Objects, ListOf(AdtType::OLink)),
    f("ONSC", 76, Objects, ListOf(AdtType::OLink)),
    // Reverse functors, page 76.
    f("ATTOF", 76, ClassAttributes, ListOf(AdtType::Class)),
    f("VATTOF", 76, ObjectAttributes, ListOf(AdtType::CAttribute)),
    f("OBJOF", 76, Objects, ListOf(AdtType::Class)),
    f("INI", 76, ClassLinks, ListOf(AdtType::Class)),
    f("FIN", 76, ClassLinks, ListOf(AdtType::Class)),
    f("EFFOF", 76, ObjectLinks, ListOf(AdtType::CLink)),
    f("OATTOF", 76, ObjectAttributes, ListOf(AdtType::Object)),
    f("OINI", 76, ObjectLinks, ListOf(AdtType::Object)),
    f("OFIN", 76, ObjectLinks, ListOf(AdtType::Object)),
    // Functors that keep the type, page 76.
    f("CONT", 76, Classes, ListOf(AdtType::Class)),
    f("CONTOF", 76, Classes, ListOf(AdtType::HClass)),
    f("INV", 76, ClassLinks, ListOf(AdtType::CLink)),
    f("OINV", 76, ObjectLinks, ListOf(AdtType::OLink)),
    // Multifunctors, page 77, and SIGMA, page 78.
    f("OPSCR", 77, Objects, ListOf(AdtType::OLink)),
    f("OPSCRC", 77, Objects, ListOf(AdtType::OLink)),
    f("ONSCRC", 77, Objects, ListOf(AdtType::OLink)),
    f("OATTN", 77, Objects, ListOf(AdtType::OAttribute)),
    f("SIGMA", 78, Lists, Unknown),
    // List functors, page 80.
    f("FIRST", 80, Lists, Element),
    f("LAST", 80, Lists, Element),
    f("PRED", 80, Lists, Element),
    f("SUCC", 80, Lists, Element),
    f("CURRENT", 80, Lists, Element),
    f("FIRSTOUT", 80, Lists, Element),
    f("LASTOUT", 80, Lists, Element),
    f("CURRENTOUT", 80, Lists, Element),
    f("POSLP", 80, Lists, Integer),
    f("SUBLIST", 80, Lists, Same),
    f("OCCUR", 80, Lists, Integer),
    f("CARD", 80, Lists, Integer),
    f("KIND", 80, Lists, Unknown),
    f("TYPE", 80, Lists, Unknown),
    f("ELMT", 80, Lists, Element),
    // Expert system functors, page 80.
    f("INFACT", 80, Any, Unknown),
    f("OUTFACT", 80, Any, Unknown),
    f("INRULE", 80, Any, Unknown),
    f("OUTRULE", 80, Any, Unknown),
];

/// Properties of an a.d.t., set with LET: pages 156 to 158.
pub const PROPERTIES: &[(&str, u16)] = &[("COND", 156), ("ACTION", 157)];

pub fn functor(key: &str) -> Option<&'static Functor> {
    FUNCTORS.iter().find(|f| f.name == key)
}

pub fn is_functor(key: &str) -> bool {
    functor(key).is_some()
}

pub fn is_property(key: &str) -> bool {
    PROPERTIES.iter().any(|(name, _)| *name == key)
}
