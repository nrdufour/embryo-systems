//! The [`ast`](super::ast) written back as source, in one canonical form:
//! keywords in upper case, names as spelled, four spaces a level, and no
//! parenthesis the grammar does not need. Reading the output back gives
//! the same tree.

use std::fmt;

use super::ast::*;
use hbds_core::value::float;

impl fmt::Display for Program {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut p = Printer::default();
        for (i, module) in self.modules.iter().enumerate() {
            if i > 0 {
                p.out.push_str("\n\n");
            }
            p.module(module);
        }
        p.out.push('\n');
        f.write_str(&p.out)
    }
}

impl fmt::Display for Fragment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut p = Printer::default();
        for (i, item) in self.items.iter().enumerate() {
            if i > 0 {
                p.out.push_str(" ;\n");
            }
            match item {
                Item::Local(d) => p.out.push_str(&declaration(d)),
                Item::Parameter(s) => p.out.push_str(&parameter_spec(s)),
                Item::Stmt(s) => p.stmt(s),
            }
        }
        p.out.push('\n');
        f.write_str(&p.out)
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut p = Printer::default();
        p.expr(self, 0);
        f.write_str(&p.out)
    }
}

/// The expression with every operation in parentheses: how it was read,
/// `(a - (b * (c ^ 2)))`.
pub fn parenthesised(expr: &Expr) -> String {
    let mut p = Printer {
        full: true,
        ..Printer::default()
    };
    p.expr(expr, 0);
    p.out
}

#[derive(Default)]
struct Printer {
    out: String,
    indent: usize,
    /// Parenthesise every operation, not only where needed.
    full: bool,
}

fn join<T>(items: &[T], show: impl Fn(&T) -> String) -> String {
    items.iter().map(show).collect::<Vec<_>>().join(", ")
}

/// Words separated by spaces, empty ones left out.
fn words(parts: &[&str]) -> String {
    let parts: Vec<&str> = parts.iter().copied().filter(|p| !p.is_empty()).collect();
    parts.join(" ")
}

fn names(names: &[Name]) -> String {
    join(names, |n| n.spelling.clone())
}

/// A string literal: a quote inside is written twice.
fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

// Types and declarations

fn fuzziness(f: &Fuzziness) -> String {
    match f {
        Fuzziness::Fuzzy => "FUZZY".into(),
        Fuzziness::Kinds {
            appr,
            prob,
            subj,
            mult,
        } => {
            let kinds = [
                (appr, "APPR"),
                (prob, "PROB"),
                (subj, "SUBJ"),
                (mult, "MULT"),
            ];
            let present: Vec<&str> = kinds.iter().filter(|k| *k.0).map(|k| k.1).collect();
            present.join(" ")
        }
    }
}

fn base_type(base: &BaseType) -> String {
    match base {
        BaseType::Numeric { kind, long } => {
            let kind = match kind {
                NumericKind::Integer => "INTEGER",
                NumericKind::Float => "FLOAT",
                NumericKind::Complex => "COMPLEX",
                NumericKind::Gauss => "GAUSS",
                NumericKind::Quaternion => "QUATERNION",
                NumericKind::Rational => "RATIONAL",
            };
            words(&[if *long { "LONG" } else { "" }, kind])
        }
        BaseType::Boolean => "BOOLEAN".into(),
        BaseType::Character => "CHARACTER".into(),
        BaseType::String => "STRING".into(),
        BaseType::Adt(adt) => match adt {
            AdtType::Class => "CLASS",
            AdtType::CAttribute => "CATTRIBUTE",
            AdtType::CLink => "CLINK",
            AdtType::Object => "OBJECT",
            AdtType::OAttribute => "OATTRIBUTE",
            AdtType::OLink => "OLINK",
            AdtType::HClass => "HCLASS",
            AdtType::HAttribute => "HATTRIBUTE",
            AdtType::HLink => "HLINK",
            AdtType::Embryo => "EMBRYO",
            AdtType::Prototype => "PROTOTYPE",
            AdtType::Structure => "STRUCTURE",
        }
        .into(),
    }
}

fn scalar(t: &ScalarType) -> String {
    words(&[&base_type(&t.base), &fuzziness(&t.fuzziness)])
}

