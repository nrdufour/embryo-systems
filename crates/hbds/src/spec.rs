//! What SPECIFY says of an a.d.t.: its type, as HBDS knows types, with
//! the fuzziness it may take (page 152). Written and read in the words of
//! the print: `LONG FLOAT APPR`, `VECTOR OF COMPLEX`, `LIST OF OBJECT`.

use std::fmt;
use std::str::FromStr;

use crate::AdtType;
use crate::value::Rank;

/// The aspects of fuzziness a value may carry; FUZZY is all four.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fuzziness {
    pub appr: bool,
    pub prob: bool,
    pub subj: bool,
    pub mult: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scalar {
    pub base: Base,
    pub long: bool,
    pub fuzziness: Fuzziness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec {
    Scalar(Scalar),
    Array {
        rank: Rank,
        element: Option<Scalar>,
    },
    List(Option<Scalar>),
    Compound,
    /// Fuzziness alone, possibly none.
    Fuzziness(Fuzziness),
}

const BASES: &[(&str, Base)] = &[
    ("INTEGER", Base::Integer),
    ("RATIONAL", Base::Rational),
    ("FLOAT", Base::Float),
    ("GAUSS", Base::Gauss),
    ("COMPLEX", Base::Complex),
    ("QUATERNION", Base::Quaternion),
    ("BOOLEAN", Base::Boolean),
    ("CHARACTER", Base::Character),
    ("STRING", Base::String),
    ("CLASS", Base::Adt(AdtType::Class)),
    ("CATTRIBUTE", Base::Adt(AdtType::ClassAttribute)),
    ("CLINK", Base::Adt(AdtType::ClassLink)),
    ("OBJECT", Base::Adt(AdtType::Object)),
    ("OATTRIBUTE", Base::Adt(AdtType::ObjectAttribute)),
    ("OLINK", Base::Adt(AdtType::ObjectLink)),
    ("HCLASS", Base::Adt(AdtType::HyperClass)),
    ("HATTRIBUTE", Base::Adt(AdtType::HyperAttribute)),
    ("HLINK", Base::Adt(AdtType::HyperLink)),
    ("EMBRYO", Base::Adt(AdtType::Embryo)),
    ("PROTOTYPE", Base::Adt(AdtType::Prototype)),
    ("STRUCTURE", Base::Adt(AdtType::Structure)),
];

const RANKS: &[(&str, Rank)] = &[
    ("VECTOR", Rank::Vector),
    ("MATRIX", Rank::Matrix),
    ("TENSOR", Rank::Tensor),
];

fn words(parts: &[String]) -> String {
    parts
        .iter()
        .filter(|p| !p.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

impl fmt::Display for Fuzziness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kinds = [
            (self.appr, "APPR"),
            (self.prob, "PROB"),
            (self.subj, "SUBJ"),
            (self.mult, "MULT"),
        ];
        let present: Vec<&str> = kinds.iter().filter(|k| k.0).map(|k| k.1).collect();
        f.write_str(&present.join(" "))
    }
}

impl fmt::Display for Scalar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let base = BASES
            .iter()
            .find(|(_, b)| *b == self.base)
            .map_or("", |(n, _)| n);
        let long = if self.long { "LONG" } else { "" };
        f.write_str(&words(&[
            long.into(),
            base.into(),
            self.fuzziness.to_string(),
        ]))
    }
}

impl fmt::Display for Spec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let of = |e: &Option<Scalar>| e.map_or(String::new(), |e| format!("OF {e}"));
        let text = match self {
            Spec::Scalar(s) => s.to_string(),
            Spec::Array { rank, element } => {
                let r = RANKS.iter().find(|(_, x)| x == rank).map_or("", |(n, _)| n);
                words(&[r.into(), of(element)])
            }
            Spec::List(element) => words(&["LIST".into(), of(element)]),
            Spec::Compound => "COMPOUND".into(),
            Spec::Fuzziness(z) => z.to_string(),
        };
        f.write_str(&text)
    }
}

fn fuzziness(words: &[&str]) -> Result<Fuzziness, String> {
    let mut z = Fuzziness::default();
    for w in words {
        match *w {
            "APPR" => z.appr = true,
            "PROB" => z.prob = true,
            "SUBJ" => z.subj = true,
            "MULT" => z.mult = true,
            "FUZZY" => {
                z = Fuzziness {
                    appr: true,
                    prob: true,
                    subj: true,
                    mult: true,
                }
            }
            w => return Err(format!("{w:?} is not a fuzziness")),
        }
    }
    Ok(z)
}

fn scalar(words: &[&str]) -> Result<Scalar, String> {
    let (long, words) = match words {
        ["LONG", rest @ ..] => (true, rest),
        rest => (false, rest),
    };
    let [base, rest @ ..] = words else {
        return Err("a type is missing".into());
    };
    let base = BASES
        .iter()
        .find(|(n, _)| n == base)
        .map(|(_, b)| *b)
        .ok_or_else(|| format!("{base:?} is not a type"))?;
    Ok(Scalar {
        base,
        long,
        fuzziness: fuzziness(rest)?,
    })
}

impl FromStr for Spec {
    type Err = String;

    fn from_str(text: &str) -> Result<Spec, String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let element = |rest: &[&str]| match rest {
            [] => Ok(None),
            ["OF", rest @ ..] => scalar(rest).map(Some),
            _ => Err(format!("{text:?} is not a specification")),
        };
        match words.as_slice() {
            ["LIST", rest @ ..] => Ok(Spec::List(element(rest)?)),
            ["COMPOUND"] => Ok(Spec::Compound),
            [r, rest @ ..] if RANKS.iter().any(|(n, _)| n == r) => Ok(Spec::Array {
                rank: RANKS.iter().find(|(n, _)| n == r).unwrap().1,
                element: element(rest)?,
            }),
            all if fuzziness(all).is_ok() => Ok(Spec::Fuzziness(fuzziness(all)?)),
            all => scalar(all).map(Spec::Scalar),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trips() {
        for text in [
            "FLOAT APPR",
            "LONG INTEGER",
            "VECTOR OF COMPLEX APPR PROB SUBJ MULT",
            "MATRIX",
            "LIST OF OBJECT",
            "LIST",
            "COMPOUND",
            "PROB SUBJ",
            "",
            "HCLASS",
        ] {
            let spec: Spec = text.parse().unwrap();
            assert_eq!(spec.to_string(), text);
        }
        assert_eq!(
            "FLOAT FUZZY".parse::<Spec>().unwrap().to_string(),
            "FLOAT APPR PROB SUBJ MULT"
        );
        assert!("FLOAT BANANA".parse::<Spec>().is_err());
        assert!("VECTOR FLOAT".parse::<Spec>().is_err());
    }
}
