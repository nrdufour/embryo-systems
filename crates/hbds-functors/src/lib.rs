//! The functor engine (project 15, pages 71 to 84): the functors on a.d.t.
//! the polycopié lists (pages 76 to 80), each with its page, what it
//! applies to and what it gives, and their implementation on GTAD. The
//! list functors are the list engine's (hbds-lists). The print calls its
//! lists partial ("there are many other multifunctors", page 77): a
//! language decides what an unknown name means. The expert-system
//! functors of page 80 are the expert-system engine's (hbds-rules).

use std::collections::BTreeSet;

use hbds_core::{AdtRef, AdtType, Functor, Gives, Id, Takes, same};
use hbds_gtad::{Error, ObjectLink, Store, Stored, error};

/// The struct, where `AdtRef::ObjectLink` is in scope.
type LinkOfObjects = ObjectLink;

use Gives::*;
use Takes::*;

const fn f(name: &'static str, page: u16, takes: Takes, gives: Gives) -> Functor {
    Functor::new(name, page, takes, gives)
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
    // Multifunctors, page 77.
    f("OPSCR", 77, Objects, ListOf(AdtType::ObjectLink)),
    f("OPSCRC", 77, Objects, ListOf(AdtType::ObjectLink)),
    f("ONSCRC", 77, Objects, ListOf(AdtType::ObjectLink)),
    f("OATTN", 77, Objects, ListOf(AdtType::ObjectAttribute)),
];

pub fn functor(key: &str) -> Option<&'static Functor> {
    FUNCTORS.iter().find(|f| f.name == key)
}

pub fn is_functor(key: &str) -> bool {
    functor(key).is_some()
}

/// A functor of the catalogue (pages 76 to 80) applied to each prefix,
/// the results united in order. `args` are a multifunctor's strings.
/// A functor of the catalogue applied to each prefix, the results united in
/// order without repeats. `args` are a multifunctor's strings.
pub fn apply(store: &Store, name: &str, prefix: &[AdtRef], args: &[String]) -> Stored<Vec<AdtRef>> {
    let s = store;
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for &p in prefix {
        if !s.visible(p) {
            continue;
        }
        for r in one(s, name, p, args)? {
            if s.visible(r) && seen.insert(r) {
                out.push(r);
            }
        }
    }
    Ok(out)
}

