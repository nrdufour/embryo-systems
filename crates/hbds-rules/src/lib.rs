//! The expert-system engine (project 16, pages 106 to 111): SES's rules
//! and facts as a.d.t. (figures 46 and 47), the four functors that walk
//! them without pattern matching (page 80, figure 39), and forward
//! chaining, CHAV and HYPOCHAV. A rule's code is a language's: the engine
//! decides what runs when, the language runs it, through [`Host`].
//! docs/hbds.md, "Expert systems", gives the rules.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use hbds_core::value::{Scalar, Value};
use hbds_core::{AdtRef, AdtType, Functor, Gives, Id, Takes, same};
use hbds_gtad::{Change, ClassKind, Code, Store, Stored, push_new};

/// The system hyperclasses: every fact's, and every rule's, RULE being
/// included in FACT (figure 47), so that rules are facts too.
pub const FACT: &str = "FACT";
pub const RULE: &str = "RULE";

/// A rule's code, figure 46: run in this order, YES if the PREMISES hold
/// and NO otherwise.
pub const PARTS: &[&str] = &["PROLOGUE", "PREMISES", "YES", "NO", "EPILOGUE"];

/// A fact's parameters (page 106): boolean expressions, THIS the object
/// concerned.
pub const FACT_PROPERTIES: &[&str] = &["ASKABLE", "EDITABLE", "MODIFIABLE"];

/// RULE's attributes (figures 46 and 47), which every rule inherits.
pub const PRIORITY: &str = "PRIORITY";
pub const RELIABILITY: &str = "RELIABILITY";
pub const EXPLANATION: &str = "EXPLANATION";
pub const TRIGGERING: &str = "TRIGGERING";
const ATTRIBUTES: &[&str] = &[PRIORITY, RELIABILITY, EXPLANATION, TRIGGERING];

/// RULE's links to RULE, with their reverses (figure 46).
pub const ACTIVATES: (&str, &str) = ("activates", "activated by");
pub const INHIBITS: (&str, &str) = ("inhibits", "inhibited by");

/// More tries than this, and CHAV does not converge.
pub const LIMIT: usize = 10_000;

/// The properties a rule or a fact carries.
pub fn is_property(key: &str) -> bool {
    PARTS.contains(&key) || FACT_PROPERTIES.contains(&key)
}

const fn f(name: &'static str, takes: Takes, gives: Gives) -> Functor {
    Functor::new(name, 80, takes, gives)
}

/// The functors of SES (page 80): a rule's consumed and produced facts,
/// a fact's producing and consuming rules.
pub const FUNCTORS: &[Functor] = &[
    f("INFACT", Takes::Any, Gives::ListOf(AdtType::Object)),
    f("OUTFACT", Takes::Any, Gives::ListOf(AdtType::Object)),
    f("INRULE", Takes::Objects, Gives::Unknown),
    f("OUTRULE", Takes::Objects, Gives::Unknown),
];

pub fn functor(key: &str) -> Option<&'static Functor> {
    FUNCTORS.iter().find(|f| f.name == key)
}

/// Whether a class name is one of the system hyperclasses.
pub fn is_system(name: &str) -> bool {
    same(name, FACT) || same(name, RULE)
}

/// Creates what is missing of FACT and RULE, their attributes and links.
pub fn install(store: &mut Store) -> Stored<()> {
    let class = |store: &mut Store, name| match store.find_class(name) {
        Some(c) => Ok(c),
        None => store.create_class(name, ClassKind::Class),
    };
    let fact = class(store, FACT)?;
    let rule = class(store, RULE)?;
    if !store.classes[&rule].hyper.contains(&fact) {
        store.include(rule, fact)?;
    }
    for a in ATTRIBUTES {
        if store.find_attribute(rule, a).is_none() {
            store.create_attribute(rule, a)?;
        }
    }
    for (relation, reverse) in [ACTIVATES, INHIBITS] {
        if store.find_class_link(rule, relation, rule).is_none() {
            store.create_class_link(rule, relation, rule, Some(reverse), false)?;
        }
    }
    Ok(())
}

