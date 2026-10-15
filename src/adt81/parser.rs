//! The ADT'81 parser: tokens to a syntax tree, by `adt81/grammar.lark`.
//!
//! Recursive descent, one function per rule. The tree is the grammar's own:
//! a node per rule, named as the grammar names it, holding every token, so
//! that `just check-grammar` compares it line for line with the tree Lark
//! builds from the same grammar. A rule the grammar marks `?` gives no node
//! when it holds a single child, as in Lark.
//!
//! The parser decides from the next token or two, but for designations,
//! whose shape is only known once read: `a "r" b` is a relation, unless a
//! `'` follows and makes it a functor's prefix; `a . "x"` is an attribute's
//! value, unless a `'` follows. There it tries each reading in turn from
//! the same token ([`Parser::attempt`]). Expressions are memoised by
//! position, so that trying never parses one twice.

use std::collections::HashMap;
use std::fmt;

use super::lexer::{self, Error, Kind, Token, Unit};

macro_rules! rules {
    ($($variant:ident = $name:literal,)*) => {
        /// A rule of the grammar, as a node of the tree.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Rule {
            $($variant,)*
        }

        impl Rule {
            pub const ALL: &[Rule] = &[$(Rule::$variant,)*];

            /// The rule's name in the grammar.
            pub fn name(self) -> &'static str {
                match self {
                    $(Rule::$variant => $name,)*
                }
            }
        }
    };
}

rules! {
    Program = "program",
    Fragment = "fragment",
    MainProgram = "main_program",
    Procedure = "procedure",
    Function = "function",
    Coroutine = "coroutine",
    Functor = "functor",
    ComputedAttribute = "computed_attribute",
    ModuleName = "module_name",
    FormalParameters = "formal_parameters",
    ParameterSpecifications = "parameter_specifications",
    ParameterSpecification = "parameter_specification",
    ParameterType = "parameter_type",
    ScalarType = "scalar_type",
    ArrayType = "array_type",
    Shape = "shape",
    Bounds = "bounds",
    Bound = "bound",
    ListType = "list_type",
    CompoundType = "compound_type",
    Type = "type",
    NumericType = "numeric_type",
    AdtType = "adt_type",
    Mode = "mode",
    Fuzziness = "fuzziness",
    Locals = "locals",
    LocalDeclaration = "local_declaration",
    DeclaredType = "declared_type",
    InitialValue = "initial_value",
    ConstantValue = "constant_value",
    Externals = "externals",
    External = "external",
    ModuleKind = "module_kind",
    FunctorResult = "functor_result",
    Body = "body",
    Statements = "statements",
    Control = "control",
    Loop = "loop",
    EachLoop = "each_loop",
    VectorLoop = "vector_loop",
    Parallel = "parallel",
    IfStatement = "if_statement",
    CaseStatement = "case_statement",
    LoopExit = "loop_exit",
    ModuleExit = "module_exit",
    Halt = "halt",
    EmptyStatement = "empty_statement",
    Assignment = "assignment",
    Reset = "reset",
    Result = "result",
    ListStatement = "list_statement",
    AdtStatement = "adt_statement",
    Create = "create",
    StateChange = "state_change",
    Design = "design",
    ClassLinkDeclarations = "class_link_declarations",
    ClassLinkDeclaration = "class_link_declaration",
    DesignationList = "designation_list",
    ClassList = "class_list",
    ClassAttributeList = "class_attribute_list",
    ClassLinkList = "class_link_list",
    AdtAssignment = "adt_assignment",
    Property = "property",
    Assertion = "assertion",
    Specify = "specify",
    Specification = "specification",
    Include = "include",
    Exclude = "exclude",
    Associate = "associate",
    Dissociate = "dissociate",
    Compose = "compose",
    Build = "build",
    ProcessStatement = "process_statement",
    Call = "call",
    Invocation = "invocation",
    Io = "io",
    Class = "class",
    ClassAttribute = "class_attribute",
    ClassLink = "class_link",
    Object = "object",
    ObjectAttribute = "object_attribute",
    Designation = "designation",
    Adt = "adt",
    Constant = "constant",
    ArithmeticConstant = "arithmetic_constant",
    IntegerConstant = "integer_constant",
    FloatConstant = "float_constant",
    ComplexConstant = "complex_constant",
    GaussConstant = "gauss_constant",
    QuaternionConstant = "quaternion_constant",
    RationalConstant = "rational_constant",
    Sign = "sign",
    BooleanConstant = "boolean_constant",
    ExpressionList = "expression_list",
    Disjunction = "disjunction",
    Conjunction = "conjunction",
    Negation = "negation",
    Relation = "relation",
    Comparator = "comparator",
    Union = "union",
    Intersection = "intersection",
    Complement = "complement",
    Concatenation = "concatenation",
    Sum = "sum",
    Product = "product",
    Power = "power",
    Operand = "operand",
    FuzzyNumber = "fuzzy_number",
    Number = "number",
    Real = "real",
    Multiple = "multiple",
    FunctionCall = "function_call",
    ObjectAttributeValue = "object_attribute_value",
    FunctorExpression = "functor_expression",
    ListExpression = "list_expression",
    ListUnion = "list_union",
    ListIntersection = "list_intersection",
    ListComplement = "list_complement",
    ListTerm = "list_term",
    AdtName = "adt_name",
    Variable = "variable",
    Name = "name",
}

use Rule as R;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tree<'a> {
    pub rule: Rule,
    pub children: Vec<Node<'a>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node<'a> {
    Tree(Tree<'a>),
    Token(Token<'a>),
}

/// One node per line, indented by depth: a rule by its name, a token as
/// `- text`. `tools/check_grammar.py --tree` prints Lark's tree the same way.
impl fmt::Display for Tree<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn write(node: &Node, depth: usize, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match node {
                Node::Tree(tree) => {
                    writeln!(f, "{:1$}{2}", "", depth * 2, tree.rule.name())?;
                    for child in &tree.children {
                        write(child, depth + 1, f)?;
                    }
                    Ok(())
                }
                Node::Token(token) => writeln!(f, "{:1$}- {2}", "", depth * 2, token.text),
            }
        }
        write(&Node::Tree(self.clone()), 0, f)
    }
}

/// Parses a whole source file: modules, from `program`.
pub fn parse_program(source: &str) -> Result<Tree<'_>, Error> {
    parse(source, Parser::program)
}

/// Parses declarations and statements outside any module, from `fragment`.
pub fn parse_fragment(source: &str) -> Result<Tree<'_>, Error> {
    parse(source, Parser::fragment)
}

fn parse<'a>(
    source: &'a str,
    start: fn(&mut Parser<'a>) -> Parse<Node<'a>>,
) -> Result<Tree<'a>, Error> {
    let mut parser = Parser {
        tokens: lexer::tokenize(source)?,
        pos: 0,
        furthest: 0,
        expected: Vec::new(),
        expressions: HashMap::new(),
    };
    match start(&mut parser) {
        Ok(Node::Tree(tree)) if parser.pos == parser.tokens.len() => Ok(tree),
        Ok(_) => {
            parser.expect(Expected::Thing("end of input"));
            Err(parser.error())
        }
        Err(Fail) => Err(parser.error()),
    }
}

/// A rule failed to match; what was expected is in [`Parser::expected`].
struct Fail;

type Parse<T> = Result<T, Fail>;

