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
                object: self.object(1),
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
