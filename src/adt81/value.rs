//! Runtime values and their arithmetic: every number type of ADT'81, and
//! fuzziness carried along (pages 152 and 153). docs/adt81.md, "Running",
//! gives the rules.
//!
//! An approximation propagates worst-case, to first order: the absolute
//! contributions of each operand add up. A probability multiplies, as for
//! independent events, until rules give the law (page 152). Opinions are
//! concatenated. A multiple value combines with others value by value,
//! each with each.

use std::cmp::Ordering;
use std::fmt;

use super::ast::{BinaryOp, Rank, UnaryOp};
use super::print::float;
use super::types::Ty;
use crate::hbds::AdtRef;

/// What went wrong, without where: the interpreter adds the position.
#[derive(Debug, Clone, PartialEq)]
pub struct Fault(pub String);

pub type Computed<T> = Result<T, Fault>;

fn fault<T>(message: impl Into<String>) -> Computed<T> {
    Err(Fault(message.into()))
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
    /// Real part, then the i, j and k parts.
    Quaternion([f64; 4]),
    Boolean(bool),
    Character(char),
    String(String),
}

/// A value and its fuzziness.
#[derive(Debug, Clone, PartialEq)]
pub struct Fz {
    pub x: Scalar,
    /// The half-width of the approximation, `+-`.
    pub appr: Option<f64>,
    /// The probability, `%`.
    pub prob: Option<f64>,
    /// The opinion, `@`.
    pub subj: Option<String>,
}