/// What a part of a rule names: the classes, attributes and link
/// relations its PREMISES read, or its other parts write.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Uses {
    pub classes: Vec<String>,
    pub attributes: Vec<String>,
    pub relations: Vec<String>,
}

/// A language's compiler, as SES needs it: what a part of a rule uses,
/// read at capture (page 80), or why it does not compile, said of the
/// part: "does not parse: ...".
pub trait Compiler {
    fn uses(&self, part: &str, code: &Code) -> Result<Uses, String>;
}

/// The facts names stand for, in the store as it is: classes, class
/// attributes and class links (figure 50).
pub fn facts(store: &Store, uses: &Uses) -> Vec<AdtRef> {
    let mut out = Vec::new();
    for name in &uses.classes {
        if let Some(c) = store.find_class(name) {
            push_new(&mut out, AdtRef::Class(c));
        }
    }
    for name in &uses.attributes {
        for (id, a) in &store.attributes {
            if same(&a.name, name) {
                push_new(&mut out, AdtRef::ClassAttribute(*id));
            }
        }
    }
    for name in &uses.relations {
        for (id, l) in &store.class_links {
            if l.relation.as_deref().is_some_and(|r| same(r, name)) {
                push_new(&mut out, AdtRef::ClassLink(*id));
            }
        }
    }
    out.retain(|r| store.visible(*r));
    out
}

/// The fact an a.d.t. is, or realizes: an object its class, a value its
/// class attribute, an object link its class link.
pub fn fact_of(store: &Store, r: AdtRef) -> AdtRef {
    match r {
        AdtRef::Object(o) => AdtRef::Class(store.objects[&o].class),
        AdtRef::ObjectAttribute(_, a) => AdtRef::ClassAttribute(a),
        AdtRef::ObjectLink(l) => AdtRef::ClassLink(store.object_links[&l].class_link),
        r => r,
    }
}

/// The facts a change realizes, and what realizes them: a created object,
/// a set value, an asserted link (both its directions).
fn realized(store: &Store, change: &Change) -> Vec<(AdtRef, AdtRef)> {
    match change {
        Change::Object { id, class, .. } => vec![(AdtRef::Class(*class), AdtRef::Object(*id))],
        Change::Set {
            object, attribute, ..
        } => vec![(
            AdtRef::ClassAttribute(*attribute),
            AdtRef::ObjectAttribute(*object, *attribute),
        )],
        Change::ObjectLink { id, class_link, .. } => {
            let mut out = vec![(AdtRef::ClassLink(*class_link), AdtRef::ObjectLink(*id))];
            if let Some(l) = store.class_links.get(class_link)
                && l.reverse != *class_link
            {
                out.push((AdtRef::ClassLink(l.reverse), AdtRef::ObjectLink(*id)));
            }
            out
        }
        _ => Vec::new(),
    }
}

/// The rules of `scope`, classes or hyperclasses, RULE when it is empty:
/// the active objects of the classes they include, at any depth, that
/// are in RULE; oldest first.
pub fn rules(store: &Store, scope: &[Id]) -> Vec<Id> {
    let Some(rule) = store.find_class(RULE) else {
        return Vec::new();
    };
    let scope = if scope.is_empty() { &[rule][..] } else { scope };
    (store.objects.iter())
        .filter(|(id, o)| {
            let lineage = store.lineage(o.class);
            store.visible(AdtRef::Object(**id))
                && lineage.contains(&rule)
                && scope.iter().any(|s| lineage.contains(s))
        })
        .map(|(id, _)| *id)
        .collect()
}

/// A part of a rule's code, if it has one.
pub fn code(store: &Store, rule: Id, part: &str) -> Option<Code> {
    let props = store.props(AdtRef::Object(rule))?;
    props.properties.get(part).cloned()
}

