//! The dialog engine (project 19, pages 133 to 149): a dialog is data, a
//! Q-Graph of questions and the answers they propose (figure 71), which
//! the engine walks (figure 72). A simple question (SQ) waits for a value
//! of its TYPE; a multiple-choice question (MCQ) for one of the answers it
//! proposes (PA). Each place may carry a PROLOG, an EPILOG and a task for
//! the empty answer, each proposed answer a TASK: the language runs them,
//! THIS the run. A run is an object too, so that where it stands, the
//! path behind it (figure 72's PILE) and the answers it memorized are in
//! the store, journalled as the rest. docs/hbds.md, "Dialog", gives the
//! rules.

use std::ops::DerefMut;

use hbds_core::value::{List, Scalar, Value};
use hbds_core::{AdtRef, Id, same};
use hbds_gtad::{ClassKind, Code, Store, Stored};

/// The classes of figure 71; QPLACE is its unnamed hyperclass of SQ and
/// MCQ, RUN and ANSWER are ours.
pub const DIALOG: &str = "DIALOG";
pub const QPLACE: &str = "QPLACE";
pub const SQ: &str = "SQ";
pub const MCQ: &str = "MCQ";
pub const PA: &str = "PA";
pub const RUN: &str = "RUN";
pub const ANSWER: &str = "ANSWER";

/// Attributes.
pub const TEXT: &str = "TEXT";
pub const FACULTATIVE: &str = "FACULTATIVE";
pub const MEMORIZE: &str = "MEMORIZE";
pub const TYPE: &str = "TYPE";
pub const DISPLAY: &str = "DISPLAY";
pub const LEARNING: &str = "LEARNING";
pub const PHASE: &str = "PHASE";
pub const LAST: &str = "LAST";
pub const PATH: &str = "PATH";

/// Code, kept as properties: a place's PROLOG, EPILOG and BLANK (the task
/// of a blank answer), a proposed answer's TASK.
pub const PROLOG: &str = "PROLOG";
pub const EPILOG: &str = "EPILOG";
pub const BLANK: &str = "BLANK";
pub const TASK: &str = "TASK";

/// Link relations.
pub const INITIAL: &str = "initial";
pub const NEXT: &str = "next";
pub const IF_BLANK: &str = "if blank";
pub const CONTROLLED_BY: &str = "controlled by";
pub const PROPOSES: &str = "proposes";
pub const DELAYED: &str = "delayed";
pub const OF: &str = "of";
pub const AT: &str = "at";
pub const CHOSE: &str = "chose";
pub const GAVE: &str = "gave";

/// What a line may say instead of an answer; `//` starts an answer that
/// begins with a slash.
pub const BACK: &str = "/back";
pub const QUIT: &str = "/quit";

/// The TYPEs an SQ may expect.
pub const TYPES: &[&str] = &["STRING", "INTEGER", "FLOAT", "BOOLEAN"];

const CLASSES: &[&str] = &[DIALOG, QPLACE, SQ, MCQ, PA, RUN, ANSWER];

pub fn is_system(name: &str) -> bool {
    CLASSES.iter().any(|c| same(name, c))
}

pub fn is_property(key: &str) -> bool {
    [PROLOG, EPILOG, BLANK, TASK].contains(&key)
}

