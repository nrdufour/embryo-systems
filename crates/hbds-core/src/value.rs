//! Values as the store keeps them: every number type HBDS knows (page
//! 156's complexes and quaternions among them), fuzziness carried beside
//! each (pages 152 and 153), lists, arrays, and a.d.t. Pure data, and its
//! canonical text: what a value means in a computation is a language's.
//!
//! Strings, lists' items and arrays' cells are shared behind a reference
//! count: a copy costs one count, and a shared one is copied only when it
//! changes (`Arc::make_mut`).

use std::fmt;
use std::sync::Arc;

use crate::AdtRef;

/// An array's rank: VECTOR, MATRIX or TENSOR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rank {
    Vector,
    Matrix,
    Tensor,
}

/// A float as the canonical text writes it, always with a point: `2.0`.
pub fn float(x: f64) -> String {
    let text = x.abs().to_string();
    let text = if text.contains('.') {
        text
    } else {
        text + ".0"
    };
    if x.is_sign_negative() {
        format!("-{text}")
    } else {
        text
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    Integer(i64),
    /// Numerator and denominator, in lowest terms, the denominator
    /// positive.
    Rational(i64, i64),
    Float(f64),
    Gauss(i64, i64),
    Complex(f64, f64),
    /// Real part, then the i, j and k parts. Boxed: it would double
    /// every value's size.
    Quaternion(Box<[f64; 4]>),
    Boolean(bool),
    Character(char),
    String(Arc<str>),
}

/// A value and its fuzziness.
#[derive(Debug, Clone, PartialEq)]
pub struct Fz {
    pub x: Scalar,
    /// None when exact, as nearly every value is: it then costs a pointer.
    fuzz: Option<Box<Fuzz>>,
}

#[derive(Debug, Clone, PartialEq)]
struct Fuzz {
    appr: Option<f64>,
    prob: Option<f64>,
    subj: Option<String>,
}

impl Fz {
    pub fn exact(x: Scalar) -> Fz {
        Fz { x, fuzz: None }
    }

    pub fn new(x: Scalar, appr: Option<f64>, prob: Option<f64>, subj: Option<String>) -> Fz {
        let fuzz = (appr.is_some() || prob.is_some() || subj.is_some())
            .then(|| Box::new(Fuzz { appr, prob, subj }));
        Fz { x, fuzz }
    }

    /// The half-width of the approximation, `+-`.
    pub fn appr(&self) -> Option<f64> {
        self.fuzz.as_ref().and_then(|f| f.appr)
    }

    /// The probability, `%`.
    pub fn prob(&self) -> Option<f64> {
        self.fuzz.as_ref().and_then(|f| f.prob)
    }

    /// The opinion, `@`.
    pub fn subj(&self) -> Option<&str> {
        self.fuzz.as_ref().and_then(|f| f.subj.as_deref())
    }

    pub fn is_exact(&self) -> bool {
        self.fuzz.is_none()
    }

    /// The same fuzziness on another value.
    pub fn with(&self, x: Scalar) -> Fz {
        Fz {
            x,
            fuzz: self.fuzz.clone(),
        }
    }

    pub fn set_prob(&mut self, prob: Option<f64>) {
        let fuzz = self.fuzz.get_or_insert_with(|| {
            Box::new(Fuzz {
                appr: None,
                prob: None,
                subj: None,
            })
        });
        fuzz.prob = prob;
        if fuzz.appr.is_none() && fuzz.prob.is_none() && fuzz.subj.is_none() {
            self.fuzz = None;
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    One(Fz),
    /// Several values between which one cannot choose (MULT).
    Many(Vec<Fz>),
    /// Boxed, being three times the size of the others.
    Array(Box<Array>),
    List(List),
    /// A class, an object, an attribute or a link, in the HBDS store.
    Adt(AdtRef),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Array {
    pub rank: Rank,
    /// The lower bound of each index.
    pub lows: Vec<i64>,
    pub dims: Vec<usize>,
    /// Row-major.
    pub cells: Arc<Vec<Value>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct List {
    pub items: Arc<Vec<Value>>,
    /// The current element, for CURRENT, PRED and SUCC.
    pub cursor: usize,
    pub circular: bool,
}

impl List {
    pub fn new(items: Vec<Value>) -> List {
        List {
            items: Arc::new(items),
            cursor: 0,
            circular: false,
        }
    }
}

impl Value {
    pub fn exact(x: Scalar) -> Value {
        Value::One(Fz::exact(x))
    }

    pub fn integer(n: i64) -> Value {
        Value::exact(Scalar::Integer(n))
    }

    pub fn boolean(b: bool) -> Value {
        Value::exact(Scalar::Boolean(b))
    }

    /// The one exact scalar this is, if it is one.
    pub fn scalar(&self) -> Option<&Scalar> {
        match self {
            Value::One(fz) => Some(&fz.x),
            _ => None,
        }
    }
}

// Text: in the literal syntax of the language the project started with,
// which is the canonical text of values.

fn signed_part(x: f64, unit: char) -> String {
    let sign = if x.is_sign_negative() { '-' } else { '+' };
    format!(" {sign} {}{unit}", float(x.abs()))
}

impl fmt::Display for Scalar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Scalar::Integer(n) => write!(f, "{n}"),
            Scalar::Rational(n, d) => write!(f, "{n} // {d}"),
            Scalar::Float(x) => f.write_str(&float(*x)),
            Scalar::Gauss(a, b) => {
                let sign = if *b < 0 { '-' } else { '+' };
                write!(f, "{a} {sign} {}i", b.unsigned_abs())
            }
            Scalar::Complex(a, b) => write!(f, "{}{}", float(*a), signed_part(*b, 'i')),
            Scalar::Quaternion(q) => {
                let [r, i, j, k] = &**q;
                write!(
                    f,
                    "{}{}{}{}",
                    float(*r),
                    signed_part(*i, 'i'),
                    signed_part(*j, 'j'),
                    signed_part(*k, 'k')
                )
            }
            Scalar::Boolean(b) => f.write_str(if *b { "TRUE" } else { "FALSE" }),
            Scalar::Character(c) => write!(f, "{c}"),
            Scalar::String(s) => f.write_str(s),
        }
    }
}

impl fmt::Display for Fz {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.x)?;
        if let Some(a) = self.appr() {
            write!(f, " +- {}", float(a))?;
        }
        if let Some(p) = self.prob() {
            write!(f, " % {}", float(p))?;
        }
        if let Some(s) = self.subj() {
            write!(f, " @\"{s}\"")?;
        }
        Ok(())
    }
}

fn join_values<T: fmt::Display>(items: &[T]) -> String {
    items
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// An array's cells, nested by index: `[[1, 2], [3, 4]]`.
fn nested(dims: &[usize], cells: &[Value]) -> String {
    match dims {
        [] | [_] => format!("[{}]", join_values(cells)),
        [n, rest @ ..] => {
            let size = rest.iter().product::<usize>().max(1);
            let rows: Vec<String> = (0..*n)
                .map(|i| nested(rest, &cells[i * size..(i + 1) * size]))
                .collect();
            format!("[{}]", rows.join(", "))
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::One(fz) => write!(f, "{fz}"),
            Value::Many(fzs) => write!(f, "{{ {} }}", join_values(fzs)),
            Value::Array(a) => f.write_str(&nested(&a.dims, &a.cells)),
            Value::List(l) => write!(f, "[{}]", join_values(&l.items)),
            Value::Adt(r) => write!(f, "{r:?}"),
        }
    }
}