fn part_facts(
    store: &Store,
    compiler: &dyn Compiler,
    rule: Id,
    parts: &[&str],
) -> Result<Vec<AdtRef>, String> {
    let mut out = Vec::new();
    for part in parts {
        if let Some(code) = code(store, rule, part) {
            let what = store.describe(AdtRef::Object(rule));
            let uses =
                (compiler.uses(part, &code)).map_err(|e| format!("the {part} of {what} {e}"))?;
            for f in facts(store, &uses) {
                push_new(&mut out, f);
            }
        }
    }
    Ok(out)
}

/// INRULE: the facts a rule consumes, those its PREMISES read.
pub fn consumed(store: &Store, compiler: &dyn Compiler, rule: Id) -> Result<Vec<AdtRef>, String> {
    part_facts(store, compiler, rule, &["PREMISES"])
}

/// OUTRULE: the facts a rule produces, those its YES and NO write.
pub fn produced(store: &Store, compiler: &dyn Compiler, rule: Id) -> Result<Vec<AdtRef>, String> {
    part_facts(store, compiler, rule, &["YES", "NO"])
}

/// A functor of SES applied to each prefix, the results united in order.
pub fn apply(
    store: &Store,
    compiler: &dyn Compiler,
    name: &str,
    prefix: &[AdtRef],
) -> Result<Vec<AdtRef>, String> {
    let mut out = Vec::new();
    for &p in prefix {
        let found = match name {
            "INRULE" | "OUTRULE" => {
                let rule = match p {
                    AdtRef::Object(r) if rules(store, &[]).contains(&r) => r,
                    p => {
                        return Err(format!(
                            "`{name}` applies to rules, not to {}",
                            store.describe(p)
                        ));
                    }
                };
                if name == "INRULE" {
                    consumed(store, compiler, rule)?
                } else {
                    produced(store, compiler, rule)?
                }
            }
            "INFACT" | "OUTFACT" => {
                let fact = fact_of(store, p);
                let mut found = Vec::new();
                for r in rules(store, &[]) {
                    let facts = if name == "INFACT" {
                        produced(store, compiler, r)?
                    } else {
                        consumed(store, compiler, r)?
                    };
                    if facts.contains(&fact) {
                        found.push(AdtRef::Object(r));
                    }
                }
                found
            }
            _ => return Err(format!("`{name}` is not a functor of SES")),
        };
        for r in found {
            push_new(&mut out, r);
        }
    }
    Ok(out)
}

/// What the engine asks of the language that runs it.
pub trait Host: Compiler {
    fn store(&self) -> &Mutex<Store>;

    /// Whether a rule's PREMISES hold, THIS the rule, and how reliable the
    /// facts they read are: the lowest probability among them, 1 without.
    fn premises(&mut self, rule: Id, code: &Code) -> Result<(bool, f64), String>;

    /// Runs a part of a rule, THIS the rule, or HYPOCHAV's hypotheses
    /// (no rule). What it writes takes `reliability` as its probability,
    /// when that is below 1.
    fn run(
        &mut self,
        rule: Option<Id>,
        part: &str,
        code: &Code,
        reliability: f64,
    ) -> Result<(), String>;

    /// Whether a fact's boolean property holds, THIS the object concerned.
    fn holds(&mut self, code: &Code, this: AdtRef) -> Result<bool, String>;

    /// Prints a line.
    fn edit(&mut self, line: &str);
}

/// What a run did.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Run {
    /// Every rule tried, in order.
    pub tried: Vec<Id>,
    /// Those whose PREMISES held.
    pub fired: Vec<Id>,
}

/// CHAV: forward chaining over the rules of `scope`, until no rule is to
/// be tried.
pub fn chav(host: &mut dyn Host, scope: &[Id]) -> Result<Run, String> {
    Chainer::new(host, scope, false).run(None)
}

/// HYPOCHAV: forward chaining from hypothetical facts, which `hypotheses`
/// sets; what is deduced from them is hypothetical too, and goes first.
/// Prints every hypothetical fact, then puts the store back as it was.
pub fn hypochav(host: &mut dyn Host, hypotheses: &Code, scope: &[Id]) -> Result<Run, String> {
    Chainer::new(host, scope, true).run(Some(hypotheses))
}

