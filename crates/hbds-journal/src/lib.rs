//! The journal, GTAD's persistence: every [`Change`] of a store, one line
//! each, appended to `DIR/journal` and synced before the statement that
//! made it goes on, as the print's exchange zone forces its writes (page
//! 103) and its file system keeps a DAYFILE (page 86). Opening a store
//! replays its journal into a fresh one, then records what follows. The
//! store holds `DIR/lock` while it is open, so that a second one, in this
//! process or another, is refused rather than writing the same journal.
//! The print's own persistence, SFSI and ZE (project 13), would be another
//! [`Recorder`].
//!
//! A line is a sequence of terms: words, numbers, quoted strings, and
//! parenthesised lists of terms. `class 1 "TOWN" class`, `set 7 3 (f 2.5)`.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::Path;

use hbds_core::value::{Array, Fz, List, Rank, Scalar, Value};
use hbds_core::{AdtRef, Id};
use hbds_gtad::{Change, ClassKind, Error, Recorder, State, Store, state_name};

pub struct Journal {
    file: File,
    /// Locked while the store is open; the system lets go when the
    /// process ends, however it ends. Not the journal itself, which
    /// compaction will replace by another file.
    _lock: File,
}

impl Recorder for Journal {
    fn record(&mut self, change: &Change) -> Result<(), String> {
        self.record_all(std::slice::from_ref(change))
    }

    /// A transaction's changes in one write, then one sync.
    fn record_all(&mut self, changes: &[Change]) -> Result<(), String> {
        let text: String = changes.iter().map(|c| encode(c) + "\n").collect();
        self.file
            .write_all(text.as_bytes())
            .and_then(|_| self.file.sync_data())
            .map_err(|e| format!("the journal cannot be written: {e}"))
    }
}

/// Opens, or creates, the store in `dir`, replaying its journal. A last
/// line cut short by a crash is dropped, with a warning. A store already
/// open is refused.
pub fn open(dir: &Path) -> Result<(Store, Vec<String>), Error> {
    let io = |e: std::io::Error| Error(format!("{}: {e}", dir.display()));
    fs::create_dir_all(dir).map_err(io)?;
    let lock = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(dir.join("lock"))
        .map_err(io)?;
    lock.try_lock().map_err(|e| match e {
        TryLockError::WouldBlock => Error(format!(
            "{}: the store is in use by another run",
            dir.display()
        )),
        TryLockError::Error(e) => io(e),
    })?;
    let path = dir.join("journal");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(io(e)),
    };
    let mut warnings = Vec::new();
    let complete = text.rfind('\n').map_or(0, |i| i + 1);
    let mut store = Store::memory();
    for (n, line) in text[..complete].lines().enumerate() {
        let change = decode(line).map_err(|e| Error(format!("journal line {}: {e}", n + 1)))?;
        store.replay(change);
    }
    if complete < text.len() {
        warnings.push(format!(
            "the journal's last line was cut short, and is dropped: {:?}",
            &text[complete..]
        ));
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(io)?;
    file.set_len(complete as u64).map_err(io)?;
    store.record_to(Box::new(Journal { file, _lock: lock }));
    Ok((store, warnings))
}

// Terms

#[derive(Debug, Clone, PartialEq)]
enum Term {
    Word(String),
    Text(String),
    List(Vec<Term>),
}

fn word(w: impl ToString) -> Term {
    Term::Word(w.to_string())
}

fn text(s: &str) -> Term {
    Term::Text(s.to_string())
}

fn list(items: Vec<Term>) -> Term {
    Term::List(items)
}

fn optional(o: Option<Term>) -> Term {
    o.unwrap_or_else(|| word("-"))
}

fn write_term(t: &Term, out: &mut String) {
    match t {
        Term::Word(w) => out.push_str(w),
        Term::Text(s) => {
            out.push('"');
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    c => out.push(c),
                }
            }
            out.push('"');
        }
        Term::List(items) => {
            out.push('(');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                write_term(item, out);
            }
            out.push(')');
        }
    }
}

