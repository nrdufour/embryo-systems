//! The engine on a fake language: a part is commands separated by `;`.
//! PREMISES: `has OBJ ATTR`, all of which must hold, or `false`; the
//! other parts: `set OBJ ATTR N`, `inc OBJ ATTR`, `log TEXT`.

use std::ops::DerefMut;
use std::sync::Mutex;

use hbds_core::value::{Fz, Scalar, Value};
use hbds_core::{AdtRef, Id};
use hbds_gtad::{ClassKind, Code, Store};
use hbds_rules::*;

struct Fake {
    store: Mutex<Store>,
    log: Vec<String>,
    lines: Vec<String>,
}

fn object(store: &Store, name: &str) -> Id {
    let found = store.objects().iter().find(|(_, o)| o.name == name);
    *found.unwrap_or_else(|| panic!("no object {name}")).0
}

fn commands(code: &Code) -> Vec<Vec<String>> {
    (code.text.split(';'))
        .map(|c| c.split_whitespace().map(String::from).collect::<Vec<_>>())
        .filter(|c| !c.is_empty())
        .collect()
}

impl Compiler for Fake {
    fn uses(&self, part: &str, code: &Code) -> Result<Uses, String> {
        let mut uses = Uses::default();
        for c in commands(code) {
            match (part, c[0].as_str()) {
                ("PREMISES", "has") | (_, "set" | "inc") => uses.attributes.push(c[2].clone()),
                ("PREMISES", "false") | (_, "log") => {}
                _ => return Err(format!("cannot read {c:?}")),
            }
        }
        Ok(uses)
    }
}

impl Host for Fake {
    fn store(&mut self) -> Box<dyn DerefMut<Target = Store> + '_> {
        Box::new(self.store.lock().unwrap())
    }

    fn premises(&mut self, _: Id, code: &Code) -> Result<(bool, f64), String> {
        let store = self.store.lock().unwrap();
        let (mut holds, mut reliability) = (true, 1.0f64);
        for c in commands(code) {
            if c[0] == "false" {
                holds = false;
                continue;
            }
            let o = object(&store, &c[1]);
            let a = store
                .find_attribute(store.objects()[&o].class, &c[2])
                .unwrap();
            match store.get(o, a) {
                Some(Value::One(fz)) => reliability = reliability.min(fz.prob.unwrap_or(1.0)),
                Some(_) => {}
                None => holds = false,
            }
        }
        Ok((holds, reliability))
    }

    fn run(&mut self, _: Option<Id>, _: &str, code: &Code, reliability: f64) -> Result<(), String> {
        for c in commands(code) {
            let mut store = self.store.lock().unwrap();
            if c[0] == "log" {
                self.log.push(c[1].clone());
                continue;
            }
            let o = object(&store, &c[1]);
            let a = store
                .find_attribute(store.objects()[&o].class, &c[2])
                .unwrap();
            let n = match c[0].as_str() {
                "inc" => match store.get(o, a).and_then(|v| v.scalar()) {
                    Some(Scalar::Integer(n)) => n + 1,
                    _ => 1,
                },
                _ => c[3].parse().unwrap(),
            };
            let mut fz = Fz::exact(Scalar::Integer(n));
            fz.prob = (reliability < 1.0).then_some(reliability);
            store.set(o, a, Value::One(fz)).map_err(|e| e.0)?;
        }
        Ok(())
    }

    fn holds(&mut self, code: &Code, _: AdtRef) -> Result<bool, String> {
        Ok(code.text == "TRUE")
    }

    fn edit(&mut self, line: &str) {
        self.lines.push(line.to_string());
    }
}

/// THINGS with x, y and z, the objects A and B, and the rule class RULES.
fn world() -> Fake {
    let mut store = Store::memory();
    install(&mut store).unwrap();
    let things = store.create_class("THINGS", ClassKind::Class).unwrap();
    for a in ["x", "y", "z"] {
        store.create_attribute(things, a).unwrap();
    }
    for o in ["A", "B"] {
        store.create_object(things, o).unwrap();
    }
    let rules = store.create_class("RULES", ClassKind::Class).unwrap();
    let rule = store.find_class(RULE).unwrap();
    store.include(rules, rule).unwrap();
    Fake {
        store: Mutex::new(store),
        log: Vec::new(),
        lines: Vec::new(),
    }
}