struct Chainer<'h> {
    host: &'h mut dyn Host,
    scope: Vec<Id>,
    /// The rules known, and the facts each consumes.
    consumes: BTreeMap<Id, Vec<AdtRef>>,
    agenda: BTreeSet<Id>,
    inhibited: BTreeSet<Id>,
    /// For multiple triggering: the consumed facts changed since the
    /// rule's last try.
    changed: BTreeMap<Id, BTreeSet<AdtRef>>,
    /// HYPOCHAV's: the hypothetical facts, the rules they trigger, and
    /// what was deduced, by which rule.
    hypothetical: Option<BTreeSet<AdtRef>>,
    urgent: BTreeSet<Id>,
    deduced: Vec<(AdtRef, Option<Id>)>,
    done: Run,
}

/// A rule's attribute, inherited from RULE.
fn attribute(store: &Store, rule: Id, name: &str) -> Option<Value> {
    let a = store.find_attribute(store.objects[&rule].class, name)?;
    store.get(rule, a).cloned()
}

fn number(v: Option<Value>) -> Option<f64> {
    match v?.scalar()? {
        Scalar::Integer(i) => Some(*i as f64),
        Scalar::Float(x) => Some(*x),
        Scalar::Rational(n, d) => Some(*n as f64 / *d as f64),
        _ => None,
    }
}

fn text(v: Option<Value>) -> Option<String> {
    match v?.scalar()? {
        Scalar::String(s) => Some(s.clone()),
        Scalar::Character(c) => Some(c.to_string()),
        _ => None,
    }
}