/// Creates what is missing of figure 71's classes, their attributes and
/// links, and of RUN and ANSWER.
pub fn install(store: &mut Store) -> Stored<()> {
    let class = |store: &mut Store, name| match store.find_class(name) {
        Some(c) => Ok(c),
        None => store.create_class(name, ClassKind::Class),
    };
    let dialog = class(store, DIALOG)?;
    let place = class(store, QPLACE)?;
    let sq = class(store, SQ)?;
    let mcq = class(store, MCQ)?;
    let pa = class(store, PA)?;
    let run = class(store, RUN)?;
    let answer = class(store, ANSWER)?;
    for c in [sq, mcq] {
        if !store.classes()[&c].hyper.contains(&place) {
            store.include(c, place)?;
        }
    }
    let attributes = [
        (place, &[TEXT, FACULTATIVE, MEMORIZE][..]),
        (sq, &[TYPE]),
        (mcq, &[DISPLAY, LEARNING]),
        (pa, &[TEXT, MEMORIZE]),
        (run, &[PHASE, LAST, PATH]),
        (answer, &[TEXT]),
    ];
    for (c, names) in attributes {
        for a in names {
            if store.find_attribute(c, a).is_none() {
                store.create_attribute(c, a)?;
            }
        }
    }
    let links = [
        (dialog, INITIAL, place, None),
        (place, NEXT, place, None),
        (place, IF_BLANK, place, None),
        (place, CONTROLLED_BY, mcq, Some("controls")),
        (mcq, PROPOSES, pa, Some("proposed by")),
        (pa, NEXT, place, None),
        (pa, DELAYED, place, None),
        (run, OF, dialog, None),
        (run, AT, place, None),
        (run, CHOSE, pa, None),
        (run, GAVE, answer, Some("given in")),
        (answer, AT, place, None),
    ];
    for (from, relation, to, reverse) in links {
        if store.find_class_link(from, relation, to).is_none() {
            store.create_class_link(from, relation, to, reverse, false)?;
        }
    }
    Ok(())
}

/// What the engine asks of the language that runs it.
pub trait Host {
    /// The store, for as long as the engine holds it: no other call to
    /// the host while it does.
    fn store(&mut self) -> Box<dyn DerefMut<Target = Store> + '_>;

    /// Runs `part`, statements carried by `owner`, THIS the run.
    fn run(&mut self, part: &str, owner: Id, code: &Code, run: Id) -> Result<(), String>;

    /// A line read as the language reads a value, or why it cannot be.
    fn read(&mut self, line: &str) -> Result<Value, String>;

    /// Writes text, as PRINT does but without a line's end.
    fn write(&mut self, text: &str);
}

fn fail<T>(e: hbds_gtad::Error) -> Result<T, String> {
    Err(e.to_string())
}

fn value(store: &Store, object: Id, name: &str) -> Option<Value> {
    let class = store.objects().get(&object)?.class;
    let a = store.find_attribute(class, name)?;
    store.get(object, a).cloned()
}

fn text(store: &Store, object: Id, name: &str) -> Option<String> {
    match value(store, object, name)?.scalar()? {
        Scalar::String(s) => Some(s.to_string()),
        _ => None,
    }
}

fn flag(store: &Store, object: Id, name: &str, default: bool) -> bool {
    match value(store, object, name).as_ref().and_then(Value::scalar) {
        Some(Scalar::Boolean(b)) => *b,
        _ => default,
    }
}

fn set(store: &mut Store, object: Id, name: &str, v: Value) -> Result<(), String> {
    let class = store.objects()[&object].class;
    let a = store.attribute(class, name).or_else(fail)?;
    store.set(object, a, v).or_else(fail)
}

fn string(s: &str) -> Value {
    Value::exact(Scalar::String(s.into()))
}

/// The active objects `from` is linked to by `relation`, oldest link
/// first.
fn targets(store: &Store, from: Id, relation: &str) -> Vec<Id> {
    (store.links_from(from).into_iter())
        .filter(|l| store.visible(AdtRef::ObjectLink(*l)))
        .filter_map(|l| {
            let link = &store.object_links()[&l];
            let rel = store.class_links()[&link.class_link].relation.as_deref();
            rel.is_some_and(|r| same(r, relation)).then_some(link.to)
        })
        .collect()
}

/// The active objects linked to `to` by `relation`.
fn sources(store: &Store, to: Id, relation: &str) -> Vec<Id> {
    (store.links_to(to).into_iter())
        .filter(|l| store.visible(AdtRef::ObjectLink(*l)))
        .filter_map(|l| {
            let link = &store.object_links()[&l];
            let rel = store.class_links()[&link.class_link].relation.as_deref();
            rel.is_some_and(|r| same(r, relation)).then_some(link.from)
        })
        .collect()
}