impl Fake {
    /// A rule, its parts as `PART=code` pairs.
    fn rule(&mut self, name: &str, parts: &[(&str, &str)]) -> Id {
        let mut store = self.store.lock().unwrap();
        let rules = store.find_class("RULES").unwrap();
        let r = store.create_object(rules, name).unwrap();
        for (part, text) in parts {
            store
                .set_property(AdtRef::Object(r), part, "FAKE", text)
                .unwrap();
        }
        r
    }

    fn set(&mut self, rule: Id, attribute: &str, v: Value) {
        let mut store = self.store.lock().unwrap();
        let a = store
            .find_attribute(store.objects()[&rule].class, attribute)
            .unwrap();
        store.set(rule, a, v).unwrap();
    }

    fn link(&mut self, from: Id, relation: &str, to: Id) {
        let mut store = self.store.lock().unwrap();
        store.assert_link(from, relation, to, None).unwrap();
    }

    fn value(&self, o: &str, a: &str) -> Option<Value> {
        let store = self.store.lock().unwrap();
        let o = object(&store, o);
        let a = store.find_attribute(store.objects()[&o].class, a).unwrap();
        store.get(o, a).cloned()
    }

    fn names(&self, ids: &[Id]) -> Vec<String> {
        let store = self.store.lock().unwrap();
        ids.iter()
            .map(|i| store.objects()[i].name.clone())
            .collect()
    }

    fn attribute(&self, name: &str) -> AdtRef {
        let store = self.store.lock().unwrap();
        let things = store.find_class("THINGS").unwrap();
        AdtRef::ClassAttribute(store.find_attribute(things, name).unwrap())
    }
}

#[test]
fn install_is_idempotent_and_rule_is_a_fact() {
    let mut store = Store::memory();
    install(&mut store).unwrap();
    let dump = store.dump();
    install(&mut store).unwrap();
    assert_eq!(store.dump(), dump);
    let (rule, fact) = (
        store.find_class(RULE).unwrap(),
        store.find_class(FACT).unwrap(),
    );
    assert!(store.classes()[&rule].hyper.contains(&fact));
    assert!(store.find_attribute(rule, PRIORITY).is_some());
    assert!(store.find_class_link(rule, "inhibited by", rule).is_some());
}

#[test]
fn highest_priority_first_then_oldest() {
    let mut w = world();
    let low = w.rule("LOW", &[]);
    let old = w.rule("OLD", &[]);
    let new = w.rule("NEW", &[]);
    w.set(low, PRIORITY, Value::integer(1));
    w.set(old, PRIORITY, Value::integer(5));
    w.set(new, PRIORITY, Value::integer(5));
    let run = chav(&mut w, &[]).unwrap();
    assert_eq!(w.names(&run.tried), ["OLD", "NEW", "LOW"]);
}

/// A priority a rule changes during the run orders what follows.
#[test]
fn a_priority_changed_by_a_rule_counts_at_once() {
    let mut w = world();
    let low = w.rule("LOW", &[]);
    let old = w.rule("OLD", &[("YES", "set LOW PRIORITY 9")]);
    let new = w.rule("NEW", &[]);
    w.set(low, PRIORITY, Value::integer(1));
    w.set(old, PRIORITY, Value::integer(5));
    w.set(new, PRIORITY, Value::integer(5));
    let run = chav(&mut w, &[]).unwrap();
    assert_eq!(w.names(&run.tried), ["OLD", "LOW", "NEW"]);
}

#[test]
fn only_the_rules_consuming_a_change_are_tried_again() {
    let mut w = world();
    w.rule("SECOND", &[("PREMISES", "has B y"), ("YES", "log second")]);
    w.rule("FIRST", &[("PREMISES", "false"), ("NO", "set B y 1")]);
    w.rule("ASIDE", &[("PREMISES", "has A z")]);
    let run = chav(&mut w, &[]).unwrap();
    // SECOND, the oldest, goes back before ASIDE's first try; ASIDE is
    // tried once only.
    assert_eq!(w.names(&run.tried), ["SECOND", "FIRST", "SECOND", "ASIDE"]);
    assert_eq!(w.names(&run.fired), ["SECOND"]);
    assert_eq!(w.log, ["second"]);
}

#[test]
fn a_change_to_nothing_new_triggers_nothing() {
    let mut w = world();
    w.rule("AGAIN", &[("PREMISES", "has A x"), ("YES", "set A x 1")]);
    w.rule("SEED", &[("YES", "set A x 1")]);
    let run = chav(&mut w, &[]).unwrap();
    // SEED sets x, AGAIN fires once and sets it to what it is.
    assert_eq!(w.names(&run.tried), ["AGAIN", "SEED", "AGAIN"]);
}