fn rank(r: Rank) -> &'static str {
    match r {
        Rank::Vector => "VECTOR",
        Rank::Matrix => "MATRIX",
        Rank::Tensor => "TENSOR",
    }
}

fn bound(b: &Bound) -> String {
    match b {
        Bound::Integer(i) => i.to_string(),
        Bound::Name(n) => n.spelling.clone(),
    }
}

fn list_type(element: &Option<ScalarType>) -> String {
    match element {
        Some(t) => format!("LIST OF {}", scalar(t)),
        None => "LIST".into(),
    }
}

fn data_type(t: &DataType) -> String {
    match t {
        DataType::Scalar(s) => scalar(s),
        DataType::Array {
            sparse,
            shape,
            element,
        } => {
            let bounds = if shape.bounds.is_empty() {
                String::new()
            } else {
                let b = join(&shape.bounds, |b| {
                    format!("{}:{}", bound(&b.low), bound(&b.high))
                });
                format!("[{b}]")
            };
            let sparse = if *sparse { "SPARSE" } else { "" };
            words(&[sparse, rank(shape.rank), &bounds, "OF", &scalar(element)])
        }
        DataType::List(element) => list_type(element),
        DataType::Compound => "COMPOUND".into(),
    }
}

/// A SPECIFY's specification, as written: `FLOAT APPR`, `VECTOR OF ...`.
pub fn specification(spec: &Specification) -> String {
    match spec {
        Specification::Scalar(s) => scalar(s),
        Specification::Array { rank: r, element } => match element {
            Some(e) => format!("{} OF {}", rank(*r), scalar(e)),
            None => rank(*r).into(),
        },
        Specification::List(element) => list_type(element),
        Specification::Compound => "COMPOUND".into(),
        Specification::Fuzziness(f) => fuzziness(f),
    }
}

fn mode(m: Mode) -> &'static str {
    match m {
        Mode::In => "IN",
        Mode::InOut => "INOUT",
        Mode::Out => "OUT",
    }
}

/// `+ 2.0` or `- 2.0`, the sign apart as the grammar writes it.
fn signed_part(x: f64, unit: &str) -> String {
    let sign = if x.is_sign_negative() { "-" } else { "+" };
    format!("{sign} {}{unit}", float(x.abs()))
}

fn constant(c: &Constant) -> String {
    match c {
        Constant::Integer(i) => i.to_string(),
        Constant::Float(f) => float(*f),
        Constant::Complex { re, im } => format!("{} {}", float(*re), signed_part(*im, "i")),
        Constant::Gauss { re, im } => {
            let sign = if *im < 0 { "-" } else { "+" };
            format!("{re} {sign} {}i", im.unsigned_abs())
        }
        Constant::Quaternion([r, i, j, k]) => format!(
            "{} {} {} {}",
            float(*r),
            signed_part(*i, "i"),
            signed_part(*j, "j"),
            signed_part(*k, "k")
        ),
        Constant::Rational(n, d) => format!("{n} // {d}"),
        Constant::Boolean(b) => boolean(*b).into(),
        Constant::String(s) => quoted(s),
    }
}

fn boolean(b: bool) -> &'static str {
    if b { "TRUE" } else { "FALSE" }
}

fn declaration(d: &Declaration) -> String {
    let init = match &d.init {
        None => String::new(),
        Some(Init::Initial(c)) => format!("INIT {}", constant(c)),
        Some(Init::Constant(c)) => format!("CONSTANTE = {}", constant(c)),
    };
    words(&[&names(&d.names), ":", &data_type(&d.ty), &init])
}

fn parameter_spec(s: &ParameterSpec) -> String {
    words(&[&names(&s.names), ":", &data_type(&s.ty), mode(s.mode)])
}

fn external(e: &External) -> String {
    let kind = match &e.kind {
        ExternalKind::Program => "PROGRAM".into(),
        ExternalKind::Subroutine => "SUBROUTINE".into(),
        ExternalKind::Coroutine => "COROUTINE".into(),
        ExternalKind::Function(t) => format!("FUNCTION RETURNS {}", scalar(t)),
        ExternalKind::Functor(t) => format!("FUNCTOR RETURNS {}", data_type(t)),
    };
    format!("{} : {kind}", names(&e.names))
}

// Expressions: the precedence ladder of grammar.lark, loosest first.

