//! Random programs, for testing: every construct of the language, at
//! random, as trees the printer can write. Printing one and reading it back
//! must give the same tree, and Lark must read the printed program one way
//! only, to the parser's tree (`just check-grammar`).
//!
//! Deterministic: a seed gives the same program on every machine.

use super::ast::*;

/// xorshift64*, enough for test data.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    /// True one time in `n`.
    fn one_in(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

/// A program of one to three modules.
pub fn program(seed: u64) -> Program {
    let mut g = Gen(Rng::new(seed));
    let modules = (0..1 + g.0.below(3)).map(|_| g.module()).collect();
    Program { modules }
}

/// Declarations and statements, as a fragment.
pub fn fragment(seed: u64) -> Fragment {
    let mut g = Gen(Rng::new(seed));
    let items = (0..g.0.below(6))
        .map(|_| match g.0.below(4) {
            0 => Item::Local(g.declaration()),
            1 => Item::Parameter(g.parameter_spec()),
            _ => Item::Stmt(g.stmt(2)),
        })
        .collect();
    Fragment { items }
}

const NAMES: &[&str] = &[
    "a", "b", "x", "y", "n", "Total", "opscrc", "v2", "p_q", "L", "M", "s",
];
const MODULES: &[&str] = &["P", "S", "F", "K", "Sib", "CARD", "OPSC"];
const STRINGS: &[&str] = &[
    "A",
    "B",
    "STREAM",
    "x > 0",
    "r",
    "neighbor of",
    "",
    "THIS . \"temp\" > 0.",
];
const DEPTH: usize = 3;

struct Gen(Rng);

fn span() -> Span {
    Span::default()
}

impl Gen {
    fn some<T>(&mut self, make: impl FnOnce(&mut Self) -> T) -> Option<T> {
        if self.0.one_in(2) {
            Some(make(self))
        } else {
            None
        }
    }

    /// `min` to `min + 2` items.
    fn several<T>(&mut self, min: usize, mut make: impl FnMut(&mut Self) -> T) -> Vec<T> {
        let n = min + self.0.below(3);
        (0..n).map(|_| make(self)).collect()
    }

    fn name(&mut self) -> Name {
        Name::new(self.0.pick(NAMES), span())
    }

    fn module_name(&mut self) -> Name {
        Name::new(self.0.pick(MODULES), span())
    }

    fn string(&mut self) -> String {
        self.0.pick(STRINGS).to_string()
    }

    fn str(&mut self) -> Str {
        Str {
            text: self.string(),
            span: span(),
        }
    }

    fn signed(&mut self) -> i64 {
        let n = self.0.below(2000) as i64;
        if self.0.one_in(3) { -n } else { n }
    }

    fn float(&mut self) -> f64 {
        let f = self.0.below(100_000) as f64 / 8.0;
        if self.0.one_in(3) { -f } else { f }
    }

    // Types

    fn fuzziness(&mut self) -> Fuzziness {
        if self.0.one_in(8) {
            return Fuzziness::Fuzzy;
        }
        Fuzziness::Kinds {
            appr: self.0.one_in(3),
            prob: self.0.one_in(3),
            subj: self.0.one_in(3),
            mult: self.0.one_in(3),
        }
    }

    fn scalar(&mut self) -> ScalarType {
        let base = match self.0.below(5) {
            0 | 1 => BaseType::Numeric {
                kind: self.0.pick(&[
                    NumericKind::Integer,
                    NumericKind::Float,
                    NumericKind::Complex,
                    NumericKind::Gauss,
                    NumericKind::Quaternion,
                    NumericKind::Rational,
                ]),
                long: self.0.one_in(3),
            },
            2 => self
                .0
                .pick(&[BaseType::Boolean, BaseType::Character, BaseType::String]),
            _ => BaseType::Adt(self.0.pick(&[
                AdtType::Class,
                AdtType::CAttribute,
                AdtType::CLink,
                AdtType::Object,
                AdtType::OAttribute,
                AdtType::OLink,
                AdtType::HClass,
                AdtType::HAttribute,
                AdtType::HLink,
                AdtType::Embryo,
                AdtType::Prototype,
                AdtType::Structure,
            ])),
        };
        ScalarType {
            base,
            fuzziness: self.fuzziness(),
        }
    }

    fn rank(&mut self) -> Rank {
        self.0.pick(&[Rank::Vector, Rank::Matrix, Rank::Tensor])
    }

    fn bound(&mut self) -> Bound {
        if self.0.one_in(3) {
            Bound::Name(self.name())
        } else {
            Bound::Integer(self.signed())
        }
    }

    /// A data type; arrays without bounds where `bounds` is false, as a
    /// functor's result has them.
    fn data_type(&mut self, bounds: bool) -> DataType {
        match self.0.below(6) {
            0 | 1 => DataType::Scalar(self.scalar()),
            2 | 3 => {
                let rank = self.rank();
                let dimensions = match rank {
                    Rank::Vector => 1,
                    Rank::Matrix => 2,
                    Rank::Tensor => 3 + self.0.below(2),
                };
                let bounds = if bounds && self.0.one_in(2) {
                    (0..dimensions)
                        .map(|_| Bounds {
                            low: self.bound(),
                            high: self.bound(),
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                DataType::Array {
                    sparse: self.0.one_in(3),
                    shape: Shape { rank, bounds },
                    element: self.scalar(),
                }
            }
            4 => DataType::List(self.some(Self::scalar)),
            _ => DataType::Compound,
        }
    }

    fn constant(&mut self) -> Constant {
        match self.0.below(8) {
            0 => Constant::Integer(self.signed()),
            1 => Constant::Float(self.float()),
            2 => Constant::Complex {
                re: self.float(),
                im: self.float(),
            },
            3 => Constant::Gauss {
                re: self.signed(),
                im: self.signed(),
            },
            4 => Constant::Quaternion([self.float(), self.float(), self.float(), self.float()]),
            5 => Constant::Rational(self.signed(), self.0.below(100) as u64),
            6 => Constant::Boolean(self.0.one_in(2)),
            _ => Constant::String(self.string()),
        }
    }

    fn declaration(&mut self) -> Declaration {
        let ty = self.data_type(true);
        let init = match ty {
            DataType::Scalar(_) => match self.0.below(3) {
                0 => None,
                1 => Some(Init::Initial(self.constant())),
                _ => Some(Init::Constant(self.constant())),
            },
            DataType::Array { .. } => self.some(|g| Init::Initial(g.constant())),
            _ => None,
        };
        Declaration {
            names: self.several(1, Self::name),
            ty,
            init,
            span: span(),
        }
    }

    fn parameter_spec(&mut self) -> ParameterSpec {
        ParameterSpec {
            names: self.several(1, Self::name),
            ty: self.data_type(true),
            mode: self.0.pick(&[Mode::In, Mode::InOut, Mode::Out]),
            span: span(),
        }
    }

    fn external(&mut self) -> External {
        let kind = match self.0.below(5) {
            0 => ExternalKind::Program,
            1 => ExternalKind::Subroutine,
            2 => ExternalKind::Coroutine,
            3 => ExternalKind::Function(self.scalar()),
            _ => ExternalKind::Functor(self.data_type(false)),
        };
        External {
            names: self.several(1, Self::module_name),
            kind,
        }
    }

    fn module(&mut self) -> Module {
        let kind = match self.0.below(5) {
            0 => ModuleKind::Program,
            1 => ModuleKind::Subroutine,
            2 => ModuleKind::Coroutine,
            3 => ModuleKind::Function {
                returns: self.scalar(),
            },
            _ => ModuleKind::Functor {
                prefix: self.some(Self::name),
                returns: self.data_type(false),
            },
        };
        let name = match kind {
            ModuleKind::Function { .. } if self.0.one_in(3) => ModuleName::Attribute {
                class: self.str(),
                attribute: self.str(),
            },
            ModuleKind::Coroutine if self.0.one_in(3) => ModuleName::Body(self.str()),
            _ => ModuleName::Name(self.module_name()),
        };
        let (parameters, specifications) = match kind {
            ModuleKind::Program => (Vec::new(), Vec::new()),
            _ => (
                self.several(0, Self::name),
                self.several(0, Self::parameter_spec),
            ),
        };
        Module {
            kind,
            name,
            parameters,
            specifications,
            locals: self.several(0, Self::declaration),
            externals: self.several(0, Self::external),
            body: self.several(0, |g| g.stmt(DEPTH)),
            span: span(),
        }
    }

    // Expressions

    fn real(&mut self) -> Real {
        if self.0.one_in(2) {
            Real::Integer(self.0.below(1000) as u64)
        } else {
            Real::Float(self.float().abs())
        }
    }

    fn fuzzy_number(&mut self) -> FuzzyNumber {
        let value = match self.real() {
            Real::Integer(i) if self.0.one_in(4) => Number::Imaginary(Real::Integer(i), Unit::I),
            Real::Float(f) if self.0.one_in(3) => {
                Number::Imaginary(Real::Float(f), self.0.pick(&[Unit::I, Unit::J, Unit::K]))
            }
            real => Number::Real(real),
        };
        FuzzyNumber {
            value,
            approximation: if self.0.one_in(4) {
                Some(self.real())
            } else {
                None
            },
            probability: if self.0.one_in(4) {
                Some(self.real())
            } else {
                None
            },
            opinion: if self.0.one_in(5) {
                Some(self.string())
            } else {
                None
            },
        }
    }

    fn expr(&mut self, depth: usize) -> Expr {
        let kind = if depth == 0 {
            self.atom(0)
        } else {
            match self.0.below(20) {
                0..=8 => {
                    use BinaryOp::*;
                    let op = self.0.pick(&[
                        Or, Nor, Xor, And, Nand, Eq, Ne, Lt, Le, Gt, Ge, In, NotIn, Union, Inter,
                        Compl, Concat, Add, Sub, ArrayAdd, Mul, Div, Mod, Ratio, ArrayMul,
                        ArrayDot, ArrayPow, Pow,
                    ]);
                    ExprKind::Binary(
                        op,
                        Box::new(self.expr(depth - 1)),
                        Box::new(self.expr(depth - 1)),
                    )
                }
                9..=11 => {
                    let op = self.0.pick(&[UnaryOp::Neg, UnaryOp::Plus, UnaryOp::Not]);
                    ExprKind::Unary(op, Box::new(self.expr(depth - 1)))
                }
                12 => ExprKind::Link(
                    Box::new(self.object(depth - 1)),
                    self.str(),
                    Box::new(self.object(depth - 1)),
                ),
                _ => self.atom(depth),
            }
        };
        Expr { kind, span: span() }
    }

    fn exprs(&mut self, min: usize, depth: usize) -> Vec<Expr> {
        self.several(min, |g| g.expr(depth))
    }

    fn atom(&mut self, depth: usize) -> ExprKind {
        let inner = depth.saturating_sub(1);
        match self.0.below(if depth == 0 { 5 } else { 9 }) {
            0 => ExprKind::Number(self.fuzzy_number()),
            1 => ExprKind::String(self.string()),
            2 if self.0.one_in(4) => ExprKind::This,
            2 => ExprKind::Boolean(self.0.one_in(2)),
            3 | 4 => ExprKind::Variable(self.variable(0)),
            5 => ExprKind::Variable(self.variable(inner)),
            6 => {
                let values = self.several(1, |g| (g.0.one_in(3), g.fuzzy_number()));
                ExprKind::Multiple(values)
            }
            7 if self.0.one_in(2) => ExprKind::Call(self.call(1, inner)),
            7 => ExprKind::Attribute(Box::new(self.object(inner)), Box::new(self.adt_name(inner))),
            _ => ExprKind::Functor(
                Box::new(self.adt(inner)),
                self.several(1, |g| g.call(0, inner)),
            ),
        }
    }

    /// A variable, indexed if `depth` allows.
    fn variable(&mut self, depth: usize) -> Variable {
        Variable {
            name: self.name(),
            indices: if depth > 0 {
                self.exprs(1, depth - 1)
            } else {
                Vec::new()
            },
        }
    }

    fn call(&mut self, min: usize, depth: usize) -> Call {
        Call {
            name: self.module_name(),
            args: self.exprs(min, depth),
        }
    }

    /// An expression as EACH, INSERT and EXTRACT take a list.
    fn list(&mut self, depth: usize) -> Expr {
        if depth == 0 || self.0.one_in(2) {
            return self.expr(depth);
        }
        let op = self
            .0
            .pick(&[BinaryOp::Union, BinaryOp::Inter, BinaryOp::Compl]);
        Expr {
            kind: ExprKind::Binary(
                op,
                Box::new(self.list(depth - 1)),
                Box::new(self.list(depth - 1)),
            ),
            span: span(),
        }
    }

    // Designations

    fn adt_name(&mut self, depth: usize) -> AdtName {
        match self.0.below(if depth == 0 { 2 } else { 4 }) {
            0 => AdtName::Name(self.name()),
            1 => AdtName::String(self.str()),
            2 => AdtName::Expr(Box::new(Expr {
                kind: ExprKind::Call(self.call(1, depth - 1)),
                span: span(),
            })),
            _ => AdtName::Expr(Box::new(self.expr(depth - 1))),
        }
    }

    fn designation(&mut self, depth: usize) -> Designation {
        match self.0.below(5) {
            0 => Designation::This(span()),
            1 | 2 => Designation::Named(self.adt_name(depth)),
            _ => Designation::Classed {
                name: self.adt_name(depth),
                class: self.adt_name(depth),
            },
        }
    }

    /// A designation as the grammar's `object` has it: a name alone, never
    /// a string or an expression.
    fn object(&mut self, depth: usize) -> Designation {
        match self.0.below(3) {
            0 => Designation::This(span()),
            1 => Designation::Named(AdtName::Name(self.name())),
            _ => Designation::Classed {
                name: self.adt_name(depth),
                class: self.adt_name(depth),
            },
        }
    }

    fn adt(&mut self, depth: usize) -> Adt {
        let part = match self.0.below(3) {
            0 => AdtPart::None,
            1 => AdtPart::Attribute(self.adt_name(depth)),
            _ => AdtPart::Link(self.str(), self.designation(depth)),
        };
        Adt {
            designation: self.designation(depth),
            part,
        }
    }

    fn class_attribute(&mut self, depth: usize) -> ClassAttribute {
        ClassAttribute {
            class: self.adt_name(depth),
            attribute: self.adt_name(depth),
        }
    }

    fn class_link(&mut self, depth: usize) -> ClassLink {
        ClassLink {
            from: self.adt_name(depth),
            relation: self.str(),
            to: self.adt_name(depth),
        }
    }

    /// Operands of CREATE, DESIGN or a state change: links declared with
    /// a reverse unless `bare`, designations limited to classes if
    /// `classes`.
    fn operands(&mut self, bare: bool, classes: bool) -> Operands {
        match self.0.below(3) {
            0 => Operands::Designations(self.several(1, |g| {
                if classes {
                    Designation::Named(g.adt_name(1))
                } else {
                    g.designation(1)
                }
            })),
            1 => Operands::Attributes(self.several(1, |g| g.class_attribute(1))),
            _ => Operands::Links(self.several(1, |g| LinkDeclaration {
                link: g.class_link(1),
                reverse: match g.0.below(if bare { 1 } else { 3 }) {
                    0 => Reverse::None,
                    1 => Reverse::Named(g.adt_name(1)),
                    _ => Reverse::Reflexive,
                },
            })),
        }
    }

    // Statements

    fn body(&mut self, depth: usize) -> Vec<Stmt> {
        self.several(0, |g| g.stmt(depth - 1))
    }

    fn range(&mut self) -> Range {
        Range {
            name: self.name(),
            from: self.expr(1),
            to: self.expr(1),
            step: self.some(|g| g.expr(1)),
        }
    }

    fn stmt(&mut self, depth: usize) -> Stmt {
        let e = DEPTH;
        // 0 to 5 hold statements: none at depth 0.
        let first = if depth == 0 { 6 } else { 0 };
        let kind = match first + self.0.below(38 - first) {
            0 => StmtKind::Loop {
                range: self.some(Self::range),
                condition: self.some(|g| g.expr(e)),
                body: self.body(depth),
                until: self.some(|g| g.expr(e)),
            },
            1 => StmtKind::Each {
                name: self.name(),
                of: self.some(|g| g.list(2)),
                filter: self.some(|g| g.expr(e)),
                body: self.body(depth),
            },
            2 => StmtKind::Vect {
                range: self.range(),
                body: self.body(depth),
            },
            3 => StmtKind::Parallel(self.several(2, |g| g.body(depth))),
            4 => StmtKind::If {
                condition: self.expr(e),
                then: self.body(depth),
                otherwise: self.some(|g| g.body(depth)),
            },
            5 => StmtKind::Case {
                subject: self.expr(e),
                arms: self.several(1, |g| CaseArm {
                    values: g.exprs(1, 1),
                    body: g.body(depth),
                }),
                others: self.some(|g| g.body(depth)),
            },
            6 => StmtKind::Exit {
                kind: self
                    .0
                    .pick(&[ExitKind::Exit, ExitKind::Next, ExitKind::Reloop]),
                count: self.some(|g| g.expr(1)),
            },
            7 => match self.0.below(7) {
                0 => StmtKind::Return,
                1 => StmtKind::Reproc,
                2 => StmtKind::Pause,
                3 => StmtKind::Stop,
                4 => StmtKind::Restart,
                5 => StmtKind::Passivate,
                _ => StmtKind::Terminate,
            },
            8..=10 => StmtKind::Assign {
                target: self.variable(1),
                value: self.expr(e),
            },
            11 => StmtKind::Reset(self.several(1, |g| g.variable(1))),
            12 => StmtKind::Result(self.expr(e)),
            13 => StmtKind::Insert {
                items: self.several(1, |g| g.list(2)),
                list: self.name(),
                position: self
                    .some(|g| (g.0.pick(&[Position::After, Position::Before]), g.expr(1))),
            },
            14 => StmtKind::Extract {
                items: self.several(1, |g| g.list(2)),
                list: self.name(),
            },
            15 => StmtKind::Empty(self.name()),
            16 => StmtKind::Organize {
                kind: self.0.pick(&[
                    Organization::Linear,
                    Organization::Circular,
                    Organization::Single,
                    Organization::Tuple,
                ]),
                lists: self.several(1, Self::name),
            },
            17 => StmtKind::Create(self.operands(false, false)),
            18 => {
                let op = self.0.pick(&[
                    StateOp::Hibern,
                    StateOp::Awake,
                    StateOp::Destroy,
                    StateOp::Resur,
                    StateOp::Purge,
                ]);
                StmtKind::StateChange(op, self.operands(true, false))
            }
            19 => StmtKind::Design(self.operands(false, true)),
            20 | 21 => StmtKind::Let {
                target: match self.0.below(3) {
                    0 => LetTarget::Variable(self.name()),
                    1 => LetTarget::Attribute {
                        object: self.object(1),
                        attribute: self.adt_name(1),
                    },
                    _ => LetTarget::Property {
                        adt: self.adt(1),
                        name: self.name(),
                    },
                },
                value: self.expr(e),
            },
            22 if self.0.one_in(4) => StmtKind::Modality {
                from: self.object(1),
                modal: [Modal::May, Modal::Must, Modal::Cannot][self.0.below(3)],
                relation: self.str(),
                to: self.object(1),
            },
            22 => StmtKind::Assert(if self.0.one_in(3) {
                Assertion::Name(self.name())
            } else {
                Assertion::Link {
                    from: self.object(1),
                    negated: self.0.one_in(2),
                    relation: self.str(),
                    probability: self.some(Self::real),
                    to: self.object(1),
                }
            }),
            23 => StmtKind::Specify {
                items: self.several(1, |g| match g.0.below(3) {
                    0 => Specified::Class(g.adt_name(1)),
                    1 => Specified::Attribute(g.class_attribute(1)),
                    _ => Specified::Link(g.class_link(1)),
                }),
                spec: match self.0.below(5) {
                    0 => Specification::Scalar(self.scalar()),
                    1 => Specification::Array {
                        rank: self.rank(),
                        element: self.some(Self::scalar),
                    },
                    2 => Specification::List(self.some(Self::scalar)),
                    3 => Specification::Compound,
                    _ => Specification::Fuzziness(self.fuzziness()),
                },
                unit: self.some(Self::name),
                computed: self.0.one_in(2),
            },
            24 => StmtKind::Include {
                classes: self.several(1, |g| g.adt_name(1)),
                into: self.several(1, |g| g.adt_name(1)),
            },
            25 => StmtKind::Exclude {
                classes: self.several(1, |g| g.adt_name(1)),
                from: self.several(1, |g| g.adt_name(1)),
            },
            26 => StmtKind::Associate(self.several(1, |g| (g.adt(1), g.designation(1)))),
            27 => StmtKind::Dissociate(self.several(1, |g| (g.adt(1), g.designation(1)))),
            28 => StmtKind::Compose {
                class: self.adt_name(1),
                with: self.several(1, |g| g.adt_name(1)),
            },
            29 => StmtKind::Build {
                classes: self.several(1, |g| g.adt_name(1)),
                model: self.adt_name(1),
            },
            30 => match self.0.below(3) {
                0 => StmtKind::Activate(self.object(1)),
                1 => StmtKind::Cancel(self.object(1)),
                _ => StmtKind::Kill(self.object(1)),
            },
            31 => StmtKind::Reactivate {
                object: self.object(1),
                when: match self.0.below(4) {
                    0 => None,
                    1 => Some(When::At(self.expr(e))),
                    2 => Some(When::Before(self.object(1))),
                    _ => Some(When::After(self.object(1))),
                },
            },
            32 => StmtKind::Delay {
                object: self.object(1),
                duration: self.expr(e),
            },
            33 => StmtKind::Hold {
                object: self.some(|s| s.object(1)),
                duration: self.expr(e),
            },
            34 => match self.0.below(3) {
                0 => StmtKind::Chain(self.module_name()),
                1 => StmtKind::Call(self.call(0, 1)),
                _ => StmtKind::Resume(self.call(0, 1)),
            },
            35 => StmtKind::Read(self.several(1, |g| g.variable(1))),
            36 => StmtKind::Print(self.exprs(1, e)),
            _ => StmtKind::Assign {
                target: self.variable(0),
                value: self.expr(1),
            },
        };
        Stmt { kind, span: span() }
    }
}

// Runnable programs, for the VM's gate: written as text, with variables
// declared and used as their types allow, so that most check; whatever
// they do, the reference and the VM must do it alike, runtime errors and
// runs past a statement budget included.

/// The types a runnable program's variables take.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum T {
    Int,
    Float,
    Appr,
    Rational,
    Gauss,
    Quaternion,
    Bool,
    Str,
    List,
    Vector,
}

const SCALARS: &[T] = &[
    T::Int,
    T::Int,
    T::Float,
    T::Appr,
    T::Rational,
    T::Gauss,
    T::Quaternion,
    T::Bool,
    T::Str,
];

fn declared(t: T) -> &'static str {
    match t {
        T::Int => "integer",
        T::Float => "float",
        T::Appr => "float appr",
        T::Rational => "rational",
        T::Gauss => "gauss",
        T::Quaternion => "quaternion",
        T::Bool => "boolean",
        T::Str => "string",
        T::List => "list of integer",
        T::Vector => "vector [1:3] of integer",
    }
}

/// A module's signature: its name, its parameters' types and modes, and
/// what it returns if it is a function.
#[derive(Clone)]
struct Signature {
    name: String,
    params: Vec<(T, &'static str)>,
    returns: Option<T>,
}

struct Writer<'g> {
    rng: &'g mut Rng,
    /// The variables in scope, by type.
    vars: Vec<(String, T)>,
    /// Loop counters in use, which the body leaves alone.
    busy: Vec<String>,
    loops: usize,
    /// The modules this one may call: those after it.
    callees: &'g [Signature],
    out: String,
    depth: usize,
    /// Whether the program made rules, "r0" to "r2" of "k0".
    rules: bool,
    /// Whether it has a coroutine `co`, and processes "w0" and "w1" of
    /// "w", whose body holds.
    coroutine: bool,
    processes: bool,
}

impl Writer<'_> {
    fn var(&mut self, t: T) -> Option<String> {
        let found: Vec<&String> = (self.vars.iter())
            .filter(|(_, u)| *u == t)
            .map(|(n, _)| n)
            .collect();
        (!found.is_empty()).then(|| found[self.rng.below(found.len())].clone())
    }

    /// A variable of type `t` the statement may change.
    fn target(&mut self, t: T) -> Option<String> {
        let free: Vec<String> = (self.vars.iter())
            .filter(|(n, u)| *u == t && !self.busy.contains(n))
            .map(|(n, _)| n.clone())
            .collect();
        (!free.is_empty()).then(|| free[self.rng.below(free.len())].clone())
    }

    fn int_literal(&mut self) -> String {
        match self.rng.below(12) {
            0 => "9223372036854775807".into(),
            1 => "0".into(),
            2 => format!("{}", self.rng.below(1000)),
            _ => format!("{}", self.rng.below(6)),
        }
    }

    /// A literal of type `t`, for a first value.
    fn literal(&mut self, t: T) -> String {
        match t {
            T::Int => format!("{}", self.rng.below(6)),
            T::Float | T::Appr => format!("{}.5", self.rng.below(9)),
            T::Rational => format!("{} // {}", self.rng.below(5), 1 + self.rng.below(4)),
            T::Gauss | T::Quaternion => self.number(t),
            T::Bool => self.rng.pick(&["true", "false"]).into(),
            T::Str => self.rng.pick(&["\"a\"", "\"bc\""]).into(),
            T::List | T::Vector => unreachable!("lists and vectors start with a value"),
        }
    }

    /// A Gauss integer or a quaternion, written out.
    fn number(&mut self, t: T) -> String {
        let shape = self.rng.below(3);
        let sign = self.rng.pick(&["+", "-"]);
        let unit = self.rng.pick(&["i", "j", "k"]);
        let mut part = |unit: &str| match t {
            T::Gauss => format!("{}{unit}", self.rng.below(7)),
            _ => format!("{}.{}{unit}", self.rng.below(7), self.rng.below(10)),
        };
        match (t, shape) {
            (T::Gauss, 0) => part("i"),
            (T::Gauss, _) => format!("({} {sign} {})", part(""), part("i")),
            (_, 0) => part(unit),
            (_, 1) => format!("({} {sign} {})", part(""), part("j")),
            _ => format!(
                "({} + {} - {} + {})",
                part(""),
                part("i"),
                part("j"),
                part("k")
            ),
        }
    }

    fn expr(&mut self, t: T, depth: usize) -> String {
        let leaf = depth == 0 || self.rng.one_in(3);
        if let Some(v) = self.var(t).filter(|_| leaf && !self.rng.one_in(4)) {
            return v;
        }
        let d = depth.saturating_sub(1);
        match t {
            T::Int if leaf => self.int_literal(),
            T::Int => match self.rng.below(12) {
                0..=3 => {
                    let op = self.rng.pick(&["+", "-", "*", "/", "MOD"]);
                    format!("({} {op} {})", self.expr(T::Int, d), self.expr(T::Int, d))
                }
                4 => format!("(-{})", self.expr(T::Int, d)),
                5 => format!("abs({})", self.expr(T::Int, d)),
                6 => match self.var(T::List) {
                    Some(l) => format!(
                        "{l} ' {}",
                        self.rng.pick(&["card", "first", "last", "sigma"])
                    ),
                    None => self.int_literal(),
                },
                7 => match self.var(T::Vector) {
                    Some(v) => format!("{v}[{}]", self.expr(T::Int, d)),
                    None => self.int_literal(),
                },
                8 => match self.var(T::List) {
                    Some(l) => match self.rng.below(4) {
                        0 => format!(
                            "{l} ' {}",
                            self.rng.pick(&["succ", "pred", "current", "firstout"])
                        ),
                        1 => format!("{l} ' elmt({})", self.expr(T::Int, d)),
                        2 => format!("{l} ' occur({}, 1)", self.expr(T::Int, d)),
                        _ => format!("{l} ' poslp({}, 1)", self.expr(T::Int, d)),
                    },
                    None => self.int_literal(),
                },
                9 => format!(
                    "({} [.] {})",
                    self.expr(T::Vector, d),
                    self.expr(T::Vector, d)
                ),
                _ => self
                    .call_expr(T::Int, d)
                    .unwrap_or_else(|| self.int_literal()),
            },
            T::Float | T::Appr if leaf => {
                let x = format!("{}.{}", self.rng.below(50), self.rng.below(100));
                if t == T::Appr && self.rng.one_in(2) {
                    format!("({x} +- 0.{})", 1 + self.rng.below(9))
                } else {
                    x
                }
            }
            T::Float | T::Appr => match self.rng.below(6) {
                0..=2 => {
                    let op = self.rng.pick(&["+", "-", "*", "/"]);
                    format!("({} {op} {})", self.expr(t, d), self.expr(t, d))
                }
                3 => self.expr(T::Int, d),
                4 => format!(
                    "{}({})",
                    self.rng.pick(&["sqrt", "sin", "exp", "ln"]),
                    self.expr(t, d)
                ),
                _ => self.call_expr(T::Float, d).unwrap_or_else(|| "1.5".into()),
            },
            T::Rational if leaf => format!("{} // {}", self.rng.below(7), 1 + self.rng.below(6)),
            T::Rational => match self.rng.below(3) {
                0 => format!(
                    "({} + {})",
                    self.expr(T::Rational, d),
                    self.expr(T::Rational, d)
                ),
                1 => format!("({} * {})", self.expr(T::Rational, d), self.expr(T::Int, d)),
                _ => self.expr(T::Int, d),
            },
            T::Gauss | T::Quaternion if leaf => self.number(t),
            T::Gauss | T::Quaternion => match self.rng.below(8) {
                0..=4 => {
                    let op = self.rng.pick(&["+", "-", "*", "*", "/"]);
                    format!("({} {op} {})", self.expr(t, d), self.expr(t, d))
                }
                5 => format!("({} ^ {})", self.expr(t, d), self.rng.below(4)),
                6 => format!("(-{})", self.expr(t, d)),
                _ => format!("({} * {})", self.expr(T::Int, d), self.expr(t, d)),
            },
            T::Bool if leaf => self.rng.pick(&["true", "false"]).into(),
            T::Bool => match self.rng.below(6) {
                0 | 1 => {
                    let op = self.rng.pick(&["<", "<=", ">", ">=", "=", "/="]);
                    let u = self.rng.pick(&[T::Int, T::Float, T::Rational]);
                    format!("({} {op} {})", self.expr(u, d), self.expr(u, d))
                }
                2 if self.rng.one_in(3) => {
                    let u = self.rng.pick(&[T::Gauss, T::Quaternion]);
                    let op = self.rng.pick(&["=", "/="]);
                    format!("({} {op} {})", self.expr(u, d), self.expr(u, d))
                }
                2 => {
                    let op = self.rng.pick(&["and", "or", "xor"]);
                    format!("({} {op} {})", self.expr(T::Bool, d), self.expr(T::Bool, d))
                }
                3 => format!("not {}", self.expr(T::Bool, d)),
                4 => match self.var(T::List) {
                    Some(l) => {
                        let op = self.rng.pick(&["in", "notin"]);
                        format!("({} {op} {l})", self.expr(T::Int, d))
                    }
                    None => "true".into(),
                },
                _ => format!("({} = {})", self.expr(T::Str, d), self.expr(T::Str, d)),
            },
            T::Str if leaf => self
                .rng
                .pick(&["\"a\"", "\"bc\"", "\"\"", "\"x y\""])
                .into(),
            // With a literal only: a string doubled in a loop would fill
            // the memory long before the budget ends the run.
            T::Str => {
                let tail = self.rng.pick(&["\"a\"", "\"bc\"", "\" \""]);
                format!("({} & {tail})", self.expr(T::Str, d))
            }
            T::List => match self.var(T::List) {
                Some(l) if !leaf && self.rng.one_in(4) => {
                    let (a, b) = (self.expr(T::Int, d), self.expr(T::Int, d));
                    format!("{l} ' sublist({a}, {b})")
                }
                Some(l) if !leaf => {
                    let op = self.rng.pick(&["union", "inter", "compl"]);
                    format!("({l} {op} {})", self.expr(T::List, d))
                }
                Some(l) => l,
                None => unreachable!("lists are always declared"),
            },
            T::Vector if leaf => self.var(T::Vector).expect("a vector is always declared"),
            T::Vector => match self.rng.below(4) {
                0 => format!("({} [*] {})", self.expr(T::Vector, d), self.expr(T::Int, d)),
                1 => format!("({} [*] {})", self.expr(T::Int, d), self.expr(T::Vector, d)),
                _ => {
                    let op = self.rng.pick(&["[+]", "[+]", "[^]"]);
                    format!(
                        "({} {op} {})",
                        self.expr(T::Vector, d),
                        self.expr(T::Vector, d)
                    )
                }
            },
        }
    }

    fn call_expr(&mut self, t: T, depth: usize) -> Option<String> {
        let fs: Vec<Signature> = (self.callees.iter())
            .filter(|s| s.returns == Some(t))
            .cloned()
            .collect();
        if fs.is_empty() {
            return None;
        }
        let f = fs[self.rng.below(fs.len())].clone();
        let args = self.args(&f, depth)?;
        Some(format!("{}({args})", f.name))
    }

    fn args(&mut self, s: &Signature, depth: usize) -> Option<String> {
        let mut args = Vec::new();
        for (t, mode) in &s.params {
            args.push(match *mode {
                "in" => self.expr(*t, depth),
                _ => self.target(*t)?,
            });
        }
        Some(args.join(", "))
    }

    fn line(&mut self, text: &str) {
        self.out.push_str(&"        ".repeat(self.depth + 1));
        self.out.push_str(text);
    }

    /// Statements separated by `;`, the last without.
    fn block(&mut self, n: usize, depth: usize) {
        let n = 1 + self.rng.below(n.max(1));
        for i in 0..n {
            self.stmt(depth);
            self.out.push_str(if i + 1 < n { " ;\n" } else { "\n" });
        }
    }

    /// A statement on the store the program built: values set and read,
    /// links asserted and tested, functors, objects created, states.
    fn store_stmt(&mut self) {
        // o0 to o2 exist from the start; o3 only once a CREATE made it.
        let o = format!("\"o{}\" [ \"c0\" ]", self.rng.below(4));
        let a = self.rng.pick(&["a0", "a1"]);
        let line = match self.rng.below(if self.rules { 12 } else { 9 }) {
            0 | 1 => format!("let {o} . \"{a}\" := {}", self.expr(T::Int, 2)),
            2 => format!("print {o} . \"{a}\""),
            3 => {
                let not = if self.rng.one_in(3) { "not " } else { "" };
                format!("assert {o} {not}\"r\" \"p0\" [ \"c1\" ]")
            }
            4 => format!("print {o} \"r\" \"p0\" [ \"c1\" ]"),
            5 => {
                let f = self.rng.pick(&[
                    "\"c0\" ' obj ' card",
                    "\"c0\" ' obj",
                    "\"c0\" ' obj ' oattn(\"a0\") ' sigma",
                    "\"p0\" [ \"c1\" ] ' opscrc(\"s\", \"c0\") ' ofin ' card",
                    "\"c0\" ' att",
                ]);
                format!("print {f}")
            }
            6 => format!("create {o}"),
            7 => format!(
                "{} {o}",
                self.rng.pick(&["destroy", "resur", "hibern", "awake"])
            ),
            8 => format!("{} {o}", self.rng.pick(&["purge", "destroy"])),
            9 => "call chav(\"k0\")".into(),
            10 => {
                let x = self.rng.below(3);
                let n = self.rng.below(100);
                format!(
                    "call hypochav(\"let \"\"o{x}\"\" [ \"\"c0\"\" ] . \"\"{a}\"\" := {n}\", \"k0\")"
                )
            }
            _ => {
                let f = self.rng.pick(&[
                    "\"r0\" [ \"k0\" ] ' inrule",
                    "\"r1\" [ \"k0\" ] ' outrule",
                    "\"c0\" . \"a0\" ' outfact",
                    "\"o0\" [ \"c0\" ] . \"a1\" ' infact",
                ]);
                format!("print {f}")
            }
        };
        self.line(&line);
    }

    /// RESUME, and the simulation's statements on "w0" and "w1".
    fn fiber_stmt(&mut self) {
        let w = |rng: &mut Rng| format!("\"w{}\" [ \"w\" ]", rng.below(2));
        let t = self.rng.below(5);
        let line = match self.rng.below(11) {
            _ if !self.processes => "resume co".into(),
            0 if self.coroutine => "resume co".into(),
            0 | 1 => format!("activate {}", w(self.rng)),
            2 => format!("reactivate {} at {t}", w(self.rng)),
            3 => format!("reactivate {} after {}", w(self.rng), w(self.rng)),
            4 if self.rng.one_in(3) => {
                format!("{} {}", self.rng.pick(&["cancel", "kill"]), w(self.rng))
            }
            5 => format!("delay {} for {t}", w(self.rng)),
            6 => {
                let p = w(self.rng);
                format!("print {p} . \"state\", {p} . \"clock\", time")
            }
            _ => format!("hold for {t}"),
        };
        self.line(&line);
    }

    fn stmt(&mut self, depth: usize) {
        if (self.coroutine || self.processes) && self.rng.one_in(6) {
            return self.fiber_stmt();
        }
        if self.rng.one_in(4) {
            return self.store_stmt();
        }
        let compound = depth > 0 && self.depth < 4;
        match self.rng.below(if compound { 20 } else { 11 }) {
            0..=3 => {
                let t = self.rng.pick(&[
                    T::Int,
                    T::Int,
                    T::Float,
                    T::Appr,
                    T::Rational,
                    T::Gauss,
                    T::Quaternion,
                    T::Bool,
                    T::Str,
                    T::List,
                    T::Vector,
                ]);
                match self.target(t) {
                    Some(v) => {
                        let e = self.expr(t, 3);
                        self.line(&format!("{v} := {e}"));
                    }
                    None => self.line("print 0"),
                }
            }
            4 => {
                let n = 1 + self.rng.below(3);
                let es: Vec<String> = (0..n)
                    .map(|_| {
                        let t = self.rng.pick(&[
                            T::Int,
                            T::Float,
                            T::Appr,
                            T::Rational,
                            T::Gauss,
                            T::Quaternion,
                            T::Bool,
                            T::Str,
                            T::List,
                            T::Vector,
                        ]);
                        self.expr(t, 2)
                    })
                    .collect();
                self.line(&format!("print {}", es.join(", ")));
            }
            5 => {
                let l = self.var(T::List).expect("declared");
                let e = self.expr(T::Int, 2);
                match self.rng.below(7) {
                    0 => self.line(&format!("extract ({e}) outof {l}")),
                    1 => {
                        let k = self.expr(T::Int, 1);
                        let side = self.rng.pick(&["after", "before"]);
                        self.line(&format!("insert ({e}) into {l} {side} {k}"))
                    }
                    5 if self.rng.one_in(3) => self.line(&format!("circular {l}")),
                    2 if self.rng.one_in(3) => self.line(&format!("empty {l}")),
                    _ => self.line(&format!("insert ({e}) into {l}")),
                }
            }
            6 => {
                let v = self.var(T::Vector).expect("declared");
                let (i, e) = (self.expr(T::Int, 1), self.expr(T::Int, 2));
                self.line(&format!("{v}[{i}] := {e}"));
            }
            7 => {
                let t = self.rng.pick(&[T::Int, T::Float, T::Str]);
                match self.target(t) {
                    Some(v) => self.line(&format!("read {v}")),
                    None => self.line("print 1"),
                }
            }
            8 if self.loops > 0 => {
                let word = self.rng.pick(&["exit", "next", "reloop"]);
                let line = match self.rng.below(4) {
                    0 => word.to_string(),
                    1 => format!("{word} {}", 1 + self.rng.below(self.loops)),
                    2 => format!("{word} {}", self.expr(T::Int, 1)),
                    _ => format!("if {} then {word} endif", self.expr(T::Bool, 2)),
                };
                self.line(&line);
            }
            9 => {
                let procs: Vec<Signature> = (self.callees.iter())
                    .filter(|s| s.returns.is_none())
                    .cloned()
                    .collect();
                match procs.first().cloned() {
                    Some(p) => match self.args(&p, 2) {
                        Some(args) if args.is_empty() => self.line(&format!("call {}", p.name)),
                        Some(args) => self.line(&format!("call {}({args})", p.name)),
                        None => self.line("print 2"),
                    },
                    None => self.line("print 3"),
                }
            }
            10 => {
                let t = self.rng.pick(SCALARS);
                match self.target(t) {
                    Some(v) => self.line(&format!("reset {v}")),
                    None => self.line("print 4"),
                }
            }
            11 | 12 => {
                let c = self.expr(T::Bool, 2);
                self.line(&format!("if {c} then\n"));
                self.nested(depth);
                if self.rng.one_in(2) {
                    self.line("else\n");
                    self.nested(depth);
                }
                self.line("endif");
            }
            13 | 14 => {
                let Some(i) = self.target(T::Int) else {
                    return self.line("print 5");
                };
                let (a, b) = (self.expr(T::Int, 1), self.expr(T::Int, 1));
                let step = match self.rng.below(4) {
                    0 => " step -1".to_string(),
                    1 => format!(" step {}", self.rng.pick(&["2", "3", "0"])),
                    _ => String::new(),
                };
                self.line(&format!("loop for {i} : {a} to {b}{step} do\n"));
                self.busy.push(i);
                self.looped(depth);
                self.busy.pop();
                self.line("endloop");
            }
            15 => {
                let c = self.expr(T::Bool, 2);
                self.line(&format!("loop while {c} do\n"));
                self.looped(depth);
                self.line("endloop");
            }
            16 => {
                self.line("loop do\n");
                self.looped(depth);
                let c = self.expr(T::Bool, 2);
                self.line(&format!("until {c} endloop"));
            }
            17 => {
                let (Some(e), Some(l)) = (self.target(T::Int), self.var(T::List)) else {
                    return self.line("print 6");
                };
                let filter = match self.rng.one_in(2) {
                    true => format!(" / {}", self.expr(T::Bool, 1)),
                    false => String::new(),
                };
                self.line(&format!("each {e} of {l}{filter} do\n"));
                self.busy.push(e);
                self.looped(depth);
                self.busy.pop();
                self.line("endeach");
            }
            18 => {
                let s = self.expr(T::Int, 1);
                self.line(&format!("case {s}\n"));
                for _ in 0..1 + self.rng.below(3) {
                    let (a, b) = (self.rng.below(4), self.rng.below(4));
                    self.line(&format!("when {a}, {b} do\n"));
                    self.nested(depth);
                }
                if self.rng.one_in(2) {
                    self.line("others\n");
                    self.nested(depth);
                }
                self.line("endcase");
            }
            19 => {
                self.line("parbegin\n");
                self.nested(depth);
                self.line("par\n");
                self.nested(depth);
                self.line("endpar");
            }
            // EXIT and the others with no loop around: a PRINT instead.
            _ => self.line("print 7"),
        }
    }

    fn nested(&mut self, depth: usize) {
        self.depth += 1;
        self.block(3, depth - 1);
        self.depth -= 1;
    }

    fn looped(&mut self, depth: usize) {
        self.loops += 1;
        self.nested(depth);
        self.loops -= 1;
    }
}

/// Three rules over the schema's objects, written as a program's
/// statements: PREMISES that read values or test links, YES and NO that
/// set values or print, priorities and reliabilities now and then, and
/// the facts' ASKABLE, EDITABLE and MODIFIABLE.
fn rule_base(rng: &mut Rng) -> String {
    let line = |text: &mut String, s: String| *text += &format!("        {s} ;\n");
    let mut text = String::new();
    line(
        &mut text,
        "create \"k0\" ; include \"k0\" into \"rule\"".into(),
    );
    line(
        &mut text,
        "create \"r0\" [ \"k0\" ], \"r1\" [ \"k0\" ], \"r2\" [ \"k0\" ]".into(),
    );
    let fact = |rng: &mut Rng| {
        let x = rng.below(3);
        let a = rng.pick(&["a0", "a1"]);
        format!("\"\"o{x}\"\" [ \"\"c0\"\" ] . \"\"{a}\"\"")
    };
    for r in 0..3 {
        let rule = format!("\"r{r}\" [ \"k0\" ]");
        if rng.one_in(2) {
            line(
                &mut text,
                format!("let {rule} . \"priority\" := {}", rng.below(4)),
            );
        }
        if rng.one_in(2) {
            let p = rng.pick(&["0.5", "0.9", "1"]);
            line(&mut text, format!("let {rule} . \"reliability\" := {p}"));
        }
        let premises = match rng.below(4) {
            0 => format!("{} = {}", fact(rng), rng.below(5)),
            1 => format!(
                "\"\"o{}\"\" [ \"\"c0\"\" ] \"\"r\"\" \"\"p0\"\" [ \"\"c1\"\" ]",
                rng.below(3)
            ),
            _ => format!("{} < {}", fact(rng), rng.below(60)),
        };
        line(
            &mut text,
            format!("let {rule} ' premises := \"{premises}\""),
        );
        if !rng.one_in(4) {
            let yes = match rng.below(3) {
                0 => "print \"\"yes\"\", this".into(),
                _ => format!("let {} := {}", fact(rng), rng.below(60)),
            };
            line(&mut text, format!("let {rule} ' yes := \"{yes}\""));
        }
        if rng.one_in(3) {
            let no = match rng.below(2) {
                0 => "print \"\"no\"\"".into(),
                _ => format!("let {} := {}", fact(rng), rng.below(60)),
            };
            line(&mut text, format!("let {rule} ' no := \"{no}\""));
        }
    }
    if rng.one_in(3) {
        let (a, b) = (rng.below(3), rng.below(3));
        line(
            &mut text,
            format!("assert \"r{a}\" [ \"k0\" ] \"activates\" \"r{b}\" [ \"k0\" ]"),
        );
    }
    if rng.one_in(3) {
        line(
            &mut text,
            "let \"c0\" . \"a1\" ' askable := \"true\"".into(),
        );
    }
    if rng.one_in(3) {
        line(
            &mut text,
            "let \"c0\" . \"a0\" ' editable := \"true\"".into(),
        );
    }
    if rng.one_in(8) {
        line(
            &mut text,
            "let \"c0\" . \"a1\" ' modifiable := \"this . \"\"a0\"\" < 10\"".into(),
        );
    }
    text
}

/// Reflexes over the schema: a value set, an object created or
/// destroyed, a link asserted launch reactions that set other values or
/// print, so that chains run breadth first, and a cycle runs out.
fn reflex_base(rng: &mut Rng) -> String {
    let line = |text: &mut String, s: &str| *text += &format!("        {s} ;\n");
    let mut text = String::new();
    line(&mut text, "create \"rx\" ; include \"rx\" into \"reflex\"");
    line(
        &mut text,
        "create \"x0\" [ \"rx\" ], \"x1\" [ \"rx\" ], \"x2\" [ \"rx\" ]",
    );
    let launchers = [
        ("set", "\"c0\" . \"a0\""),
        ("set", "\"c0\" . \"a1\""),
        ("create", "\"c0\""),
        ("destroy", "\"c0\""),
        ("assert", "\"c0\" \"r\" \"c1\""),
    ];
    let reactions = [
        "print \"\"reflex\"\", this",
        "let this . \"\"a1\"\" := 1",
        "let this . \"\"a0\"\" := 2",
        "print this . \"\"a0\"\"",
    ];
    for x in 0..3 {
        let r = format!("\"x{x}\" [ \"rx\" ]");
        let (event, launcher) = launchers[rng.below(launchers.len())];
        line(&mut text, &format!("let {r} . \"event\" := \"{event}\""));
        line(&mut text, &format!("assoc {launcher} with {r}"));
        if !rng.one_in(5) {
            let reaction = reactions[rng.below(reactions.len())];
            line(&mut text, &format!("let {r} ' reaction := \"{reaction}\""));
        }
        if rng.one_in(4) {
            line(
                &mut text,
                &format!("let {r} . \"delay\" := {}", rng.below(3)),
            );
        }
    }
    text
}

/// Constraints over the schema: a value set, an object created, a link
/// asserted concern FORMULAs that print, set values, or refuse, so that
/// they run once per statement, settle, and refuse it whole.
fn constraint_base(rng: &mut Rng) -> String {
    let line = |text: &mut String, s: &str| *text += &format!("        {s} ;\n");
    let mut text = String::new();
    line(
        &mut text,
        "create \"kx\" ; include \"kx\" into \"constraint\"",
    );
    line(&mut text, "create \"k0\" [ \"kx\" ], \"k1\" [ \"kx\" ]");
    let components = [
        "\"c0\" . \"a0\"",
        "\"c0\" . \"a1\"",
        "\"c0\"",
        "\"c0\" \"r\" \"c1\"",
        "\"o0\" [ \"c0\" ]",
    ];
    let formulas = [
        "print \"\"formula\"\", this",
        "let \"\"o1\"\" [ \"\"c0\"\" ] . \"\"a1\"\" := 3",
        "formula := false",
        "if \"\"o0\"\" [ \"\"c0\"\" ] . \"\"a0\"\" > 2 then formula := false endif",
    ];
    for k in 0..2 {
        let c = format!("\"k{k}\" [ \"kx\" ]");
        line(
            &mut text,
            &format!("assoc {} with {c}", components[rng.below(components.len())]),
        );
        if !rng.one_in(5) {
            let formula = formulas[rng.below(formulas.len())];
            line(&mut text, &format!("let {c} ' formula := \"{formula}\""));
        }
    }
    text
}

/// A program of a few modules that checks, mostly, and its input: for
/// the VM's gate. Deterministic, as `program` is.
pub fn runnable(seed: u64) -> (String, String) {
    let mut rng = Rng::new(seed);
    let count = rng.below(4);
    let mut signatures = Vec::new();
    for k in 0..count {
        let returns = rng.one_in(2).then(|| rng.pick(&[T::Int, T::Float]));
        // A function has a parameter: `f()` is not ADT'81.
        let params = (0..rng.below(3) + returns.is_some() as usize)
            .map(|_| {
                let t = rng.pick(&[T::Int, T::Int, T::Float, T::Str, T::Bool]);
                (t, rng.pick(&["in", "in", "out", "inout"]))
            })
            .collect();
        signatures.push(Signature {
            name: format!("m{k}"),
            params,
            returns,
        });
    }
    let rules = rng.one_in(2);
    // The coroutine's INOUT parameter, if it has one.
    let coroutine = rng.one_in(2).then(|| rng.one_in(2));
    let processes = rng.one_in(2);
    let mut text = String::new();
    for k in 0..=count {
        let (header, mut vars, sig) = match k {
            0 => ("program p ;\n".to_string(), Vec::new(), None),
            k => {
                let s = signatures[k - 1].clone();
                let names: Vec<String> = (0..s.params.len()).map(|i| format!("a{i}")).collect();
                let head = match s.returns {
                    Some(t) => format!(
                        "function {} ({}) returns {} ;\n",
                        s.name,
                        names.join(", "),
                        declared(t)
                    ),
                    None => format!("subroutine {} ({}) ;\n", s.name, names.join(", ")),
                };
                let mut head = head.replace(" () ", " ");
                let mut vars = Vec::new();
                for (n, (t, mode)) in names.iter().zip(&s.params) {
                    head += &format!("        {n} : {} {mode} ;\n", declared(*t));
                    vars.push((n.clone(), *t));
                }
                (head, vars, Some(s))
            }
        };
        text += &header;
        text += "local\n";
        let mut locals = vec![
            ("n".to_string(), T::Int),
            ("k".to_string(), T::Int),
            ("l".to_string(), T::List),
            ("m".to_string(), T::List),
            ("v".to_string(), T::Vector),
            ("u".to_string(), T::Vector),
        ];
        for i in 0..2 + rng.below(4) {
            locals.push((format!("x{i}"), rng.pick(SCALARS)));
        }
        for (n, t) in &locals {
            let init = match (t, rng.one_in(4)) {
                (T::Int, true) => format!(" init {}", rng.below(5)),
                _ => String::new(),
            };
            text += &format!("        {n} : {}{init} ;\n", declared(*t));
        }
        vars.extend(locals);
        // The last declaration takes no `;` before BEGIN? It does: each
        // declaration ends with one.
        text += "begin\n";
        if k == 0 {
            // The store's schema, its CONDs and ACTIONs now and then: a
            // class's ACTION runs as its objects are created, two that
            // set each other's attribute nest until refused.
            text += "        create \"c0\", \"c1\" ;\n";
            text += "        create \"c0\" . \"a0\", \"c0\" . \"a1\", \"c1\" . \"a0\" ;\n";
            text += "        create \"c0\" \"r\" \"c1\" reverse \"s\" ;\n";
            if rng.one_in(3) {
                text += "        let \"c0\" . \"a0\" ' cond := \"this . \"\"a0\"\" < 50\" ;\n";
            }
            if rng.one_in(3) {
                text += "        let \"c0\" . \"a0\" ' action := \"let this . \"\"a1\"\" := this . \"\"a0\"\" + 1\" ;\n";
            }
            if rng.one_in(6) {
                text += "        let \"c0\" . \"a1\" ' action := \"let this . \"\"a0\"\" := this . \"\"a1\"\" + 1\" ;\n";
            }
            if rng.one_in(3) {
                text += "        let \"c0\" ' action := \"print \"\"made\"\", this\" ;\n";
            }
            text += "        create \"o0\" [ \"c0\" ], \"o1\" [ \"c0\" ], \"o2\" [ \"c0\" ], \"p0\" [ \"c1\" ] ;\n";
            if rules {
                text += &rule_base(&mut rng);
            }
            if rng.one_in(3) {
                text += &reflex_base(&mut rng);
            }
            if rng.one_in(3) {
                text += &constraint_base(&mut rng);
            }
            if processes {
                text += "        create \"w\" ; include \"w\" into \"process\" ;\n";
                text += "        create \"w0\" [ \"w\" ], \"w1\" [ \"w\" ] ;\n";
                text += "        activate \"w0\" [ \"w\" ] ; activate \"w1\" [ \"w\" ] ;\n";
                if rng.one_in(8) {
                    text += "        let \"c0\" ' action := \"hold for 1\" ;\n";
                }
            }
            match coroutine {
                Some(true) => text += "        n := 3 ;\n        resume co(n) ;\n",
                Some(false) if rng.one_in(8) => {
                    text += "        let \"c0\" ' action := \"resume co\" ;\n";
                }
                _ => {}
            }
        }
        let mut w = Writer {
            rng: &mut rng,
            vars,
            busy: Vec::new(),
            loops: 0,
            callees: &signatures[k.min(signatures.len())..],
            out: String::new(),
            depth: 0,
            rules,
            coroutine: coroutine.is_some(),
            processes,
        };
        // Most variables get a value first, so that the runs go further
        // than `has no value`; one now and then is left without.
        let scalars: Vec<(String, T)> = (w.vars.iter())
            .filter(|(n, t)| n.starts_with('x') || (SCALARS.contains(t) && n.len() == 1))
            .cloned()
            .collect();
        for (n, t) in scalars {
            if !w.rng.one_in(12) {
                let e = w.literal(t);
                w.line(&format!("{n} := {e} ;\n"));
            }
        }
        // The vectors' cells too, most of the time.
        for v in ["v", "u"] {
            if !w.rng.one_in(6) {
                let cells: Vec<String> = (1..=3)
                    .map(|i| format!("{v}[{i}] := {}", w.rng.below(6)))
                    .collect();
                w.line(&format!("{} ;\n", cells.join(" ; ")));
            }
        }
        w.block(8, 3);
        if let Some(Signature {
            returns: Some(t), ..
        }) = sig
        {
            let trimmed = w.out.trim_end().to_string();
            w.out = trimmed;
            let e = w.expr(t, 2);
            w.out += &format!(" ;\n        result {e}\n");
        }
        text += &w.out;
        text += "end ;\n\n";
    }
    if let Some(param) = coroutine {
        let (head, step) = match param {
            true => (
                "coroutine co (z) ;\n        z : integer inout ;\n",
                "z := z + 1 ; ",
            ),
            false => ("coroutine co ;\n", ""),
        };
        text += head;
        text += &format!(
            "local\n        j : integer ;\nbegin\n        loop for j : 1 to {} do\n                {step}print \"co\", j ;\n                pause\n        endloop\nend ;\n\n",
            rng.below(5) + 1
        );
    }
    if processes {
        text += &format!(
            "coroutine \"w\" ;\nlocal\n        i : integer ;\nbegin\n        loop for i : 1 to {} do\n                print this, time ;\n                hold for {}\n        endloop\nend ;\n\n",
            rng.below(4) + 1,
            rng.below(3) + 1
        );
    }
    let input: Vec<String> = (0..rng.below(6))
        .map(|_| match rng.below(6) {
            0 => format!("{}", rng.below(100)),
            1 => format!("{}.5", rng.below(10)),
            2 => "1.5 +- 0.2".into(),
            3 => "word".into(),
            4 => "2 + 3".into(),
            _ => format!("{}", rng.below(4)),
        })
        .collect();
    (text, input.join("\n"))
}