#[test]
fn activation_and_inhibition() {
    let mut w = world();
    let boss = w.rule("BOSS", &[("YES", "set A x 1")]);
    let muted = w.rule("MUTED", &[("PREMISES", "has A x"), ("YES", "log muted")]);
    let woken = w.rule("WOKEN", &[("PREMISES", "has B z"), ("YES", "log woken")]);
    w.link(boss, ACTIVATES.0, woken);
    w.link(boss, INHIBITS.0, muted);
    let run = chav(&mut w, &[]).unwrap();
    // MUTED and WOKEN wait on the agenda; BOSS, the oldest, goes first,
    // inhibits MUTED, and puts WOKEN back (it was there anyway, once).
    assert_eq!(w.names(&run.tried), ["BOSS", "WOKEN"]);
    assert!(w.log.is_empty());
}

#[test]
fn activation_brings_a_rule_back() {
    let mut w = world();
    let again = w.rule("AGAIN", &[("YES", "log again")]);
    let boss = w.rule("BOSS", &[("YES", "log boss")]);
    w.link(boss, ACTIVATES.0, again);
    let run = chav(&mut w, &[]).unwrap();
    assert_eq!(w.names(&run.tried), ["AGAIN", "BOSS", "AGAIN"]);
}

#[test]
fn multiple_triggering_waits_for_every_consumed_fact() {
    let mut w = world();
    let both = w.rule(
        "BOTH",
        &[("PREMISES", "has A x; has A y"), ("YES", "log both")],
    );
    w.set(
        both,
        TRIGGERING,
        Value::exact(Scalar::String("multiple".into())),
    );
    let x = w.rule("X", &[("YES", "set A x 1")]);
    let y = w.rule("Y", &[("YES", "set A y 1")]);
    w.set(x, PRIORITY, Value::integer(2));
    w.set(y, PRIORITY, Value::integer(1));
    let run = chav(&mut w, &[]).unwrap();
    // X sets x: BOTH waits; BOTH's first try fails; Y sets y: BOTH again.
    assert_eq!(w.names(&run.tried), ["X", "Y", "BOTH"]);
    assert_eq!(w.log, ["both"]);
    let mut w = world();
    w.rule(
        "BOTH",
        &[("PREMISES", "has A x; has A y"), ("YES", "log both")],
    );
    let x = w.rule("X", &[("YES", "set A x 1")]);
    w.set(x, PRIORITY, Value::integer(2));
    let run = chav(&mut w, &[]).unwrap();
    // Simple triggering: x alone is enough to try it again.
    assert_eq!(w.names(&run.tried), ["X", "BOTH"]);
    let mut w = world();
    let both = w.rule(
        "BOTH",
        &[("PREMISES", "has A x; has A y"), ("YES", "log both")],
    );
    w.set(
        both,
        TRIGGERING,
        Value::exact(Scalar::String("MULTIPLE".into())),
    );
    w.set(both, PRIORITY, Value::integer(9));
    let x = w.rule("X", &[("YES", "set A x 1")]);
    w.set(x, PRIORITY, Value::integer(2));
    let run = chav(&mut w, &[]).unwrap();
    assert_eq!(w.names(&run.tried), ["BOTH", "X"]);
}

#[test]
fn a_rule_that_always_changes_what_it_reads_does_not_converge() {
    let mut w = world();
    w.rule("LOOP", &[("PREMISES", "has A x"), ("YES", "inc A x")]);
    w.rule("SEED", &[("YES", "set A x 1")]);
    let e = chav(&mut w, &[]).unwrap_err();
    assert_eq!(e, "10000 tries, and CHAV does not converge");
    // Refused whole: nothing the run did is left.
    assert_eq!(w.value("A", "x"), None);
}

#[test]
fn reliability_is_the_rules_times_the_premises() {
    let mut w = world();
    let r = w.rule("HALF", &[("PREMISES", "has A x"), ("YES", "set B y 1")]);
    w.set(r, RELIABILITY, Value::exact(Scalar::Float(0.5)));
    let a = w.store.lock().unwrap().find_class("THINGS").unwrap();
    {
        let mut store = w.store.lock().unwrap();
        let o = object(&store, "A");
        let x = store.find_attribute(a, "x").unwrap();
        let mut fz = Fz::exact(Scalar::Integer(1));
        fz.prob = Some(0.8);
        store.set(o, x, Value::One(fz)).unwrap();
    }
    chav(&mut w, &[]).unwrap();
    let Some(Value::One(fz)) = w.value("B", "y") else {
        panic!("no value")
    };
    assert_eq!(fz.prob, Some(0.4));
}