fn one(store: &Store, name: &str, p: AdtRef, args: &[String]) -> Stored<Vec<AdtRef>> {
    let s = store;
    use AdtRef::*;
    let wrong = || error(format!("`{name}` does not apply to {}", s.describe(p)));
    let arg = |i: usize| -> Stored<&str> {
        args.get(i)
            .map(String::as_str)
            .ok_or_else(|| Error(format!("`{name}` takes {} argument(s)", i + 1)))
    };
    let objects_of = |c: Id| s.objects_of(c).into_iter().map(Object);
    let attributes_of = |c: Id| s.attributes_of(c).into_iter();
    // The links among `found` that `f` accepts.
    let links = |found: Vec<Id>, f: &dyn Fn(&LinkOfObjects) -> bool| -> Vec<AdtRef> {
        (found.into_iter())
            .filter(|i| f(&s.object_links()[i]))
            .map(ObjectLink)
            .collect()
    };
    let relation = |l: &LinkOfObjects| {
        s.class_links()
            .get(&l.class_link)
            .and_then(|c| c.relation.clone())
    };
    let class_of = |o: Id| {
        s.objects()
            .get(&o)
            .map(|o| s.classes()[&o.class].name.clone())
    };
    Ok(match (name, p) {
        ("ATT", Class(c)) => attributes_of(c).map(ClassAttribute).collect(),
        ("VATT", ClassAttribute(a)) => {
            let c = s.attributes()[&a].class;
            objects_of(c)
                .map(|o| match o {
                    Object(o) => ObjectAttribute(o, a),
                    _ => unreachable!(),
                })
                .collect()
        }
        ("OBJ", Class(c)) => objects_of(c).collect(),
        ("PSC", Class(c)) => s.class_links_from(c).into_iter().map(ClassLink).collect(),
        ("NSC", Class(c)) => s.class_links_to(c).into_iter().map(ClassLink).collect(),
        ("EFF", ClassLink(l)) => links(s.links_of(l), &|_| true),
        ("OATT", Object(o)) => attributes_of(s.objects()[&o].class)
            .map(|a| ObjectAttribute(o, a))
            .collect(),
        ("OPSC", Object(o)) => links(s.links_from(o), &|_| true),
        ("ONSC", Object(o)) => links(s.links_to(o), &|_| true),
        ("ATTOF", ClassAttribute(a)) => vec![Class(s.attributes()[&a].class)],
        ("VATTOF", ObjectAttribute(_, a)) => vec![ClassAttribute(a)],
        ("OBJOF", Object(o)) => vec![Class(s.objects()[&o].class)],
        ("INI", ClassLink(l)) => vec![Class(s.class_links()[&l].from)],
        ("FIN", ClassLink(l)) => vec![Class(s.class_links()[&l].to)],
        ("EFFOF", ObjectLink(l)) => vec![ClassLink(s.object_links()[&l].class_link)],
        ("OATTOF", ObjectAttribute(o, _)) => vec![Object(o)],
        ("OINI", ObjectLink(l)) => vec![Object(s.object_links()[&l].from)],
        ("OFIN", ObjectLink(l)) => vec![Object(s.object_links()[&l].to)],
        ("CONT", Class(h)) => s.members(h).into_iter().map(Class).collect(),
        ("CONTOF", Class(c)) => s.classes()[&c].hyper.iter().map(|h| Class(*h)).collect(),
        ("INV", ClassLink(l)) => vec![ClassLink(s.class_links()[&l].reverse)],
        ("OINV", ObjectLink(l)) => vec![ObjectLink(s.object_links()[&l].reverse)],
        ("OPSCR", Object(o)) => {
            let r = arg(0)?;
            links(s.links_from(o), &|l| {
                relation(l).is_some_and(|x| same(&x, r))
            })
        }
        ("OPSCRC", Object(o)) => {
            let (r, c) = (arg(0)?, arg(1)?);
            links(s.links_from(o), &|l| {
                relation(l).is_some_and(|x| same(&x, r))
                    && class_of(l.to).is_some_and(|x| same(&x, c))
            })
        }
        ("ONSCRC", Object(o)) => {
            let (r, c) = (arg(0)?, arg(1)?);
            links(s.links_to(o), &|l| {
                relation(l).is_some_and(|x| same(&x, r))
                    && class_of(l.from).is_some_and(|x| same(&x, c))
            })
        }
        ("OATTN", Object(o)) => {
            let a = arg(0)?;
            match s.find_attribute(s.objects()[&o].class, a) {
                Some(a) => vec![ObjectAttribute(o, a)],
                None => Vec::new(),
            }
        }
        _ => return wrong(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use AdtRef::*;
    use hbds_core::value::Value;
    use hbds_gtad::{ClassKind, State};
    use std::collections::BTreeMap;

    /// TOWN, RIVER, DAM; RIVER "crosses" TOWN, reversed "crossed by";
    /// DAM "feeds" RIVER; TOWN "neighbor" TOWN, reflexive; objects.
    fn world() -> (Store, BTreeMap<&'static str, Id>) {
        let mut s = Store::memory();
        let mut ids = BTreeMap::new();
        for c in ["TOWN", "RIVER", "DAM", "HYDRO"] {
            ids.insert(c, s.create_class(c, ClassKind::Class).unwrap());
        }
        ids.insert("AREA", s.create_attribute(ids["TOWN"], "AREA").unwrap());
        ids.insert("VOL", s.create_attribute(ids["DAM"], "VOL").unwrap());
        ids.insert(
            "crosses",
            s.create_class_link(
                ids["RIVER"],
                "crosses",
                ids["TOWN"],
                Some("crossed by"),
                false,
            )
            .unwrap(),
        );
        s.create_class_link(ids["DAM"], "feeds", ids["RIVER"], None, false)
            .unwrap();
        ids.insert(
            "neighbor",
            s.create_class_link(ids["TOWN"], "neighbor", ids["TOWN"], None, true)
                .unwrap(),
        );
        for (o, c) in [
            ("Lyon", "TOWN"),
            ("Arles", "TOWN"),
            ("Rhone", "RIVER"),
            ("Vouglans", "DAM"),
        ] {
            ids.insert(o, s.create_object(ids[c], o).unwrap());
        }
        s.assert_link(ids["Rhone"], "crosses", ids["Lyon"], None)
            .unwrap();
        s.assert_link(ids["Rhone"], "crosses", ids["Arles"], Some(0.9))
            .unwrap();
        s.assert_link(ids["Vouglans"], "feeds", ids["Rhone"], None)
            .unwrap();
        s.assert_link(ids["Lyon"], "neighbor", ids["Arles"], None)
            .unwrap();
        s.set(ids["Vouglans"], ids["VOL"], Value::integer(605))
            .unwrap();
        s.include(ids["TOWN"], ids["HYDRO"]).unwrap();
        s.include(ids["RIVER"], ids["HYDRO"]).unwrap();
        (s, ids)
    }

    fn names(s: &Store, refs: &[AdtRef]) -> Vec<String> {
        refs.iter().map(|r| s.describe(*r)).collect()
    }

    fn query(s: &Store, name: &str, prefix: &[AdtRef], args: &[&str]) -> Vec<String> {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        names(s, &super::apply(s, name, prefix, &args).unwrap())
    }

    #[test]
    fn functors() {
        let (s, i) = world();
        assert_eq!(
            query(&s, "OBJ", &[Class(i["TOWN"])], &[]),
            ["Lyon[TOWN]", "Arles[TOWN]"]
        );
        assert_eq!(query(&s, "ATT", &[Class(i["TOWN"])], &[]), ["TOWN.AREA"]);
        assert_eq!(
            query(&s, "VATT", &[ClassAttribute(i["AREA"])], &[]),
            ["Lyon[TOWN].AREA", "Arles[TOWN].AREA"]
        );
        assert_eq!(
            query(&s, "PSC", &[Class(i["RIVER"])], &[]),
            ["RIVER \"crosses\" TOWN", "RIVER (unnamed) DAM"]
        );
        assert_eq!(
            query(&s, "NSC", &[Class(i["RIVER"])], &[]),
            ["TOWN \"crossed by\" RIVER", "DAM \"feeds\" RIVER"]
        );
        assert_eq!(query(&s, "EFF", &[ClassLink(i["crosses"])], &[]).len(), 2);
        assert_eq!(
            query(&s, "OPSCRC", &[Object(i["Rhone"])], &["crosses", "TOWN"]),
            [
                "Rhone[RIVER] \"crosses\" Lyon[TOWN]",
                "Rhone[RIVER] \"crosses\" Arles[TOWN]"
            ]
        );
        let lyon = [Object(i["Lyon"])];
        let crossed =
            super::apply(&s, "OPSCRC", &lyon, &["crossed by".into(), "RIVER".into()]).unwrap();
        assert_eq!(query(&s, "OFIN", &crossed, &[]), ["Rhone[RIVER]"]);
        assert_eq!(query(&s, "OINI", &crossed, &[]), ["Lyon[TOWN]"]);
        let rhone = [Object(i["Rhone"])];
        let fed = super::apply(&s, "ONSCRC", &rhone, &["feeds".into(), "DAM".into()]).unwrap();
        assert_eq!(query(&s, "OINI", &fed, &[]), ["Vouglans[DAM]"]);
        assert_eq!(
            query(&s, "OATTN", &[Object(i["Vouglans"])], &["VOL"]),
            ["Vouglans[DAM].VOL"]
        );
        assert_eq!(query(&s, "OBJOF", &rhone, &[]), ["RIVER"]);
        assert_eq!(
            query(&s, "INV", &[ClassLink(i["crosses"])], &[]),
            ["TOWN \"crossed by\" RIVER"]
        );
        assert_eq!(
            query(&s, "CONT", &[Class(i["HYDRO"])], &[]),
            ["TOWN", "RIVER"]
        );
        assert_eq!(query(&s, "CONTOF", &[Class(i["TOWN"])], &[]), ["HYDRO"]);
        // A list prefix: results united, in order, without repeats.
        assert_eq!(
            query(
                &s,
                "OBJOF",
                &[Object(i["Lyon"]), Object(i["Arles"]), Object(i["Rhone"])],
                &[]
            ),
            ["TOWN", "RIVER"]
        );
        assert!(super::apply(&s, "OBJ", &rhone, &[]).is_err());
    }

    #[test]
    fn states() {
        let (mut s, i) = world();
        let lyon = Object(i["Lyon"]);
        s.change_state(lyon, State::Active, State::Hibernating)
            .unwrap();
        assert_eq!(query(&s, "OBJ", &[Class(i["TOWN"])], &[]), ["Arles[TOWN]"]);
        assert!(s.purge_adt(lyon).is_err());
        s.change_state(lyon, State::Hibernating, State::Active)
            .unwrap();
        s.change_state(lyon, State::Active, State::Destroyed)
            .unwrap();
        assert!(
            s.change_state(lyon, State::Hibernating, State::Active)
                .is_err()
        );
        s.purge_adt(lyon).unwrap();
        assert_eq!(query(&s, "OBJ", &[Class(i["TOWN"])], &[]), ["Arles[TOWN]"]);
        // Its links went with it.
        assert_eq!(
            query(&s, "OPSC", &[Object(i["Rhone"])], &[]),
            [
                "Rhone[RIVER] \"crosses\" Arles[TOWN]",
                "Rhone[RIVER] (unnamed) Vouglans[DAM]"
            ]
        );
        assert!(s.object(i["TOWN"], "Lyon").is_err());
    }

    #[test]
    fn arguments_compare_folded() {
        let (s, i) = world();
        assert_eq!(
            query(&s, "OPSCRC", &[Object(i["Rhone"])], &["Crosses", "town"]).len(),
            2
        );
        assert_eq!(
            query(&s, "OATTN", &[Object(i["Vouglans"])], &["vol"]),
            ["Vouglans[DAM].VOL"]
        );
    }
}