fn read_terms(line: &str) -> Result<Vec<Term>, String> {
    let mut stack: Vec<Vec<Term>> = vec![Vec::new()];
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' => {}
            '(' => stack.push(Vec::new()),
            ')' => {
                let items = stack
                    .pop()
                    .filter(|_| !stack.is_empty())
                    .ok_or("a `)` too many")?;
                stack.last_mut().unwrap().push(Term::List(items));
            }
            '"' => {
                let mut s = String::new();
                loop {
                    match chars.next().ok_or("a string not closed")? {
                        '"' => break,
                        '\\' => s.push(match chars.next().ok_or("a string not closed")? {
                            'n' => '\n',
                            c => c,
                        }),
                        c => s.push(c),
                    }
                }
                stack.last_mut().unwrap().push(Term::Text(s));
            }
            c => {
                let mut w = c.to_string();
                while let Some(&c) = chars.peek() {
                    if matches!(c, ' ' | '(' | ')' | '"') {
                        break;
                    }
                    w.push(c);
                    chars.next();
                }
                stack.last_mut().unwrap().push(Term::Word(w));
            }
        }
    }
    match stack.len() {
        1 => Ok(stack.pop().unwrap()),
        _ => Err("a `(` not closed".into()),
    }
}

// Encoding

fn adt_ref(r: AdtRef) -> Term {
    let (tag, ids): (&str, Vec<Id>) = match r {
        AdtRef::Class(i) => ("c", vec![i]),
        AdtRef::Object(i) => ("o", vec![i]),
        AdtRef::ClassAttribute(i) => ("ca", vec![i]),
        AdtRef::ObjectAttribute(o, a) => ("oa", vec![o, a]),
        AdtRef::ClassLink(i) => ("cl", vec![i]),
        AdtRef::ObjectLink(i) => ("ol", vec![i]),
    };
    let mut items = vec![word(tag)];
    items.extend(ids.into_iter().map(word));
    list(items)
}

fn float(x: f64) -> Term {
    word(format!("{x:?}"))
}

fn scalar(s: &Scalar) -> Term {
    list(match s {
        Scalar::Integer(n) => vec![word("i"), word(n)],
        Scalar::Rational(n, d) => vec![word("r"), word(n), word(d)],
        Scalar::Float(x) => vec![word("f"), float(*x)],
        Scalar::Gauss(a, b) => vec![word("g"), word(a), word(b)],
        Scalar::Complex(a, b) => vec![word("x"), float(*a), float(*b)],
        Scalar::Quaternion(q) => {
            let mut items = vec![word("q")];
            items.extend(q.iter().map(|x| float(*x)));
            items
        }
        Scalar::Boolean(b) => vec![word("b"), word(b)],
        Scalar::Character(c) => vec![word("ch"), text(&c.to_string())],
        Scalar::String(s) => vec![word("s"), text(s)],
    })
}

fn fuzzy(fz: &Fz) -> Term {
    if fz.appr.is_none() && fz.prob.is_none() && fz.subj.is_none() {
        return scalar(&fz.x);
    }
    list(vec![
        word("fz"),
        scalar(&fz.x),
        optional(fz.appr.map(float)),
        optional(fz.prob.map(float)),
        optional(fz.subj.as_deref().map(text)),
    ])
}

fn value(v: &Value) -> Term {
    match v {
        Value::One(fz) => fuzzy(fz),
        Value::Many(fzs) => {
            let mut items = vec![word("many")];
            items.extend(fzs.iter().map(fuzzy));
            list(items)
        }
        Value::List(l) => {
            let mut items = vec![word("list"), word(l.cursor), word(l.circular)];
            items.extend(l.items.iter().map(value));
            list(items)
        }
        Value::Array(a) => {
            let rank = match a.rank {
                Rank::Vector => "vector",
                Rank::Matrix => "matrix",
                Rank::Tensor => "tensor",
            };
            let mut items = vec![
                word("array"),
                word(rank),
                list(a.lows.iter().map(word).collect()),
                list(a.dims.iter().map(word).collect()),
            ];
            items.extend(a.cells.iter().map(value));
            list(items)
        }
        Value::Adt(r) => list(vec![word("adt"), adt_ref(*r)]),
    }
}

fn state_word(s: State) -> Term {
    word(state_name(s))
}