impl Fz {
    pub fn exact(x: Scalar) -> Fz {
        Fz {
            x,
            appr: None,
            prob: None,
            subj: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    One(Fz),
    /// Several values between which one cannot choose (MULT).
    Many(Vec<Fz>),
    Array(Array),
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
    pub cells: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct List {
    pub items: Vec<Value>,
    /// The current element, for CURRENT, PRED and SUCC.
    pub cursor: usize,
    pub circular: bool,
}

impl List {
    pub fn new(items: Vec<Value>) -> List {
        List {
            items,
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

    pub fn as_integer(&self) -> Computed<i64> {
        match self.scalar() {
            Some(Scalar::Integer(n)) => Ok(*n),
            _ => fault(format!("{self} is not an integer")),
        }
    }

    /// The truth of a condition: a boolean, fuzzy or not, but one.
    pub fn truth(&self) -> Computed<bool> {
        match self {
            Value::One(Fz {
                x: Scalar::Boolean(b),
                ..
            }) => Ok(*b),
            Value::Many(_) => fault("a multiple value is not a condition"),
            _ => fault(format!("{self} is not a boolean")),
        }
    }
}

// Numbers

/// The number types, along the widening chains: integer, rational,
/// float, complex, quaternion; integer, Gauss, complex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Integer,
    Rational,
    Gauss,
    Float,
    Complex,
    Quaternion,
}

fn level(s: &Scalar) -> Option<Level> {
    Some(match s {
        Scalar::Integer(_) => Level::Integer,
        Scalar::Rational(..) => Level::Rational,
        Scalar::Float(_) => Level::Float,
        Scalar::Gauss(..) => Level::Gauss,
        Scalar::Complex(..) => Level::Complex,
        Scalar::Quaternion(_) => Level::Quaternion,
        _ => return None,
    })
}

fn height(l: Level) -> u8 {
    match l {
        Level::Integer => 0,
        Level::Rational | Level::Gauss => 1,
        Level::Float => 2,
        Level::Complex => 3,
        Level::Quaternion => 4,
    }
}

fn join(a: Level, b: Level) -> Level {
    use Level::*;
    match (a, b) {
        _ if a == b => a,
        (Gauss, Rational | Float) | (Rational | Float, Gauss) => Complex,
        _ if height(a) >= height(b) => a,
        _ => b,
    }
}

/// `s` as a number of level `to`, which is at least its own.
fn widen(s: &Scalar, to: Level) -> Scalar {
    use Scalar::*;
    let quaternion = |r: f64, i: f64| Quaternion([r, i, 0.0, 0.0]);
    match (s, to) {
        (Integer(n), Level::Rational) => Rational(*n, 1),
        (Integer(n), Level::Float) => Float(*n as f64),
        (Integer(n), Level::Gauss) => Gauss(*n, 0),
        (Integer(n), Level::Complex) => Complex(*n as f64, 0.0),
        (Integer(n), Level::Quaternion) => quaternion(*n as f64, 0.0),
        (Rational(n, d), Level::Float) => Float(*n as f64 / *d as f64),
        (Rational(n, d), Level::Complex) => Complex(*n as f64 / *d as f64, 0.0),
        (Rational(n, d), Level::Quaternion) => quaternion(*n as f64 / *d as f64, 0.0),
        (Float(f), Level::Complex) => Complex(*f, 0.0),
        (Float(f), Level::Quaternion) => quaternion(*f, 0.0),
        (Gauss(a, b), Level::Complex) => Complex(*a as f64, *b as f64),
        (Gauss(a, b), Level::Quaternion) => quaternion(*a as f64, *b as f64),
        (Complex(a, b), Level::Quaternion) => quaternion(*a, *b),
        _ => s.clone(),
    }
}

/// A value converted to be stored as `to`: numbers widen, a character
/// becomes a string, a one-character string a character. Narrowing is a
/// fault.
pub fn convert(v: Value, to: &Ty) -> Computed<Value> {
    let target = match to {
        Ty::Integer => Level::Integer,
        Ty::Rational => Level::Rational,
        Ty::Float => Level::Float,
        Ty::Gauss => Level::Gauss,
        Ty::Complex => Level::Complex,
        Ty::Quaternion => Level::Quaternion,
        Ty::Character | Ty::String => {
            let character = *to == Ty::Character;
            return map_singles(v, &|fz| {
                let x = match (&fz.x, character) {
                    (Scalar::Character(c), false) => Scalar::String(c.to_string()),
                    (Scalar::String(s), true) => {
                        let mut chars = s.chars();
                        match (chars.next(), chars.next()) {
                            (Some(c), None) => Scalar::Character(c),
                            _ => return fault(format!("\"{s}\" is not one character")),
                        }
                    }
                    (x, _) => x.clone(),
                };
                Ok(Fz { x, ..fz.clone() })
            });
        }
        _ => return Ok(v),
    };
    map_singles(v, &|fz| match level(&fz.x) {
        Some(from) if join(from, target) == target => Ok(Fz {
            x: widen(&fz.x, target),
            ..fz.clone()
        }),
        Some(_) => fault(format!("cannot store {} in {}", fz.x, to.a())),
        None => Ok(fz.clone()),
    })
}

fn map_singles(v: Value, f: &dyn Fn(&Fz) -> Computed<Fz>) -> Computed<Value> {
    Ok(match v {
        Value::One(fz) => Value::One(f(&fz)?),
        Value::Many(fzs) => Value::Many(fzs.iter().map(f).collect::<Computed<_>>()?),
        v => v,
    })
}

/// The value an array's cells start with: zero, false, a space, an empty
/// string.
pub fn zero(ty: &Ty) -> Value {
    Value::exact(match ty {
        Ty::Rational => Scalar::Rational(0, 1),
        Ty::Float => Scalar::Float(0.0),
        Ty::Gauss => Scalar::Gauss(0, 0),
        Ty::Complex => Scalar::Complex(0.0, 0.0),
        Ty::Quaternion => Scalar::Quaternion([0.0; 4]),
        Ty::Boolean => Scalar::Boolean(false),
        Ty::Character => Scalar::Character(' '),
        Ty::String => Scalar::String(String::new()),
        _ => Scalar::Integer(0),
    })
}

/// A rational in lowest terms, if it fits.
pub fn rational(n: i128, d: i128) -> Computed<Scalar> {
    if d == 0 {
        return fault("division by zero");
    }
    let g = gcd(n.unsigned_abs(), d.unsigned_abs()).max(1) as i128;
    let (mut n, mut d) = (n / g, d / g);
    if d < 0 {
        (n, d) = (-n, -d);
    }
    match (i64::try_from(n), i64::try_from(d)) {
        (Ok(n), Ok(d)) => Ok(Scalar::Rational(n, d)),
        _ => fault("integer overflow"),
    }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

fn overflow<T>(v: Option<T>) -> Computed<T> {
    v.ok_or_else(|| Fault("integer overflow".into()))
}

fn hamilton(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let [a1, b1, c1, d1] = a;
    let [a2, b2, c2, d2] = b;
    [
        a1 * a2 - b1 * b2 - c1 * c2 - d1 * d2,
        a1 * b2 + b1 * a2 + c1 * d2 - d1 * c2,
        a1 * c2 - b1 * d2 + c1 * a2 + d1 * b2,
        a1 * d2 + b1 * c2 - c1 * b2 + d1 * a2,
    ]
}

fn quaternion_inverse(q: [f64; 4]) -> Computed<[f64; 4]> {
    let norm = q.iter().map(|x| x * x).sum::<f64>();
    if norm == 0.0 {
        return fault("division by zero");
    }
    Ok([q[0] / norm, -q[1] / norm, -q[2] / norm, -q[3] / norm])
}

/// `+ - * /` on two numbers, widened to their join first. Two integers
/// divide truncating toward zero.
fn arith(op: BinaryOp, a: &Scalar, b: &Scalar) -> Computed<Scalar> {
    use Scalar::*;
    let (Some(la), Some(lb)) = (level(a), level(b)) else {
        return fault(format!("`{}` takes numbers", super::print::op_text(op)));
    };
    let to = join(la, lb);
    let (a, b) = (widen(a, to), widen(b, to));
    Ok(match (op, a, b) {
        (BinaryOp::Add, Integer(x), Integer(y)) => Integer(overflow(x.checked_add(y))?),
        (BinaryOp::Sub, Integer(x), Integer(y)) => Integer(overflow(x.checked_sub(y))?),
        (BinaryOp::Mul, Integer(x), Integer(y)) => Integer(overflow(x.checked_mul(y))?),
        (BinaryOp::Div, Integer(_), Integer(0)) => return fault("division by zero"),
        (BinaryOp::Div, Integer(x), Integer(y)) => Integer(overflow(x.checked_div(y))?),
        (op, Rational(n1, d1), Rational(n2, d2)) => {
            let (n1, d1, n2, d2) = (n1 as i128, d1 as i128, n2 as i128, d2 as i128);
            match op {
                BinaryOp::Add => rational(n1 * d2 + n2 * d1, d1 * d2)?,
                BinaryOp::Sub => rational(n1 * d2 - n2 * d1, d1 * d2)?,
                BinaryOp::Mul => rational(n1 * n2, d1 * d2)?,
                _ => rational(n1 * d2, d1 * n2)?,
            }
        }
        (BinaryOp::Add, Float(x), Float(y)) => Float(x + y),
        (BinaryOp::Sub, Float(x), Float(y)) => Float(x - y),
        (BinaryOp::Mul, Float(x), Float(y)) => Float(x * y),
        (BinaryOp::Div, Float(_), Float(0.0)) => return fault("division by zero"),
        (BinaryOp::Div, Float(x), Float(y)) => Float(x / y),
        (op, Gauss(a, b), Gauss(c, d)) => {
            let (a, b, c, d) = (a as i128, b as i128, c as i128, d as i128);
            let (re, im) = match op {
                BinaryOp::Add => (a + c, b + d),
                BinaryOp::Sub => (a - c, b - d),
                BinaryOp::Mul => (a * c - b * d, a * d + b * c),
                _ => {
                    let norm = c * c + d * d;
                    if norm == 0 {
                        return fault("division by zero");
                    }
                    let (re, im) = (a * c + b * d, b * c - a * d);
                    if re % norm != 0 || im % norm != 0 {
                        return fault("this Gauss integer division does not fall exact");
                    }
                    (re / norm, im / norm)
                }
            };
            match (i64::try_from(re), i64::try_from(im)) {
                (Ok(re), Ok(im)) => Gauss(re, im),
                _ => return fault("integer overflow"),
            }
        }
        (op, Complex(a, b), Complex(c, d)) => match op {
            BinaryOp::Add => Complex(a + c, b + d),
            BinaryOp::Sub => Complex(a - c, b - d),
            BinaryOp::Mul => Complex(a * c - b * d, a * d + b * c),
            _ => {
                let norm = c * c + d * d;
                if norm == 0.0 {
                    return fault("division by zero");
                }
                Complex((a * c + b * d) / norm, (b * c - a * d) / norm)
            }
        },
        (op, Quaternion(p), Quaternion(q)) => Quaternion(match op {
            BinaryOp::Add => [p[0] + q[0], p[1] + q[1], p[2] + q[2], p[3] + q[3]],
            BinaryOp::Sub => [p[0] - q[0], p[1] - q[1], p[2] - q[2], p[3] - q[3]],
            BinaryOp::Mul => hamilton(p, q),
            _ => hamilton(p, quaternion_inverse(q)?),
        }),
        _ => unreachable!("numbers widened to one level"),
    })
}

/// A real number's value, for the approximation's arithmetic.
fn real(s: &Scalar) -> Option<f64> {
    match s {
        Scalar::Integer(n) => Some(*n as f64),
        Scalar::Rational(n, d) => Some(*n as f64 / *d as f64),
        Scalar::Float(f) => Some(*f),
        _ => None,
    }
}

fn int_pow(base: i64, exponent: i64) -> Computed<i64> {
    let e = u32::try_from(exponent).map_err(|_| Fault("integer overflow".into()))?;
    overflow(base.checked_pow(e))
}

/// `^`: integers and rationals to an integer stay exact, a negative
/// integer exponent making a rational; complex numbers go polar.
fn pow(a: &Scalar, b: &Scalar) -> Computed<Scalar> {
    use Scalar::*;
    Ok(match (a, b) {
        (Integer(x), Integer(n)) if *n >= 0 => Integer(int_pow(*x, *n)?),
        (Integer(x), Integer(n)) => {
            if *x == 0 {
                return fault("division by zero");
            }
            rational(1, int_pow(*x, -n)? as i128)?
        }
        (Rational(p, q), Integer(n)) if *n >= 0 => {
            rational(int_pow(*p, *n)? as i128, int_pow(*q, *n)? as i128)?
        }
        (Rational(p, q), Integer(n)) => {
            rational(int_pow(*q, -n)? as i128, int_pow(*p, -n)? as i128)?
        }
        (Gauss(..) | Quaternion(_), Integer(n)) => {
            let to = if matches!(a, Gauss(..)) && *n >= 0 {
                Level::Gauss
            } else {
                Level::Quaternion
            };
            let base = widen(a, to);
            let mut result = widen(&Integer(1), to);
            for _ in 0..n.unsigned_abs() {
                result = arith(BinaryOp::Mul, &result, &base)?;
            }
            if *n < 0 {
                result = arith(BinaryOp::Div, &widen(&Integer(1), to), &result)?;
            }
            result
        }
        (Gauss(..) | Complex(..), e) => {
            let Some(e) = real(e) else {
                return fault("an exponent is a real number");
            };
            let Complex(re, im) = widen(a, Level::Complex) else {
                unreachable!()
            };
            let (r, theta) = (re.hypot(im), im.atan2(re));
            let r = r.powf(e);
            Complex(r * (theta * e).cos(), r * (theta * e).sin())
        }
        (Quaternion(_), _) => return fault("a quaternion to a non-integer power: not yet"),
        (x, e) => match (real(x), real(e)) {
            (Some(x), Some(e)) => {
                let v = x.powf(e);
                if v.is_nan() {
                    return fault("not a real number");
                }
                if v.is_infinite() {
                    return fault("division by zero");
                }
                Float(v)
            }
            _ => return fault("`^` takes numbers"),
        },
    })
}

fn negate(s: &Scalar) -> Computed<Scalar> {
    use Scalar::*;
    Ok(match s {
        Integer(n) => Integer(overflow(n.checked_neg())?),
        Rational(n, d) => Rational(overflow(n.checked_neg())?, *d),
        Float(f) => Float(-f),
        Gauss(a, b) => Gauss(overflow(a.checked_neg())?, overflow(b.checked_neg())?),
        Complex(a, b) => Complex(-a, -b),
        Quaternion(q) => Quaternion([-q[0], -q[1], -q[2], -q[3]]),
        _ => return fault("a sign applies to a number"),
    })
}

/// Text, for `&` and comparisons.
fn text(s: &Scalar) -> Option<String> {
    match s {
        Scalar::Character(c) => Some(c.to_string()),
        Scalar::String(s) => Some(s.clone()),
        _ => None,
    }
}

fn compare(op: BinaryOp, a: &Scalar, b: &Scalar) -> Computed<bool> {
    use BinaryOp::*;
    let ordering = match (a, b) {
        (Scalar::Boolean(x), Scalar::Boolean(y)) => x.cmp(y),
        _ if text(a).is_some() && text(b).is_some() => text(a).cmp(&text(b)),
        _ => {
            let (Some(la), Some(lb)) = (level(a), level(b)) else {
                return fault("these values do not compare");
            };
            let to = join(la, lb);
            match (widen(a, to), widen(b, to)) {
                (Scalar::Integer(x), Scalar::Integer(y)) => x.cmp(&y),
                (Scalar::Rational(n1, d1), Scalar::Rational(n2, d2)) => {
                    (n1 as i128 * d2 as i128).cmp(&(n2 as i128 * d1 as i128))
                }
                (Scalar::Float(x), Scalar::Float(y)) => {
                    x.partial_cmp(&y).unwrap_or(Ordering::Equal)
                }
                (x, y) if matches!(op, Eq | Ne) => {
                    return Ok((x == y) == (op == Eq));
                }
                _ => return fault("complex numbers have no order (page 27)"),
            }
        }
    };
    if matches!(a, Scalar::Boolean(_)) && !matches!(op, Eq | Ne) {
        return fault("booleans have no order");
    }
    Ok(match op {
        Eq => ordering == Ordering::Equal,
        Ne => ordering != Ordering::Equal,
        Lt => ordering == Ordering::Less,
        Le => ordering != Ordering::Greater,
        Gt => ordering == Ordering::Greater,
        _ => ordering != Ordering::Less,
    })
}

fn logic(op: BinaryOp, a: &Scalar, b: &Scalar) -> Computed<bool> {
    use BinaryOp::*;
    let (Scalar::Boolean(x), Scalar::Boolean(y)) = (a, b) else {
        return fault(format!("`{}` takes booleans", super::print::op_text(op)));
    };
    Ok(match op {
        Or => *x || *y,
        Nor => !(*x || *y),
        Xor => x != y,
        And => *x && *y,
        _ => !(*x && *y),
    })
}

// Fuzziness

fn probability(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(1.0) * b.unwrap_or(1.0)),
    }
}

fn opinion(a: &Option<String>, b: &Option<String>) -> Option<String> {
    match (a, b) {
        (Some(a), Some(b)) => Some(format!("{a}; {b}")),
        (a, b) => a.clone().or_else(|| b.clone()),
    }
}

/// The approximation of `a op b`, worst-case to first order.
fn approximation(op: BinaryOp, a: &Fz, b: &Fz) -> Computed<Option<f64>> {
    use BinaryOp::*;
    if a.appr.is_none() && b.appr.is_none() {
        return Ok(None);
    }
    let (Some(x), Some(y)) = (real(&a.x), real(&b.x)) else {
        return fault("the approximation of a complex number: not yet (page 153)");
    };
    let (dx, dy) = (a.appr.unwrap_or(0.0), b.appr.unwrap_or(0.0));
    Ok(Some(match op {
        Add | Sub => dx + dy,
        Mul => y.abs() * dx + x.abs() * dy,
        Div => dx / y.abs() + x.abs() * dy / (y * y),
        Pow => {
            let mut d = (y * x.powf(y - 1.0)).abs() * dx;
            if dy > 0.0 {
                if x <= 0.0 {
                    return fault("an approximate exponent needs a positive base");
                }
                d += (x.powf(y) * x.ln()).abs() * dy;
            }
            d
        }
        _ => return fault("an approximate value has no exact remainder or ratio"),
    }))
}

/// One binary operation on two single values.
pub fn binary_fz(op: BinaryOp, a: &Fz, b: &Fz) -> Computed<Fz> {
    use BinaryOp::*;
    let (x, appr) = match op {
        Or | Nor | Xor | And | Nand => (Scalar::Boolean(logic(op, &a.x, &b.x)?), None),
        Eq | Ne | Lt | Le | Gt | Ge => (Scalar::Boolean(compare(op, &a.x, &b.x)?), None),
        Concat => match (text(&a.x), text(&b.x)) {
            (Some(s), Some(t)) => (Scalar::String(s + &t), None),
            _ => return fault("`&` takes characters and strings"),
        },
        Add | Sub | Mul | Div => (arith(op, &a.x, &b.x)?, approximation(op, a, b)?),
        Pow => (pow(&a.x, &b.x)?, approximation(op, a, b)?),
        Mod | Ratio => {
            let (Scalar::Integer(x), Scalar::Integer(y)) = (&a.x, &b.x) else {
                return fault(format!("`{}` takes integers", super::print::op_text(op)));
            };
            if *y == 0 {
                return fault("division by zero");
            }
            let x = if op == Mod {
                Scalar::Integer(overflow(x.checked_rem_euclid(*y))?)
            } else {
                rational(*x as i128, *y as i128)?
            };
            (x, approximation(op, a, b)?)
        }
        _ => return fault("an array operator on single values"),
    };
    Ok(Fz {
        x,
        appr,
        prob: probability(a.prob, b.prob),
        subj: opinion(&a.subj, &b.subj),
    })
}

pub fn unary_fz(op: UnaryOp, a: &Fz) -> Computed<Fz> {
    let x = match op {
        UnaryOp::Neg => negate(&a.x)?,
        UnaryOp::Plus => {
            negate(&a.x)?;
            a.x.clone()
        }
        UnaryOp::Not => match a.x {
            Scalar::Boolean(b) => Scalar::Boolean(!b),
            _ => return fault("NOT takes a boolean"),
        },
    };
    Ok(Fz { x, ..a.clone() })
}

/// A function of a real variable and its derivative, applied with the
/// approximation it propagates: |f'(x)| dx.
pub fn real_function(
    a: &Fz,
    f: impl Fn(f64) -> f64,
    derivative: impl Fn(f64) -> f64,
) -> Computed<Fz> {
    let Some(x) = real(&a.x) else {
        return fault("a function of a real number, not of this one");
    };
    let v = f(x);
    if v.is_nan() || v.is_infinite() {
        return fault("outside the function's domain");
    }
    Ok(Fz {
        x: Scalar::Float(v),
        appr: a.appr.map(|d| derivative(x).abs() * d),
        ..a.clone()
    })
}

// Values: multiple values combine each with each.

fn singles(v: &Value) -> Computed<Vec<&Fz>> {
    match v {
        Value::One(fz) => Ok(vec![fz]),
        Value::Many(fzs) => Ok(fzs.iter().collect()),
        Value::Array(_) => fault("an array takes the array operators [+] [*] [.] [^]"),
        Value::List(_) => fault("a list takes UNION, INTER, COMPL and the list functors"),
        Value::Adt(_) => fault("an a.d.t. takes = and /=, and the functors"),
    }
}

fn lift(values: Vec<Fz>, many: bool) -> Value {
    if many {
        Value::Many(values)
    } else {
        Value::One(values.into_iter().next().expect("one value"))
    }
}

pub fn unary(op: UnaryOp, a: &Value) -> Computed<Value> {
    let many = matches!(a, Value::Many(_));
    let values = singles(a)?
        .into_iter()
        .map(|fz| unary_fz(op, fz))
        .collect::<Computed<_>>()?;
    Ok(lift(values, many))
}

pub fn binary(op: BinaryOp, a: &Value, b: &Value) -> Computed<Value> {
    use BinaryOp::*;
    match op {
        Union | Inter | Compl => return list_op(op, a, b),
        In | NotIn => {
            let Value::List(l) = b else {
                return fault("IN looks in a list");
            };
            return Ok(Value::boolean(l.items.contains(a) == (op == In)));
        }
        ArrayAdd | ArrayMul | ArrayDot | ArrayPow => return array_op(op, a, b),
        Eq | Ne if matches!(a, Value::Adt(_)) || matches!(b, Value::Adt(_)) => {
            return Ok(Value::boolean((a == b) == (op == Eq)));
        }
        _ => {}
    }
    let many = matches!(a, Value::Many(_)) || matches!(b, Value::Many(_));
    let mut values = Vec::new();
    for x in singles(a)? {
        for y in singles(b)? {
            values.push(binary_fz(op, x, y)?);
        }
    }
    Ok(lift(values, many))
}

fn list_op(op: BinaryOp, a: &Value, b: &Value) -> Computed<Value> {
    let (Value::List(l), Value::List(m)) = (a, b) else {
        return fault("UNION, INTER and COMPL take lists");
    };
    let items = match op {
        BinaryOp::Union => {
            let mut items = l.items.clone();
            items.extend(m.items.iter().filter(|x| !l.items.contains(x)).cloned());
            items
        }
        BinaryOp::Inter => l
            .items
            .iter()
            .filter(|x| m.items.contains(x))
            .cloned()
            .collect(),
        _ => l
            .items
            .iter()
            .filter(|x| !m.items.contains(x))
            .cloned()
            .collect(),
    };
    Ok(Value::List(List::new(items)))
}

/// Page 153: `[+]` adds arrays, `[*]` multiplies matrices or an array by a
/// number, `[.]` and `[^]` are the scalar and vectorial products.
fn array_op(op: BinaryOp, a: &Value, b: &Value) -> Computed<Value> {
    use BinaryOp::*;
    let cellwise = |x: &Array, f: &dyn Fn(&Value) -> Computed<Value>| -> Computed<Value> {
        let cells = x.cells.iter().map(f).collect::<Computed<_>>()?;
        Ok(Value::Array(Array { cells, ..x.clone() }))
    };
    match (op, a, b) {
        (ArrayAdd, Value::Array(x), Value::Array(y)) => {
            if x.dims != y.dims {
                return fault("[+] adds arrays of the same shape");
            }
            let cells = x
                .cells
                .iter()
                .zip(&y.cells)
                .map(|(p, q)| binary(Add, p, q))
                .collect::<Computed<_>>()?;
            Ok(Value::Array(Array { cells, ..x.clone() }))
        }
        (ArrayMul, Value::Array(x), Value::Array(y))
            if x.rank == Rank::Matrix && y.rank == Rank::Matrix =>
        {
            let (rows, inner, cols) = (x.dims[0], x.dims[1], y.dims[1]);
            if inner != y.dims[0] {
                return fault("[*] multiplies matrices whose shapes agree");
            }
            let mut cells = Vec::with_capacity(rows * cols);
            for r in 0..rows {
                for c in 0..cols {
                    let mut sum = binary(Mul, &x.cells[r * inner], &y.cells[c])?;
                    for k in 1..inner {
                        let term = binary(Mul, &x.cells[r * inner + k], &y.cells[k * cols + c])?;
                        sum = binary(Add, &sum, &term)?;
                    }
                    cells.push(sum);
                }
            }
            Ok(Value::Array(Array {
                rank: Rank::Matrix,
                lows: vec![x.lows[0], y.lows[1]],
                dims: vec![rows, cols],
                cells,
            }))
        }
        (ArrayMul, s, Value::Array(x)) | (ArrayMul, Value::Array(x), s)
            if !matches!(s, Value::Array(_)) =>
        {
            cellwise(x, &|c| binary(Mul, s, c))
        }
        (ArrayDot, Value::Array(x), Value::Array(y))
            if x.rank == Rank::Vector && y.rank == Rank::Vector =>
        {
            if x.dims != y.dims || x.cells.is_empty() {
                return fault("[.] takes two vectors of the same length");
            }
            let mut sum = binary(Mul, &x.cells[0], &y.cells[0])?;
            for (p, q) in x.cells.iter().zip(&y.cells).skip(1) {
                sum = binary(Add, &sum, &binary(Mul, p, q)?)?;
            }
            Ok(sum)
        }
        (ArrayPow, Value::Array(x), Value::Array(y))
            if x.rank == Rank::Vector && y.rank == Rank::Vector =>
        {
            if x.dims != [3] || y.dims != [3] {
                return fault("[^] takes two vectors of three components");
            }
            let (u, v) = (&x.cells, &y.cells);
            let term = |i: usize, j: usize| -> Computed<Value> {
                binary(
                    Sub,
                    &binary(Mul, &u[i], &v[j])?,
                    &binary(Mul, &u[j], &v[i])?,
                )
            };
            Ok(Value::Array(Array {
                cells: vec![term(1, 2)?, term(2, 0)?, term(0, 1)?],
                ..x.clone()
            }))
        }
        _ => fault(format!(
            "`{}` does not apply to these operands (page 153)",
            super::print::op_text(op)
        )),
    }
}

// Formatting, in the printer's literal syntax.

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
            Scalar::Quaternion([r, i, j, k]) => write!(
                f,
                "{}{}{}{}",
                float(*r),
                signed_part(*i, 'i'),
                signed_part(*j, 'j'),
                signed_part(*k, 'k')
            ),
            Scalar::Boolean(b) => f.write_str(if *b { "TRUE" } else { "FALSE" }),
            Scalar::Character(c) => write!(f, "{c}"),
            Scalar::String(s) => f.write_str(s),
        }
    }
}

