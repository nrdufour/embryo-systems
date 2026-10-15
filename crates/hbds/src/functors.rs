//! The functors HBDS provides, as the polycopié lists them (pages 76 to
//! 80), each with its page, what it applies to and what it gives; and
//! their implementation on the store. The print calls its lists partial
//! ("there are many other multifunctors", page 77; "many other list
//! functors", page 80): a language decides what an unknown name means.

use crate::AdtType;
use crate::same;
use crate::store::{AdtRef, Error, Id, ObjectLink, Store, Stored, error, push_new};

/// The struct, where `AdtRef::ObjectLink` is in scope.
type LinkOfObjects = ObjectLink;

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
    f("ATT", 76, Classes, ListOf(AdtType::ClassAttribute)),
    f(
        "VATT",
        76,
        ClassAttributes,
        ListOf(AdtType::ObjectAttribute),
    ),
    f("OBJ", 76, Classes, ListOf(AdtType::Object)),
    f("PSC", 76, Classes, ListOf(AdtType::ClassLink)),
    f("NSC", 76, Classes, ListOf(AdtType::ClassLink)),
    f("EFF", 76, ClassLinks, ListOf(AdtType::ObjectLink)),
    f("OATT", 76, Objects, ListOf(AdtType::ObjectAttribute)),
    f("OPSC", 76, Objects, ListOf(AdtType::ObjectLink)),
    f("ONSC", 76, Objects, ListOf(AdtType::ObjectLink)),
    // Reverse functors, page 76.
    f("ATTOF", 76, ClassAttributes, ListOf(AdtType::Class)),
    f(
        "VATTOF",
        76,
        ObjectAttributes,
        ListOf(AdtType::ClassAttribute),
    ),
    f("OBJOF", 76, Objects, ListOf(AdtType::Class)),
    f("INI", 76, ClassLinks, ListOf(AdtType::Class)),
    f("FIN", 76, ClassLinks, ListOf(AdtType::Class)),
    f("EFFOF", 76, ObjectLinks, ListOf(AdtType::ClassLink)),
    f("OATTOF", 76, ObjectAttributes, ListOf(AdtType::Object)),
    f("OINI", 76, ObjectLinks, ListOf(AdtType::Object)),
    f("OFIN", 76, ObjectLinks, ListOf(AdtType::Object)),
    // Functors that keep the type, page 76.
    f("CONT", 76, Classes, ListOf(AdtType::Class)),
    f("CONTOF", 76, Classes, ListOf(AdtType::HyperClass)),
    f("INV", 76, ClassLinks, ListOf(AdtType::ClassLink)),
    f("OINV", 76, ObjectLinks, ListOf(AdtType::ObjectLink)),
    // Multifunctors, page 77, and SIGMA, page 78.
    f("OPSCR", 77, Objects, ListOf(AdtType::ObjectLink)),
    f("OPSCRC", 77, Objects, ListOf(AdtType::ObjectLink)),
    f("ONSCRC", 77, Objects, ListOf(AdtType::ObjectLink)),
    f("OATTN", 77, Objects, ListOf(AdtType::ObjectAttribute)),
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

impl Store {
    /// A functor of the catalogue (pages 76 to 80) applied to each prefix,
    /// the results united in order. `args` are a multifunctor's strings.
    pub fn functor(&self, name: &str, prefix: &[AdtRef], args: &[String]) -> Stored<Vec<AdtRef>> {
        let mut out = Vec::new();
        for &p in prefix {
            if !self.active(p) {
                continue;
            }
            for r in self.functor_one(name, p, args)? {
                if self.active(r) {
                    push_new(&mut out, r);
                }
            }
        }
        Ok(out)
    }

