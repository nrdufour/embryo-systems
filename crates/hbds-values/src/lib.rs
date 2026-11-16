//! What operations do to values, HBDS's whatever the language
//! (docs/architecture.md, decision 4): every number type and the widening
//! between them, comparisons, text, arrays (page 153), and fuzziness
//! carried along (pages 152 and 153). The values themselves are
//! hbds-core's data; a language maps its operators onto [`Op`].
//!
//! An approximation propagates worst-case, to first order: the absolute
//! contributions of each operand add up. A probability multiplies, as for
//! independent events, until rules give the law (page 152). Opinions are
//! concatenated. A multiple value combines with others value by value,
//! each with each.

use std::cmp::Ordering;
use std::fmt;

pub use hbds_core::spec::{Base, Fuzziness};
pub use hbds_core::value::{Array, Fz, List, Rank, Scalar, Value, float};

/// What went wrong, without where: a language adds the position.
#[derive(Debug, Clone, PartialEq)]
pub struct Fault(pub String);

pub type Computed<T> = Result<T, Fault>;

fn fault<T>(message: impl Into<String>) -> Computed<T> {
    Err(Fault(message.into()))
}

/// The integer a value is, for an index, a count or a bound.
pub fn integer(v: &Value) -> Computed<i64> {
    match v.scalar() {
        Some(Scalar::Integer(n)) => Ok(*n),
        _ => fault(format!("{v} is not an integer")),
    }
}

/// The most cells an array may have (docs/architecture.md, decision 4).
pub const CELLS: usize = 1 << 24;

/// An array's lows, its dimensions and how many cells it has, from its
/// bounds, each a low and a high; no cells without bounds.
pub fn shape(bounds: &[(i64, i64)]) -> Computed<(Vec<i64>, Vec<usize>, usize)> {
    let (mut lows, mut dims) = (Vec::new(), Vec::new());
    let mut size: usize = if bounds.is_empty() { 0 } else { 1 };
    for &(low, high) in bounds {
        if high < low {
            return fault(format!("the bounds {low}:{high} are empty"));
        }
        let dim = usize::try_from(high as i128 - low as i128 + 1).ok();
        let cells = dim.and_then(|d| size.checked_mul(d));
        let (Some(dim), Some(cells)) = (dim, cells.filter(|c| *c <= CELLS)) else {
            return fault(format!("an array of more than {CELLS} cells"));
        };
        lows.push(low);
        dims.push(dim);
        size = cells;
    }
    Ok((lows, dims, size))
}

/// Where the cell at `indices` is in an array's row-major cells.
pub fn offset(a: &Array, indices: &[i64]) -> Computed<usize> {
    if indices.len() != a.dims.len() {
        return fault(format!(
            "{} index(es) for an array of {}",
            indices.len(),
            a.dims.len()
        ));
    }
    let mut at = 0;
    for ((i, low), dim) in indices.iter().zip(&a.lows).zip(&a.dims) {
        match i.checked_sub(*low) {
            Some(k) if k >= 0 && k < *dim as i64 => at = at * dim + k as usize,
            _ => {
                return fault(format!(
                    "the index {i} is outside {low}:{}",
                    low + *dim as i64 - 1
                ));
            }
        }
    }
    Ok(at)
}

/// The truth of a condition: a boolean, fuzzy or not, but one.
pub fn truth(v: &Value) -> Computed<bool> {
    match v {
        Value::One(Fz {
            x: Scalar::Boolean(b),
            ..
        }) => Ok(*b),
        Value::Many(_) => fault("a multiple value is not a condition"),
        _ => fault(format!("{v} is not a boolean")),
    }
}

/// The operations on two values. Lists' union, intersection and
/// complement are the list engine's (hbds-lists).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Or,
    Nor,
    Xor,
    And,
    Nand,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// Membership in a list.
    In,
    NotIn,
    /// Text joined.
    Concat,
    Add,
    Sub,
    /// Arrays added cell by cell.
    ArrayAdd,
    Mul,
    /// The quotient: of integers an integer.
    Div,
    Mod,
    /// The exact quotient, a rational.
    Ratio,
    /// The matrix product, or an array times a number.
    ArrayMul,
    /// The scalar product.
    ArrayDot,
    /// The vectorial product.
    ArrayPow,
    Pow,
}