impl fmt::Display for Fz {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.x)?;
        if let Some(a) = self.appr {
            write!(f, " +- {}", float(a))?;
        }
        if let Some(p) = self.prob {
            write!(f, " % {}", float(p))?;
        }
        if let Some(s) = &self.subj {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(x: f64, d: f64) -> Fz {
        Fz {
            appr: Some(d),
            ..Fz::exact(Scalar::Float(x))
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
    }

    /// The worst case of the first order, measured: the largest change of
    /// the result for changes of the operands within their approximations,
    /// on the box's corners, for small approximations.
    fn measured(op: BinaryOp, x: f64, dx: f64, y: f64, dy: f64) -> f64 {
        let at = |a: f64, b: f64| match pow_or(op, a, b) {
            Scalar::Float(v) => v,
            s => panic!("{s:?}"),
        };
        let centre = at(x, y);
        [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)]
            .iter()
            .map(|(s, t)| (at(x + s * dx, y + t * dy) - centre).abs())
            .fold(0.0, f64::max)
    }

    fn pow_or(op: BinaryOp, a: f64, b: f64) -> Scalar {
        let (a, b) = (Scalar::Float(a), Scalar::Float(b));
        if op == BinaryOp::Pow {
            pow(&a, &b).unwrap()
        } else {
            arith(op, &a, &b).unwrap()
        }
    }

    #[test]
    fn approximation_is_the_first_order_worst_case() {
        use BinaryOp::*;
        let (x, y, dx, dy) = (3.7, 1.9, 1e-6, 2e-6);
        for op in [Add, Sub, Mul, Div, Pow] {
            let z = binary_fz(op, &approx(x, dx), &approx(y, dy)).unwrap();
            let expected = measured(op, x, dx, y, dy);
            let ours = z.appr.unwrap();
            assert!(
                (ours - expected).abs() <= 1e-3 * expected,
                "{op:?}: {ours} vs {expected}"
            );
        }
    }

    #[test]
    fn page_152_product() {
        // X := 273.25 +- 0.22 ; Y := 122.34 +- 0.31 ; Z := X * Y
        let z = binary_fz(BinaryOp::Mul, &approx(273.25, 0.22), &approx(122.34, 0.31)).unwrap();
        assert!(matches!(z.x, Scalar::Float(v) if close(v, 273.25 * 122.34)));
        assert!(close(z.appr.unwrap(), 122.34 * 0.22 + 273.25 * 0.31));
    }

    #[test]
    fn probability_and_opinion() {
        let a = Fz {
            prob: Some(0.5),
            subj: Some("doubtful".into()),
            ..Fz::exact(Scalar::Integer(2))
        };
        let b = Fz {
            prob: Some(0.4),
            ..Fz::exact(Scalar::Integer(3))
        };
        let z = binary_fz(BinaryOp::Add, &a, &b).unwrap();
        assert!(close(z.prob.unwrap(), 0.2));
        assert_eq!(z.subj.as_deref(), Some("doubtful"));
        let c = Fz {
            subj: Some("unexpected".into()),
            ..Fz::exact(Scalar::Integer(1))
        };
        let z = binary_fz(BinaryOp::Mul, &a, &c).unwrap();
        assert_eq!(z.subj.as_deref(), Some("doubtful; unexpected"));
        assert_eq!(z.prob, Some(0.5));
    }

    #[test]
    fn multiple_values_combine_each_with_each() {
        let many =
            |xs: &[i64]| Value::Many(xs.iter().map(|x| Fz::exact(Scalar::Integer(*x))).collect());
        let z = binary(BinaryOp::Mul, &many(&[1, 2, 3, 4]), &many(&[10, 20, 30])).unwrap();
        let Value::Many(values) = z else { panic!() };
        assert_eq!(values.len(), 12);
        assert_eq!(values[5].x, Scalar::Integer(60));
    }

    #[test]
    fn numbers() {
        use BinaryOp::*;
        use Scalar::*;
        assert_eq!(rational(6, -4).unwrap(), Rational(-3, 2));
        assert_eq!(
            arith(Add, &Rational(1, 2), &Rational(1, 3)).unwrap(),
            Rational(5, 6)
        );
        assert_eq!(
            arith(Add, &Integer(1), &Rational(1, 2)).unwrap(),
            Rational(3, 2)
        );
        assert_eq!(arith(Div, &Integer(-7), &Integer(2)).unwrap(), Integer(-3));
        assert_eq!(
            arith(Mul, &Gauss(1, 2), &Gauss(3, -1)).unwrap(),
            Gauss(5, 5)
        );
        assert_eq!(
            arith(Div, &Gauss(5, 5), &Gauss(3, -1)).unwrap(),
            Gauss(1, 2)
        );
        assert!(arith(Div, &Gauss(1, 0), &Gauss(0, 2)).is_err());
        assert_eq!(
            arith(Add, &Gauss(1, 1), &Float(0.5)).unwrap(),
            Complex(1.5, 1.0)
        );
        // i j = k, j i = -k
        let (i, j) = (
            Quaternion([0.0, 1.0, 0.0, 0.0]),
            Quaternion([0.0, 0.0, 1.0, 0.0]),
        );
        assert_eq!(
            arith(Mul, &i, &j).unwrap(),
            Quaternion([0.0, 0.0, 0.0, 1.0])
        );
        assert_eq!(
            arith(Mul, &j, &i).unwrap(),
            Quaternion([0.0, 0.0, 0.0, -1.0])
        );
        assert_eq!(pow(&Integer(2), &Integer(-2)).unwrap(), Rational(1, 4));
        assert_eq!(pow(&Integer(2), &Integer(10)).unwrap(), Integer(1024));
        assert_eq!(pow(&Gauss(0, 1), &Integer(2)).unwrap(), Gauss(-1, 0));
        assert_eq!(
            arith(Mul, &Integer(i64::MAX), &Integer(2)),
            Err(Fault("integer overflow".into()))
        );
        assert_eq!(
            arith(Div, &Integer(1), &Integer(0)),
            Err(Fault("division by zero".into()))
        );
        assert!(compare(BinaryOp::Lt, &Rational(1, 3), &Float(0.34)).unwrap());
        assert!(compare(BinaryOp::Lt, &Complex(1.0, 0.0), &Complex(2.0, 0.0)).is_err());
        assert!(compare(BinaryOp::Eq, &Integer(2), &Complex(2.0, 0.0)).unwrap());
    }

    #[test]
    fn formatting() {
        let v = Value::One(Fz {
            x: Scalar::Float(25.3),
            appr: Some(0.37),
            prob: Some(0.73),
            subj: Some("conform".into()),
        });
        assert_eq!(v.to_string(), "25.3 +- 0.37 % 0.73 @\"conform\"");
        assert_eq!(Scalar::Complex(1.0, -2.0).to_string(), "1.0 - 2.0i");
        assert_eq!(Scalar::Gauss(1, -2).to_string(), "1 - 2i");
        assert_eq!(Scalar::Rational(3, 4).to_string(), "3 // 4");
        let m = Value::Array(Array {
            rank: Rank::Matrix,
            lows: vec![1, 1],
            dims: vec![2, 2],
            cells: (1..=4).map(Value::integer).collect(),
        });
        assert_eq!(m.to_string(), "[[1, 2], [3, 4]]");
    }
}
