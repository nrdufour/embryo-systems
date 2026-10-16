//! The list engine (project 14; page 80): lists of any values, each with
//! a cursor, linear or circular; the list functors, which move the cursor
//! or take an element out; union, intersection and complement. The
//! arithmetic SIGMA needs is a language's, given to it.

use std::sync::Arc;

use hbds_core::value::{List, Scalar, Value};
use hbds_core::{Functor, Gives, Takes};

use Gives::*;
use Takes::*;

const fn f(name: &'static str, page: u16, takes: Takes, gives: Gives) -> Functor {
    Functor::new(name, page, takes, gives)
}

/// The list functors, upper case, with their page: page 80's, and SIGMA
/// (page 78), which adds a list's values.
pub const FUNCTORS: &[Functor] = &[
    f("SIGMA", 78, Lists, Unknown),
    f("FIRST", 80, Lists, Element),
    f("LAST", 80, Lists, Element),
    f("PRED", 80, Lists, Element),
    f("SUCC", 80, Lists, Element),
    f("CURRENT", 80, Lists, Element),
    f("FIRSTOUT", 80, Lists, Element),
    f("LASTOUT", 80, Lists, Element),
    f("CURRENTOUT", 80, Lists, Element),
    f("POSLP", 80, Lists, Integer),
    f("SUBLIST", 80, Lists, Same),
    f("OCCUR", 80, Lists, Integer),
    f("CARD", 80, Lists, Integer),
    f("KIND", 80, Lists, Unknown),
    f("TYPE", 80, Lists, Unknown),
    f("ELMT", 80, Lists, Element),
];

pub fn functor(key: &str) -> Option<&'static Functor> {
    FUNCTORS.iter().find(|f| f.name == key)
}

type Listed<T> = Result<T, String>;

fn integer(v: &Value) -> Listed<i64> {
    match v.scalar() {
        Some(Scalar::Integer(n)) => Ok(*n),
        _ => Err(format!("{v} is not an integer")),
    }
}

/// A list functor applied to `l`, which the functors that move the cursor
/// or take an element out change. `add` adds two values, for SIGMA.
pub fn apply(
    key: &str,
    l: &mut List,
    args: &[Value],
    add: &dyn Fn(&Value, &Value) -> Listed<Value>,
) -> Listed<Value> {
    let position = |v: &Value, l: &List| -> Listed<usize> {
        let p = integer(v)?;
        match usize::try_from(p) {
            Ok(p) if p >= 1 && p <= l.items.len() => Ok(p - 1),
            _ => Err(format!("no element {p} in a list of {}", l.items.len())),
        }
    };
    if l.items.is_empty() && !matches!(key, "CARD" | "SUBLIST" | "OCCUR" | "POSLP" | "SIGMA") {
        return Err("the list is empty".into());
    }
    let last = l.items.len().saturating_sub(1);
    Ok(match (key, args) {
        ("FIRST", []) => {
            l.cursor = 0;
            l.items[0].clone()
        }
        ("LAST", []) => {
            l.cursor = last;
            l.items[last].clone()
        }
        ("CURRENT", []) => l.items[l.cursor].clone(),
        ("SUCC", []) => {
            if l.cursor == last {
                if !l.circular {
                    return Err("the current element has no successor".into());
                }
                l.cursor = 0;
            } else {
                l.cursor += 1;
            }
            l.items[l.cursor].clone()
        }
        ("PRED", []) => {
            if l.cursor == 0 {
                if !l.circular {
                    return Err("the current element has no predecessor".into());
                }
                l.cursor = last;
            } else {
                l.cursor -= 1;
            }
            l.items[l.cursor].clone()
        }
        ("FIRSTOUT", []) => {
            l.cursor = 0;
            Arc::make_mut(&mut l.items).remove(0)
        }
        ("LASTOUT", []) => {
            l.cursor = l.cursor.min(last.saturating_sub(1));
            Arc::make_mut(&mut l.items).remove(last)
        }
        ("CURRENTOUT", []) => {
            let v = Arc::make_mut(&mut l.items).remove(l.cursor);
            l.cursor = l.cursor.min(l.items.len().saturating_sub(1));
            v
        }
        ("CARD", []) => Value::integer(l.items.len() as i64),
        ("ELMT", [p]) => l.items[position(p, l)?].clone(),
        ("SUBLIST", [p, n]) => {
            let start = position(p, l)?;
            let n = usize::try_from(integer(n)?).unwrap_or(0);
            let end = (start + n).min(l.items.len());
            Value::List(List::new(l.items[start..end].to_vec()))
        }
        ("OCCUR", [x, p]) => {
            let start = position(p, l)?;
            Value::integer(l.items[start..].iter().filter(|v| *v == x).count() as i64)
        }
        ("POSLP", [x, p]) => {
            let start = position(p, l)?;
            let found = l.items[start..].iter().position(|v| v == x);
            Value::integer(found.map_or(0, |i| (start + i + 1) as i64))
        }
        ("SIGMA", []) => {
            let mut sum = Value::integer(0);
            for v in l.items.iter() {
                sum = add(&sum, v)?;
            }
            sum
        }
        (name, _) => return Err(format!("`{name}` with these arguments: not yet")),
    })
}