/// An operator's level: 1 for OR, up to 11 for `^`.
fn level(op: BinaryOp) -> u8 {
    use BinaryOp::*;
    match op {
        Or | Nor | Xor => 1,
        And | Nand => 2,
        Eq | Ne | Lt | Le | Gt | Ge | In | NotIn => 4,
        Union => 5,
        Inter => 6,
        Compl => 7,
        Concat => 8,
        Add | Sub | ArrayAdd => 9,
        Mul | Div | Mod | Ratio | ArrayMul | ArrayDot | ArrayPow => 10,
        Pow => 11,
    }
}

const NOT: u8 = 3;
const LINK: u8 = 4;
const SIGN: u8 = 9;
const OPERAND: u8 = 12;

/// The lowest level each side of an operator may hold unparenthesised:
/// chains are left-associative, and comparisons, COMPL and `^` take one
/// operator only.
fn sides(op: BinaryOp) -> (u8, u8) {
    match level(op) {
        4 => (5, 5),
        7 => (8, 8),
        11 => (OPERAND, OPERAND),
        l => (l, l + 1),
    }
}

fn expr_level(e: &Expr) -> u8 {
    match &e.kind {
        ExprKind::Binary(op, ..) => level(*op),
        ExprKind::Unary(UnaryOp::Not, _) => NOT,
        ExprKind::Unary(..) => SIGN,
        ExprKind::Link(..) => LINK,
        _ => OPERAND,
    }
}

pub(super) fn op_text(op: BinaryOp) -> &'static str {
    use BinaryOp::*;
    match op {
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
        Union => "UNION",
        Inter => "INTER",
        Compl => "COMPL",
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
    }
}

fn real(r: &Real) -> String {
    match r {
        Real::Integer(i) => i.to_string(),
        Real::Float(f) => float(*f),
    }
}

fn fuzzy_number(n: &FuzzyNumber) -> String {
    let mut out = match &n.value {
        Number::Real(r) => real(r),
        Number::Imaginary(r, unit) => {
            let unit = match unit {
                Unit::I => "i",
                Unit::J => "j",
                Unit::K => "k",
            };
            real(r) + unit
        }
    };
    if let Some(a) = &n.approximation {
        out += &format!(" +- {}", real(a));
    }
    if let Some(p) = &n.probability {
        out += &format!(" % {}", real(p));
    }
    if let Some(o) = &n.opinion {
        out += &format!(" @{}", quoted(o));
    }
    out
}

impl Printer {
    fn push(&mut self, text: &str) {
        self.out.push_str(text);
    }

    fn newline(&mut self) {
        self.out.push('\n');
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
    }

    fn sub(&self) -> Printer {
        Printer {
            full: self.full,
            ..Printer::default()
        }
    }

    fn expr_text(&self, e: &Expr, min: u8) -> String {
        let mut p = self.sub();
        p.expr(e, min);
        p.out
    }

    fn exprs(&self, es: &[Expr]) -> String {
        join(es, |e| self.expr_text(e, 0))
    }

    fn expr(&mut self, e: &Expr, min: u8) {
        let level = expr_level(e);
        let parens = level < min || (self.full && level < OPERAND);
        if parens {
            self.push("(");
        }
        match &e.kind {
            ExprKind::Binary(op, left, right) => {
                let (l, r) = sides(*op);
                self.expr(left, l);
                self.push(&format!(" {} ", op_text(*op)));
                self.expr(right, r);
            }
            ExprKind::Unary(op, operand) => {
                let (text, min) = match op {
                    UnaryOp::Not => ("NOT ", NOT),
                    UnaryOp::Neg => ("-", SIGN + 1),
                    UnaryOp::Plus => ("+", SIGN + 1),
                };
                self.push(text);
                self.expr(operand, min);
            }
            ExprKind::Link(from, relation, to) => {
                let text = format!(
                    "{} {} {}",
                    self.designation(from),
                    quoted(&relation.text),
                    self.designation(to)
                );
                self.push(&text);
            }
            ExprKind::Number(n) => self.push(&fuzzy_number(n)),
            ExprKind::Multiple(values) => {
                let values = join(values, |(negative, n)| {
                    let sign = if *negative { "-" } else { "" };
                    format!("{sign}{}", fuzzy_number(n))
                });
                self.push(&format!("{{ {values} }}"));
            }
            ExprKind::String(s) => self.push(&quoted(s)),
            ExprKind::Boolean(b) => self.push(boolean(*b)),
            ExprKind::This => self.push("THIS"),
            ExprKind::Variable(v) => {
                let text = self.variable(v);
                self.push(&text);
            }
            ExprKind::Call(c) => {
                let text = self.call(c);
                self.push(&text);
            }
            ExprKind::Functor(adt, calls) => {
                let mut text = self.adt(adt);
                for c in calls {
                    text += &format!(" ' {}", self.call(c));
                }
                self.push(&text);
            }
            ExprKind::Attribute(object, attribute) => {
                let text = format!(
                    "{} . {}",
                    self.designation(object),
                    self.adt_name(attribute)
                );
                self.push(&text);
            }
        }
        if parens {
            self.push(")");
        }
    }

