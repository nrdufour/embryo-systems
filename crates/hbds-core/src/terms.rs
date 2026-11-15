//! Terms, the notation the journal and A-code's text write values in:
//! words, numbers, quoted strings and parenthesised lists of terms.
//! `(i 522000)`, `(fz (f 284.5) 0.5 - -)`, `(list 0 false (i 1))`.
//! Read back exactly: floats as Rust prints them, which round-trips.

use crate::value::{Array, Fz, List, Rank, Scalar, Value};
use crate::{AdtRef, Id};

#[derive(Debug, Clone, PartialEq)]
pub enum Term {
    Word(String),
    Text(String),
    List(Vec<Term>),
}

pub fn word(w: impl ToString) -> Term {
    Term::Word(w.to_string())
}

pub fn text(s: &str) -> Term {
    Term::Text(s.to_string())
}

pub fn list(items: Vec<Term>) -> Term {
    Term::List(items)
}

pub fn optional(o: Option<Term>) -> Term {
    o.unwrap_or_else(|| word("-"))
}

pub fn write_term(t: &Term, out: &mut String) {
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

pub fn read_terms(line: &str) -> Result<Vec<Term>, String> {
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

pub fn adt_ref(r: AdtRef) -> Term {
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

pub fn float(x: f64) -> Term {
    word(format!("{x:?}"))
}

pub fn scalar(s: &Scalar) -> Term {
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

pub fn fuzzy(fz: &Fz) -> Term {
    if fz.is_exact() {
        return scalar(&fz.x);
    }
    list(vec![
        word("fz"),
        scalar(&fz.x),
        optional(fz.appr().map(float)),
        optional(fz.prob().map(float)),
        optional(fz.subj().map(text)),
    ])
}

pub fn value(v: &Value) -> Term {
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

pub struct Reader<'t> {
    terms: std::slice::Iter<'t, Term>,
}

impl<'t> Reader<'t> {
    pub fn new(terms: &'t [Term]) -> Reader<'t> {
        Reader {
            terms: terms.iter(),
        }
    }

    pub fn term(&mut self) -> Result<&'t Term, String> {
        self.terms.next().ok_or_else(|| "a term is missing".into())
    }

    /// Whether terms are left.
    pub fn more(&self) -> bool {
        self.terms.len() > 0
    }

    pub fn word(&mut self) -> Result<&'t str, String> {
        match self.term()? {
            Term::Word(w) => Ok(w),
            t => Err(format!("a word expected, found {t:?}")),
        }
    }

    pub fn parse<T: std::str::FromStr>(&mut self) -> Result<T, String> {
        let w = self.word()?;
        w.parse().map_err(|_| format!("{w:?} does not read"))
    }

    pub fn id(&mut self) -> Result<Id, String> {
        self.parse()
    }

    pub fn text(&mut self) -> Result<String, String> {
        match self.term()? {
            Term::Text(s) => Ok(s.clone()),
            t => Err(format!("a string expected, found {t:?}")),
        }
    }

    pub fn optional<T>(
        &mut self,
        read: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<Option<T>, String> {
        if matches!(self.terms.clone().next(), Some(Term::Word(w)) if w == "-") {
            self.term()?;
            return Ok(None);
        }
        read(self).map(Some)
    }

    pub fn list(&mut self) -> Result<Reader<'t>, String> {
        match self.term()? {
            Term::List(items) => Ok(Reader::new(items)),
            t => Err(format!("a list expected, found {t:?}")),
        }
    }

    pub fn end(&mut self) -> Result<(), String> {
        match self.terms.next() {
            None => Ok(()),
            Some(t) => Err(format!("{t:?} too many")),
        }
    }

    pub fn rest<T>(
        &mut self,
        read: impl Fn(&mut Self) -> Result<T, String>,
    ) -> Result<Vec<T>, String> {
        let mut out = Vec::new();
        while self.terms.clone().next().is_some() {
            out.push(read(self)?);
        }
        Ok(out)
    }

    pub fn adt_ref(&mut self) -> Result<AdtRef, String> {
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

    pub fn scalar(&mut self, tag: &str) -> Result<Scalar, String> {
        Ok(match tag {
            "i" => Scalar::Integer(self.parse()?),
            "r" => Scalar::Rational(self.parse()?, self.parse()?),
            "f" => Scalar::Float(self.parse()?),
            "g" => Scalar::Gauss(self.parse()?, self.parse()?),
            "x" => Scalar::Complex(self.parse()?, self.parse()?),
            "q" => Scalar::Quaternion(Box::new([
                self.parse()?,
                self.parse()?,
                self.parse()?,
                self.parse()?,
            ])),
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

    pub fn fuzzy(&mut self) -> Result<Fz, String> {
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
        let fz = Fz::new(
            x,
            r.optional(|r| r.parse())?,
            r.optional(|r| r.parse())?,
            r.optional(|r| r.text())?,
        );
        r.end()?;
        Ok(fz)
    }

    pub fn value(&mut self) -> Result<Value, String> {
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
                Value::Array(Box::new(Array {
                    rank,
                    lows,
                    dims,
                    cells: r.rest(|r| r.value())?.into(),
                }))
            }
            _ => Value::Adt(r.adt_ref()?),
        };
        r.end()?;
        Ok(v)
    }
}
