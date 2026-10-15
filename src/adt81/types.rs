//! The types the checker reasons with: the declared types of the
//! [`ast`](super::ast), flattened, with the fuzziness a value carries
//! beside them. docs/adt81.md, "Kinds and types", gives the rules.
//!
//! Numbers widen: integer to rational, float, complex and quaternion, and
//! integer to Gauss, Gauss to complex. [`Ty::Unknown`] is what the program
//! cannot know (an object's attribute, an unknown functor's result): it
//! fits anywhere, so that only what is provably wrong is reported.

use std::fmt;

use super::ast::{AdtType, BaseType, DataType, Fuzziness, NumericKind, Rank, ScalarType};

#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    Unknown,
    Integer,
    Rational,
    Float,
    Gauss,
    Complex,
    Quaternion,
    Boolean,
    Character,
    String,
    Adt(AdtType),
    /// Of elements of a type, Unknown when the declaration gives none.
    List(Box<Ty>),
    Array(Rank, Box<Ty>),
    Compound,
}

/// The aspects of fuzziness a value carries, or a variable may receive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fuzz {
    pub appr: bool,
    pub prob: bool,
    pub subj: bool,
    pub mult: bool,
}

impl Fuzz {
    pub const NONE: Fuzz = Fuzz {
        appr: false,
        prob: false,
        subj: false,
        mult: false,
    };

    pub fn of(f: &Fuzziness) -> Fuzz {
        match *f {
            Fuzziness::Fuzzy => Fuzz {
                appr: true,
                prob: true,
                subj: true,
                mult: true,
            },
            Fuzziness::Kinds {
                appr,
                prob,
                subj,
                mult,
            } => Fuzz {
                appr,
                prob,
                subj,
                mult,
            },
        }
    }

    pub fn union(self, other: Fuzz) -> Fuzz {
        Fuzz {
            appr: self.appr || other.appr,
            prob: self.prob || other.prob,
            subj: self.subj || other.subj,
            mult: self.mult || other.mult,
        }
    }

    /// The aspects `self` carries that `target` does not take, by keyword.
    pub fn beyond(self, target: Fuzz) -> Vec<&'static str> {
        [
            (self.appr && !target.appr, "APPR"),
            (self.prob && !target.prob, "PROB"),
            (self.subj && !target.subj, "SUBJ"),
            (self.mult && !target.mult, "MULT"),
        ]
        .into_iter()
        .filter(|(beyond, _)| *beyond)
        .map(|(_, word)| word)
        .collect()
    }
}

/// A type and the fuzziness that goes with it.
#[derive(Debug, Clone, PartialEq)]
pub struct Typed {
    pub ty: Ty,
    pub fuzz: Fuzz,
}

impl Typed {
    pub fn plain(ty: Ty) -> Typed {
        Typed {
            ty,
            fuzz: Fuzz::NONE,
        }
    }

    pub const UNKNOWN: Typed = Typed {
        ty: Ty::Unknown,
        fuzz: Fuzz::NONE,
    };

    pub fn of_scalar(t: &ScalarType) -> Typed {
        let ty = match t.base {
            BaseType::Numeric { kind, .. } => match kind {
                NumericKind::Integer => Ty::Integer,
                NumericKind::Float => Ty::Float,
                NumericKind::Complex => Ty::Complex,
                NumericKind::Gauss => Ty::Gauss,
                NumericKind::Quaternion => Ty::Quaternion,
                NumericKind::Rational => Ty::Rational,
            },
            BaseType::Boolean => Ty::Boolean,
            BaseType::Character => Ty::Character,
            BaseType::String => Ty::String,
            BaseType::Adt(a) => Ty::Adt(a),
        };
        Typed {
            ty,
            fuzz: Fuzz::of(&t.fuzziness),
        }
    }