/// The operations on one value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unary {
    Neg,
    /// The value itself, which must be a number.
    Number,
    Not,
}

/// An operation as the print writes it, for messages.
impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Op::*;
        f.write_str(match self {
            Or => "OR",
            Nor => "NOR",
            Xor => "XOR",
            And => "AND",
            Nand => "NAND",
            Eq => "=",
            Ne => "/=",
            Lt => "<",
            Le => "<=",
            Gt => ">",
            Ge => ">=",
            In => "IN",
            NotIn => "NOTIN",
            Concat => "&",
            Add => "+",
            Sub => "-",
            ArrayAdd => "[+]",
            Mul => "*",
            Div => "/",
            Mod => "MOD",
            Ratio => "//",
            ArrayMul => "[*]",
            ArrayDot => "[.]",
            ArrayPow => "[^]",
            Pow => "^",
        })
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
    let quaternion = |r: f64, i: f64| Quaternion(Box::new([r, i, 0.0, 0.0]));
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
/// fault, and so is a value of another sort, a string for a number;
/// a.d.t. are not converted.
pub fn convert(v: Value, to: Base) -> Computed<Value> {
    let target = match to {
        Base::Integer => Level::Integer,
        Base::Rational => Level::Rational,
        Base::Float => Level::Float,
        Base::Gauss => Level::Gauss,
        Base::Complex => Level::Complex,
        Base::Quaternion => Level::Quaternion,
        Base::Character | Base::String => {
            let character = to == Base::Character;
            return map_singles(v, &|fz| {
                let x = match (&fz.x, character) {
                    (Scalar::Character(c), false) => Scalar::String(c.to_string().into()),
                    (Scalar::String(s), true) => {
                        let mut chars = s.chars();
                        match (chars.next(), chars.next()) {
                            (Some(c), None) => Scalar::Character(c),
                            _ => return fault(format!("\"{s}\" is not one character")),
                        }
                    }
                    (x @ (Scalar::Character(_) | Scalar::String(_)), _) => x.clone(),
                    (x, _) => return fault(format!("cannot store {x} in {}", a(to))),
                };
                Ok(fz.with(x))
            });
        }
        Base::Boolean => {
            return map_singles(v, &|fz| match fz.x {
                Scalar::Boolean(_) => Ok(fz.clone()),
                _ => fault(format!("cannot store {} in {}", fz.x, a(to))),
            });
        }
        Base::Adt(_) => return Ok(v),
    };
    map_singles(v, &|fz| match level(&fz.x) {
        Some(from) if join(from, target) == target => Ok(fz.with(widen(&fz.x, target))),
        _ => fault(format!("cannot store {} in {}", fz.x, a(to))),
    })
}

/// The aspects of fuzziness `v` carries that `allowed` does not take, by
/// keyword: what a variable not declared so may not receive (page 152).
pub fn beyond(v: &Value, allowed: Fuzziness) -> Vec<&'static str> {
    let singles: &[Fz] = match v {
        Value::One(fz) => std::slice::from_ref(fz),
        Value::Many(fzs) => fzs,
        _ => &[],
    };
    let carried = [
        (
            singles.iter().any(|fz| fz.appr().is_some()),
            allowed.appr,
            "APPR",
        ),
        (
            singles.iter().any(|fz| fz.prob().is_some()),
            allowed.prob,
            "PROB",
        ),
        (
            singles.iter().any(|fz| fz.subj().is_some()),
            allowed.subj,
            "SUBJ",
        ),
        (matches!(v, Value::Many(_)), allowed.mult, "MULT"),
    ];
    (carried.into_iter())
        .filter(|(carried, allowed, _)| *carried && !*allowed)
        .map(|(_, _, word)| word)
        .collect()
}

/// Refuses `v` for the variable `name`, declared with the fuzziness
/// `allowed`, if it carries more: what the checker says of a value whose
/// fuzziness it knows, said here of one known only when it arrives.
pub fn plain(v: &Value, allowed: Fuzziness, name: &str) -> Computed<()> {
    let beyond = beyond(v, allowed);
    if beyond.is_empty() {
        return Ok(());
    }
    fault(format!(
        "`{name}` is not declared {}, yet receives such a value (page 152)",
        beyond.join(" ")
    ))
}