    fn variable(&self, v: &Variable) -> String {
        if v.indices.is_empty() {
            v.name.spelling.clone()
        } else {
            format!("{}[{}]", v.name.spelling, self.exprs(&v.indices))
        }
    }

    fn call(&self, c: &Call) -> String {
        if c.args.is_empty() {
            c.name.spelling.clone()
        } else {
            format!("{}({})", c.name.spelling, self.exprs(&c.args))
        }
    }

    /// A list expression, as EACH, INSERT and EXTRACT take one: list
    /// operators over variables and functor results, anything else in
    /// parentheses.
    fn list(&self, e: &Expr, min: u8) -> String {
        let level = match &e.kind {
            ExprKind::Binary(BinaryOp::Union, ..) => 1,
            ExprKind::Binary(BinaryOp::Inter, ..) => 2,
            ExprKind::Binary(BinaryOp::Compl, ..) => 3,
            ExprKind::Variable(_) | ExprKind::Functor(..) => 4,
            _ => 0,
        };
        if level == 4 {
            return self.expr_text(e, OPERAND);
        }
        match &e.kind {
            ExprKind::Binary(op, left, right) if level >= min && !self.full => {
                let (l, r) = match level {
                    1 => (1, 2),
                    2 => (2, 3),
                    _ => (4, 4),
                };
                format!(
                    "{} {} {}",
                    self.list(left, l),
                    op_text(*op),
                    self.list(right, r)
                )
            }
            _ => format!("({})", self.expr_text(e, 0)),
        }
    }

    // Designations

    fn adt_name(&self, n: &AdtName) -> String {
        match n {
            AdtName::Name(n) => n.spelling.clone(),
            AdtName::String(s) => quoted(&s.text),
            AdtName::Expr(e) => match &e.kind {
                ExprKind::Call(c) => self.call(c),
                // Fully parenthesised already, if an operation.
                _ if self.full && expr_level(e) < OPERAND => self.expr_text(e, 0),
                _ => format!("({})", self.expr_text(e, 0)),
            },
        }
    }

    fn designation(&self, d: &Designation) -> String {
        match d {
            Designation::Named(n) => self.adt_name(n),
            Designation::Classed { name, class } => {
                format!("{} [ {} ]", self.adt_name(name), self.adt_name(class))
            }
            Designation::This(_) => "THIS".into(),
        }
    }

    fn adt(&self, a: &Adt) -> String {
        let d = self.designation(&a.designation);
        match &a.part {
            AdtPart::None => d,
            AdtPart::Attribute(n) => format!("{d} . {}", self.adt_name(n)),
            AdtPart::Link(r, to) => format!("{d} {} {}", quoted(&r.text), self.designation(to)),
        }
    }

    fn class_attribute(&self, a: &ClassAttribute) -> String {
        format!(
            "{} . {}",
            self.adt_name(&a.class),
            self.adt_name(&a.attribute)
        )
    }

    fn class_link(&self, l: &ClassLink) -> String {
        format!(
            "{} {} {}",
            self.adt_name(&l.from),
            quoted(&l.relation.text),
            self.adt_name(&l.to)
        )
    }

    fn classes(&self, cs: &[AdtName]) -> String {
        join(cs, |c| self.adt_name(c))
    }