    /// An array or a list is as fuzzy as its elements (page 153).
    pub fn of(t: &DataType) -> Typed {
        match t {
            DataType::Scalar(s) => Typed::of_scalar(s),
            DataType::Array { shape, element, .. } => {
                let e = Typed::of_scalar(element);
                Typed {
                    ty: Ty::Array(shape.rank, Box::new(e.ty)),
                    fuzz: e.fuzz,
                }
            }
            DataType::List(element) => {
                let e = element.as_ref().map_or(Typed::UNKNOWN, Typed::of_scalar);
                Typed {
                    ty: Ty::List(Box::new(e.ty)),
                    fuzz: e.fuzz,
                }
            }
            DataType::Compound => Typed::plain(Ty::Compound),
        }
    }
}

/// What an a.d.t. is, whatever its exact type: hyperclasses, embryos and
/// prototypes are classes, and so on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdtKind {
    Class,
    Object,
    ClassAttribute,
    ObjectAttribute,
    ClassLink,
    ObjectLink,
}

impl AdtKind {
    pub fn of(t: AdtType) -> AdtKind {
        match t {
            AdtType::Class
            | AdtType::HClass
            | AdtType::Embryo
            | AdtType::Prototype
            | AdtType::Structure => AdtKind::Class,
            AdtType::Object => AdtKind::Object,
            AdtType::CAttribute | AdtType::HAttribute => AdtKind::ClassAttribute,
            AdtType::OAttribute => AdtKind::ObjectAttribute,
            AdtType::CLink | AdtType::HLink => AdtKind::ClassLink,
            AdtType::OLink => AdtKind::ObjectLink,
        }
    }
}

impl AdtKind {
    /// With its article: "a class", "an object".
    pub fn a(&self) -> String {
        match self {
            AdtKind::Object | AdtKind::ObjectAttribute | AdtKind::ObjectLink => {
                format!("an {self}")
            }
            _ => format!("a {self}"),
        }
    }

    pub fn plural(&self) -> String {
        match self {
            AdtKind::Class => "classes".into(),
            _ => format!("{self}s"),
        }
    }
}

impl fmt::Display for AdtKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            AdtKind::Class => "class",
            AdtKind::Object => "object",
            AdtKind::ClassAttribute => "class attribute",
            AdtKind::ObjectAttribute => "object attribute",
            AdtKind::ClassLink => "class link",
            AdtKind::ObjectLink => "object link",
        })
    }
}

/// The widening chains: integer, rational, float, complex, quaternion;
/// integer, Gauss, complex.
fn numeric_level(t: &Ty) -> Option<u8> {
    match t {
        Ty::Integer => Some(0),
        Ty::Rational => Some(1),
        Ty::Float => Some(2),
        Ty::Gauss => Some(1),
        Ty::Complex => Some(3),
        Ty::Quaternion => Some(4),
        _ => None,
    }
}

impl Ty {
    pub fn is_numeric(&self) -> bool {
        numeric_level(self).is_some()
    }

    /// Whether a value of type `self` may be used where `to` is expected.
    pub fn widens_to(&self, to: &Ty) -> bool {
        match (self, to) {
            (Ty::Unknown, _) | (_, Ty::Unknown) => true,
            (a, b) if a == b => true,
            // Gauss sits beside rational and float, not below them.
            (Ty::Gauss, Ty::Rational | Ty::Float) | (Ty::Rational | Ty::Float, Ty::Gauss) => false,
            (a, b) if a.is_numeric() && b.is_numeric() => numeric_level(a) <= numeric_level(b),
            (Ty::Character, Ty::String) => true,
            (Ty::List(a), Ty::List(b)) => a.widens_to(b),
            (Ty::Array(r, a), Ty::Array(s, b)) => r == s && a.widens_to(b),
            _ => false,
        }
    }

    /// The narrowest number both widen to.
    pub fn join(&self, other: &Ty) -> Option<Ty> {
        match (self, other) {
            (Ty::Unknown, _) | (_, Ty::Unknown) => Some(Ty::Unknown),
            (a, b) if a.widens_to(b) => Some(b.clone()),
            (a, b) if b.widens_to(a) => Some(a.clone()),
            // Gauss with rational or float.
            (a, b) if a.is_numeric() && b.is_numeric() => Some(Ty::Complex),
            _ => None,
        }
    }