impl<'h> Chainer<'h> {
    fn new(host: &'h mut dyn Host, scope: &[Id], hypothetical: bool) -> Chainer<'h> {
        Chainer {
            host,
            scope: scope.to_vec(),
            consumes: BTreeMap::new(),
            agenda: BTreeSet::new(),
            inhibited: BTreeSet::new(),
            changed: BTreeMap::new(),
            hypothetical: hypothetical.then(BTreeSet::new),
            urgent: BTreeSet::new(),
            deduced: Vec::new(),
            done: Run::default(),
        }
    }

    fn pending(&self) -> usize {
        self.host.store().lock().unwrap().pending().len()
    }

    fn since(&self, mark: usize) -> Vec<Change> {
        self.host.store().lock().unwrap().pending()[mark..].to_vec()
    }

    /// The whole run is a transaction of the store's, nested in any open.
    fn run(mut self, hypotheses: Option<&Code>) -> Result<Run, String> {
        let point = {
            let mut store = self.host.store().lock().unwrap();
            store.begin();
            store.savepoint()
        };
        let result = self.chain(hypotheses);
        let mut store = self.host.store().lock().unwrap();
        if result.is_ok()
            && self.hypothetical.is_some()
            && let Some(point) = point
        {
            store.rollback(point);
        }
        let ended = store.end(result.is_ok());
        drop(store);
        result?;
        ended.map_err(|e| e.0)?;
        Ok(self.done)
    }

    fn chain(&mut self, hypotheses: Option<&Code>) -> Result<(), String> {
        self.refresh()?;
        if let Some(code) = hypotheses {
            let mark = self.pending();
            self.host.run(None, "HYPOTHESES", code, 1.0)?;
            let changes = self.since(mark);
            self.react(&changes, None, true, None)?;
        }
        while let Some(r) = self.pick() {
            if self.done.tried.len() >= LIMIT {
                return Err(format!("{} tries, and CHAV does not converge", LIMIT));
            }
            self.agenda.remove(&r);
            let urgent = self.urgent.remove(&r);
            self.changed.remove(&r);
            self.try_rule(r, urgent)?;
        }
        if self.hypothetical.is_some() {
            for (fact, rule) in std::mem::take(&mut self.deduced) {
                let line = self.establishes(fact, rule);
                self.host.edit(&line);
            }
        }
        Ok(())
    }

    /// The rules of the scope, compiled again: new ones go on the agenda.
    fn refresh(&mut self) -> Result<(), String> {
        let store = self.host.store().lock().unwrap();
        let mut consumes = BTreeMap::new();
        for r in rules(&store, &self.scope) {
            consumes.insert(r, consumed(&store, &*self.host, r)?);
            if !self.consumes.contains_key(&r) {
                self.agenda.insert(r);
            }
        }
        drop(store);
        self.agenda.retain(|r| consumes.contains_key(r));
        self.consumes = consumes;
        Ok(())
    }

    /// The highest priority, the oldest first among equals; in HYPOCHAV,
    /// the rules hypothetical facts triggered before all others.
    fn pick(&self) -> Option<Id> {
        let store = self.host.store().lock().unwrap();
        let urgent: Vec<Id> = self.agenda.intersection(&self.urgent).copied().collect();
        let candidates: Vec<Id> = if urgent.is_empty() {
            self.agenda.iter().copied().collect()
        } else {
            urgent
        };
        let priority = |r: Id| number(attribute(&store, r, PRIORITY)).unwrap_or(0.0);
        let mut best: Option<(f64, Id)> = None;
        for r in candidates {
            let p = priority(r);
            if best.is_none_or(|(q, _)| p > q) {
                best = Some((p, r));
            }
        }
        best.map(|(_, r)| r)
    }

    fn try_rule(&mut self, r: Id, urgent: bool) -> Result<(), String> {
        self.done.tried.push(r);
        let (part, reliability) = {
            let store = self.host.store().lock().unwrap();
            let part = |name| code(&store, r, name);
            let reliability = number(attribute(&store, r, RELIABILITY)).unwrap_or(1.0);
            (
                [
                    part("PROLOGUE"),
                    part("PREMISES"),
                    part("YES"),
                    part("NO"),
                    part("EPILOGUE"),
                ],
                reliability,
            )
        };
        let [prologue, premises, yes, no, epilogue] = part;
        let start = self.pending();
        if let Some(c) = &prologue {
            self.host.run(Some(r), "PROLOGUE", c, 1.0)?;
        }
        let asking = self.pending();
        let (holds, read) = match &premises {
            Some(c) => self.host.premises(r, c)?,
            None => (true, 1.0),
        };
        let consequent = self.pending();
        let (name, code) = if holds { ("YES", &yes) } else { ("NO", &no) };
        if let Some(c) = code {
            self.host.run(Some(r), name, c, reliability * read)?;
        }
        let after = self.pending();
        if let Some(c) = &epilogue {
            self.host.run(Some(r), "EPILOGUE", c, 1.0)?;
        }
        if holds {
            self.done.fired.push(r);
        }
        let established = self.since(consequent);
        let established = &established[..after - consequent];
        if self.hypothetical.is_none() {
            self.edit(established, r)?;
        }
        if holds {
            self.links(r);
        }
        let changes = self.since(start);
        if changes.iter().any(|c| {
            matches!(
                c,
                Change::Property { .. } | Change::Object { .. } | Change::State { .. }
            )
        }) {
            self.refresh()?;
        }
        // What the PREMISES asked, the rule has seen: it triggers the
        // others only.
        let (before, rest) = changes.split_at(asking - start);
        let (asked, rest) = rest.split_at(consequent - asking);
        self.react(before, Some(r), urgent, None)?;
        self.react(asked, Some(r), urgent, Some(r))?;
        self.react(rest, Some(r), urgent, None)
    }

    /// The facts changes realize put the rules that consume them on the
    /// agenda; in HYPOCHAV, those deduced from hypotheses are hypothetical.
    fn react(
        &mut self,
        changes: &[Change],
        by: Option<Id>,
        urgent: bool,
        except: Option<Id>,
    ) -> Result<(), String> {
        let store = self.host.store().lock().unwrap();
        let mut facts = BTreeSet::new();
        for change in changes {
            for (fact, realization) in realized(&store, change) {
                facts.insert(fact);
                if urgent && let Some(h) = &mut self.hypothetical {
                    h.insert(fact);
                    if !self.deduced.iter().any(|(f, _)| *f == realization) {
                        self.deduced.push((realization, by));
                    }
                }
            }
        }
        for (q, consumes) in &self.consumes {
            if self.inhibited.contains(q) || except == Some(*q) {
                continue;
            }
            let touched: Vec<AdtRef> = consumes
                .iter()
                .filter(|f| facts.contains(f))
                .copied()
                .collect();
            if touched.is_empty() {
                continue;
            }
            let multiple =
                text(attribute(&store, *q, TRIGGERING)).is_some_and(|t| same(&t, "MULTIPLE"));
            if multiple {
                let changed = self.changed.entry(*q).or_default();
                changed.extend(touched.iter().copied());
                if !consumes.iter().all(|f| changed.contains(f)) {
                    continue;
                }
            }
            self.agenda.insert(*q);
            let hypothetical = self.hypothetical.as_ref();
            if hypothetical.is_some_and(|h| touched.iter().any(|f| h.contains(f))) {
                self.urgent.insert(*q);
            }
        }
        Ok(())
    }

    /// The rules a fired rule activates go on the agenda again; those it
    /// inhibits leave it, for the rest of the run.
    fn links(&mut self, r: Id) {
        let store = self.host.store().lock().unwrap();
        let targets = |relation: &str| -> Vec<Id> {
            (store.object_links.iter())
                .filter(|(id, l)| {
                    l.from == r
                        && store.visible(AdtRef::ObjectLink(**id))
                        && (store.class_links[&l.class_link].relation.as_deref())
                            .is_some_and(|n| same(n, relation))
                })
                .map(|(_, l)| l.to)
                .collect()
        };
        let (activated, inhibited) = (targets(ACTIVATES.0), targets(INHIBITS.0));
        drop(store);
        for q in activated {
            self.inhibited.remove(&q);
            if self.consumes.contains_key(&q) {
                self.agenda.insert(q);
            }
        }
        for q in inhibited {
            self.inhibited.insert(q);
            self.agenda.remove(&q);
            self.urgent.remove(&q);
        }
    }

    /// Prints the editable facts a rule's consequent established.
    fn edit(&mut self, changes: &[Change], r: Id) -> Result<(), String> {
        let mut seen = Vec::new();
        for change in changes {
            let found = {
                let store = self.host.store().lock().unwrap();
                realized(&store, change)
                    .into_iter()
                    .take(1)
                    .filter_map(|(fact, realization)| {
                        let props = store.props(fact)?;
                        let code = props.properties.get("EDITABLE")?.clone();
                        Some((code, realization))
                    })
                    .collect::<Vec<_>>()
            };
            for (code, realization) in found {
                let this = match realization {
                    AdtRef::ObjectAttribute(o, _) => AdtRef::Object(o),
                    AdtRef::ObjectLink(l) => {
                        let store = self.host.store().lock().unwrap();
                        AdtRef::Object(store.object_links[&l].from)
                    }
                    r => r,
                };
                if !seen.contains(&realization) && self.host.holds(&code, this)? {
                    seen.push(realization);
                    let line = self.establishes(realization, Some(r));
                    self.host.edit(&line);
                }
            }
        }
        Ok(())
    }

    /// `PIPE[P1].pressure = 2.5, by R1[RULES]: why`
    fn establishes(&self, fact: AdtRef, rule: Option<Id>) -> String {
        let store = self.host.store().lock().unwrap();
        let mut line = store.describe(fact);
        if let AdtRef::ObjectAttribute(o, a) = fact
            && let Some(v) = store.get(o, a)
        {
            line += &format!(" = {}", store.show(v));
        }
        match rule {
            Some(r) => {
                line += &format!(", by {}", store.describe(AdtRef::Object(r)));
                if let Some(why) = text(attribute(&store, r, EXPLANATION)) {
                    line += &format!(": {why}");
                }
            }
            None => line += ", by hypothesis",
        }
        line
    }
}