/// A loop's counter after a step, or none when the sum is past what its
/// type holds: the loop is over, whatever its bound.
pub fn step(i: &Value, by: &Value) -> Computed<Option<Value>> {
    match binary(Op::Add, i, by) {
        Ok(v) => Ok(Some(v)),
        Err(Fault(m)) if m == INTEGER_OVERFLOW || m == FLOAT_OVERFLOW => Ok(None),
        Err(f) => Err(f),
    }
}

const INTEGER_OVERFLOW: &str = "integer overflow";
const FLOAT_OVERFLOW: &str = "float overflow";

/// A float that is a number: a result too large for a float is a fault,
/// as an integer's is, and so no infinity is ever stored, nor the NaN
/// that two of them make.
fn finite(x: f64) -> Computed<f64> {
    if x.is_finite() {
        Ok(x)
    } else {
        fault(FLOAT_OVERFLOW)
    }
}

/// A number type with its article, as messages name it.
fn a(base: Base) -> &'static str {
    match base {
        Base::Integer => "an integer",
        Base::Rational => "a rational",
        Base::Float => "a float",
        Base::Gauss => "a Gauss integer",
        Base::Complex => "a complex number",
        Base::Quaternion => "a quaternion",
        Base::Boolean => "a boolean",
        Base::Character => "a character",
        Base::String => "a string",
        _ => "this type",
    }
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
pub fn zero(base: Base) -> Value {
    Value::exact(match base {
        Base::Rational => Scalar::Rational(0, 1),
        Base::Float => Scalar::Float(0.0),
        Base::Gauss => Scalar::Gauss(0, 0),
        Base::Complex => Scalar::Complex(0.0, 0.0),
        Base::Quaternion => Scalar::Quaternion(Box::new([0.0; 4])),
        Base::Boolean => Scalar::Boolean(false),
        Base::Character => Scalar::Character(' '),
        Base::String => Scalar::String("".into()),
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
    v.ok_or_else(|| Fault(INTEGER_OVERFLOW.into()))
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
fn arith(op: Op, a: &Scalar, b: &Scalar) -> Computed<Scalar> {
    use Scalar::*;
    let (Some(la), Some(lb)) = (level(a), level(b)) else {
        return fault(format!("`{}` takes numbers", op));
    };
    let to = join(la, lb);
    let (a, b) = (widen(a, to), widen(b, to));
    Ok(match (op, a, b) {
        (Op::Add, Integer(x), Integer(y)) => Integer(overflow(x.checked_add(y))?),
        (Op::Sub, Integer(x), Integer(y)) => Integer(overflow(x.checked_sub(y))?),
        (Op::Mul, Integer(x), Integer(y)) => Integer(overflow(x.checked_mul(y))?),
        (Op::Div, Integer(_), Integer(0)) => return fault("division by zero"),
        (Op::Div, Integer(x), Integer(y)) => Integer(overflow(x.checked_div(y))?),
        (op, Rational(n1, d1), Rational(n2, d2)) => {
            let (n1, d1, n2, d2) = (n1 as i128, d1 as i128, n2 as i128, d2 as i128);
            match op {
                Op::Add => rational(n1 * d2 + n2 * d1, d1 * d2)?,
                Op::Sub => rational(n1 * d2 - n2 * d1, d1 * d2)?,
                Op::Mul => rational(n1 * n2, d1 * d2)?,
                _ => rational(n1 * d2, d1 * n2)?,
            }
        }
        (Op::Add, Float(x), Float(y)) => Float(finite(x + y)?),
        (Op::Sub, Float(x), Float(y)) => Float(finite(x - y)?),
        (Op::Mul, Float(x), Float(y)) => Float(finite(x * y)?),
        (Op::Div, Float(_), Float(0.0)) => return fault("division by zero"),
        (Op::Div, Float(x), Float(y)) => Float(finite(x / y)?),
        (op, Gauss(a, b), Gauss(c, d)) => {
            // Each product fits an i128; a sum of two may not.
            let (a, b, c, d) = (a as i128, b as i128, c as i128, d as i128);
            let (re, im) = match op {
                Op::Add => (a + c, b + d),
                Op::Sub => (a - c, b - d),
                Op::Mul => (
                    overflow((a * c).checked_sub(b * d))?,
                    overflow((a * d).checked_add(b * c))?,
                ),
                _ => {
                    let norm = overflow((c * c).checked_add(d * d))?;
                    if norm == 0 {
                        return fault("division by zero");
                    }
                    let re = overflow((a * c).checked_add(b * d))?;
                    let im = overflow((b * c).checked_sub(a * d))?;
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
        (op, Complex(a, b), Complex(c, d)) => {
            let (re, im) = match op {
                Op::Add => (a + c, b + d),
                Op::Sub => (a - c, b - d),
                Op::Mul => (a * c - b * d, a * d + b * c),
                _ => {
                    let norm = c * c + d * d;
                    if norm == 0.0 {
                        return fault("division by zero");
                    }
                    ((a * c + b * d) / norm, (b * c - a * d) / norm)
                }
            };
            Complex(finite(re)?, finite(im)?)
        }
        (op, Quaternion(p), Quaternion(q)) => {
            let r = match op {
                Op::Add => [p[0] + q[0], p[1] + q[1], p[2] + q[2], p[3] + q[3]],
                Op::Sub => [p[0] - q[0], p[1] - q[1], p[2] - q[2], p[3] - q[3]],
                Op::Mul => hamilton(*p, *q),
                _ => hamilton(*p, quaternion_inverse(*q)?),
            };
            for x in r {
                finite(x)?;
            }
            Quaternion(Box::new(r))
        }
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

fn int_pow(base: i64, exponent: u64) -> Computed<i64> {
    match (base, u32::try_from(exponent)) {
        (_, Ok(e)) => overflow(base.checked_pow(e)),
        (0 | 1, _) => Ok(base),
        (-1, _) => Ok(if exponent.is_multiple_of(2) { 1 } else { -1 }),
        _ => fault("integer overflow"),
    }
}

/// `^`: integers and rationals to an integer stay exact, a negative
/// integer exponent making a rational; complex numbers go polar.
fn pow(a: &Scalar, b: &Scalar) -> Computed<Scalar> {
    use Scalar::*;
    Ok(match (a, b) {
        (Integer(x), Integer(n)) if *n >= 0 => Integer(int_pow(*x, n.unsigned_abs())?),
        (Integer(x), Integer(n)) => {
            if *x == 0 {
                return fault("division by zero");
            }
            rational(1, int_pow(*x, n.unsigned_abs())? as i128)?
        }
        (Rational(p, q), Integer(n)) => {
            let e = n.unsigned_abs();
            let (p, q) = (int_pow(*p, e)? as i128, int_pow(*q, e)? as i128);
            if *n >= 0 {
                rational(p, q)?
            } else {
                rational(q, p)?
            }
        }
        (Gauss(..) | Quaternion(_), Integer(n)) => {
            let to = if matches!(a, Gauss(..)) && *n >= 0 {
                Level::Gauss
            } else {
                Level::Quaternion
            };
            // By squares: as many products as the exponent has bits.
            let mut square = widen(a, to);
            let mut result = widen(&Integer(1), to);
            let mut left = n.unsigned_abs();
            while left > 0 {
                if left & 1 == 1 {
                    result = arith(Op::Mul, &result, &square)?;
                }
                left >>= 1;
                if left > 0 {
                    square = arith(Op::Mul, &square, &square)?;
                }
            }
            if *n < 0 {
                result = arith(Op::Div, &widen(&Integer(1), to), &result)?;
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
            if r == 0.0 && e < 0.0 {
                return fault("division by zero");
            }
            let r = r.powf(e);
            Complex(
                finite(r * (theta * e).cos())?,
                finite(r * (theta * e).sin())?,
            )
        }
        (Quaternion(_), _) => return fault("a quaternion to a non-integer power: not yet"),
        (x, e) => match (real(x), real(e)) {
            (Some(x), Some(e)) => {
                let v = x.powf(e);
                if v.is_nan() {
                    return fault("not a real number");
                }
                if v.is_infinite() && x == 0.0 {
                    return fault("division by zero");
                }
                Float(finite(v)?)
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
        Quaternion(q) => Quaternion(Box::new(q.map(|x| -x))),
        _ => return fault("a sign applies to a number"),
    })
}

/// Text, for `&` and comparisons.
fn text(s: &Scalar) -> Option<String> {
    match s {
        Scalar::Character(c) => Some(c.to_string()),
        Scalar::String(s) => Some(s.to_string()),
        _ => None,
    }
}

fn compare(op: Op, a: &Scalar, b: &Scalar) -> Computed<bool> {
    use Op::*;
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

fn logic(op: Op, a: &Scalar, b: &Scalar) -> Computed<bool> {
    use Op::*;
    let (Scalar::Boolean(x), Scalar::Boolean(y)) = (a, b) else {
        return fault(format!("`{}` takes booleans", op));
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

fn opinion(a: Option<&str>, b: Option<&str>) -> Option<String> {
    match (a, b) {
        (Some(a), Some(b)) => Some(format!("{a}; {b}")),
        (a, b) => a.or(b).map(str::to_string),
    }
}

/// The approximation of `a op b`, worst-case to first order.
fn approximation(op: Op, a: &Fz, b: &Fz) -> Computed<Option<f64>> {
    use Op::*;
    if a.appr().is_none() && b.appr().is_none() {
        return Ok(None);
    }
    let (Some(x), Some(y)) = (real(&a.x), real(&b.x)) else {
        return fault("the approximation of a complex number: not yet (page 153)");
    };
    let (dx, dy) = (a.appr().unwrap_or(0.0), b.appr().unwrap_or(0.0));
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
pub fn binary_fz(op: Op, a: &Fz, b: &Fz) -> Computed<Fz> {
    use Op::*;
    let (x, appr) = match op {
        Or | Nor | Xor | And | Nand => (Scalar::Boolean(logic(op, &a.x, &b.x)?), None),
        Eq | Ne | Lt | Le | Gt | Ge => (Scalar::Boolean(compare(op, &a.x, &b.x)?), None),
        Concat => match (text(&a.x), text(&b.x)) {
            (Some(s), Some(t)) => (Scalar::String((s + &t).into()), None),
            _ => return fault("`&` takes characters and strings"),
        },
        Add | Sub | Mul | Div => (arith(op, &a.x, &b.x)?, approximation(op, a, b)?),
        Pow => (pow(&a.x, &b.x)?, approximation(op, a, b)?),
        Mod | Ratio => {
            let (Scalar::Integer(x), Scalar::Integer(y)) = (&a.x, &b.x) else {
                return fault(format!("`{}` takes integers", op));
            };
            if *y == 0 {
                return fault("division by zero");
            }
            let x = if op == Mod {
                // The smallest integer by -1 overflows as a division;
                // its remainder is 0.
                Scalar::Integer(x.checked_rem_euclid(*y).unwrap_or(0))
            } else {
                rational(*x as i128, *y as i128)?
            };
            (x, approximation(op, a, b)?)
        }
        _ => return fault("an array operator on single values"),
    };
    Ok(Fz::new(
        x,
        appr,
        probability(a.prob(), b.prob()),
        opinion(a.subj(), b.subj()),
    ))
}

pub fn unary_fz(op: Unary, a: &Fz) -> Computed<Fz> {
    let x = match op {
        Unary::Neg => negate(&a.x)?,
        Unary::Number => {
            negate(&a.x)?;
            a.x.clone()
        }
        Unary::Not => match a.x {
            Scalar::Boolean(b) => Scalar::Boolean(!b),
            _ => return fault("NOT takes a boolean"),
        },
    };
    Ok(a.with(x))
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
    Ok(Fz::new(
        Scalar::Float(v),
        a.appr().map(|d| derivative(x).abs() * d),
        a.prob(),
        a.subj().map(str::to_string),
    ))
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

pub fn unary(op: Unary, a: &Value) -> Computed<Value> {
    let many = matches!(a, Value::Many(_));
    let values = singles(a)?
        .into_iter()
        .map(|fz| unary_fz(op, fz))
        .collect::<Computed<_>>()?;
    Ok(lift(values, many))
}

pub fn binary(op: Op, a: &Value, b: &Value) -> Computed<Value> {
    use Op::*;
    match op {
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

/// Page 153: `[+]` adds arrays, `[*]` multiplies matrices or an array by a
/// number, `[.]` and `[^]` are the scalar and vectorial products.
fn array_op(op: Op, a: &Value, b: &Value) -> Computed<Value> {
    use Op::*;
    let cellwise = |x: &Array, f: &dyn Fn(&Value) -> Computed<Value>| -> Computed<Value> {
        let cells = x.cells.iter().map(f).collect::<Computed<Vec<_>>>()?;
        Ok(Value::Array(Box::new(Array {
            cells: cells.into(),
            ..x.clone()
        })))
    };
    match (op, a, b) {
        (ArrayAdd, Value::Array(x), Value::Array(y)) => {
            if x.dims != y.dims {
                return fault("[+] adds arrays of the same shape");
            }
            let cells = x
                .cells
                .iter()
                .zip(y.cells.iter())
                .map(|(p, q)| binary(Add, p, q))
                .collect::<Computed<Vec<_>>>()?;
            Ok(Value::Array(Box::new(Array {
                cells: cells.into(),
                ..(**x).clone()
            })))
        }
        (ArrayMul, Value::Array(x), Value::Array(y))
            if x.rank == Rank::Matrix && y.rank == Rank::Matrix =>
        {
            // A matrix declared without bounds has no dimensions yet.
            let (&[rows, inner], &[same, cols]) = (x.dims.as_slice(), y.dims.as_slice()) else {
                return fault("[*] multiplies matrices that have cells");
            };
            if inner != same {
                return fault("[*] multiplies matrices whose shapes agree");
            }
            if inner == 0 {
                return fault("[*] multiplies matrices that have cells");
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
            Ok(Value::Array(Box::new(Array {
                rank: Rank::Matrix,
                lows: vec![x.lows[0], y.lows[1]],
                dims: vec![rows, cols],
                cells: cells.into(),
            })))
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
            for (p, q) in x.cells.iter().zip(y.cells.iter()).skip(1) {
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
            Ok(Value::Array(Box::new(Array {
                cells: vec![term(1, 2)?, term(2, 0)?, term(0, 1)?].into(),
                ..(**x).clone()
            })))
        }
        _ => fault(format!(
            "`{}` does not apply to these operands (page 153)",
            op
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(x: f64, d: f64) -> Fz {
        Fz::new(Scalar::Float(x), Some(d), None, None)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
    }

    /// The worst case of the first order, measured: the largest change of
    /// the result for changes of the operands within their approximations,
    /// on the box's corners, for small approximations.
    fn measured(op: Op, x: f64, dx: f64, y: f64, dy: f64) -> f64 {
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

    fn pow_or(op: Op, a: f64, b: f64) -> Scalar {
        let (a, b) = (Scalar::Float(a), Scalar::Float(b));
        if op == Op::Pow {
            pow(&a, &b).unwrap()
        } else {
            arith(op, &a, &b).unwrap()
        }
    }

    #[test]
    fn approximation_is_the_first_order_worst_case() {
        use Op::*;
        let (x, y, dx, dy) = (3.7, 1.9, 1e-6, 2e-6);
        for op in [Add, Sub, Mul, Div, Pow] {
            let z = binary_fz(op, &approx(x, dx), &approx(y, dy)).unwrap();
            let expected = measured(op, x, dx, y, dy);
            let ours = z.appr().unwrap();
            assert!(
                (ours - expected).abs() <= 1e-3 * expected,
                "{op:?}: {ours} vs {expected}"
            );
        }
    }

    #[test]
    fn page_152_product() {
        // X := 273.25 +- 0.22 ; Y := 122.34 +- 0.31 ; Z := X * Y
        let z = binary_fz(Op::Mul, &approx(273.25, 0.22), &approx(122.34, 0.31)).unwrap();
        assert!(matches!(z.x, Scalar::Float(v) if close(v, 273.25 * 122.34)));
        assert!(close(z.appr().unwrap(), 122.34 * 0.22 + 273.25 * 0.31));
    }

    #[test]
    fn probability_and_opinion() {
        let a = Fz::new(Scalar::Integer(2), None, Some(0.5), Some("doubtful".into()));
        let b = Fz::new(Scalar::Integer(3), None, Some(0.4), None);
        let z = binary_fz(Op::Add, &a, &b).unwrap();
        assert!(close(z.prob().unwrap(), 0.2));
        assert_eq!(z.subj(), Some("doubtful"));
        let c = Fz::new(Scalar::Integer(1), None, None, Some("unexpected".into()));
        let z = binary_fz(Op::Mul, &a, &c).unwrap();
        assert_eq!(z.subj(), Some("doubtful; unexpected"));
        assert_eq!(z.prob(), Some(0.5));
    }

    #[test]
    fn multiple_values_combine_each_with_each() {
        let many =
            |xs: &[i64]| Value::Many(xs.iter().map(|x| Fz::exact(Scalar::Integer(*x))).collect());
        let z = binary(Op::Mul, &many(&[1, 2, 3, 4]), &many(&[10, 20, 30])).unwrap();
        let Value::Many(values) = z else { panic!() };
        assert_eq!(values.len(), 12);
        assert_eq!(values[5].x, Scalar::Integer(60));
    }

    #[test]
    fn powers_go_by_squares() {
        use Scalar::*;
        let slowly = |base: &Scalar, n: u32| {
            let mut result = widen(&Integer(1), level(base).unwrap());
            for _ in 0..n {
                result = arith(Op::Mul, &result, base).unwrap();
            }
            result
        };
        let q = Quaternion(Box::new([0.5, 0.25, -0.5, 1.0]));
        for n in 0..12 {
            let g = Gauss(1, -2);
            assert_eq!(pow(&g, &Integer(n as i64)).unwrap(), slowly(&g, n));
            let (Quaternion(fast), Quaternion(slow)) =
                (pow(&q, &Integer(n as i64)).unwrap(), slowly(&q, n))
            else {
                panic!()
            };
            assert!(fast.iter().zip(slow.iter()).all(|(x, y)| close(*x, *y)));
        }
        // 300 million products before; 29 now.
        assert_eq!(
            pow(&Gauss(0, 1), &Integer(300_000_000)).unwrap(),
            Gauss(1, 0)
        );
        assert!(pow(&Gauss(1, 1), &Integer(300_000_000)).is_err());
    }

    #[test]
    fn the_smallest_integer_is_an_error_not_a_panic() {
        use Scalar::*;
        let min = i64::MIN;
        assert!(pow(&Rational(2, 3), &Integer(min)).is_err());
        assert!(pow(&Integer(2), &Integer(min)).is_err());
        assert_eq!(pow(&Integer(-1), &Integer(i64::MAX)).unwrap(), Integer(-1));
        assert_eq!(pow(&Integer(1), &Integer(min)).unwrap(), Rational(1, 1));
        let g = Gauss(min, min);
        assert!(arith(Op::Mul, &g, &g).is_err());
        assert!(arith(Op::Div, &g, &g).is_err());
    }

    #[test]
    fn an_array_has_a_size() {
        assert_eq!(shape(&[]).unwrap(), (vec![], vec![], 0));
        assert_eq!(
            shape(&[(0, 9), (1, 3)]).unwrap(),
            (vec![0, 1], vec![10, 3], 30)
        );
        assert!(shape(&[(1, CELLS as i64)]).is_ok());
        for bounds in [
            vec![(3, 2)],
            vec![(0, CELLS as i64)],
            vec![(1, i64::MAX)],
            vec![(i64::MIN, i64::MAX)],
            vec![(1, 1 << 32), (1, 1 << 32)],
        ] {
            assert!(shape(&bounds).is_err(), "{bounds:?}");
        }
    }

    #[test]
    fn a_matrix_without_cells_does_not_multiply() {
        let matrix = |dims: &[usize]| {
            Value::Array(Box::new(Array {
                rank: Rank::Matrix,
                lows: vec![1; dims.len()],
                dims: dims.to_vec(),
                cells: Vec::new().into(),
            }))
        };
        for (x, y) in [(&[2, 0][..], &[0, 2][..]), (&[], &[])] {
            assert!(binary(Op::ArrayMul, &matrix(x), &matrix(y)).is_err());
        }
    }

    #[test]
    fn a_type_takes_its_sort_only() {
        use Scalar::*;
        let one = |x| Value::exact(x);
        assert_eq!(convert(one(Integer(3)), Base::Float), Ok(one(Float(3.0))));
        assert_eq!(
            convert(one(Character('a')), Base::String),
            Ok(one(String("a".into())))
        );
        for (x, to) in [
            (String("abc".into()), Base::Integer),
            (Boolean(true), Base::Float),
            (Integer(1), Base::String),
            (Integer(1), Base::Boolean),
            (Float(1.5), Base::Integer),
        ] {
            assert!(convert(one(x.clone()), to).is_err(), "{x:?} as {to:?}");
        }
    }

    #[test]
    fn fuzziness_beyond_what_is_declared() {
        let none = Fuzziness::default();
        let exact = Value::integer(1);
        assert!(beyond(&exact, none).is_empty());
        let approximate = Value::One(approx(1.0, 0.5));
        assert_eq!(beyond(&approximate, none), ["APPR"]);
        let appr = Fuzziness { appr: true, ..none };
        assert!(beyond(&approximate, appr).is_empty());
        let doubted = Fz::new(Scalar::Integer(1), None, Some(0.5), Some("so".into()));
        let many = Value::Many(vec![doubted]);
        assert_eq!(beyond(&many, appr), ["PROB", "SUBJ", "MULT"]);
        let e = plain(&many, appr, "n").unwrap_err().0;
        assert_eq!(
            e,
            "`n` is not declared PROB SUBJ MULT, yet receives such a value (page 152)"
        );
    }

    #[test]
    fn floats_stay_finite() {
        use Scalar::*;
        let big = Float(f64::MAX);
        for op in [Op::Add, Op::Mul] {
            assert_eq!(arith(op, &big, &big), fault("float overflow"));
        }
        assert_eq!(
            arith(Op::Sub, &big, &Float(-f64::MAX)),
            fault("float overflow")
        );
        assert_eq!(
            arith(Op::Div, &big, &Float(f64::MIN_POSITIVE)),
            fault("float overflow")
        );
        assert_eq!(pow(&Float(10.0), &Integer(400)), fault("float overflow"));
        assert_eq!(pow(&Float(0.0), &Integer(-1)), fault("division by zero"));
        let z = Complex(f64::MAX, 1.0);
        assert_eq!(arith(Op::Add, &z, &z), fault("float overflow"));
    }

    #[test]
    fn results_that_exist_at_the_edges() {
        let int = Value::integer;
        assert_eq!(binary(Op::Mod, &int(i64::MIN), &int(-1)), Ok(int(0)));
        assert_eq!(step(&int(i64::MAX - 1), &int(1)), Ok(Some(int(i64::MAX))));
        assert_eq!(step(&int(i64::MAX), &int(1)), Ok(None));
        assert_eq!(step(&int(i64::MIN), &int(-1)), Ok(None));
        let float = |x| Value::exact(Scalar::Float(x));
        assert_eq!(step(&float(f64::MAX), &float(f64::MAX)), Ok(None));
        assert!(step(&int(1), &Value::boolean(true)).is_err());
    }

    #[test]
    fn numbers() {
        use Op::*;
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
            Quaternion(Box::new([0.0, 1.0, 0.0, 0.0])),
            Quaternion(Box::new([0.0, 0.0, 1.0, 0.0])),
        );
        assert_eq!(
            arith(Mul, &i, &j).unwrap(),
            Quaternion(Box::new([0.0, 0.0, 0.0, 1.0]))
        );
        assert_eq!(
            arith(Mul, &j, &i).unwrap(),
            Quaternion(Box::new([0.0, 0.0, 0.0, -1.0]))
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
        assert!(compare(Op::Lt, &Rational(1, 3), &Float(0.34)).unwrap());
        assert!(compare(Op::Lt, &Complex(1.0, 0.0), &Complex(2.0, 0.0)).is_err());
        assert!(compare(Op::Eq, &Integer(2), &Complex(2.0, 0.0)).unwrap());
    }

    #[test]
    fn formatting() {
        let v = Value::One(Fz::new(
            Scalar::Float(25.3),
            Some(0.37),
            Some(0.73),
            Some("conform".into()),
        ));
        assert_eq!(v.to_string(), "25.3 +- 0.37 % 0.73 @\"conform\"");
        assert_eq!(Scalar::Complex(1.0, -2.0).to_string(), "1.0 - 2.0i");
        assert_eq!(Scalar::Gauss(1, -2).to_string(), "1 - 2i");
        assert_eq!(Scalar::Rational(3, 4).to_string(), "3 // 4");
        let m = Value::Array(Box::new(Array {
            rank: Rank::Matrix,
            lows: vec![1, 1],
            dims: vec![2, 2],
            cells: (1..=4).map(Value::integer).collect::<Vec<_>>().into(),
        }));
        assert_eq!(m.to_string(), "[[1, 2], [3, 4]]");
    }
}