fn encode(change: &Change) -> String {
    let terms = match change {
        Change::Class { id, name, kind } => {
            let kind = match kind {
                ClassKind::Class => word("class"),
                ClassKind::Embryo => word("embryo"),
                ClassKind::Prototype(e) => {
                    let mut items = vec![word("prototype")];
                    items.extend(e.iter().map(word));
                    list(items)
                }
                ClassKind::Structure(p) => list(vec![word("structure"), word(p)]),
            };
            vec![word("class"), word(id), text(name), kind]
        }
        Change::Object { id, class, name } => {
            vec![word("object"), word(id), word(class), text(name)]
        }
        Change::Attribute { id, class, name } => {
            vec![word("attribute"), word(id), word(class), text(name)]
        }
        Change::ClassLink {
            id,
            reverse,
            from,
            relation,
            to,
            reverse_relation,
        } => vec![
            word("classlink"),
            word(id),
            word(reverse),
            word(from),
            text(relation),
            word(to),
            optional(reverse_relation.as_deref().map(text)),
        ],
        Change::ObjectLink {
            id,
            reverse,
            class_link,
            from,
            to,
            probability,
        } => vec![
            word("objectlink"),
            word(id),
            word(reverse),
            word(class_link),
            word(from),
            word(to),
            optional(probability.map(float)),
        ],
        Change::Unlink { id } => vec![word("unlink"), word(id)],
        Change::Set {
            object,
            attribute,
            value: v,
        } => vec![word("set"), word(object), word(attribute), value(v)],
        Change::Specify {
            target,
            spec,
            unit,
            computed,
        } => vec![
            word("specify"),
            adt_ref(*target),
            optional(spec.map(|s| text(&s.to_string()))),
            optional(unit.as_deref().map(text)),
            word(computed),
        ],
        Change::State { target, state } => {
            vec![word("state"), adt_ref(*target), state_word(*state)]
        }
        Change::Purge { target } => vec![word("purge"), adt_ref(*target)],
        Change::Include { class, hyper } => vec![word("include"), word(class), word(hyper)],
        Change::Exclude { class, hyper } => vec![word("exclude"), word(class), word(hyper)],
        Change::Assoc { a, b } => vec![word("assoc"), adt_ref(*a), adt_ref(*b)],
        Change::Dissoc { a, b } => vec![word("dissoc"), adt_ref(*a), adt_ref(*b)],
        Change::Property {
            target,
            name,
            language,
            text: t,
        } => vec![
            word("property"),
            adt_ref(*target),
            text(name),
            text(language),
            text(t),
        ],
    };
    let mut out = String::new();
    for (i, t) in terms.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        write_term(t, &mut out);
    }
    out
}

// Decoding

struct Reader<'t> {
    terms: std::slice::Iter<'t, Term>,
}

impl<'t> Reader<'t> {
    fn new(terms: &'t [Term]) -> Reader<'t> {
        Reader {
            terms: terms.iter(),
        }
    }

    fn next(&mut self) -> Result<&'t Term, String> {
        self.terms.next().ok_or_else(|| "a term is missing".into())
    }