#[test]
fn editable_facts_are_printed_with_their_rule_and_explanation() {
    let mut w = world();
    let r = w.rule("TELL", &[("YES", "set B y 1; set B z 2")]);
    w.set(
        r,
        EXPLANATION,
        Value::exact(Scalar::String("because".into())),
    );
    let y = w.attribute("y");
    let mut store = w.store.lock().unwrap();
    store.set_property(y, "EDITABLE", "FAKE", "TRUE").unwrap();
    drop(store);
    chav(&mut w, &[]).unwrap();
    assert_eq!(w.lines, ["B[THINGS].y = 1, by TELL[RULES]: because"]);
}

#[test]
fn hypochav_tries_what_hypotheses_trigger_first_then_forgets() {
    let mut w = world();
    let busy = w.rule("BUSY", &[("PREMISES", "has A z"), ("YES", "log busy")]);
    w.set(busy, PRIORITY, Value::integer(9));
    w.rule("DEDUCE", &[("PREMISES", "has A x"), ("YES", "set B y 2")]);
    let hypotheses = Code {
        language: "FAKE".into(),
        text: "set A x 1".into(),
    };
    let run = hypochav(&mut w, &hypotheses, &[]).unwrap();
    assert_eq!(w.names(&run.tried), ["DEDUCE", "BUSY"]);
    assert_eq!(
        w.lines,
        [
            "A[THINGS].x = 1, by hypothesis",
            "B[THINGS].y = 2, by DEDUCE[RULES]"
        ]
    );
    assert_eq!(w.value("A", "x"), None);
    assert_eq!(w.value("B", "y"), None);
}

#[test]
fn the_four_functors() {
    let mut w = world();
    let r = w.rule(
        "R",
        &[
            ("PREMISES", "has A x"),
            ("YES", "set B y 1"),
            ("NO", "set B z 1"),
        ],
    );
    let (x, y, z) = (w.attribute("x"), w.attribute("y"), w.attribute("z"));
    let store = w.store.lock().unwrap();
    let rule = [AdtRef::Object(r)];
    assert_eq!(apply(&store, &w, "INRULE", &rule).unwrap(), [x]);
    assert_eq!(apply(&store, &w, "OUTRULE", &rule).unwrap(), [y, z]);
    assert_eq!(apply(&store, &w, "INFACT", &[y]).unwrap(), rule);
    assert_eq!(apply(&store, &w, "OUTFACT", &[x]).unwrap(), rule);
    assert!(apply(&store, &w, "INFACT", &[x]).unwrap().is_empty());
    // A value stands for its class attribute.
    let a = object(&store, "A");
    let AdtRef::ClassAttribute(xa) = x else {
        unreachable!()
    };
    assert_eq!(
        apply(&store, &w, "OUTFACT", &[AdtRef::ObjectAttribute(a, xa)]).unwrap(),
        rule
    );
    let e = apply(&store, &w, "INRULE", &[AdtRef::Object(a)]).unwrap_err();
    assert_eq!(e, "`INRULE` applies to rules, not to A[THINGS]");
}

#[test]
fn names_resolve_against_the_store_as_it_is() {
    let mut w = world();
    let r = w.rule("LATER", &[("YES", "set B w 1")]);
    let store = &w.store;
    let out = |w: &Fake| apply(&store.lock().unwrap(), w, "OUTRULE", &[AdtRef::Object(r)]).unwrap();
    assert!(out(&w).is_empty());
    let mut s = store.lock().unwrap();
    let things = s.find_class("THINGS").unwrap();
    let wa = s.create_attribute(things, "W").unwrap();
    drop(s);
    assert_eq!(out(&w), [AdtRef::ClassAttribute(wa)]);
}

#[test]
fn rules_are_the_objects_of_classes_in_rule() {
    let mut w = world();
    let r = w.rule("R", &[]);
    let store = w.store.lock().unwrap();
    let rules_class = store.find_class("RULES").unwrap();
    assert_eq!(rules(&store, &[]), [r]);
    assert_eq!(rules(&store, &[rules_class]), [r]);
    let things = store.find_class("THINGS").unwrap();
    assert!(rules(&store, &[things]).is_empty());
}