    fn functor_one(&self, name: &str, p: AdtRef, args: &[String]) -> Stored<Vec<AdtRef>> {
        use AdtRef::*;
        let wrong = || error(format!("`{name}` does not apply to {}", self.describe(p)));
        let arg = |i: usize| -> Stored<&str> {
            args.get(i)
                .map(String::as_str)
                .ok_or_else(|| Error(format!("`{name}` takes {} argument(s)", i + 1)))
        };
        let objects_of = |c: Id| {
            (self.objects.iter())
                .filter(move |(_, o)| o.class == c)
                .map(|(i, _)| Object(*i))
        };
        let attributes_of = |c: Id| {
            (self.attributes.iter())
                .filter(move |(_, a)| a.class == c)
                .map(|(i, _)| *i)
        };
        let links = |f: &dyn Fn(&LinkOfObjects) -> bool| -> Vec<AdtRef> {
            (self.object_links.iter())
                .filter(|(_, l)| f(l))
                .map(|(i, _)| ObjectLink(*i))
                .collect()
        };
        let relation = |l: &LinkOfObjects| {
            self.class_links
                .get(&l.class_link)
                .and_then(|c| c.relation.clone())
        };
        let class_of = |o: Id| {
            self.objects
                .get(&o)
                .map(|o| self.classes[&o.class].name.clone())
        };
        Ok(match (name, p) {
            ("ATT", Class(c)) => attributes_of(c).map(ClassAttribute).collect(),
            ("VATT", ClassAttribute(a)) => {
                let c = self.attributes[&a].class;
                objects_of(c)
                    .map(|o| match o {
                        Object(o) => ObjectAttribute(o, a),
                        _ => unreachable!(),
                    })
                    .collect()
            }
            ("OBJ", Class(c)) => objects_of(c).collect(),
            ("PSC" | "NSC", Class(c)) => (self.class_links.iter())
                .filter(|(_, l)| {
                    if name == "PSC" {
                        l.from == c
                    } else {
                        l.to == c
                    }
                })
                .map(|(i, _)| ClassLink(*i))
                .collect(),
            ("EFF", ClassLink(l)) => links(&|o| o.class_link == l),
            ("OATT", Object(o)) => attributes_of(self.objects[&o].class)
                .map(|a| ObjectAttribute(o, a))
                .collect(),
            ("OPSC", Object(o)) => links(&|l| l.from == o),
            ("ONSC", Object(o)) => links(&|l| l.to == o),
            ("ATTOF", ClassAttribute(a)) => vec![Class(self.attributes[&a].class)],
            ("VATTOF", ObjectAttribute(_, a)) => vec![ClassAttribute(a)],
            ("OBJOF", Object(o)) => vec![Class(self.objects[&o].class)],
            ("INI", ClassLink(l)) => vec![Class(self.class_links[&l].from)],
            ("FIN", ClassLink(l)) => vec![Class(self.class_links[&l].to)],
            ("EFFOF", ObjectLink(l)) => vec![ClassLink(self.object_links[&l].class_link)],
            ("OATTOF", ObjectAttribute(o, _)) => vec![Object(o)],
            ("OINI", ObjectLink(l)) => vec![Object(self.object_links[&l].from)],
            ("OFIN", ObjectLink(l)) => vec![Object(self.object_links[&l].to)],
            ("CONT", Class(h)) => (self.classes.iter())
                .filter(|(_, c)| c.hyper.contains(&h))
                .map(|(i, _)| Class(*i))
                .collect(),
            ("CONTOF", Class(c)) => self.classes[&c].hyper.iter().map(|h| Class(*h)).collect(),
            ("INV", ClassLink(l)) => vec![ClassLink(self.class_links[&l].reverse)],
            ("OINV", ObjectLink(l)) => vec![ObjectLink(self.object_links[&l].reverse)],
            ("OPSCR", Object(o)) => {
                let r = arg(0)?;
                links(&|l| l.from == o && relation(l).is_some_and(|x| same(&x, r)))
            }
            ("OPSCRC", Object(o)) => {
                let (r, c) = (arg(0)?, arg(1)?);
                links(&|l| {
                    l.from == o
                        && relation(l).is_some_and(|x| same(&x, r))
                        && class_of(l.to).is_some_and(|x| same(&x, c))
                })
            }
            ("ONSCRC", Object(o)) => {
                let (r, c) = (arg(0)?, arg(1)?);
                links(&|l| {
                    l.to == o
                        && relation(l).is_some_and(|x| same(&x, r))
                        && class_of(l.from).is_some_and(|x| same(&x, c))
                })
            }
            ("OATTN", Object(o)) => {
                let a = arg(0)?;
                match self.find_attribute(self.objects[&o].class, a) {
                    Some(a) => vec![ObjectAttribute(o, a)],
                    None => Vec::new(),
                }
            }
            _ => return wrong(),
        })
    }
}