    fn word(&mut self) -> Result<&'t str, String> {
        match self.next()? {
            Term::Word(w) => Ok(w),
            t => Err(format!("a word expected, found {t:?}")),
        }
    }

    fn parse<T: std::str::FromStr>(&mut self) -> Result<T, String> {
        let w = self.word()?;
        w.parse().map_err(|_| format!("{w:?} does not read"))
    }

    fn id(&mut self) -> Result<Id, String> {
        self.parse()
    }

    fn text(&mut self) -> Result<String, String> {
        match self.next()? {
            Term::Text(s) => Ok(s.clone()),
            t => Err(format!("a string expected, found {t:?}")),
        }
    }

    fn optional<T>(
        &mut self,
        read: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<Option<T>, String> {
        if matches!(self.terms.clone().next(), Some(Term::Word(w)) if w == "-") {
            self.next()?;
            return Ok(None);
        }
        read(self).map(Some)
    }

    fn list(&mut self) -> Result<Reader<'t>, String> {
        match self.next()? {
            Term::List(items) => Ok(Reader::new(items)),
            t => Err(format!("a list expected, found {t:?}")),
        }
    }

    fn end(&mut self) -> Result<(), String> {
        match self.terms.next() {
            None => Ok(()),
            Some(t) => Err(format!("{t:?} too many")),
        }
    }

    fn rest<T>(&mut self, read: impl Fn(&mut Self) -> Result<T, String>) -> Result<Vec<T>, String> {
        let mut out = Vec::new();
        while self.terms.clone().next().is_some() {
            out.push(read(self)?);
        }
        Ok(out)
    }

    fn adt_ref(&mut self) -> Result<AdtRef, String> {
        let mut r = self.list()?;
        let tag = r.word()?;
        let a = r.id()?;
        let adt = match tag {
            "c" => AdtRef::Class(a),
            "o" => AdtRef::Object(a),
            "ca" => AdtRef::ClassAttribute(a),
            "oa" => AdtRef::ObjectAttribute(a, r.id()?),
            "cl" => AdtRef::ClassLink(a),
            "ol" => AdtRef::ObjectLink(a),
            t => return Err(format!("{t:?} is not an a.d.t.")),
        };
        r.end()?;
        Ok(adt)
    }

    fn scalar(&mut self, tag: &str) -> Result<Scalar, String> {
        Ok(match tag {
            "i" => Scalar::Integer(self.parse()?),
            "r" => Scalar::Rational(self.parse()?, self.parse()?),
            "f" => Scalar::Float(self.parse()?),
            "g" => Scalar::Gauss(self.parse()?, self.parse()?),
            "x" => Scalar::Complex(self.parse()?, self.parse()?),
            "q" => Scalar::Quaternion([self.parse()?, self.parse()?, self.parse()?, self.parse()?]),
            "b" => Scalar::Boolean(self.parse()?),
            "ch" => {
                let s = self.text()?;
                let mut chars = s.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Scalar::Character(c),
                    _ => return Err(format!("{s:?} is not one character")),
                }
            }
            "s" => Scalar::String(self.text()?.into()),
            t => return Err(format!("{t:?} is not a value")),
        })
    }

    fn fuzzy(&mut self) -> Result<Fz, String> {
        let mut r = self.list()?;
        let tag = r.word()?;
        if tag != "fz" {
            let x = r.scalar(tag)?;
            r.end()?;
            return Ok(Fz::exact(x));
        }
        let mut s = r.list()?;
        let tag = s.word()?;
        let x = s.scalar(tag)?;
        s.end()?;
        let fz = Fz {
            x,
            appr: r.optional(|r| r.parse())?,
            prob: r.optional(|r| r.parse())?,
            subj: r.optional(|r| r.text())?,
        };
        r.end()?;
        Ok(fz)
    }

    fn value(&mut self) -> Result<Value, String> {
        let tag = match self.terms.clone().next() {
            Some(Term::List(items)) => match items.first() {
                Some(Term::Word(w)) => w.clone(),
                _ => return Err("a value expected".into()),
            },
            _ => return Err("a value expected".into()),
        };
        if !matches!(tag.as_str(), "many" | "list" | "array" | "adt") {
            return Ok(Value::One(self.fuzzy()?));
        }
        let mut r = self.list()?;
        r.word()?;
        let v = match tag.as_str() {
            "many" => Value::Many(r.rest(|r| r.fuzzy())?),
            "list" => {
                let cursor = r.parse()?;
                let circular = r.parse()?;
                Value::List(List {
                    cursor,
                    circular,
                    items: r.rest(|r| r.value())?.into(),
                })
            }
            "array" => {
                let rank = match r.word()? {
                    "vector" => Rank::Vector,
                    "matrix" => Rank::Matrix,
                    "tensor" => Rank::Tensor,
                    w => return Err(format!("{w:?} is not a rank")),
                };
                let lows = r.list()?.rest(|r| r.parse())?;
                let dims = r.list()?.rest(|r| r.parse())?;
                Value::Array(Array {
                    rank,
                    lows,
                    dims,
                    cells: r.rest(|r| r.value())?.into(),
                })
            }
            _ => Value::Adt(r.adt_ref()?),
        };
        r.end()?;
        Ok(v)
    }

    fn state(&mut self) -> Result<State, String> {
        Ok(match self.word()? {
            "active" => State::Active,
            "hibernating" => State::Hibernating,
            "destroyed" => State::Destroyed,
            w => return Err(format!("{w:?} is not a state")),
        })
    }
}

