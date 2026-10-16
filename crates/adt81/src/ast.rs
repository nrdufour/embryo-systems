//! The ADT'81 abstract syntax tree: what a program means to the checker,
//! without the grammar's punctuation, precedence chains and single-child
//! rules. [`lower`](super::lower) builds it from the parser's tree, and
//! [`print`](super::print) writes it back as source.
//!
//! Positions are kept for diagnostics, but never compared: [`Span`]'s
//! equality always holds, so two trees are equal when they have the same
//! structure, wherever they were read from. Names compare folded, as the
//! language is case-insensitive.

// Built once and walked: boxing the larger variants would cost clarity
// for no gain.
#![allow(clippy::large_enum_variant)]

use std::hash::{Hash, Hasher};

pub use super::lexer::Unit;

/// Where a node starts in the source, 1-based.
#[derive(Debug, Clone, Copy, Default)]
pub struct Span {
    pub line: usize,
    pub column: usize,
}

/// Always equal: trees compare by structure, not by position.
impl PartialEq for Span {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// An identifier as written, and its folded form, by which it compares.
#[derive(Debug, Clone)]
pub struct Name {
    pub spelling: String,
    pub key: String,
    pub span: Span,
}

impl Name {
    pub fn new(spelling: &str, span: Span) -> Name {
        Name {
            spelling: spelling.to_string(),
            key: super::lexer::fold_name(spelling),
            span,
        }
    }
}

impl PartialEq for Name {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for Name {}

impl Hash for Name {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}

/// A string literal, without its quotes.
#[derive(Debug, Clone, PartialEq)]
pub struct Str {
    pub text: String,
    pub span: Span,
}

// Modules, page 19

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub modules: Vec<Module>,
}

/// Declarations and statements outside any module.
#[derive(Debug, Clone, PartialEq)]
pub struct Fragment {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Local(Declaration),
    Parameter(ParameterSpec),
    Stmt(Stmt),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub kind: ModuleKind,
    pub name: ModuleName,
    pub parameters: Vec<Name>,
    pub specifications: Vec<ParameterSpec>,
    pub locals: Vec<Declaration>,
    pub externals: Vec<External>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModuleKind {
    Program,
    Subroutine,
    Coroutine,
    Function {
        returns: ScalarType,
    },
    /// `FUNCTOR C ' SIBLINGS`: the prefix names what the functor applies to.
    Functor {
        prefix: Option<Name>,
        returns: DataType,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModuleName {
    Name(Name),
    /// A computed attribute's algorithm, `FUNCTION "ARC"."LENGTH"`.
    Attribute {
        class: Str,
        attribute: Str,
    },
    /// A process class's body, `COROUTINE "PUMP"` (page 112).
    Body(Str),
}

// Types, pages 20 and 21

#[derive(Debug, Clone, PartialEq)]
pub enum DataType {
    Scalar(ScalarType),
    Array {
        sparse: bool,
        shape: Shape,
        element: ScalarType,
    },
    /// `LIST`, or `LIST OF` an element type.
    List(Option<ScalarType>),
    Compound,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScalarType {
    pub base: BaseType,
    pub fuzziness: Fuzziness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseType {
    Numeric { kind: NumericKind, long: bool },
    Boolean,
    Character,
    String,
    Adt(AdtType),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericKind {
    Integer,
    Float,
    Complex,
    Gauss,
    Quaternion,
    Rational,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdtType {
    Class,
    CAttribute,
    CLink,
    Object,
    OAttribute,
    OLink,
    HClass,
    HAttribute,
    HLink,
    Embryo,
    Prototype,
    Structure,
}

/// The kinds of fuzziness a type carries. `FUZZY` stays its own case: the
/// print does not say whether it means all four.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fuzziness {
    Kinds {
        appr: bool,
        prob: bool,
        subj: bool,
        mult: bool,
    },
    Fuzzy,
}

impl Fuzziness {
    pub const NONE: Fuzziness = Fuzziness::Kinds {
        appr: false,
        prob: false,
        subj: false,
        mult: false,
    };
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub rank: Rank,
    /// Empty when the shape gives none.
    pub bounds: Vec<Bounds>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rank {
    Vector,
    Matrix,
    Tensor,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bounds {
    pub low: Bound,
    pub high: Bound,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Bound {
    Integer(i64),
    Name(Name),
}

// Declarations, page 20

#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    pub names: Vec<Name>,
    pub ty: DataType,
    pub init: Option<Init>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Init {
    /// `INIT c`
    Initial(Constant),
    /// `CONSTANTE = c`
    Constant(Constant),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParameterSpec {
    pub names: Vec<Name>,
    pub ty: DataType,
    pub mode: Mode,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    In,
    InOut,
    Out,
}

#[derive(Debug, Clone, PartialEq)]
pub struct External {
    pub names: Vec<Name>,
    pub kind: ExternalKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExternalKind {
    Program,
    Subroutine,
    Coroutine,
    Function(ScalarType),
    Functor(DataType),
}

// Constants, pages 25 and 26: signs folded into the values.

#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    Integer(i64),
    Float(f64),
    Complex {
        re: f64,
        im: f64,
    },
    Gauss {
        re: i64,
        im: i64,
    },
    /// Real part, then the i, j and k parts.
    Quaternion([f64; 4]),
    Rational(i64, u64),
    Boolean(bool),
    String(String),
}

// Expressions, pages 26 to 28

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Number(FuzzyNumber),
    /// A value with several possibilities, `{ 25.3, -25.7 }`: a minus sign
    /// for each that has one.
    Multiple(Vec<(bool, FuzzyNumber)>),
    String(String),
    Boolean(bool),
    Variable(Variable),
    Call(Call),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    /// `a ' F ' G(x)`: functors applied in turn to an a.d.t.
    Functor(Box<Adt>, Vec<Call>),
    /// An object's attribute, `o . "X"`.
    Attribute(Box<Designation>, Box<AdtName>),
    /// Whether two objects are linked, `THIS "retro" a`.
    Link(Box<Designation>, Str, Box<Designation>),
    /// The object THIS stands for.
    This,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variable {
    pub name: Name,
    pub indices: Vec<Expr>,
}

/// A function call, or a module or functor invoked: `F(a, b)`, `CARD`.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub name: Name,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Plus,
    Not,
}

/// Every binary operator, loosest first, as the grammar ranks them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
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
    In,
    NotIn,
    Union,
    Inter,
    Compl,
    Concat,
    Add,
    Sub,
    /// `[+]`
    ArrayAdd,
    Mul,
    Div,
    Mod,
    /// `//`
    Ratio,
    /// `[*]`
    ArrayMul,
    /// `[.]`
    ArrayDot,
    /// `[^]`
    ArrayPow,
    Pow,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FuzzyNumber {
    pub value: Number,
    /// `+- 0.5`
    pub approximation: Option<Real>,
    /// `% 0.95`
    pub probability: Option<Real>,
    /// `@"estimated"`
    pub opinion: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Number {
    Real(Real),
    /// `2i` (a Gauss integer's part), `3.5j`.
    Imaginary(Real, Unit),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Real {
    Integer(u64),
    Float(f64),
}

// A.d.t. designations, pages 23 and 24

/// One term naming an a.d.t.: a name, a string, or an expression (a call,
/// or anything in parentheses).
#[derive(Debug, Clone, PartialEq)]
pub enum AdtName {
    Name(Name),
    String(Str),
    Expr(Box<Expr>),
}

/// A class or an object: the grammar's `designation` and `object` alike,
/// which the checker tells apart.
#[derive(Debug, Clone, PartialEq)]
pub enum Designation {
    Named(AdtName),
    /// `"P" [ "POINT" ]`
    Classed {
        name: AdtName,
        class: AdtName,
    },
    This(Span),
}

/// A designation, or an attribute or a link of one.
#[derive(Debug, Clone, PartialEq)]
pub struct Adt {
    pub designation: Designation,
    pub part: AdtPart,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AdtPart {
    None,
    Attribute(AdtName),
    Link(Str, Designation),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassAttribute {
    pub class: AdtName,
    pub attribute: AdtName,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassLink {
    pub from: AdtName,
    pub relation: Str,
    pub to: AdtName,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinkDeclaration {
    pub link: ClassLink,
    pub reverse: Reverse,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Reverse {
    None,
    Named(AdtName),
    Reflexive,
}

// Statements, pages 21 to 25

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    Assign {
        target: Variable,
        value: Expr,
    },
    Loop {
        range: Option<Range>,
        condition: Option<Expr>,
        body: Vec<Stmt>,
        until: Option<Expr>,
    },
    Each {
        name: Name,
        of: Option<Expr>,
        filter: Option<Expr>,
        body: Vec<Stmt>,
    },
    Vect {
        range: Range,
        body: Vec<Stmt>,
    },
    Parallel(Vec<Vec<Stmt>>),
    If {
        condition: Expr,
        then: Vec<Stmt>,
        otherwise: Option<Vec<Stmt>>,
    },
    Case {
        subject: Expr,
        arms: Vec<CaseArm>,
        others: Option<Vec<Stmt>>,
    },
    Exit {
        kind: ExitKind,
        count: Option<Expr>,
    },
    Return,
    Reproc,
    Pause,
    Stop,
    Restart,
    Reset(Vec<Variable>),
    Result(Expr),
    Insert {
        items: Vec<Expr>,
        list: Name,
        position: Option<(Position, Expr)>,
    },
    Extract {
        items: Vec<Expr>,
        list: Name,
    },
    Empty(Name),
    Organize {
        kind: Organization,
        lists: Vec<Name>,
    },
    Create(Operands),
    StateChange(StateOp, Operands),
    Design(Operands),
    Let {
        target: LetTarget,
        value: Expr,
    },
    Assert(Assertion),
    /// MAY, MUST or CANNOT: what holds two objects to a link (page 159).
    Modality {
        from: Designation,
        modal: Modal,
        relation: Str,
        to: Designation,
    },
    Specify {
        items: Vec<Specified>,
        spec: Specification,
        unit: Option<Name>,
        computed: bool,
    },
    Include {
        classes: Vec<AdtName>,
        into: Vec<AdtName>,
    },
    Exclude {
        classes: Vec<AdtName>,
        from: Vec<AdtName>,
    },
    Associate(Vec<(Adt, Designation)>),
    Dissociate(Vec<(Adt, Designation)>),
    Compose {
        class: AdtName,
        with: Vec<AdtName>,
    },
    Build {
        classes: Vec<AdtName>,
        model: AdtName,
    },
    Activate(Designation),
    Reactivate {
        object: Designation,
        when: Option<When>,
    },
    Cancel(Designation),
    Kill(Designation),
    Delay {
        object: Designation,
        duration: Expr,
    },
    /// The current process, `HOLD FOR dt` (page 125), or one named.
    Hold {
        object: Option<Designation>,
        duration: Expr,
    },
    Passivate,
    Terminate,
    Chain(Name),
    Call(Call),
    Resume(Call),
    Read(Vec<Variable>),
    Print(Vec<Expr>),
}

/// `FOR i : 1 TO n STEP 2`
#[derive(Debug, Clone, PartialEq)]
pub struct Range {
    pub name: Name,
    pub from: Expr,
    pub to: Expr,
    pub step: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaseArm {
    pub values: Vec<Expr>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitKind {
    Exit,
    Next,
    Reloop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    After,
    Before,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Organization {
    Linear,
    Circular,
    Single,
    Tuple,
}

/// What CREATE, DESIGN and the state changes act on; DESIGN's
/// designations are classes, and the state changes' links declare
/// nothing.
#[derive(Debug, Clone, PartialEq)]
pub enum Operands {
    Designations(Vec<Designation>),
    Attributes(Vec<ClassAttribute>),
    Links(Vec<LinkDeclaration>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateOp {
    Hibern,
    Awake,
    Destroy,
    Resur,
    Purge,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LetTarget {
    Variable(Name),
    Attribute {
        object: Designation,
        attribute: AdtName,
    },
    /// A property of an a.d.t., `"A" . "x" ' COND`.
    Property {
        adt: Adt,
        name: Name,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modal {
    May,
    Must,
    Cannot,
}

impl Modal {
    pub fn word(self) -> &'static str {
        match self {
            Modal::May => "MAY",
            Modal::Must => "MUST",
            Modal::Cannot => "CANNOT",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Assertion {
    Link {
        from: Designation,
        negated: bool,
        relation: Str,
        probability: Option<Real>,
        to: Designation,
    },
    Name(Name),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Specified {
    Class(AdtName),
    Attribute(ClassAttribute),
    Link(ClassLink),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Specification {
    Scalar(ScalarType),
    Array {
        rank: Rank,
        element: Option<ScalarType>,
    },
    List(Option<ScalarType>),
    Compound,
    /// Fuzziness alone, possibly none.
    Fuzziness(Fuzziness),
}

#[derive(Debug, Clone, PartialEq)]
pub enum When {
    At(Expr),
    Before(Designation),
    After(Designation),
}