fn is_a(store: &Store, object: Id, class: &str) -> bool {
    let Some(o) = store.objects().get(&object) else {
        return false;
    };
    (store.lineage(o.class).into_iter()).any(|c| same(&store.classes()[&c].name, class))
}

fn describe(store: &Store, object: Id) -> String {
    store.describe(AdtRef::Object(object))
}

/// A name no object of `class` has yet: 1, 2, ...
fn fresh(store: &Store, class: Id, stem: &str) -> String {
    let sep = if stem.is_empty() { "" } else { " " };
    (1..)
        .map(|n| format!("{stem}{sep}{n}"))
        .find(|name| store.find_object(class, name).is_none())
        .expect("a free name")
}

/// Starts a run of `dialog` at its initial place: the run, an object of
/// RUN, which [`step`] takes from there.
pub fn start(store: &mut Store, dialog: Id) -> Result<Id, String> {
    if !is_a(store, dialog, DIALOG) || !store.visible(AdtRef::Object(dialog)) {
        let what = match store.objects().get(&dialog) {
            Some(_) => describe(store, dialog),
            None => "it".into(),
        };
        return Err(format!(
            "DIAL takes a dialog, an object of DIALOG; not {what}"
        ));
    }
    let Some(&initial) = targets(store, dialog, INITIAL).first() else {
        return Err(format!(
            "{} has no \"initial\" place",
            describe(store, dialog)
        ));
    };
    let class = store.class(RUN).or_else(fail)?;
    let name = fresh(store, class, "");
    let run = store.create_object(class, &name).or_else(fail)?;
    store.assert_link(run, OF, dialog, None).or_else(fail)?;
    store.assert_link(run, AT, initial, None).or_else(fail)?;
    set(store, run, PHASE, string("ask"))?;
    set(store, run, PATH, Value::List(List::new(Vec::new())))?;
    Ok(run)
}

/// Whether the run's next step waits for a line.
pub fn wants(store: &Store, run: Id) -> bool {
    text(store, run, PHASE).is_some_and(|p| p == "wait")
}

/// One step of a run: asking its place's question, or taking the line
/// answering it. True once the run is done.
pub fn step(host: &mut dyn Host, run: Id, line: Option<&str>) -> Result<bool, String> {
    let (phase, place) = {
        let store = host.store();
        let phase = text(&store, run, PHASE).unwrap_or_default();
        (phase, targets(&store, run, AT).first().copied())
    };
    let Some(place) = place else {
        return Ok(true);
    };
    match phase.as_str() {
        "ask" => {
            task(host, PROLOG, place, run)?;
            show(host, place);
            let mut store = host.store();
            set(&mut store, run, PHASE, string("wait"))?;
            Ok(false)
        }
        "wait" => {
            let line = line.ok_or("the dialog finds no more input")?;
            answer(host, run, place, line)
        }
        _ => Ok(true),
    }
}

/// Runs a part `owner` carries, if it carries it.
fn task(host: &mut dyn Host, part: &str, owner: Id, run: Id) -> Result<(), String> {
    let code = {
        let store = host.store();
        store.objects()[&owner].props.properties.get(part).cloned()
    };
    match code {
        Some(code) => host.run(part, owner, &code, run),
        None => Ok(()),
    }
}

/// A place's question: its TEXT, and an MCQ's proposed answers numbered
/// unless it does not DISPLAY them.
fn show(host: &mut dyn Host, place: Id) {
    let out = {
        let store = host.store();
        let question = text(&store, place, TEXT).unwrap_or_default();
        if is_a(&store, place, MCQ) {
            let mut out = format!("{question}\n");
            if flag(&store, place, DISPLAY, true) {
                for (i, pa) in targets(&store, place, PROPOSES).into_iter().enumerate() {
                    let t = text(&store, pa, TEXT).unwrap_or_default();
                    out += &format!("{}. {t}\n", i + 1);
                }
            }
            out + "? "
        } else {
            format!("{question} ? ")
        }
    };
    host.write(&out);
}

/// Says why a line is not taken, and asks again.
fn again(host: &mut dyn Host, place: Id, why: &str) -> Result<bool, String> {
    host.write(&format!("{why}\n"));
    show(host, place);
    Ok(false)
}