fn decode(line: &str) -> Result<Change, String> {
    let terms = read_terms(line)?;
    let mut r = Reader::new(&terms);
    let change = match r.word()? {
        "class" => {
            let id = r.id()?;
            let name = r.text()?;
            let kind = match r.next()? {
                Term::Word(w) if w == "class" => ClassKind::Class,
                Term::Word(w) if w == "embryo" => ClassKind::Embryo,
                Term::List(items) => {
                    let mut k = Reader::new(items);
                    match k.word()? {
                        "prototype" => ClassKind::Prototype(k.rest(|k| k.id())?),
                        "structure" => ClassKind::Structure(k.id()?),
                        w => return Err(format!("{w:?} is not a kind of class")),
                    }
                }
                t => return Err(format!("{t:?} is not a kind of class")),
            };
            Change::Class { id, name, kind }
        }
        "object" => Change::Object {
            id: r.id()?,
            class: r.id()?,
            name: r.text()?,
        },
        "attribute" => Change::Attribute {
            id: r.id()?,
            class: r.id()?,
            name: r.text()?,
        },
        "classlink" => Change::ClassLink {
            id: r.id()?,
            reverse: r.id()?,
            from: r.id()?,
            relation: r.text()?,
            to: r.id()?,
            reverse_relation: r.optional(|r| r.text())?,
        },
        "objectlink" => Change::ObjectLink {
            id: r.id()?,
            reverse: r.id()?,
            class_link: r.id()?,
            from: r.id()?,
            to: r.id()?,
            probability: r.optional(|r| r.parse())?,
        },
        "unlink" => Change::Unlink { id: r.id()? },
        "set" => Change::Set {
            object: r.id()?,
            attribute: r.id()?,
            value: r.value()?,
        },
        "specify" => Change::Specify {
            target: r.adt_ref()?,
            spec: r.optional(|r| r.text()?.parse())?,
            unit: r.optional(|r| r.text())?,
            computed: r.parse()?,
        },
        "state" => Change::State {
            target: r.adt_ref()?,
            state: r.state()?,
        },
        "purge" => Change::Purge {
            target: r.adt_ref()?,
        },
        "include" => Change::Include {
            class: r.id()?,
            hyper: r.id()?,
        },
        "exclude" => Change::Exclude {
            class: r.id()?,
            hyper: r.id()?,
        },
        "assoc" => Change::Assoc {
            a: r.adt_ref()?,
            b: r.adt_ref()?,
        },
        "dissoc" => Change::Dissoc {
            a: r.adt_ref()?,
            b: r.adt_ref()?,
        },
        "property" => Change::Property {
            target: r.adt_ref()?,
            name: r.text()?,
            language: r.text()?,
            text: r.text()?,
        },
        w => return Err(format!("{w:?} is not a change")),
    };
    r.end()?;
    Ok(change)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> Vec<Value> {
        let fz = |x| Fz::exact(x);
        vec![
            Value::integer(-42),
            Value::exact(Scalar::Rational(-3, 4)),
            Value::exact(Scalar::Float(0.1)),
            Value::exact(Scalar::Float(-0.0)),
            Value::exact(Scalar::Float(1e300)),
            Value::exact(Scalar::Gauss(1, -2)),
            Value::exact(Scalar::Complex(1.5, -2.25)),
            Value::exact(Scalar::Quaternion([1.0, 2.0, -3.0, 4.5])),
            Value::boolean(true),
            Value::exact(Scalar::Character('"')),
            Value::exact(Scalar::String("a \"quoted\" \\ line\nand (parens)".into())),
            Value::One(Fz {
                x: Scalar::Float(25.3),
                appr: Some(0.37),
                prob: Some(0.73),
                subj: Some("conform; unexpected".into()),
            }),
            Value::One(Fz {
                x: Scalar::Integer(1),
                appr: None,
                prob: Some(0.5),
                subj: None,
            }),
            Value::Many(vec![fz(Scalar::Integer(1)), fz(Scalar::Float(2.5))]),
            Value::List(List {
                items: vec![Value::integer(1), Value::Adt(AdtRef::ObjectAttribute(3, 4))].into(),
                cursor: 1,
                circular: true,
            }),
            Value::Array(Array {
                rank: Rank::Matrix,
                lows: vec![0, -1],
                dims: vec![1, 2],
                cells: vec![Value::integer(1), Value::integer(2)].into(),
            }),
            Value::Adt(AdtRef::ClassLink(9)),
        ]
    }

    #[test]
    fn values_round_trip() {
        for v in values() {
            let change = Change::Set {
                object: 1,
                attribute: 2,
                value: v,
            };
            let line = encode(&change);
            assert_eq!(decode(&line), Ok(change), "{line}");
        }
    }

    /// Values of every kind, nested, at random: each round-trips.
    #[test]
    fn random_values_round_trip() {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        fn value(next: &mut dyn FnMut(u64) -> u64, depth: u32) -> Value {
            let float = |next: &mut dyn FnMut(u64) -> u64| {
                // Any finite float, signs and tiny and huge ones included.
                let x = f64::from_bits(next(u64::MAX));
                if x.is_finite() {
                    x
                } else {
                    next(10_000) as f64 / 7.0
                }
            };
            let text = |next: &mut dyn FnMut(u64) -> u64| {
                let pool = ["", "a", "\"", "\\", "(x)", "line\nbreak", "é ü"];
                pool[next(pool.len() as u64) as usize].to_string()
            };
            let scalar = |next: &mut dyn FnMut(u64) -> u64| match next(9) {
                0 => Scalar::Integer(next(u64::MAX) as i64),
                1 => Scalar::Rational(next(1000) as i64 - 500, next(1000) as i64 + 1),
                2 => Scalar::Float(float(next)),
                3 => Scalar::Gauss(next(100) as i64 - 50, next(100) as i64 - 50),
                4 => Scalar::Complex(float(next), float(next)),
                5 => Scalar::Quaternion([float(next), float(next), float(next), float(next)]),
                6 => Scalar::Boolean(next(2) == 0),
                7 => Scalar::Character(['a', '"', '\\', ' '][next(4) as usize]),
                _ => Scalar::String(text(next).into()),
            };
            let fz = |next: &mut dyn FnMut(u64) -> u64| Fz {
                x: scalar(next),
                appr: (next(3) == 0).then(|| float(next)),
                prob: (next(3) == 0).then(|| next(100) as f64 / 100.0),
                subj: (next(3) == 0).then(|| text(next)),
            };
            match next(if depth == 0 { 3 } else { 6 }) {
                0 | 1 => Value::One(fz(next)),
                2 => Value::Adt(AdtRef::ObjectAttribute(next(99), next(99))),
                3 => Value::Many((0..next(3)).map(|_| fz(next)).collect()),
                4 => Value::List(List {
                    items: (0..next(4))
                        .map(|_| value(next, depth - 1))
                        .collect::<Vec<_>>()
                        .into(),
                    cursor: next(4) as usize,
                    circular: next(2) == 0,
                }),
                _ => Value::Array(Array {
                    rank: Rank::Vector,
                    lows: vec![next(5) as i64 - 2],
                    dims: vec![2],
                    cells: (0..2)
                        .map(|_| value(next, depth - 1))
                        .collect::<Vec<_>>()
                        .into(),
                }),
            }
        }
        for _ in 0..2000 {
            let v = value(&mut next, 3);
            let change = Change::Set {
                object: 1,
                attribute: 2,
                value: v,
            };
            let line = encode(&change);
            assert_eq!(decode(&line), Ok(change), "{line}");
        }
    }

    #[test]
    fn changes_round_trip() {
        let changes = [
            Change::Class {
                id: 1,
                name: "TOWN".into(),
                kind: ClassKind::Class,
            },
            Change::Class {
                id: 2,
                name: "NET".into(),
                kind: ClassKind::Prototype(vec![3, 4]),
            },
            Change::Class {
                id: 5,
                name: "R1".into(),
                kind: ClassKind::Structure(2),
            },
            Change::ClassLink {
                id: 6,
                reverse: 7,
                from: 1,
                relation: "a b".into(),
                to: 1,
                reverse_relation: None,
            },
            Change::ObjectLink {
                id: 8,
                reverse: 9,
                class_link: 6,
                from: 10,
                to: 11,
                probability: Some(0.9),
            },
            Change::Specify {
                target: AdtRef::ObjectAttribute(1, 2),
                spec: Some("FLOAT APPR".parse().unwrap()),
                unit: None,
                computed: true,
            },
            Change::State {
                target: AdtRef::Object(3),
                state: State::Hibernating,
            },
            Change::Property {
                target: AdtRef::ClassAttribute(2),
                name: "COND".into(),
                language: "ADT'81".into(),
                text: "THIS . \"temp\" > 0.".into(),
            },
        ];
        for c in changes {
            let line = encode(&c);
            assert_eq!(decode(&line), Ok(c), "{line}");
        }
    }

    #[test]
    fn bad_lines() {
        assert!(decode("class 1").is_err());
        assert!(decode("class 1 \"A\" class extra").is_err());
        assert!(decode("set 1 2 (f").is_err());
        assert!(decode("teleport 1").is_err());
    }
}
