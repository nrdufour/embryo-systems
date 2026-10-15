//! What a part of a rule uses, read at capture as SES compiles rules
//! (page 80): the classes, attributes and link relations its PREMISES
//! read, or its other parts write. Only names written as strings count; a
//! name a variable holds is known only when the rule runs.

use hbds_core::fold;
use hbds_gtad::Code;
use hbds_rules::{Compiler, Uses};

use super::ast::*;
use super::interp::LANGUAGE;
use super::lower;

/// The properties that are boolean expressions; the others (ACTION, and
/// a rule's PROLOGUE, YES, NO and EPILOGUE) are statements.
pub fn is_expression(property: &str) -> bool {
    matches!(
        property,
        "COND" | "PREMISES" | "ASKABLE" | "EDITABLE" | "MODIFIABLE"
    )
}

/// A property's code, parsed as what it is.
pub enum Part {
    Expr(Expr),
    Stmts(Fragment),
}

pub fn parse(property: &str, text: &str) -> Result<Part, String> {
    if !is_expression(property) {
        return lower::parse_fragment(text)
            .map(Part::Stmts)
            .map_err(|e| e.to_string());
    }
    let parsed = lower::parse_fragment(&format!("x := {text}")).map_err(|e| e.to_string())?;
    match parsed.items.as_slice() {
        [
            Item::Stmt(Stmt {
                kind: StmtKind::Assign { value, .. },
                ..
            }),
        ] => Ok(Part::Expr(value.clone())),
        _ => Err("not an expression".into()),
    }
}

/// ADT'81's compiler, as SES needs it.
pub struct Adt81;

impl Compiler for Adt81 {
    fn uses(&self, part: &str, code: &Code) -> Result<Uses, String> {
        if code.language != LANGUAGE {
            return Err(format!(
                "is written in {}, which ADT'81 does not run",
                code.language
            ));
        }
        let mut walk = Walk::default();
        match parse(part, &code.text).map_err(|e| format!("does not parse: {e}"))? {
            Part::Expr(e) => walk.expr(&e),
            Part::Stmts(f) => {
                for item in &f.items {
                    if let Item::Stmt(s) = item {
                        walk.stmt(s);
                    }
                }
            }
        }
        Ok(walk.uses)
    }
}

#[derive(Default)]
struct Walk {
    uses: Uses,
}

fn add(names: &mut Vec<String>, name: &str) {
    if !names.iter().any(|n| fold(n) == fold(name)) {
        names.push(name.to_string());
    }
}

impl Walk {
    /// What an expression reads.
    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Number(_)
            | ExprKind::Multiple(_)
            | ExprKind::String(_)
            | ExprKind::Boolean(_) => {}
            ExprKind::Variable(v) => v.indices.iter().for_each(|i| self.expr(i)),
            ExprKind::Call(c) => c.args.iter().for_each(|a| self.expr(a)),
            ExprKind::Unary(_, x) => self.expr(x),
            ExprKind::Binary(_, a, b) => {
                self.expr(a);
                self.expr(b);
            }
            ExprKind::Functor(adt, calls) => {
                self.adt(adt);
                calls
                    .iter()
                    .flat_map(|c| &c.args)
                    .for_each(|a| self.expr(a));
            }
            ExprKind::Attribute(d, name) => {
                self.designation(d);
                self.name(name, |u| &mut u.attributes);
            }
            ExprKind::Link(a, relation, b) => {
                self.designation(a);
                self.designation(b);
                add(&mut self.uses.relations, &relation.text);
            }
        }
    }

    /// A string names what `field` lists; an expression is walked.
    fn name(&mut self, n: &AdtName, field: fn(&mut Uses) -> &mut Vec<String>) {
        match n {
            AdtName::String(s) => add(field(&mut self.uses), &s.text),
            AdtName::Expr(e) => self.expr(e),
            AdtName::Name(_) => {}
        }
    }

    /// A functor's prefix: a class read whole, an attribute, a link.
    fn adt(&mut self, a: &Adt) {
        if let Designation::Named(n) = &a.designation {
            self.name(n, |u| &mut u.classes);
        } else {
            self.designation(&a.designation);
        }
        match &a.part {
            AdtPart::None => {}
            AdtPart::Attribute(n) => self.name(n, |u| &mut u.attributes),
            AdtPart::Link(relation, to) => {
                add(&mut self.uses.relations, &relation.text);
                self.designation(to);
            }
        }
    }

    /// An object designated reads only what computes its names.
    fn designation(&mut self, d: &Designation) {
        let mut walk = |n: &AdtName| {
            if let AdtName::Expr(e) = n {
                self.expr(e);
            }
        };
        match d {
            Designation::Named(n) => walk(n),
            Designation::Classed { name, class } => {
                walk(name);
                walk(class);
            }
            Designation::This(_) => {}
        }
    }

    /// What statements write: objects created, values set, links
    /// asserted.
    fn stmt(&mut self, s: &Stmt) {
        let block = |w: &mut Walk, b: &[Stmt]| b.iter().for_each(|s| w.stmt(s));
        match &s.kind {
            StmtKind::Loop { body, .. }
            | StmtKind::Each { body, .. }
            | StmtKind::Vect { body, .. } => block(self, body),
            StmtKind::Parallel(branches) => branches.iter().for_each(|b| block(self, b)),
            StmtKind::If {
                then, otherwise, ..
            } => {
                block(self, then);
                block(self, otherwise.as_deref().unwrap_or_default());
            }
            StmtKind::Case { arms, others, .. } => {
                arms.iter().for_each(|a| block(self, &a.body));
                block(self, others.as_deref().unwrap_or_default());
            }
            StmtKind::Create(Operands::Designations(ds)) => {
                for d in ds {
                    if let Designation::Classed {
                        class: AdtName::String(c),
                        ..
                    } = d
                    {
                        add(&mut self.uses.classes, &c.text);
                    }
                }
            }
            StmtKind::Let {
                target:
                    LetTarget::Attribute {
                        attribute: AdtName::String(a),
                        ..
                    },
                ..
            } => add(&mut self.uses.attributes, &a.text),
            StmtKind::Assert(Assertion::Link {
                negated: false,
                relation,
                ..
            }) => add(&mut self.uses.relations, &relation.text),
            _ => {}
        }
    }
}