    /// The a.d.t. kind, or of the elements of a list of a.d.t.
    pub fn adt_kind(&self) -> Option<AdtKind> {
        match self {
            Ty::Adt(t) => Some(AdtKind::of(*t)),
            Ty::List(e) => e.adt_kind(),
            _ => None,
        }
    }

    /// The type with its article, for messages: "an integer".
    pub fn a(&self) -> String {
        let text = self.to_string();
        let article = if text.starts_with(['a', 'e', 'i', 'o', 'u']) {
            "an"
        } else {
            "a"
        };
        format!("{article} {text}")
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ty::Unknown => f.write_str("value of unknown type"),
            Ty::Integer => f.write_str("integer"),
            Ty::Rational => f.write_str("rational"),
            Ty::Float => f.write_str("float"),
            Ty::Gauss => f.write_str("Gauss integer"),
            Ty::Complex => f.write_str("complex number"),
            Ty::Quaternion => f.write_str("quaternion"),
            Ty::Boolean => f.write_str("boolean"),
            Ty::Character => f.write_str("character"),
            Ty::String => f.write_str("string"),
            Ty::Adt(t) => f.write_str(match t {
                AdtType::Class => "class",
                AdtType::CAttribute => "class attribute",
                AdtType::CLink => "class link",
                AdtType::Object => "object",
                AdtType::OAttribute => "object attribute",
                AdtType::OLink => "object link",
                AdtType::HClass => "hyperclass",
                AdtType::HAttribute => "hyperclass attribute",
                AdtType::HLink => "hyperclass link",
                AdtType::Embryo => "embryo",
                AdtType::Prototype => "prototype",
                AdtType::Structure => "structure",
            }),
            Ty::List(e) if **e == Ty::Unknown => f.write_str("list"),
            Ty::List(e) => write!(f, "list of {e}"),
            Ty::Array(rank, e) => {
                let rank = match rank {
                    Rank::Vector => "vector",
                    Rank::Matrix => "matrix",
                    Rank::Tensor => "tensor",
                };
                if **e == Ty::Unknown {
                    f.write_str(rank)
                } else {
                    write!(f, "{rank} of {e}")
                }
            }
            Ty::Compound => f.write_str("compound"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widening() {
        assert!(Ty::Integer.widens_to(&Ty::Float));
        assert!(Ty::Integer.widens_to(&Ty::Gauss));
        assert!(Ty::Gauss.widens_to(&Ty::Complex));
        assert!(Ty::Rational.widens_to(&Ty::Quaternion));
        assert!(!Ty::Float.widens_to(&Ty::Integer));
        assert!(!Ty::Gauss.widens_to(&Ty::Float));
        assert!(!Ty::Float.widens_to(&Ty::Gauss));
        assert!(Ty::Character.widens_to(&Ty::String));
        assert!(!Ty::String.widens_to(&Ty::Character));
        assert!(Ty::Unknown.widens_to(&Ty::Boolean));
        assert!(Ty::Boolean.widens_to(&Ty::Unknown));
        let list = |t| Ty::List(Box::new(t));
        assert!(list(Ty::Integer).widens_to(&list(Ty::Float)));
        assert!(!list(Ty::Integer).widens_to(&Ty::Integer));
    }

    #[test]
    fn joins() {
        assert_eq!(Ty::Integer.join(&Ty::Float), Some(Ty::Float));
        assert_eq!(Ty::Gauss.join(&Ty::Float), Some(Ty::Complex));
        assert_eq!(Ty::Gauss.join(&Ty::Quaternion), Some(Ty::Quaternion));
        assert_eq!(Ty::Integer.join(&Ty::Boolean), None);
    }

    #[test]
    fn names() {
        assert_eq!(Ty::Integer.a(), "an integer");
        assert_eq!(
            Ty::List(Box::new(Ty::Adt(AdtType::Object))).a(),
            "a list of object"
        );
    }
}