    fn operands(&self, o: &Operands) -> String {
        match o {
            Operands::Designations(ds) => join(ds, |d| self.designation(d)),
            Operands::Attributes(attributes) => join(attributes, |a| self.class_attribute(a)),
            Operands::Links(links) => join(links, |l| {
                let link = self.class_link(&l.link);
                match &l.reverse {
                    Reverse::None => link,
                    Reverse::Named(n) => format!("{link} REVERSE {}", self.adt_name(n)),
                    Reverse::Reflexive => format!("{link} REFLEXIVE"),
                }
            }),
        }
    }

    // Modules and statements

    fn module(&mut self, m: &Module) {
        let name = match &m.name {
            ModuleName::Name(n) => n.spelling.clone(),
            ModuleName::Attribute { class, attribute } => {
                format!("{} . {}", quoted(&class.text), quoted(&attribute.text))
            }
            ModuleName::Body(class) => quoted(&class.text),
        };
        let parameters = if m.parameters.is_empty() {
            String::new()
        } else {
            format!("({})", names(&m.parameters))
        };
        let heading = match &m.kind {
            ModuleKind::Program => format!("PROGRAM {name}"),
            ModuleKind::Subroutine => words(&["SUBROUTINE", &name, &parameters]),
            ModuleKind::Coroutine => words(&["COROUTINE", &name, &parameters]),
            ModuleKind::Function { returns } => {
                words(&["FUNCTION", &name, &parameters, "RETURNS", &scalar(returns)])
            }
            ModuleKind::Functor { prefix, returns } => {
                let prefix = match prefix {
                    Some(p) => format!("{} '", p.spelling),
                    None => String::new(),
                };
                let returns = data_type(returns);
                words(&["FUNCTOR", &prefix, &name, &parameters, "RETURNS", &returns])
            }
        };
        self.push(&format!("{heading} ;"));
        self.indent += 1;
        for s in &m.specifications {
            self.newline();
            self.push(&format!("{} ;", parameter_spec(s)));
        }
        self.indent -= 1;
        if !m.locals.is_empty() {
            self.push("\nLOCAL");
            self.indent += 1;
            for d in &m.locals {
                self.newline();
                self.push(&format!("{} ;", declaration(d)));
            }
            self.indent -= 1;
        }
        if !m.externals.is_empty() {
            self.push("\nEXTERNAL");
            self.indent += 1;
            for e in &m.externals {
                self.newline();
                self.push(&format!("{} ;", external(e)));
            }
            self.indent -= 1;
        }
        self.push("\nBEGIN");
        self.block(&m.body);
        self.push("\nEND ;");
    }

    /// Statements one a line, a level in, `;` between them; the caller
    /// starts the next line.
    fn block(&mut self, stmts: &[Stmt]) {
        self.indent += 1;
        for (i, s) in stmts.iter().enumerate() {
            self.newline();
            self.stmt(s);
            if i + 1 < stmts.len() {
                self.push(" ;");
            }
        }
        self.indent -= 1;
    }

    /// A block, then its closing keyword on a line of its own.
    fn block_then(&mut self, stmts: &[Stmt], closing: &str) {
        self.block(stmts);
        self.newline();
        self.push(closing);
    }

    fn range(&self, r: &Range) -> String {
        let mut text = format!(
            "FOR {} : {} TO {}",
            r.name.spelling,
            self.expr_text(&r.from, 0),
            self.expr_text(&r.to, 0)
        );
        if let Some(step) = &r.step {
            text += &format!(" STEP {}", self.expr_text(step, 0));
        }
        text
    }