fn answer(host: &mut dyn Host, run: Id, place: Id, line: &str) -> Result<bool, String> {
    if line == QUIT {
        finish(&mut host.store(), run, place)?;
        return Ok(true);
    }
    if line == BACK {
        let back = {
            let mut store = host.store();
            let mut path = path(&store, run);
            let back = path.pop();
            if let Some(previous) = back {
                set(&mut store, run, PATH, list(path))?;
                go(&mut store, run, place, previous)?;
            }
            back
        };
        return match back {
            Some(_) => Ok(false),
            None => again(host, place, "nothing to go back to"),
        };
    }
    let line = line
        .strip_prefix('/')
        .filter(|l| l.starts_with('/'))
        .unwrap_or(line);
    let mcq = is_a(&host.store(), place, MCQ);
    if line.trim().is_empty() {
        if !flag(&host.store(), place, FACULTATIVE, false) {
            return again(host, place, "an answer is required");
        }
        set(&mut host.store(), run, LAST, string(""))?;
        task(host, BLANK, place, run)?;
        let to = {
            let store = host.store();
            match targets(&store, place, IF_BLANK).first() {
                Some(&to) => Some(to),
                None => exit(&store, run, place, None)?,
            }
        };
        return leave(&mut host.store(), run, place, to);
    }
    let (said, chosen) = if mcq {
        match choose(host, run, place, line)? {
            Some((said, pa)) => (said, Some(pa)),
            None => return Ok(false),
        }
    } else {
        let ty = text(&host.store(), place, TYPE).unwrap_or_else(|| "STRING".into());
        if !TYPES.iter().any(|t| same(t, &ty)) {
            let store = host.store();
            return Err(format!(
                "the TYPE of {} is {ty:?}, not one of {}",
                describe(&store, place),
                TYPES.join(", ")
            ));
        }
        if !typed(host, &ty, line) {
            return again(host, place, &format!("{} expected", ty.to_uppercase()));
        }
        (line.to_string(), None)
    };
    {
        let mut store = host.store();
        set(&mut store, run, LAST, string(&said))?;
        let kept = flag(&store, place, MEMORIZE, false)
            && chosen.is_none_or(|pa| flag(&store, pa, MEMORIZE, true));
        if kept {
            let class = store.class(ANSWER).or_else(fail)?;
            let name = fresh(&store, class, "");
            let a = store.create_object(class, &name).or_else(fail)?;
            set(&mut store, a, TEXT, string(&said))?;
            store.assert_link(a, AT, place, None).or_else(fail)?;
            store.assert_link(run, GAVE, a, None).or_else(fail)?;
        }
    }
    if let Some(pa) = chosen {
        task(host, TASK, pa, run)?;
    }
    task(host, EPILOG, place, run)?;
    let to = exit(&host.store(), run, place, chosen)?;
    leave(&mut host.store(), run, place, to)
}

/// Whether a line is a value of `ty`, as the language reads it.
fn typed(host: &mut dyn Host, ty: &str, line: &str) -> bool {
    if same(ty, "STRING") {
        return true;
    }
    let Ok(v) = host.read(line) else {
        return false;
    };
    matches!(
        (ty.to_uppercase().as_str(), v.scalar()),
        ("INTEGER", Some(Scalar::Integer(_)))
            | (
                "FLOAT",
                Some(Scalar::Integer(_) | Scalar::Rational(..) | Scalar::Float(_))
            )
            | ("BOOLEAN", Some(Scalar::Boolean(_)))
    )
}