/// A rule's function, for the rules that take one.
type Item<'a> = fn(&mut Parser<'a>) -> Parse<Node<'a>>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Expected {
    /// A keyword or a symbol, quoted in messages.
    Word(&'static str),
    Thing(&'static str),
}

const MODES: &[&str] = &["IN", "INOUT", "OUT"];
const NUMERIC_TYPES: &[&str] = &[
    "INTEGER",
    "FLOAT",
    "COMPLEX",
    "GAUSS",
    "QUATERNION",
    "RATIONAL",
];
const ADT_TYPES: &[&str] = &[
    "CLASS",
    "CATTRIBUTE",
    "CLINK",
    "OBJECT",
    "OATTRIBUTE",
    "OLINK",
    "HCLASS",
    "HATTRIBUTE",
    "HLINK",
    "EMBRYO",
    "PROTOTYPE",
    "STRUCTURE",
];
const SHAPES: &[&str] = &["VECTOR", "MATRIX", "TENSOR"];
const COMPARATORS: &[&str] = &["=", "/=", "<", "<=", ">", ">="];

struct Parser<'a> {
    tokens: Vec<Token<'a>>,
    pos: usize,
    /// The furthest position a rule failed at, and what it expected there:
    /// the error reported if the parse fails.
    furthest: usize,
    expected: Vec<Expected>,
    /// Every expression parsed, by start position: the node and the end.
    expressions: HashMap<usize, Option<(Node<'a>, usize)>>,
}

fn node(rule: Rule, children: Vec<Node<'_>>) -> Node<'_> {
    Node::Tree(Tree { rule, children })
}

/// A `?` rule: its only child, or a node.
fn inline(rule: Rule, mut children: Vec<Node<'_>>) -> Node<'_> {
    if children.len() == 1 {
        children.pop().unwrap()
    } else {
        node(rule, children)
    }
}

impl<'a> Parser<'a> {
    // Tokens

    fn peek(&self, ahead: usize) -> Option<Token<'a>> {
        self.tokens.get(self.pos + ahead).copied()
    }

    fn is_kind(&self, ahead: usize, kind: Kind) -> bool {
        self.peek(ahead).is_some_and(|t| t.kind == kind)
    }

    fn is_kw(&self, ahead: usize, word: &str) -> bool {
        self.peek(ahead)
            .is_some_and(|t| t.kind == Kind::Keyword && t.text.eq_ignore_ascii_case(word))
    }

    fn is_any_kw(&self, words: &[&str]) -> bool {
        words.iter().any(|w| self.is_kw(0, w))
    }

    fn is_sym(&self, ahead: usize, symbol: &str) -> bool {
        self.peek(ahead)
            .is_some_and(|t| t.kind == Kind::Symbol && t.text == symbol)
    }

    fn is_sign(&self, ahead: usize) -> bool {
        self.is_sym(ahead, "+") || self.is_sym(ahead, "-")
    }

    /// An imaginary number with this unit, of floats or integers.
    fn is_imaginary(&self, ahead: usize, unit: Unit, float: bool) -> bool {
        self.peek(ahead)
            .is_some_and(|t| t.kind == Kind::Imaginary(unit) && t.text.contains('.') == float)
    }

    /// Whether an expression can start here: FIRST(expression).
    fn starts_expression(&self) -> bool {
        let Some(t) = self.peek(0) else {
            return false;
        };
        match t.kind {
            Kind::Ident | Kind::Integer | Kind::Float | Kind::Imaginary(_) | Kind::String => true,
            Kind::Symbol => ["(", "{", "+", "-"].contains(&t.text),
            Kind::Keyword => ["NOT", "TRUE", "FALSE", "THIS"]
                .iter()
                .any(|w| t.text.eq_ignore_ascii_case(w)),
        }
    }

    fn take(&mut self) -> Node<'a> {
        let token = self.tokens[self.pos];
        self.pos += 1;
        Node::Token(token)
    }

    fn expect(&mut self, what: Expected) {
        if self.pos > self.furthest {
            self.furthest = self.pos;
            self.expected.clear();
        }
        if self.pos == self.furthest && !self.expected.contains(&what) {
            self.expected.push(what);
        }
    }

    fn fail<T>(&mut self, what: Expected) -> Parse<T> {
        self.expect(what);
        Err(Fail)
    }

    fn kw(&mut self, word: &'static str) -> Parse<Node<'a>> {
        if self.is_kw(0, word) {
            Ok(self.take())
        } else {
            self.fail(Expected::Word(word))
        }
    }

    fn kw_of(&mut self, words: &[&'static str]) -> Parse<Node<'a>> {
        if self.is_any_kw(words) {
            return Ok(self.take());
        }
        for word in words {
            self.expect(Expected::Word(word));
        }
        Err(Fail)
    }

    fn sym(&mut self, symbol: &'static str) -> Parse<Node<'a>> {
        if self.is_sym(0, symbol) {
            Ok(self.take())
        } else {
            self.fail(Expected::Word(symbol))
        }
    }

    fn string(&mut self) -> Parse<Node<'a>> {
        if self.is_kind(0, Kind::String) {
            Ok(self.take())
        } else {
            self.fail(Expected::Thing("a string"))
        }
    }

    /// An optional keyword: taken into `children` if there.
    fn opt_kw(&mut self, word: &'static str, children: &mut Vec<Node<'a>>) -> bool {
        let there = self.is_kw(0, word);
        if there {
            children.push(self.take());
        } else {
            self.expect(Expected::Word(word));
        }
        there
    }

    fn opt_sym(&mut self, symbol: &'static str, children: &mut Vec<Node<'a>>) -> bool {
        let there = self.is_sym(0, symbol);
        if there {
            children.push(self.take());
        } else {
            self.expect(Expected::Word(symbol));
        }
        there
    }

    /// An operator among `ops`, keywords or symbols. Not recorded as
    /// expected when absent: every expression could go on with one.
    fn opt_op(&mut self, ops: &[&str]) -> Option<Node<'a>> {
        let there = ops.iter().any(|op| {
            if op.starts_with(|c: char| c.is_ascii_alphabetic()) {
                self.is_kw(0, op)
            } else {
                self.is_sym(0, op)
            }
        });
        there.then(|| self.take())
    }

    /// Tries `rule` here; on failure, back to where it started.
    fn attempt<T>(&mut self, rule: impl FnOnce(&mut Self) -> Parse<T>) -> Option<T> {
        let start = self.pos;
        let result = rule(self).ok();
        if result.is_none() {
            self.pos = start;
        }
        result
    }

    /// `item ("," item)*`
    fn list(&mut self, rule: Rule, item: Item<'a>) -> Parse<Node<'a>> {
        let mut c = vec![item(self)?];
        while self.opt_sym(",", &mut c) {
            c.push(item(self)?);
        }
        Ok(node(rule, c))
    }

    /// `item (op item)*`, a `?` rule if `inlined`.
    fn chain(
        &mut self,
        rule: Rule,
        item: Item<'a>,
        ops: &[&str],
        inlined: bool,
    ) -> Parse<Node<'a>> {
        let mut c = vec![item(self)?];
        while let Some(op) = self.opt_op(ops) {
            c.push(op);
            c.push(item(self)?);
        }
        Ok(if inlined {
            inline(rule, c)
        } else {
            node(rule, c)
        })
    }

    fn error(&self) -> Error {
        let (line, column, found) = match self.tokens.get(self.furthest) {
            Some(t) => (t.line, t.column, format!("`{}`", t.text)),
            None => match self.tokens.last() {
                Some(t) => (
                    t.line,
                    t.column + t.text.chars().count(),
                    "end of input".into(),
                ),
                None => (1, 1, "end of input".into()),
            },
        };
        let expected: Vec<String> = self
            .expected
            .iter()
            .map(|e| match e {
                Expected::Word(w) => format!("`{w}`"),
                Expected::Thing(t) => t.to_string(),
            })
            .collect();
        let message = match expected.as_slice() {
            [] => format!("unexpected {found}"),
            [one] => format!("expected {one}, found {found}"),
            [init @ .., last] => format!("expected {} or {last}, found {found}", init.join(", ")),
        };
        Error {
            message,
            line,
            column,
        }
    }

    // Modules, page 19

    fn program(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.module()?];
        while self.pos < self.tokens.len() {
            c.push(self.module()?);
        }
        Ok(node(R::Program, c))
    }

    fn module(&mut self) -> Parse<Node<'a>> {
        if self.is_kw(0, "PROGRAM") {
            self.main_program()
        } else if self.is_kw(0, "SUBROUTINE") {
            self.procedure()
        } else if self.is_kw(0, "FUNCTION") {
            self.function()
        } else if self.is_kw(0, "COROUTINE") {
            self.coroutine()
        } else if self.is_kw(0, "FUNCTOR") {
            self.functor()
        } else {
            self.kw_of(&["PROGRAM", "SUBROUTINE", "FUNCTION", "COROUTINE", "FUNCTOR"])
        }
    }

    fn fragment(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.fragment_item()?];
        while self.opt_sym(";", &mut c) {
            c.push(self.fragment_item()?);
        }
        Ok(node(R::Fragment, c))
    }

    fn fragment_item(&mut self) -> Parse<Node<'a>> {
        if self.is_kind(0, Kind::Ident) && (self.is_sym(1, ",") || self.is_sym(1, ":")) {
            self.declaration(true, true)
        } else {
            self.statement()
        }
    }

    fn main_program(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.kw("PROGRAM")?, self.module_name()?, self.sym(";")?];
        c.push(self.locals()?);
        c.push(self.externals()?);
        c.push(self.body()?);
        Ok(node(R::MainProgram, c))
    }

    fn procedure(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.kw("SUBROUTINE")?, self.module_name()?];
        c.push(self.formal_parameters()?);
        c.push(self.sym(";")?);
        self.module_tail(&mut c)?;
        Ok(node(R::Procedure, c))
    }

    fn function(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.kw("FUNCTION")?];
        if self.is_kind(0, Kind::String) {
            let attribute = vec![self.take(), self.sym(".")?, self.string()?];
            c.push(node(R::ComputedAttribute, attribute));
        } else {
            c.push(self.module_name()?);
        }
        c.push(self.formal_parameters()?);
        c.push(self.kw("RETURNS")?);
        c.push(self.type_()?);
        c.push(self.fuzziness());
        c.push(self.sym(";")?);
        self.module_tail(&mut c)?;
        Ok(node(R::Function, c))
    }

    fn coroutine(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.kw("COROUTINE")?, self.module_name()?];
        c.push(self.formal_parameters()?);
        c.push(self.sym(";")?);
        self.module_tail(&mut c)?;
        Ok(node(R::Coroutine, c))
    }

    fn functor(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.kw("FUNCTOR")?];
        if self.is_kind(0, Kind::Ident) && self.is_sym(1, "'") {
            c.push(self.name()?);
            c.push(self.take());
        }
        c.push(self.module_name()?);
        c.push(self.formal_parameters()?);
        c.push(self.kw("RETURNS")?);
        c.push(self.functor_result()?);
        c.push(self.sym(";")?);
        self.module_tail(&mut c)?;
        Ok(node(R::Functor, c))
    }

    /// What follows a module's heading: parameter specifications, locals,
    /// externals, body.
    fn module_tail(&mut self, c: &mut Vec<Node<'a>>) -> Parse<()> {
        let mut specifications = Vec::new();
        while self.is_kind(0, Kind::Ident) {
            specifications.push(self.declaration(false, true)?);
            specifications.push(self.sym(";")?);
        }
        c.push(node(R::ParameterSpecifications, specifications));
        c.push(self.locals()?);
        c.push(self.externals()?);
        c.push(self.body()?);
        Ok(())
    }

    fn module_name(&mut self) -> Parse<Node<'a>> {
        Ok(node(R::ModuleName, vec![self.name()?]))
    }

    fn name(&mut self) -> Parse<Node<'a>> {
        if self.is_kind(0, Kind::Ident) {
            Ok(node(R::Name, vec![self.take()]))
        } else {
            self.fail(Expected::Thing("a name"))
        }
    }

    fn formal_parameters(&mut self) -> Parse<Node<'a>> {
        let mut c = Vec::new();
        if self.opt_sym("(", &mut c) {
            c.push(self.name()?);
            while self.opt_sym(",", &mut c) {
                c.push(self.name()?);
            }
            c.push(self.sym(")")?);
        }
        Ok(node(R::FormalParameters, c))
    }

    /// `names ":" type`, then a mode for a parameter specification, or an
    /// initial value for a local declaration: whichever is allowed and
    /// found.
    fn declaration(&mut self, local: bool, parameter: bool) -> Parse<Node<'a>> {
        let mut c = vec![self.name()?];
        while self.opt_sym(",", &mut c) {
            c.push(self.name()?);
        }
        c.push(self.sym(":")?);
        let (data_type, scalar) = self.data_type()?;
        if parameter && (!local || self.is_any_kw(MODES)) {
            c.push(node(R::ParameterType, vec![data_type]));
            c.push(node(R::Mode, vec![self.kw_of(MODES)?]));
            return Ok(node(R::ParameterSpecification, c));
        }
        if parameter {
            for mode in MODES {
                self.expect(Expected::Word(mode));
            }
        }
        let mut d = vec![data_type];
        if scalar.is_some() {
            if self.is_kw(0, "INIT") {
                d.push(self.initial_value()?);
            } else if scalar == Some(true) && self.is_kw(0, "CONSTANTE") {
                let mut v = vec![self.take(), self.sym("=")?];
                v.push(self.constant()?);
                d.push(node(R::ConstantValue, v));
            } else {
                self.expect(Expected::Word("INIT"));
                if scalar == Some(true) {
                    self.expect(Expected::Word("CONSTANTE"));
                }
            }
        }
        c.push(node(R::DeclaredType, d));
        Ok(node(R::LocalDeclaration, c))
    }

    // Types, pages 20 and 21

    /// A scalar, array, list or compound type, and whether it takes an
    /// initial value: Some(true) a scalar (or a constant), Some(false) an
    /// array, None neither.
    fn data_type(&mut self) -> Parse<(Node<'a>, Option<bool>)> {
        if self.is_kw(0, "SPARSE") || self.is_any_kw(SHAPES) {
            Ok((self.array_type()?, Some(false)))
        } else if self.is_kw(0, "LIST") {
            Ok((self.list_type()?, None))
        } else if self.is_kw(0, "COMPOUND") {
            Ok((node(R::CompoundType, vec![self.take()]), None))
        } else {
            let c = vec![self.type_()?, self.fuzziness()];
            Ok((node(R::ScalarType, c), Some(true)))
        }
    }

    fn array_type(&mut self) -> Parse<Node<'a>> {
        let mut c = Vec::new();
        self.opt_kw("SPARSE", &mut c);
        c.push(self.shape()?);
        c.push(self.kw("OF")?);
        c.push(self.type_()?);
        c.push(self.fuzziness());
        Ok(node(R::ArrayType, c))
    }

    fn shape(&mut self) -> Parse<Node<'a>> {
        let shape = self.kw_of(SHAPES)?;
        let dimensions = match shape {
            Node::Token(t) if t.text.eq_ignore_ascii_case("VECTOR") => 1,
            Node::Token(t) if t.text.eq_ignore_ascii_case("MATRIX") => 2,
            _ => 3,
        };
        let mut c = vec![shape];
        if self.opt_sym("[", &mut c) {
            c.push(self.bounds()?);
            for _ in 1..dimensions {
                c.push(self.sym(",")?);
                c.push(self.bounds()?);
            }
            if dimensions == 3 {
                while self.opt_sym(",", &mut c) {
                    c.push(self.bounds()?);
                }
            }
            c.push(self.sym("]")?);
        }
        Ok(node(R::Shape, c))
    }

    fn bounds(&mut self) -> Parse<Node<'a>> {
        let c = vec![self.bound()?, self.sym(":")?, self.bound()?];
        Ok(node(R::Bounds, c))
    }

    fn bound(&mut self) -> Parse<Node<'a>> {
        let bound = if self.is_kind(0, Kind::Ident) {
            self.name()?
        } else {
            self.integer_constant()?
        };
        Ok(node(R::Bound, vec![bound]))
    }

    fn list_type(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.kw("LIST")?];
        if self.opt_kw("OF", &mut c) {
            c.push(self.type_()?);
            c.push(self.fuzziness());
        }
        Ok(node(R::ListType, c))
    }

    fn starts_type(&self) -> bool {
        self.is_kw(0, "LONG")
            || self.is_any_kw(NUMERIC_TYPES)
            || self.is_any_kw(&["BOOLEAN", "CHARACTER", "STRING"])
            || self.is_any_kw(ADT_TYPES)
    }

    fn type_(&mut self) -> Parse<Node<'a>> {
        let child = if self.is_kw(0, "LONG") || self.is_any_kw(NUMERIC_TYPES) {
            let mut c = Vec::new();
            self.opt_kw("LONG", &mut c);
            c.push(self.kw_of(NUMERIC_TYPES)?);
            node(R::NumericType, c)
        } else if self.is_any_kw(ADT_TYPES) {
            node(R::AdtType, vec![self.take()])
        } else if self.is_any_kw(&["BOOLEAN", "CHARACTER", "STRING"]) {
            self.take()
        } else {
            return self.fail(Expected::Thing("a type"));
        };
        Ok(node(R::Type, vec![child]))
    }

    /// Never fails: no fuzziness is an empty node.
    fn fuzziness(&mut self) -> Node<'a> {
        let mut c = Vec::new();
        if !self.opt_kw("FUZZY", &mut c) {
            for word in ["APPR", "PROB", "SUBJ", "MULT"] {
                self.opt_kw(word, &mut c);
            }
        }
        node(R::Fuzziness, c)
    }

    // Declarations, page 20

    fn locals(&mut self) -> Parse<Node<'a>> {
        let mut c = Vec::new();
        if self.opt_kw("LOCAL", &mut c) {
            while self.is_kind(0, Kind::Ident) {
                c.push(self.declaration(true, false)?);
                c.push(self.sym(";")?);
            }
        }
        Ok(node(R::Locals, c))
    }

    fn initial_value(&mut self) -> Parse<Node<'a>> {
        let c = vec![self.kw("INIT")?, self.constant()?];
        Ok(node(R::InitialValue, c))
    }

    fn externals(&mut self) -> Parse<Node<'a>> {
        let mut c = Vec::new();
        if self.opt_kw("EXTERNAL", &mut c) {
            while self.is_kind(0, Kind::Ident) {
                c.push(self.external()?);
                c.push(self.sym(";")?);
            }
        }
        Ok(node(R::Externals, c))
    }

    fn external(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.module_name()?];
        while self.opt_sym(",", &mut c) {
            c.push(self.module_name()?);
        }
        c.push(self.sym(":")?);
        let function = self.is_kw(0, "FUNCTION");
        let functor = self.is_kw(0, "FUNCTOR");
        let mut k =
            vec![self.kw_of(&["PROGRAM", "SUBROUTINE", "COROUTINE", "FUNCTION", "FUNCTOR"])?];
        if function {
            k.push(self.kw("RETURNS")?);
            k.push(self.type_()?);
            k.push(self.fuzziness());
        } else if functor {
            k.push(self.kw("RETURNS")?);
            k.push(self.functor_result()?);
        }
        c.push(node(R::ModuleKind, k));
        Ok(node(R::External, c))
    }

    fn functor_result(&mut self) -> Parse<Node<'a>> {
        let c = if self.is_kw(0, "SPARSE") || self.is_any_kw(SHAPES) {
            let mut c = Vec::new();
            self.opt_kw("SPARSE", &mut c);
            c.push(self.kw_of(SHAPES)?);
            c.push(self.kw("OF")?);
            c.push(self.type_()?);
            c.push(self.fuzziness());
            c
        } else if self.is_kw(0, "LIST") {
            vec![self.list_type()?]
        } else if self.is_kw(0, "COMPOUND") {
            vec![node(R::CompoundType, vec![self.take()])]
        } else {
            vec![self.type_()?, self.fuzziness()]
        };
        Ok(node(R::FunctorResult, c))
    }

    // Statements, pages 21 to 25

    fn body(&mut self) -> Parse<Node<'a>> {
        let c = vec![
            self.kw("BEGIN")?,
            self.statements()?,
            self.kw("END")?,
            self.sym(";")?,
        ];
        Ok(node(R::Body, c))
    }

    fn statements(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.statement()?];
        while self.opt_sym(";", &mut c) {
            c.push(self.statement()?);
        }
        Ok(node(R::Statements, c))
    }

    /// Anything that starts no statement is the empty statement.
    fn statement(&mut self) -> Parse<Node<'a>> {
        let Some(t) = self.peek(0) else {
            return Ok(node(R::EmptyStatement, vec![]));
        };
        if t.kind == Kind::Ident {
            return self.assignment();
        }
        if t.kind != Kind::Keyword {
            return Ok(node(R::EmptyStatement, vec![]));
        }
        let wrap = |rule, child: Parse<Node<'a>>| Ok(node(rule, vec![child?]));
        match t.text.to_ascii_uppercase().as_str() {
            "LOOP" => wrap(R::Control, self.loop_()),
            "EACH" => wrap(R::Control, self.each_loop()),
            "VECT" => wrap(R::Control, self.vector_loop()),
            "PARBEGIN" => wrap(R::Control, self.parallel()),
            "IF" => wrap(R::Control, self.if_statement()),
            "CASE" => wrap(R::Control, self.case_statement()),
            "EXIT" | "NEXT" | "RELOOP" => {
                let mut c = vec![self.take()];
                if self.starts_expression() {
                    c.push(self.expression()?);
                }
                wrap(R::Control, Ok(node(R::LoopExit, c)))
            }
            "RETURN" | "REPROC" => {
                let exit = node(R::ModuleExit, vec![self.take()]);
                wrap(R::Control, Ok(exit))
            }
            "PAUSE" | "STOP" | "RESTART" => {
                let halt = node(R::Halt, vec![self.take()]);
                wrap(R::Control, Ok(halt))
            }
            "INSERT" | "EXTRACT" | "EMPTY" | "LINEAR" | "CIRCULAR" | "SINGLE" | "TUPLE" => {
                self.list_statement()
            }
            "CREATE" => wrap(R::AdtStatement, self.create()),
            "HIBERN" | "AWAKE" | "DESTROY" | "RESUR" | "PURGE" => {
                wrap(R::AdtStatement, self.state_change())
            }
            "DESIGN" => wrap(R::AdtStatement, self.design()),
            "LET" => wrap(R::AdtStatement, self.adt_assignment()),
            "ASSERT" => wrap(R::AdtStatement, self.assertion()),
            "SPECIFY" => wrap(R::AdtStatement, self.specify()),
            "INCLUDE" | "EXCLUDE" => wrap(R::AdtStatement, self.inclusion()),
            "ASSOC" | "DISSOC" => wrap(R::AdtStatement, self.association()),
            "COMPOSE" => wrap(R::AdtStatement, self.compose()),
            "BUILD" => wrap(R::AdtStatement, self.build()),
            "ACTIVATE" | "REACTIVATE" | "CANCEL" | "KILL" | "DELAY" | "HOLD" | "PASSIVATE"
            | "TERMINATE" => self.process_statement(),
            "CHAIN" => {
                let c = vec![self.take(), self.module_name()?];
                Ok(node(R::Call, c))
            }
            "CALL" | "RESUME" => {
                let c = vec![self.take(), self.invocation()?];
                Ok(node(R::Call, c))
            }
            "READ" => {
                let mut c = vec![self.take(), self.variable()?];
                while self.opt_sym(",", &mut c) {
                    c.push(self.variable()?);
                }
                Ok(node(R::Io, c))
            }
            "PRINT" => {
                let c = vec![self.take(), self.expression_list()?];
                Ok(node(R::Io, c))
            }
            "RESET" => {
                let mut c = vec![self.take(), self.variable()?];
                while self.opt_sym(",", &mut c) {
                    c.push(self.variable()?);
                }
                Ok(node(R::Reset, c))
            }
            "RESULT" => {
                let c = vec![self.take(), self.expression()?];
                Ok(node(R::Result, c))
            }
            _ => Ok(node(R::EmptyStatement, vec![])),
        }
    }

    /// `FOR name ":" expression TO expression (STEP expression)?`
    fn for_clause(&mut self, c: &mut Vec<Node<'a>>) -> Parse<()> {
        c.push(self.kw("FOR")?);
        c.push(self.name()?);
        c.push(self.sym(":")?);
        c.push(self.expression()?);
        c.push(self.kw("TO")?);
        c.push(self.expression()?);
        if self.opt_kw("STEP", c) {
            c.push(self.expression()?);
        }
        Ok(())
    }

    fn loop_(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.kw("LOOP")?];
        if self.is_kw(0, "FOR") {
            self.for_clause(&mut c)?;
        } else {
            self.expect(Expected::Word("FOR"));
        }
        if self.opt_kw("WHILE", &mut c) {
            c.push(self.expression()?);
        }
        c.push(self.kw("DO")?);
        c.push(self.statements()?);
        if self.opt_kw("UNTIL", &mut c) {
            c.push(self.expression()?);
        }
        c.push(self.kw("ENDLOOP")?);
        Ok(node(R::Loop, c))
    }

    fn each_loop(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.kw("EACH")?, self.name()?];
        if self.opt_kw("OF", &mut c) {
            c.push(self.list_expression()?);
        }
        if self.opt_sym("/", &mut c) {
            c.push(self.expression()?);
        }
        c.push(self.kw("DO")?);
        c.push(self.statements()?);
        c.push(self.kw("ENDEACH")?);
        Ok(node(R::EachLoop, c))
    }

    fn vector_loop(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.kw("VECT")?];
        self.for_clause(&mut c)?;
        c.push(self.kw("DO")?);
        c.push(self.statements()?);
        c.push(self.kw("ENDVECT")?);
        Ok(node(R::VectorLoop, c))
    }

    fn parallel(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.kw("PARBEGIN")?, self.statements()?, self.kw("PAR")?];
        c.push(self.statements()?);
        while self.opt_kw("PAR", &mut c) {
            c.push(self.statements()?);
        }
        c.push(self.kw("ENDPAR")?);
        Ok(node(R::Parallel, c))
    }

    fn if_statement(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.kw("IF")?, self.expression()?, self.kw("THEN")?];
        c.push(self.statements()?);
        if self.opt_kw("ELSE", &mut c) {
            c.push(self.statements()?);
        }
        c.push(self.kw("ENDIF")?);
        Ok(node(R::IfStatement, c))
    }

    fn case_statement(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.kw("CASE")?, self.expression()?, self.kw("WHEN")?];
        loop {
            c.push(self.expression_list()?);
            c.push(self.kw("DO")?);
            c.push(self.statements()?);
            if !self.opt_kw("WHEN", &mut c) {
                break;
            }
        }
        if self.opt_kw("OTHERS", &mut c) {
            c.push(self.statements()?);
        }
        c.push(self.kw("ENDCASE")?);
        Ok(node(R::CaseStatement, c))
    }

    fn assignment(&mut self) -> Parse<Node<'a>> {
        let c = vec![self.variable()?, self.sym(":=")?, self.expression()?];
        Ok(node(R::Assignment, c))
    }

    fn list_statement(&mut self) -> Parse<Node<'a>> {
        let mut c = Vec::new();
        if self.is_any_kw(&["INSERT", "EXTRACT"]) {
            let insert = self.is_kw(0, "INSERT");
            c.push(self.take());
            c.push(self.list_expression()?);
            while self.opt_sym(",", &mut c) {
                c.push(self.list_expression()?);
            }
            c.push(self.kw(if insert { "INTO" } else { "OUTOF" })?);
            c.push(self.name()?);
            if insert && self.is_any_kw(&["AFTER", "BEFORE"]) {
                c.push(self.take());
                c.push(self.expression()?);
            } else if insert {
                self.expect(Expected::Word("AFTER"));
                self.expect(Expected::Word("BEFORE"));
            }
        } else if self.is_kw(0, "EMPTY") {
            c.push(self.take());
            c.push(self.name()?);
        } else {
            c.push(self.take());
            c.push(self.name()?);
            while self.opt_sym(",", &mut c) {
                c.push(self.name()?);
            }
        }
        Ok(node(R::ListStatement, c))
    }

    // A.d.t. statements, pages 23 and 24

    fn create(&mut self) -> Parse<Node<'a>> {
        let c = vec![
            self.kw("CREATE")?,
            self.operands(
                (R::ClassLinkDeclarations, Self::class_link_declaration),
                (R::DesignationList, Self::designation),
            )?,
        ];
        Ok(node(R::Create, c))
    }

    fn state_change(&mut self) -> Parse<Node<'a>> {
        let c = vec![
            self.take(),
            self.operands(
                (R::ClassLinkList, Self::class_link),
                (R::DesignationList, Self::designation),
            )?,
        ];
        Ok(node(R::StateChange, c))
    }

    fn design(&mut self) -> Parse<Node<'a>> {
        let c = vec![
            self.kw("DESIGN")?,
            self.operands(
                (R::ClassLinkDeclarations, Self::class_link_declaration),
                (R::ClassList, Self::class),
            )?,
        ];
        Ok(node(R::Design, c))
    }

    /// The operands of CREATE, DESIGN and the state changes: a list of
    /// links, of class attributes, or else of `others`; the first operand
    /// tells which.
    fn operands(&mut self, links: (Rule, Item<'a>), others: (Rule, Item<'a>)) -> Parse<Node<'a>> {
        if let Some(list) = self.attempt(|p| p.list(links.0, links.1)) {
            Ok(list)
        } else if let Some(list) =
            self.attempt(|p| p.list(R::ClassAttributeList, Self::class_attribute))
        {
            Ok(list)
        } else {
            self.list(others.0, others.1)
        }
    }

    fn class_link_declaration(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.class_link()?];
        if self.opt_kw("REVERSE", &mut c) {
            c.push(self.adt_name()?);
        } else {
            self.opt_kw("REFLEXIVE", &mut c);
        }
        Ok(node(R::ClassLinkDeclaration, c))
    }

    fn adt_assignment(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.kw("LET")?];
        match self.attempt(Self::property) {
            Some(property) => c.push(property),
            None => c.push(self.object_attribute()?),
        }
        c.push(self.sym(":=")?);
        c.push(self.expression()?);
        Ok(node(R::AdtAssignment, c))
    }

    fn property(&mut self) -> Parse<Node<'a>> {
        let c = vec![self.adt()?, self.sym("'")?, self.name()?];
        Ok(node(R::Property, c))
    }

    fn object_attribute(&mut self) -> Parse<Node<'a>> {
        let attribute = self.attempt(|p| Ok(vec![p.object()?, p.sym(".")?, p.adt_name()?]));
        let c = match attribute {
            Some(c) => c,
            None => vec![self.name()?],
        };
        Ok(node(R::ObjectAttribute, c))
    }

    fn assertion(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.kw("ASSERT")?];
        let starts_object = self.is_kind(0, Kind::Ident)
            || self.is_kind(0, Kind::String)
            || self.is_sym(0, "(")
            || self.is_kw(0, "THIS");
        let link = starts_object
            .then(|| {
                self.attempt(|p| {
                    let mut c = vec![p.object()?];
                    p.opt_kw("NOT", &mut c);
                    c.push(p.string()?);
                    if p.opt_sym("%", &mut c) {
                        c.push(p.real()?);
                    }
                    c.push(p.object()?);
                    Ok(c)
                })
            })
            .flatten();
        match link {
            Some(link) => c.extend(link),
            None => c.push(self.name()?),
        }
        Ok(node(R::Assertion, c))
    }

    fn specify(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.kw("SPECIFY")?, self.specified()?];
        while self.opt_sym(",", &mut c) {
            c.push(self.specified()?);
        }
        self.opt_sym(":", &mut c);
        c.push(self.specification()?);
        if self.opt_kw("UNIT", &mut c) {
            c.push(self.sym("(")?);
            c.push(self.name()?);
            c.push(self.sym(")")?);
        }
        self.opt_kw("COMPUTED", &mut c);
        Ok(node(R::Specify, c))
    }

    /// A class, a class attribute or a class link.
    fn specified(&mut self) -> Parse<Node<'a>> {
        let class = self.class()?;
        if self.is_sym(0, ".") {
            let c = vec![class, self.take(), self.adt_name()?];
            Ok(node(R::ClassAttribute, c))
        } else if self.is_kind(0, Kind::String) {
            let c = vec![class, self.take(), self.class()?];
            Ok(node(R::ClassLink, c))
        } else {
            self.expect(Expected::Word("."));
            self.expect(Expected::Thing("a string"));
            Ok(class)
        }
    }

    fn specification(&mut self) -> Parse<Node<'a>> {
        let c = if self.is_any_kw(SHAPES) {
            let mut c = vec![self.take()];
            if self.opt_kw("OF", &mut c) {
                c.push(self.type_()?);
                c.push(self.fuzziness());
            }
            c
        } else if self.is_kw(0, "LIST") {
            vec![self.list_type()?]
        } else if self.is_kw(0, "COMPOUND") {
            vec![node(R::CompoundType, vec![self.take()])]
        } else if self.starts_type() {
            vec![self.type_()?, self.fuzziness()]
        } else {
            self.expect(Expected::Thing("a type"));
            vec![self.fuzziness()]
        };
        Ok(node(R::Specification, c))
    }

    /// INCLUDE classes INTO classes, EXCLUDE classes OUTOF classes.
    fn inclusion(&mut self) -> Parse<Node<'a>> {
        let include = self.is_kw(0, "INCLUDE");
        let c = vec![
            self.take(),
            self.list(R::ClassList, Self::class)?,
            self.kw(if include { "INTO" } else { "OUTOF" })?,
            self.list(R::ClassList, Self::class)?,
        ];
        Ok(node(if include { R::Include } else { R::Exclude }, c))
    }

    /// ASSOC a WITH d, ...; DISSOC a FROM d, ...
    fn association(&mut self) -> Parse<Node<'a>> {
        let assoc = self.is_kw(0, "ASSOC");
        let with = if assoc { "WITH" } else { "FROM" };
        let mut c = vec![self.take()];
        loop {
            c.push(self.adt()?);
            c.push(self.kw(with)?);
            c.push(self.designation()?);
            if !self.opt_sym(",", &mut c) {
                break;
            }
        }
        Ok(node(if assoc { R::Associate } else { R::Dissociate }, c))
    }

    fn compose(&mut self) -> Parse<Node<'a>> {
        let c = vec![
            self.kw("COMPOSE")?,
            self.class()?,
            self.kw("WITH")?,
            self.list(R::ClassList, Self::class)?,
        ];
        Ok(node(R::Compose, c))
    }

    fn build(&mut self) -> Parse<Node<'a>> {
        let c = vec![
            self.kw("BUILD")?,
            self.list(R::ClassList, Self::class)?,
            self.kw("AS")?,
            self.class()?,
        ];
        Ok(node(R::Build, c))
    }

    fn process_statement(&mut self) -> Parse<Node<'a>> {
        let word = self.tokens[self.pos].text.to_ascii_uppercase();
        let mut c = vec![self.take()];
        match word.as_str() {
            "PASSIVATE" | "TERMINATE" => {}
            "REACTIVATE" => {
                c.push(self.object()?);
                if self.opt_kw("AT", &mut c) {
                    c.push(self.expression()?);
                } else if self.is_any_kw(&["BEFORE", "AFTER"]) {
                    c.push(self.take());
                    c.push(self.object()?);
                } else {
                    self.expect(Expected::Word("BEFORE"));
                    self.expect(Expected::Word("AFTER"));
                }
            }
            "DELAY" | "HOLD" => {
                c.push(self.object()?);
                c.push(self.kw("FOR")?);
                c.push(self.expression()?);
            }
            _ => c.push(self.object()?),
        }
        Ok(node(R::ProcessStatement, c))
    }

    fn invocation(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.module_name()?];
        if self.opt_sym("(", &mut c) {
            c.push(self.expression_list()?);
            c.push(self.sym(")")?);
        }
        Ok(node(R::Invocation, c))
    }

    // A.d.t. designations, pages 23 and 24

    fn class(&mut self) -> Parse<Node<'a>> {
        Ok(node(R::Class, vec![self.adt_name()?]))
    }

    fn class_attribute(&mut self) -> Parse<Node<'a>> {
        let c = vec![self.class()?, self.sym(".")?, self.adt_name()?];
        Ok(node(R::ClassAttribute, c))
    }

    fn class_link(&mut self) -> Parse<Node<'a>> {
        let c = vec![self.class()?, self.string()?, self.class()?];
        Ok(node(R::ClassLink, c))
    }

    /// A name alone, or a name of class: `p`, `"P" [ "POINT" ]`, THIS.
    fn object(&mut self) -> Parse<Node<'a>> {
        let c = if self.is_kw(0, "THIS") {
            vec![self.take()]
        } else if self.is_kind(0, Kind::Ident) && !self.is_sym(1, "[") && !self.is_sym(1, "(") {
            vec![self.name()?]
        } else {
            let name = self.adt_name()?;
            vec![name, self.sym("[")?, self.class()?, self.sym("]")?]
        };
        Ok(node(R::Object, c))
    }

    fn designation(&mut self) -> Parse<Node<'a>> {
        let c = if self.is_kw(0, "THIS") {
            vec![self.take()]
        } else {
            let name = self.adt_name()?;
            if self.is_sym(0, "[") {
                vec![name, self.take(), self.class()?, self.sym("]")?]
            } else {
                self.expect(Expected::Word("["));
                vec![node(R::Class, vec![name])]
            }
        };
        Ok(node(R::Designation, c))
    }

    fn adt(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.designation()?];
        if self.opt_sym(".", &mut c) {
            c.push(self.adt_name()?);
        } else if self.is_kind(0, Kind::String) {
            c.push(self.take());
            c.push(self.designation()?);
        } else {
            self.expect(Expected::Thing("a string"));
        }
        Ok(node(R::Adt, c))
    }

    fn adt_name(&mut self) -> Parse<Node<'a>> {
        let c = if self.is_kind(0, Kind::Ident) && self.is_sym(1, "(") {
            vec![self.function_call()?]
        } else if self.is_kind(0, Kind::Ident) {
            vec![self.name()?]
        } else if self.is_kind(0, Kind::String) {
            vec![self.take()]
        } else if self.is_sym(0, "(") {
            vec![self.take(), self.expression()?, self.sym(")")?]
        } else {
            return self.fail(Expected::Thing("a name, a string or `(`"));
        };
        Ok(node(R::AdtName, c))
    }

    // Constants, pages 25 and 26

    fn constant(&mut self) -> Parse<Node<'a>> {
        let child = if self.is_kind(0, Kind::String) {
            self.take()
        } else if self.is_any_kw(&["TRUE", "FALSE"]) {
            node(R::BooleanConstant, vec![self.take()])
        } else {
            self.arithmetic_constant()?
        };
        Ok(node(R::Constant, vec![child]))
    }

    fn sign(&mut self) -> Node<'a> {
        node(R::Sign, vec![self.take()])
    }

    fn arithmetic_constant(&mut self) -> Parse<Node<'a>> {
        let at = usize::from(self.is_sign(0));
        let (rule, parts) = if self.is_kind(at, Kind::Float) {
            if !(self.is_sign(at + 1) && self.is_imaginary(at + 2, Unit::I, true)) {
                (R::FloatConstant, 0)
            } else if self.is_sign(at + 3) && self.is_imaginary(at + 4, Unit::J, true) {
                (R::QuaternionConstant, 3)
            } else {
                (R::ComplexConstant, 1)
            }
        } else if self.is_kind(at, Kind::Integer) {
            if self.is_sym(at + 1, "//") {
                (R::RationalConstant, 0)
            } else if self.is_sign(at + 1) && self.is_imaginary(at + 2, Unit::I, false) {
                (R::GaussConstant, 1)
            } else {
                (R::IntegerConstant, 0)
            }
        } else {
            return self.fail(Expected::Thing("a constant"));
        };
        let mut c = Vec::new();
        if at == 1 {
            c.push(self.sign());
        }
        c.push(self.take());
        if rule == R::RationalConstant {
            c.push(self.take());
            if !self.is_kind(0, Kind::Integer) {
                return self.fail(Expected::Thing("an integer"));
            }
            c.push(self.take());
        }
        for unit in [Unit::I, Unit::J, Unit::K].into_iter().take(parts) {
            if !self.is_sign(0) {
                return self.fail(Expected::Thing("a sign"));
            }
            c.push(self.sign());
            if !self.is_imaginary(0, unit, rule != R::GaussConstant) {
                return self.fail(Expected::Thing("an imaginary number"));
            }
            c.push(self.take());
        }
        Ok(node(R::ArithmeticConstant, vec![node(rule, c)]))
    }

    fn integer_constant(&mut self) -> Parse<Node<'a>> {
        let mut c = Vec::new();
        if self.is_sign(0) {
            c.push(self.sign());
        }
        if !self.is_kind(0, Kind::Integer) {
            return self.fail(Expected::Thing("an integer"));
        }
        c.push(self.take());
        Ok(node(R::IntegerConstant, c))
    }

    // Expressions, pages 26 to 28, loosest operator first

    fn expression(&mut self) -> Parse<Node<'a>> {
        let start = self.pos;
        if let Some(memo) = self.expressions.get(&start) {
            return match memo.clone() {
                Some((node, end)) => {
                    self.pos = end;
                    Ok(node)
                }
                None => Err(Fail),
            };
        }
        let result = self.disjunction();
        let memo = result.as_ref().ok().map(|node| (node.clone(), self.pos));
        self.expressions.insert(start, memo);
        result
    }

    fn expression_list(&mut self) -> Parse<Node<'a>> {
        self.list(R::ExpressionList, Self::expression)
    }

    fn disjunction(&mut self) -> Parse<Node<'a>> {
        self.chain(
            R::Disjunction,
            Self::conjunction,
            &["OR", "NOR", "XOR"],
            true,
        )
    }

    fn conjunction(&mut self) -> Parse<Node<'a>> {
        self.chain(R::Conjunction, Self::negation, &["AND", "NAND"], true)
    }

    fn negation(&mut self) -> Parse<Node<'a>> {
        if self.is_kw(0, "NOT") {
            let c = vec![self.take(), self.negation()?];
            Ok(node(R::Negation, c))
        } else {
            self.relation()
        }
    }

    /// The object link, `a "r" b`, is tried first: it is a relation unless
    /// a `'` follows, which makes it a functor's prefix.
    fn relation(&mut self) -> Parse<Node<'a>> {
        let starts_object = self.is_kind(0, Kind::Ident)
            || self.is_kind(0, Kind::String)
            || self.is_sym(0, "(")
            || self.is_kw(0, "THIS");
        if starts_object {
            let link = self.attempt(|p| {
                let c = vec![p.object()?, p.string()?, p.object()?];
                if p.is_sym(0, "'") { Err(Fail) } else { Ok(c) }
            });
            if let Some(c) = link {
                return Ok(node(R::Relation, c));
            }
        }
        let left = self.union()?;
        let op = if let Some(op) = self.opt_op(COMPARATORS) {
            node(R::Comparator, vec![op])
        } else if let Some(op) = self.opt_op(&["IN", "NOTIN"]) {
            op
        } else {
            return Ok(left);
        };
        let c = vec![left, op, self.union()?];
        Ok(node(R::Relation, c))
    }

    fn union(&mut self) -> Parse<Node<'a>> {
        self.chain(R::Union, Self::intersection, &["UNION"], true)
    }

    fn intersection(&mut self) -> Parse<Node<'a>> {
        self.chain(R::Intersection, Self::complement, &["INTER"], true)
    }

    fn complement(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.concatenation()?];
        if let Some(op) = self.opt_op(&["COMPL"]) {
            c.push(op);
            c.push(self.concatenation()?);
        }
        Ok(inline(R::Complement, c))
    }

    fn concatenation(&mut self) -> Parse<Node<'a>> {
        self.chain(R::Concatenation, Self::sum, &["&"], true)
    }

    fn sum(&mut self) -> Parse<Node<'a>> {
        let mut c = Vec::new();
        if self.is_sign(0) {
            c.push(self.sign());
        }
        c.push(self.product()?);
        while let Some(op) = self.opt_op(&["+", "-", "[+]"]) {
            c.push(op);
            c.push(self.product()?);
        }
        Ok(inline(R::Sum, c))
    }

    fn product(&mut self) -> Parse<Node<'a>> {
        let ops = ["*", "/", "MOD", "//", "[*]", "[.]", "[^]"];
        self.chain(R::Product, Self::power, &ops, true)
    }

    fn power(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.operand()?];
        if let Some(op) = self.opt_op(&["^"]) {
            c.push(op);
            c.push(self.operand()?);
        }
        Ok(inline(R::Power, c))
    }

    /// A designation's reading is tried first, a functor's result, then an
    /// object's attribute: `a` alone is a variable, `a ' F` a functor's.
    fn operand(&mut self) -> Parse<Node<'a>> {
        let Some(t) = self.peek(0) else {
            return self.fail(Expected::Thing("an expression"));
        };
        match t.kind {
            Kind::Integer | Kind::Float | Kind::Imaginary(_) => return self.fuzzy_number(),
            Kind::Keyword if self.is_any_kw(&["TRUE", "FALSE"]) => {
                return Ok(node(R::BooleanConstant, vec![self.take()]));
            }
            Kind::Symbol if t.text == "{" => return self.multiple(),
            Kind::Ident | Kind::String => {}
            Kind::Symbol if t.text == "(" => {}
            Kind::Keyword if self.is_kw(0, "THIS") => {}
            _ => return self.fail(Expected::Thing("an expression")),
        }
        if let Some(functor) = self.attempt(Self::functor_expression) {
            return Ok(functor);
        }
        let attribute = self.attempt(|p| Ok(vec![p.object()?, p.sym(".")?, p.adt_name()?]));
        if let Some(c) = attribute {
            return Ok(node(R::ObjectAttributeValue, c));
        }
        match t.kind {
            Kind::Ident if self.is_sym(1, "(") => self.function_call(),
            Kind::Ident => self.variable(),
            Kind::String => Ok(self.take()),
            Kind::Symbol => {
                let c = vec![self.take(), self.expression()?, self.sym(")")?];
                Ok(node(R::Operand, c))
            }
            _ => Err(Fail),
        }
    }

    fn fuzzy_number(&mut self) -> Parse<Node<'a>> {
        let number = match self.peek(0).map(|t| t.kind) {
            Some(Kind::Integer | Kind::Float) => node(R::Number, vec![self.real()?]),
            Some(Kind::Imaginary(_)) => node(R::Number, vec![self.take()]),
            _ => return self.fail(Expected::Thing("a number")),
        };
        let mut c = vec![number];
        if self.opt_sym("+-", &mut c) {
            c.push(self.real()?);
        }
        if self.opt_sym("%", &mut c) {
            c.push(self.real()?);
        }
        if self.opt_sym("@", &mut c) {
            c.push(self.string()?);
        }
        Ok(node(R::FuzzyNumber, c))
    }

    fn real(&mut self) -> Parse<Node<'a>> {
        if self.is_kind(0, Kind::Integer) || self.is_kind(0, Kind::Float) {
            Ok(node(R::Real, vec![self.take()]))
        } else {
            self.fail(Expected::Thing("a real number"))
        }
    }

    fn multiple(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.sym("{")?];
        loop {
            if self.is_sign(0) {
                c.push(self.sign());
            }
            c.push(self.fuzzy_number()?);
            if !self.opt_sym(",", &mut c) {
                break;
            }
        }
        c.push(self.sym("}")?);
        Ok(node(R::Multiple, c))
    }

    fn function_call(&mut self) -> Parse<Node<'a>> {
        let c = vec![
            self.module_name()?,
            self.sym("(")?,
            self.expression_list()?,
            self.sym(")")?,
        ];
        Ok(node(R::FunctionCall, c))
    }

    fn functor_expression(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.adt()?, self.sym("'")?, self.invocation()?];
        while self.opt_sym("'", &mut c) {
            c.push(self.invocation()?);
        }
        Ok(node(R::FunctorExpression, c))
    }

    fn list_expression(&mut self) -> Parse<Node<'a>> {
        let union = self.chain(R::ListUnion, Self::list_intersection, &["UNION"], false)?;
        Ok(node(R::ListExpression, vec![union]))
    }

    fn list_intersection(&mut self) -> Parse<Node<'a>> {
        self.chain(
            R::ListIntersection,
            Self::list_complement,
            &["INTER"],
            false,
        )
    }

    fn list_complement(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.list_term()?];
        if let Some(op) = self.opt_op(&["COMPL"]) {
            c.push(op);
            c.push(self.list_term()?);
        }
        Ok(node(R::ListComplement, c))
    }

    fn list_term(&mut self) -> Parse<Node<'a>> {
        let c = if let Some(functor) = self.attempt(Self::functor_expression) {
            vec![functor]
        } else if self.is_kind(0, Kind::Ident) {
            vec![self.variable()?]
        } else if self.is_sym(0, "(") {
            vec![self.take(), self.expression()?, self.sym(")")?]
        } else {
            return self.fail(Expected::Thing("a list"));
        };
        Ok(node(R::ListTerm, c))
    }

    fn variable(&mut self) -> Parse<Node<'a>> {
        let mut c = vec![self.name()?];
        if self.opt_sym("[", &mut c) {
            c.push(self.expression_list()?);
            c.push(self.sym("]")?);
        }
        Ok(node(R::Variable, c))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_match_the_grammar() {
        let grammar = include_str!("../../adt81/grammar.lark");
        let mut defined: Vec<&str> = grammar
            .lines()
            .filter_map(|l| l.trim_start_matches('?').split_once(':'))
            .map(|(name, _)| name)
            .filter(|name| {
                !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            })
            .collect();
        // The start symbol, and `?` rules that only ever hold one child.
        let never_nodes = [
            "start",
            "module",
            "fragment_item",
            "statement",
            "specified",
            "expression",
        ];
        defined.retain(|name| !never_nodes.contains(name));
        defined.sort_unstable();
        let mut ours: Vec<&str> = Rule::ALL.iter().map(|r| r.name()).collect();
        ours.sort_unstable();
        assert_eq!(ours, defined);
    }

    #[test]
    fn tree_display() {
        let tree = parse_fragment("x := 1").unwrap();
        let expected = "\
fragment
  assignment
    variable
      name
        - x
    - :=
    fuzzy_number
      number
        real
          - 1
";
        assert_eq!(tree.to_string(), expected);
    }

    #[test]
    fn errors() {
        let error = |source| parse_fragment(source).unwrap_err().to_string();
        assert_eq!(error("x := ;"), "1:6: expected an expression, found `;`");
        assert_eq!(
            error("IF a THEN b := 1"),
            "1:17: expected `+-`, `%`, `@`, `;`, `ELSE` or `ENDIF`, found end of input"
        );
        assert_eq!(
            error("x := (a + b ;"),
            "1:13: expected `[`, `.`, a string, `'` or `)`, found `;`"
        );
        assert_eq!(
            parse_program("x := 1").unwrap_err().to_string(),
            "1:1: expected `PROGRAM`, `SUBROUTINE`, `FUNCTION`, `COROUTINE` or `FUNCTOR`, found `x`"
        );
    }
}