    fn stmt(&mut self, s: &Stmt) {
        let text = match &s.kind {
            StmtKind::Loop {
                range,
                condition,
                body,
                until,
            } => {
                let mut head = String::from("LOOP");
                if let Some(r) = range {
                    head += &format!(" {}", self.range(r));
                }
                if let Some(c) = condition {
                    head += &format!(" WHILE {}", self.expr_text(c, 0));
                }
                self.push(&format!("{head} DO"));
                self.block(body);
                if let Some(u) = until {
                    self.newline();
                    let until = format!("UNTIL {}", self.expr_text(u, 0));
                    self.push(&until);
                }
                self.newline();
                self.push("ENDLOOP");
                return;
            }
            StmtKind::Each {
                name,
                of,
                filter,
                body,
            } => {
                let mut head = format!("EACH {}", name.spelling);
                if let Some(of) = of {
                    head += &format!(" OF {}", self.list(of, 1));
                }
                if let Some(f) = filter {
                    head += &format!(" / {}", self.expr_text(f, 0));
                }
                self.push(&format!("{head} DO"));
                self.block_then(body, "ENDEACH");
                return;
            }
            StmtKind::Vect { range, body } => {
                let head = format!("VECT {} DO", self.range(range));
                self.push(&head);
                self.block_then(body, "ENDVECT");
                return;
            }
            StmtKind::Parallel(branches) => {
                self.push("PARBEGIN");
                for (i, b) in branches.iter().enumerate() {
                    if i > 0 {
                        self.newline();
                        self.push("PAR");
                    }
                    self.block(b);
                }
                self.newline();
                self.push("ENDPAR");
                return;
            }
            StmtKind::If {
                condition,
                then,
                otherwise,
            } => {
                let head = format!("IF {} THEN", self.expr_text(condition, 0));
                self.push(&head);
                self.block(then);
                if let Some(o) = otherwise {
                    self.newline();
                    self.push("ELSE");
                    self.block(o);
                }
                self.newline();
                self.push("ENDIF");
                return;
            }
            StmtKind::Case {
                subject,
                arms,
                others,
            } => {
                let head = format!("CASE {}", self.expr_text(subject, 0));
                self.push(&head);
                for arm in arms {
                    self.newline();
                    let when = format!("WHEN {} DO", self.exprs(&arm.values));
                    self.push(&when);
                    self.block(&arm.body);
                }
                if let Some(o) = others {
                    self.newline();
                    self.push("OTHERS");
                    self.block(o);
                }
                self.newline();
                self.push("ENDCASE");
                return;
            }
            StmtKind::Assign { target, value } => {
                format!("{} := {}", self.variable(target), self.expr_text(value, 0))
            }
            StmtKind::Exit { kind, count } => {
                let word = match kind {
                    ExitKind::Exit => "EXIT",
                    ExitKind::Next => "NEXT",
                    ExitKind::Reloop => "RELOOP",
                };
                match count {
                    Some(c) => format!("{word} {}", self.expr_text(c, 0)),
                    None => word.into(),
                }
            }
            StmtKind::Return => "RETURN".into(),
            StmtKind::Reproc => "REPROC".into(),
            StmtKind::Pause => "PAUSE".into(),
            StmtKind::Stop => "STOP".into(),
            StmtKind::Restart => "RESTART".into(),
            StmtKind::Reset(vs) => format!("RESET {}", join(vs, |v| self.variable(v))),
            StmtKind::Result(e) => format!("RESULT {}", self.expr_text(e, 0)),
            StmtKind::Insert {
                items,
                list,
                position,
            } => {
                let items = join(items, |i| self.list(i, 1));
                let mut text = format!("INSERT {items} INTO {}", list.spelling);
                if let Some((p, e)) = position {
                    let p = match p {
                        Position::After => "AFTER",
                        Position::Before => "BEFORE",
                    };
                    text += &format!(" {p} {}", self.expr_text(e, 0));
                }
                text
            }
            StmtKind::Extract { items, list } => {
                let items = join(items, |i| self.list(i, 1));
                format!("EXTRACT {items} OUTOF {}", list.spelling)
            }
            StmtKind::Empty(list) => format!("EMPTY {}", list.spelling),
            StmtKind::Organize { kind, lists } => {
                let word = match kind {
                    Organization::Linear => "LINEAR",
                    Organization::Circular => "CIRCULAR",
                    Organization::Single => "SINGLE",
                    Organization::Tuple => "TUPLE",
                };
                format!("{word} {}", names(lists))
            }
            StmtKind::Create(o) => format!("CREATE {}", self.operands(o)),
            StmtKind::StateChange(op, o) => {
                let word = match op {
                    StateOp::Hibern => "HIBERN",
                    StateOp::Awake => "AWAKE",
                    StateOp::Destroy => "DESTROY",
                    StateOp::Resur => "RESUR",
                    StateOp::Purge => "PURGE",
                };
                format!("{word} {}", self.operands(o))
            }
            StmtKind::Design(o) => format!("DESIGN {}", self.operands(o)),
            StmtKind::Let { target, value } => {
                let target = match target {
                    LetTarget::Variable(n) => n.spelling.clone(),
                    LetTarget::Attribute { object, attribute } => {
                        format!(
                            "{} . {}",
                            self.designation(object),
                            self.adt_name(attribute)
                        )
                    }
                    LetTarget::Property { adt, name } => {
                        format!("{} ' {}", self.adt(adt), name.spelling)
                    }
                };
                format!("LET {target} := {}", self.expr_text(value, 0))
            }
            StmtKind::Assert(Assertion::Name(n)) => format!("ASSERT {}", n.spelling),
            StmtKind::Assert(Assertion::Link {
                from,
                negated,
                relation,
                probability,
                to,
            }) => {
                let probability = match probability {
                    Some(p) => format!("% {}", real(p)),
                    None => String::new(),
                };
                words(&[
                    "ASSERT",
                    &self.designation(from),
                    if *negated { "NOT" } else { "" },
                    &quoted(&relation.text),
                    &probability,
                    &self.designation(to),
                ])
            }
            StmtKind::Specify {
                items,
                spec,
                unit,
                computed,
            } => {
                let items = join(items, |i| match i {
                    Specified::Class(c) => self.adt_name(c),
                    Specified::Attribute(a) => self.class_attribute(a),
                    Specified::Link(l) => self.class_link(l),
                });
                let spec = specification(spec);
                let unit = match unit {
                    Some(u) => format!("UNIT ({})", u.spelling),
                    None => String::new(),
                };
                let computed = if *computed { "COMPUTED" } else { "" };
                words(&["SPECIFY", &items, ":", &spec, &unit, computed])
            }
            StmtKind::Include { classes, into } => {
                format!(
                    "INCLUDE {} INTO {}",
                    self.classes(classes),
                    self.classes(into)
                )
            }
            StmtKind::Exclude { classes, from } => {
                format!(
                    "EXCLUDE {} OUTOF {}",
                    self.classes(classes),
                    self.classes(from)
                )
            }
            StmtKind::Associate(pairs) | StmtKind::Dissociate(pairs) => {
                let (word, link) = match &s.kind {
                    StmtKind::Associate(_) => ("ASSOC", "WITH"),
                    _ => ("DISSOC", "FROM"),
                };
                let pairs = join(pairs, |(a, d)| {
                    format!("{} {link} {}", self.adt(a), self.designation(d))
                });
                format!("{word} {pairs}")
            }
            StmtKind::Compose { class, with } => format!(
                "COMPOSE {} WITH {}",
                self.adt_name(class),
                self.classes(with)
            ),
            StmtKind::Build { classes, model } => format!(
                "BUILD {} AS {}",
                self.classes(classes),
                self.adt_name(model)
            ),
            StmtKind::Activate(o) => format!("ACTIVATE {}", self.designation(o)),
            StmtKind::Reactivate { object, when } => {
                let object = self.designation(object);
                match when {
                    None => format!("REACTIVATE {object}"),
                    Some(When::At(e)) => {
                        format!("REACTIVATE {object} AT {}", self.expr_text(e, 0))
                    }
                    Some(When::Before(o)) => {
                        format!("REACTIVATE {object} BEFORE {}", self.designation(o))
                    }
                    Some(When::After(o)) => {
                        format!("REACTIVATE {object} AFTER {}", self.designation(o))
                    }
                }
            }
            StmtKind::Cancel(o) => format!("CANCEL {}", self.designation(o)),
            StmtKind::Kill(o) => format!("KILL {}", self.designation(o)),
            StmtKind::Delay { object, duration } => format!(
                "DELAY {} FOR {}",
                self.designation(object),
                self.expr_text(duration, 0)
            ),
            StmtKind::Hold { object, duration } => match object {
                Some(o) => format!(
                    "HOLD {} FOR {}",
                    self.designation(o),
                    self.expr_text(duration, 0)
                ),
                None => format!("HOLD FOR {}", self.expr_text(duration, 0)),
            },
            StmtKind::Passivate => "PASSIVATE".into(),
            StmtKind::Terminate => "TERMINATE".into(),
            StmtKind::Chain(n) => format!("CHAIN {}", n.spelling),
            StmtKind::Call(c) => format!("CALL {}", self.call(c)),
            StmtKind::Resume(c) => format!("RESUME {}", self.call(c)),
            StmtKind::Read(vs) => format!("READ {}", join(vs, |v| self.variable(v))),
            StmtKind::Print(es) => format!("PRINT {}", self.exprs(es)),
        };
        self.push(&text);
    }
}
