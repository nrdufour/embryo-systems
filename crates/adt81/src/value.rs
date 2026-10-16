//! Values as ADT'81 uses them. What operations do to values is HBDS's,
//! in hbds-values (docs/architecture.md, decision 4); here ADT'81's
//! operators map onto its operations, and its types onto HBDS's.
//! docs/adt81.md, "Running", gives the rules.

use super::ast::{BinaryOp, UnaryOp};
use super::types::Ty;
pub use hbds_values::{
    Array, Base, Computed, Fault, Fz, List, Op, Rank, Scalar, Unary, Value, float, rational,
    real_function,
};

/// An array's rank as the store keeps it.
pub fn rank(r: super::ast::Rank) -> Rank {
    match r {
        super::ast::Rank::Vector => Rank::Vector,
        super::ast::Rank::Matrix => Rank::Matrix,
        super::ast::Rank::Tensor => Rank::Tensor,
    }
}

fn fault<T>(message: impl Into<String>) -> Computed<T> {
    Err(Fault(message.into()))
}

/// What ADT'81 asks of a value: an integer, or the truth of a condition.
pub trait Ask {
    fn as_integer(&self) -> Computed<i64>;
    /// The truth of a condition: a boolean, fuzzy or not, but one.
    fn truth(&self) -> Computed<bool>;
}

impl Ask for Value {
    fn as_integer(&self) -> Computed<i64> {
        match self.scalar() {
            Some(Scalar::Integer(n)) => Ok(*n),
            _ => fault(format!("{self} is not an integer")),
        }
    }

    fn truth(&self) -> Computed<bool> {
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

/// The operation an operator stands for; none for UNION, INTER and
/// COMPL, the list engine's.
pub fn op(o: BinaryOp) -> Option<Op> {
    use BinaryOp as B;
    Some(match o {
        B::Or => Op::Or,
        B::Nor => Op::Nor,
        B::Xor => Op::Xor,
        B::And => Op::And,
        B::Nand => Op::Nand,
        B::Eq => Op::Eq,
        B::Ne => Op::Ne,
        B::Lt => Op::Lt,
        B::Le => Op::Le,
        B::Gt => Op::Gt,
        B::Ge => Op::Ge,
        B::In => Op::In,
        B::NotIn => Op::NotIn,
        B::Union | B::Inter | B::Compl => return None,
        B::Concat => Op::Concat,
        B::Add => Op::Add,
        B::Sub => Op::Sub,
        B::ArrayAdd => Op::ArrayAdd,
        B::Mul => Op::Mul,
        B::Div => Op::Div,
        B::Mod => Op::Mod,
        B::Ratio => Op::Ratio,
        B::ArrayMul => Op::ArrayMul,
        B::ArrayDot => Op::ArrayDot,
        B::ArrayPow => Op::ArrayPow,
        B::Pow => Op::Pow,
    })
}

fn unary_op(o: UnaryOp) -> Unary {
    match o {
        UnaryOp::Neg => Unary::Neg,
        UnaryOp::Plus => Unary::Number,
        UnaryOp::Not => Unary::Not,
    }
}

/// The type HBDS converts to, for those it converts: numbers, booleans,
/// characters and strings.
fn base(ty: &Ty) -> Option<Base> {
    Some(match ty {
        Ty::Integer => Base::Integer,
        Ty::Rational => Base::Rational,
        Ty::Float => Base::Float,
        Ty::Gauss => Base::Gauss,
        Ty::Complex => Base::Complex,
        Ty::Quaternion => Base::Quaternion,
        Ty::Boolean => Base::Boolean,
        Ty::Character => Base::Character,
        Ty::String => Base::String,
        _ => return None,
    })
}

pub fn binary(o: BinaryOp, a: &Value, b: &Value) -> Computed<Value> {
    match op(o) {
        Some(o) => hbds_values::binary(o, a, b),
        None => list_op(o, a, b),
    }
}

pub fn unary(o: UnaryOp, a: &Value) -> Computed<Value> {
    hbds_values::unary(unary_op(o), a)
}

pub fn unary_fz(o: UnaryOp, a: &Fz) -> Computed<Fz> {
    hbds_values::unary_fz(unary_op(o), a)
}

/// A value converted to be stored as `to`.
pub fn convert(v: Value, to: &Ty) -> Computed<Value> {
    match base(to) {
        Some(b) => hbds_values::convert(v, b),
        None => Ok(v),
    }
}

/// The value an array's cells start with.
pub fn zero(ty: &Ty) -> Value {
    hbds_values::zero(base(ty).unwrap_or(Base::Integer))
}

/// UNION, INTER and COMPL: the list engine's.
fn list_op(o: BinaryOp, a: &Value, b: &Value) -> Computed<Value> {
    let (Value::List(l), Value::List(m)) = (a, b) else {
        return fault("UNION, INTER and COMPL take lists");
    };
    Ok(Value::List(match o {
        BinaryOp::Union => hbds_lists::union(l, m),
        BinaryOp::Inter => hbds_lists::inter(l, m),
        _ => hbds_lists::compl(l, m),
    }))
}