/// UNION: `l`'s elements, then `m`'s that `l` lacks.
pub fn union(l: &List, m: &List) -> List {
    let mut items = (*l.items).clone();
    items.extend(m.items.iter().filter(|x| !l.items.contains(x)).cloned());
    List::new(items)
}

/// INTER: `l`'s elements that `m` has.
pub fn inter(l: &List, m: &List) -> List {
    List::new(
        l.items
            .iter()
            .filter(|x| m.items.contains(x))
            .cloned()
            .collect(),
    )
}

/// COMPL: `l`'s elements that `m` lacks.
pub fn compl(l: &List, m: &List) -> List {
    List::new(
        l.items
            .iter()
            .filter(|x| !m.items.contains(x))
            .cloned()
            .collect(),
    )
}

/// INSERT: `items` at `at`, counted from 0 (`AFTER k` is `k`, `BEFORE k`
/// is `k - 1`), or at the end.
pub fn insert(l: &mut List, items: Vec<Value>, at: Option<usize>) -> Listed<()> {
    let at = match at {
        None => l.items.len(),
        Some(k) if k <= l.items.len() => k,
        Some(_) => return Err("this position is outside the list".into()),
    };
    Arc::make_mut(&mut l.items).splice(at..at, items);
    Ok(())
}

/// EXTRACT: every element equal to one of `items` out.
pub fn extract(l: &mut List, items: &[Value]) {
    Arc::make_mut(&mut l.items).retain(|x| !items.contains(x));
    l.cursor = l.cursor.min(l.items.len().saturating_sub(1));
}

/// EMPTY.
pub fn empty(l: &mut List) {
    Arc::make_mut(&mut l.items).clear();
    l.cursor = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(xs: &[i64]) -> List {
        List::new(xs.iter().map(|x| Value::integer(*x)).collect())
    }

    fn add(a: &Value, b: &Value) -> Listed<Value> {
        match (a.scalar(), b.scalar()) {
            (Some(Scalar::Integer(x)), Some(Scalar::Integer(y))) => Ok(Value::integer(x + y)),
            _ => Err("integers only".into()),
        }
    }

    fn run(key: &str, l: &mut List, args: &[i64]) -> Listed<Value> {
        let args: Vec<Value> = args.iter().map(|x| Value::integer(*x)).collect();
        apply(key, l, &args, &add)
    }

    #[test]
    fn the_cursor_moves() {
        let mut l = list(&[10, 20, 30]);
        assert_eq!(run("FIRST", &mut l, &[]), Ok(Value::integer(10)));
        assert_eq!(run("SUCC", &mut l, &[]), Ok(Value::integer(20)));
        assert_eq!(run("CURRENT", &mut l, &[]), Ok(Value::integer(20)));
        assert_eq!(run("LAST", &mut l, &[]), Ok(Value::integer(30)));
        assert!(run("SUCC", &mut l, &[]).is_err());
        l.circular = true;
        assert_eq!(run("SUCC", &mut l, &[]), Ok(Value::integer(10)));
        assert_eq!(run("PRED", &mut l, &[]), Ok(Value::integer(30)));
    }

    #[test]
    fn functors() {
        let mut l = list(&[1, 2, 2, 3]);
        assert_eq!(run("CARD", &mut l, &[]), Ok(Value::integer(4)));
        assert_eq!(run("OCCUR", &mut l, &[2, 1]), Ok(Value::integer(2)));
        assert_eq!(run("POSLP", &mut l, &[3, 1]), Ok(Value::integer(4)));
        assert_eq!(run("ELMT", &mut l, &[2]), Ok(Value::integer(2)));
        assert_eq!(run("SIGMA", &mut l, &[]), Ok(Value::integer(8)));
        assert_eq!(
            run("SUBLIST", &mut l, &[2, 2]),
            Ok(Value::List(list(&[2, 2])))
        );
        assert_eq!(run("FIRSTOUT", &mut l, &[]), Ok(Value::integer(1)));
        assert_eq!(l, list(&[2, 2, 3]));
        assert_eq!(
            run("ELMT", &mut l, &[9]),
            Err("no element 9 in a list of 3".into())
        );
        assert_eq!(
            run("FIRST", &mut list(&[]), &[]),
            Err("the list is empty".into())
        );
    }

    #[test]
    fn operations() {
        let (a, b) = (list(&[1, 2, 3]), list(&[3, 4]));
        assert_eq!(union(&a, &b), list(&[1, 2, 3, 4]));
        assert_eq!(inter(&a, &b), list(&[3]));
        assert_eq!(compl(&a, &b), list(&[1, 2]));
        let mut l = list(&[1, 3]);
        insert(&mut l, vec![Value::integer(2)], Some(1)).unwrap();
        assert_eq!(l, list(&[1, 2, 3]));
        assert!(insert(&mut l, vec![], Some(9)).is_err());
        extract(&mut l, &[Value::integer(2)]);
        assert_eq!(l, list(&[1, 3]));
        empty(&mut l);
        assert_eq!(l, list(&[]));
    }
}