/// The proposed answer a line picks at an MCQ, by its number or its
/// TEXT, or a new one if the MCQ is LEARNING, recorded as the run's
/// choice there, with its TEXT. None if the line picks nothing: the
/// question is asked again.
fn choose(
    host: &mut dyn Host,
    run: Id,
    place: Id,
    line: &str,
) -> Result<Option<(String, Id)>, String> {
    let line = line.trim();
    let picked = {
        let mut store = host.store();
        let proposed = targets(&store, place, PROPOSES);
        let by_number = (line.parse::<usize>().ok())
            .filter(|n| (1..=proposed.len()).contains(n))
            .map(|n| proposed[n - 1]);
        let by_text = (proposed.iter().copied())
            .find(|pa| text(&store, *pa, TEXT).is_some_and(|t| same(&t, line)));
        match by_number.or(by_text) {
            Some(pa) => Ok(pa),
            None if flag(&store, place, LEARNING, false) => {
                let class = store.class(PA).or_else(fail)?;
                let stem = store.objects()[&place].name.clone();
                let name = fresh(&store, class, &stem);
                let pa = store.create_object(class, &name).or_else(fail)?;
                set(&mut store, pa, TEXT, string(line))?;
                store.assert_link(place, PROPOSES, pa, None).or_else(fail)?;
                Ok(pa)
            }
            None => Err(proposed.len()),
        }
    };
    let pa = match picked {
        Ok(pa) => pa,
        Err(n) => {
            again(host, place, &format!("choose 1 to {n}"))?;
            return Ok(None);
        }
    };
    let mut store = host.store();
    for old in targets(&store, run, CHOSE) {
        if sources(&store, old, PROPOSES).contains(&place) {
            store.retract_link(run, CHOSE, old).or_else(fail)?;
        }
    }
    store.assert_link(run, CHOSE, pa, None).or_else(fail)?;
    Ok(Some((text(&store, pa, TEXT).unwrap_or_default(), pa)))
}

/// Where a run goes from `place`: a place controlled by an MCQ follows
/// the "delayed" link of the answer chosen there (figures 69 and 70);
/// else an answer's own "next", else the place's "next" (figure 68's
/// single exit). None ends the run.
fn exit(store: &Store, run: Id, place: Id, chosen: Option<Id>) -> Result<Option<Id>, String> {
    if let Some(&mcq) = targets(store, place, CONTROLLED_BY).first() {
        let controlled = sources(store, mcq, CONTROLLED_BY);
        if controlled.len() > 1 {
            return Err(format!(
                "{} controls more than one place",
                describe(store, mcq)
            ));
        }
        let made = (targets(store, run, CHOSE).into_iter())
            .find(|pa| sources(store, *pa, PROPOSES).contains(&mcq));
        let Some(pa) = made else {
            return Err(format!(
                "{} branches as {} was answered, and it was not",
                describe(store, place),
                describe(store, mcq)
            ));
        };
        return Ok(targets(store, pa, DELAYED).first().copied());
    }
    let next = chosen.and_then(|pa| targets(store, pa, NEXT).first().copied());
    Ok(next.or_else(|| targets(store, place, NEXT).first().copied()))
}

fn path(store: &Store, run: Id) -> Vec<Id> {
    match value(store, run, PATH) {
        Some(Value::List(l)) => (l.items.iter())
            .filter_map(|v| match v {
                Value::Adt(AdtRef::Object(o)) => Some(*o),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn list(path: Vec<Id>) -> Value {
    let items = path.into_iter().map(|o| Value::Adt(AdtRef::Object(o)));
    Value::List(List::new(items.collect()))
}

/// The run at `to`, its question to ask.
fn go(store: &mut Store, run: Id, from: Id, to: Id) -> Result<(), String> {
    store.retract_link(run, AT, from).or_else(fail)?;
    store.assert_link(run, AT, to, None).or_else(fail)?;
    set(store, run, PHASE, string("ask"))
}

/// The run leaves `place` for `to`, the place kept on its path; or ends.
fn leave(store: &mut Store, run: Id, place: Id, to: Option<Id>) -> Result<bool, String> {
    let Some(to) = to else {
        finish(store, run, place)?;
        return Ok(true);
    };
    let mut p = path(store, run);
    p.push(place);
    set(store, run, PATH, list(p))?;
    go(store, run, place, to)?;
    Ok(false)
}

fn finish(store: &mut Store, run: Id, place: Id) -> Result<(), String> {
    store.retract_link(run, AT, place).or_else(fail)?;
    set(store, run, PHASE, string("done"))
}
