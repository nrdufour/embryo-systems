//! From the parser's tree to the [`ast`](super::ast): punctuation dropped,
//! precedence chains folded into left-associative binary operators,
//! parentheses and empty statements gone, names and numbers read.
//!
//! The tree is the parser's own, so its shapes are known: a shape this
//! module does not expect is a bug in one of the two, and panics. The only
//! error is a number too large for its type.

use super::ast::*;
use super::lexer::{Error, Kind, Token};
use super::parser::{self, Node, Rule as R, Tree};

type Lowered<T> = Result<T, Error>;

/// Parses and lowers a whole source file.
pub fn parse_program(source: &str) -> Lowered<Program> {
    program(&parser::parse_program(source)?)
}

/// Parses and lowers declarations and statements outside any module.
pub fn parse_fragment(source: &str) -> Lowered<Fragment> {
    fragment(&parser::parse_fragment(source)?)
}

pub fn program(tree: &Tree) -> Lowered<Program> {
    let modules = trees(tree).map(module).collect::<Lowered<_>>()?;
    Ok(Program { modules })
}

pub fn fragment(tree: &Tree) -> Lowered<Fragment> {
    let mut items = Vec::new();
    for item in trees(tree) {
        items.push(match item.rule {
            R::LocalDeclaration => Item::Local(declaration(item)?),
            R::ParameterSpecification => Item::Parameter(parameter_spec(item)?),
            _ => match statement(item)? {
                Some(stmt) => Item::Stmt(stmt),
                None => continue,
            },
        });
    }
    Ok(Fragment { items })
}

// Reading the tree

fn first_token<'a>(node: &Node<'a>) -> Option<Token<'a>> {
    match node {
        Node::Token(t) => Some(*t),
        Node::Tree(t) => t.children.iter().find_map(first_token),
    }
}

fn span(node: &Node) -> Span {
    first_token(node)
        .map(|t| Span {
            line: t.line,
            column: t.column,
        })
        .unwrap_or_default()
}

fn tree_span(tree: &Tree) -> Span {
    tree.children.iter().map(span).next().unwrap_or_default()
}

fn as_tree<'t, 'a>(node: &'t Node<'a>) -> &'t Tree<'a> {
    match node {
        Node::Tree(t) => t,
        Node::Token(t) => panic!("expected a node, found the token {:?}", t.text),
    }
}

fn as_token<'a>(node: &Node<'a>) -> Token<'a> {
    match node {
        Node::Token(t) => *t,
        Node::Tree(t) => panic!("expected a token, found {}", t.rule.name()),
    }
}

/// The child nodes, tokens aside.
fn trees<'t, 'a>(tree: &'t Tree<'a>) -> impl Iterator<Item = &'t Tree<'a>> {
    tree.children.iter().filter_map(|c| match c {
        Node::Tree(t) => Some(t),
        Node::Token(_) => None,
    })
}

fn trees_of<'t, 'a>(tree: &'t Tree<'a>, rule: R) -> impl Iterator<Item = &'t Tree<'a>> {
    trees(tree).filter(move |t| t.rule == rule)
}

fn child<'t, 'a>(tree: &'t Tree<'a>, rule: R) -> Option<&'t Tree<'a>> {
    trees_of(tree, rule).next()
}

fn the<'t, 'a>(tree: &'t Tree<'a>, rule: R) -> &'t Tree<'a> {
    child(tree, rule).unwrap_or_else(|| panic!("{} without {}", tree.rule.name(), rule.name()))
}

fn is_punctuation(node: &Node) -> bool {
    matches!(node, Node::Token(t) if matches!(t.kind, Kind::Keyword | Kind::Symbol))
}

/// Whether a keyword or symbol is among the direct children.
fn has(tree: &Tree, word: &str) -> bool {
    tree.children
        .iter()
        .any(|c| is_punctuation(c) && as_token(c).text.eq_ignore_ascii_case(word))
}

/// The first token's text, upper-cased: the keyword a rule starts with.
fn keyword(tree: &Tree) -> String {
    as_token(&tree.children[0]).text.to_ascii_uppercase()
}

type Sections<'t, 'a> = Vec<(String, Vec<&'t Node<'a>>)>;

/// The children cut at each keyword or symbol, each section holding what
/// follows it up to the next; the first, keyed "", what precedes them all.
fn sections<'t, 'a>(tree: &'t Tree<'a>) -> Sections<'t, 'a> {
    let mut out = vec![(String::new(), Vec::new())];
    for c in &tree.children {
        if is_punctuation(c) {
            out.push((as_token(c).text.to_ascii_uppercase(), Vec::new()));
        } else {
            out.last_mut().unwrap().1.push(c);
        }
    }
    out
}

/// What follows the first `key` with anything after it.
fn after<'t, 'a>(sections: &Sections<'t, 'a>, key: &str) -> Option<&'t Node<'a>> {
    sections
        .iter()
        .find(|(k, nodes)| k == key && !nodes.is_empty())
        .map(|(_, nodes)| nodes[0])
}

fn name(tree: &Tree) -> Name {
    Name::new(as_token(&tree.children[0]).text, tree_span(tree))
}

fn names(tree: &Tree) -> Vec<Name> {
    trees_of(tree, R::Name).map(name).collect()
}

fn module_name(tree: &Tree) -> Name {
    name(the(tree, R::Name))
}

fn string(node: &Node) -> Str {
    let token = as_token(node);
    Str {
        text: token.text[1..token.text.len() - 1].replace("\"\"", "\""),
        span: span(node),
    }
}

fn error(node: &Node, message: &str) -> Error {
    let span = span(node);
    Error {
        message: message.to_string(),
        line: span.line,
        column: span.column,
    }
}

// Modules, page 19

fn module(tree: &Tree) -> Lowered<Module> {
    let mut name = None;
    let mut prefix = None;
    let mut parameters = Vec::new();
    let mut specifications = Vec::new();
    let mut locals = Vec::new();
    let mut externals = Vec::new();
    let mut body = Vec::new();
    let mut functor_returns = None;
    for c in trees(tree) {
        match c.rule {
            R::ModuleName => name = Some(ModuleName::Name(module_name(c))),
            R::ProcessClass => name = Some(ModuleName::Body(string(&c.children[0]))),
            R::ComputedAttribute => {
                name = Some(ModuleName::Attribute {
                    class: string(&c.children[0]),
                    attribute: string(&c.children[2]),
                });
            }
            R::Name => prefix = Some(self::name(c)),
            R::FormalParameters => parameters = names(c),
            R::ParameterSpecifications => {
                specifications = trees(c).map(parameter_spec).collect::<Lowered<_>>()?;
            }
            R::Locals => locals = trees(c).map(declaration).collect::<Lowered<_>>()?,
            R::Externals => externals = trees(c).map(external).collect::<Lowered<_>>()?,
            R::Body => body = statements(the(c, R::Statements))?,
            R::FunctorResult => functor_returns = Some(functor_result(c)),
            R::Type | R::Fuzziness => {}
            rule => panic!("{} in a module", rule.name()),
        }
    }
    let kind = match tree.rule {
        R::MainProgram => ModuleKind::Program,
        R::Procedure => ModuleKind::Subroutine,
        R::Coroutine => ModuleKind::Coroutine,
        R::Function => ModuleKind::Function {
            returns: scalar_in(tree).expect("a function without its type"),
        },
        R::Functor => ModuleKind::Functor {
            prefix,
            returns: functor_returns.expect("a functor without its result"),
        },
        rule => panic!("{} is not a module", rule.name()),
    };
    Ok(Module {
        kind,
        name: name.expect("a module without its name"),
        parameters,
        specifications,
        locals,
        externals,
        body,
        span: tree_span(tree),
    })
}

fn parameter_spec(tree: &Tree) -> Lowered<ParameterSpec> {
    let ty = the(tree, R::ParameterType);
    Ok(ParameterSpec {
        names: names(tree),
        ty: data_type(as_tree(&ty.children[0]))?,
        mode: match keyword(the(tree, R::Mode)).as_str() {
            "IN" => Mode::In,
            "INOUT" => Mode::InOut,
            _ => Mode::Out,
        },
        span: tree_span(tree),
    })
}

fn declaration(tree: &Tree) -> Lowered<Declaration> {
    let declared = the(tree, R::DeclaredType);
    let init = match trees(declared).nth(1) {
        None => None,
        Some(init) => {
            let value = constant(the(init, R::Constant))?;
            Some(match init.rule {
                R::InitialValue => Init::Initial(value),
                _ => Init::Constant(value),
            })
        }
    };
    Ok(Declaration {
        names: names(tree),
        ty: data_type(as_tree(&declared.children[0]))?,
        init,
        span: tree_span(tree),
    })
}

fn external(tree: &Tree) -> Lowered<External> {
    let kind = the(tree, R::ModuleKind);
    Ok(External {
        names: trees_of(tree, R::ModuleName).map(module_name).collect(),
        kind: match keyword(kind).as_str() {
            "PROGRAM" => ExternalKind::Program,
            "SUBROUTINE" => ExternalKind::Subroutine,
            "COROUTINE" => ExternalKind::Coroutine,
            "FUNCTION" => ExternalKind::Function(scalar_in(kind).expect("a type")),
            _ => ExternalKind::Functor(functor_result(the(kind, R::FunctorResult))),
        },
    })
}

// Types, pages 20 and 21

/// The `type fuzziness` among a node's children, if there.
fn scalar_in(tree: &Tree) -> Option<ScalarType> {
    let ty = child(tree, R::Type)?;
    Some(ScalarType {
        base: base_type(ty),
        fuzziness: fuzziness(the(tree, R::Fuzziness)),
    })
}

fn data_type(tree: &Tree) -> Lowered<DataType> {
    Ok(match tree.rule {
        R::ScalarType => DataType::Scalar(scalar_in(tree).expect("a type")),
        R::ArrayType => DataType::Array {
            sparse: has(tree, "SPARSE"),
            shape: shape(the(tree, R::Shape))?,
            element: scalar_in(tree).expect("an element type"),
        },
        R::ListType => DataType::List(scalar_in(tree)),
        R::CompoundType => DataType::Compound,
        rule => panic!("{} is not a type", rule.name()),
    })
}

fn rank(word: &str) -> Option<Rank> {
    match word.to_ascii_uppercase().as_str() {
        "VECTOR" => Some(Rank::Vector),
        "MATRIX" => Some(Rank::Matrix),
        "TENSOR" => Some(Rank::Tensor),
        _ => None,
    }
}

/// The rank a VECTOR, MATRIX or TENSOR among the children gives, if any.
fn rank_in(tree: &Tree) -> Option<Rank> {
    tree.children
        .iter()
        .filter(|c| is_punctuation(c))
        .find_map(|c| rank(as_token(c).text))
}

fn shape(tree: &Tree) -> Lowered<Shape> {
    let mut bounds = Vec::new();
    for b in trees(tree) {
        let mut ends = trees(b).map(bound);
        let low = ends.next().expect("a lower bound")?;
        let high = ends.next().expect("an upper bound")?;
        bounds.push(Bounds { low, high });
    }
    Ok(Shape {
        rank: rank_in(tree).expect("a rank"),
        bounds,
    })
}

fn bound(tree: &Tree) -> Lowered<Bound> {
    let b = as_tree(&tree.children[0]);
    Ok(match b.rule {
        R::IntegerConstant => Bound::Integer(signed_integers(b)?[0]),
        _ => Bound::Name(name(b)),
    })
}

fn base_type(tree: &Tree) -> BaseType {
    match &tree.children[0] {
        Node::Tree(t) if t.rule == R::NumericType => BaseType::Numeric {
            kind: match as_token(t.children.last().unwrap())
                .text
                .to_ascii_uppercase()
                .as_str()
            {
                "INTEGER" => NumericKind::Integer,
                "FLOAT" => NumericKind::Float,
                "COMPLEX" => NumericKind::Complex,
                "GAUSS" => NumericKind::Gauss,
                "QUATERNION" => NumericKind::Quaternion,
                _ => NumericKind::Rational,
            },
            long: has(t, "LONG"),
        },
        Node::Tree(t) => BaseType::Adt(match keyword(t).as_str() {
            "CLASS" => AdtType::Class,
            "CATTRIBUTE" => AdtType::CAttribute,
            "CLINK" => AdtType::CLink,
            "OBJECT" => AdtType::Object,
            "OATTRIBUTE" => AdtType::OAttribute,
            "OLINK" => AdtType::OLink,
            "HCLASS" => AdtType::HClass,
            "HATTRIBUTE" => AdtType::HAttribute,
            "HLINK" => AdtType::HLink,
            "EMBRYO" => AdtType::Embryo,
            "PROTOTYPE" => AdtType::Prototype,
            _ => AdtType::Structure,
        }),
        Node::Token(t) => match t.text.to_ascii_uppercase().as_str() {
            "BOOLEAN" => BaseType::Boolean,
            "CHARACTER" => BaseType::Character,
            _ => BaseType::String,
        },
    }
}

fn fuzziness(tree: &Tree) -> Fuzziness {
    if has(tree, "FUZZY") {
        Fuzziness::Fuzzy
    } else {
        Fuzziness::Kinds {
            appr: has(tree, "APPR"),
            prob: has(tree, "PROB"),
            subj: has(tree, "SUBJ"),
            mult: has(tree, "MULT"),
        }
    }
}

fn functor_result(tree: &Tree) -> DataType {
    if let Some(list) = child(tree, R::ListType) {
        DataType::List(scalar_in(list))
    } else if child(tree, R::CompoundType).is_some() {
        DataType::Compound
    } else if let Some(rank) = rank_in(tree) {
        DataType::Array {
            sparse: has(tree, "SPARSE"),
            shape: Shape {
                rank,
                bounds: Vec::new(),
            },
            element: scalar_in(tree).expect("an element type"),
        }
    } else {
        DataType::Scalar(scalar_in(tree).expect("a type"))
    }
}

// Statements, pages 21 to 25

fn statements(tree: &Tree) -> Lowered<Vec<Stmt>> {
    let mut out = Vec::new();
    for s in trees(tree) {
        out.extend(statement(s)?);
    }
    Ok(out)
}

fn statements_after(sections: &Sections, key: &str) -> Lowered<Vec<Stmt>> {
    statements(as_tree(after(sections, key).expect("statements")))
}

fn expr_after(sections: &Sections, key: &str) -> Lowered<Option<Expr>> {
    after(sections, key).map(expr).transpose()
}

fn range(sections: &Sections) -> Lowered<Option<Range>> {
    let Some(name) = after(sections, "FOR") else {
        return Ok(None);
    };
    Ok(Some(Range {
        name: self::name(as_tree(name)),
        from: expr_after(sections, ":")?.expect("a start"),
        to: expr_after(sections, "TO")?.expect("an end"),
        step: expr_after(sections, "STEP")?,
    }))
}

/// None for the empty statement.
fn statement(tree: &Tree) -> Lowered<Option<Stmt>> {
    let s = sections(tree);
    let kind = match tree.rule {
        R::EmptyStatement => return Ok(None),
        R::Control | R::AdtStatement => return statement(as_tree(&tree.children[0])),
        R::Assignment => StmtKind::Assign {
            target: variable(the(tree, R::Variable))?,
            value: expr(&tree.children[2])?,
        },
        R::Loop => StmtKind::Loop {
            range: range(&s)?,
            condition: expr_after(&s, "WHILE")?,
            body: statements_after(&s, "DO")?,
            until: expr_after(&s, "UNTIL")?,
        },
        R::EachLoop => StmtKind::Each {
            name: name(the(tree, R::Name)),
            of: expr_after(&s, "OF")?,
            filter: expr_after(&s, "/")?,
            body: statements_after(&s, "DO")?,
        },
        R::VectorLoop => StmtKind::Vect {
            range: range(&s)?.expect("a range"),
            body: statements_after(&s, "DO")?,
        },
        R::Parallel => {
            let branches = trees(tree).map(statements);
            StmtKind::Parallel(branches.collect::<Lowered<_>>()?)
        }
        R::IfStatement => StmtKind::If {
            condition: expr_after(&s, "IF")?.expect("a condition"),
            then: statements_after(&s, "THEN")?,
            otherwise: match after(&s, "ELSE") {
                Some(other) => Some(statements(as_tree(other))?),
                None => None,
            },
        },
        R::CaseStatement => case(&s)?,
        R::LoopExit => StmtKind::Exit {
            kind: match keyword(tree).as_str() {
                "EXIT" => ExitKind::Exit,
                "NEXT" => ExitKind::Next,
                _ => ExitKind::Reloop,
            },
            count: tree.children.get(1).map(expr).transpose()?,
        },
        R::ModuleExit | R::Halt => match keyword(tree).as_str() {
            "RETURN" => StmtKind::Return,
            "REPROC" => StmtKind::Reproc,
            "PAUSE" => StmtKind::Pause,
            "STOP" => StmtKind::Stop,
            _ => StmtKind::Restart,
        },
        R::Reset => StmtKind::Reset(variables(tree)?),
        R::Result => StmtKind::Result(expr(&tree.children[1])?),
        R::ListStatement => list_statement(tree)?,
        R::Create => StmtKind::Create(operands(as_tree(&tree.children[1]))?),
        R::StateChange => StmtKind::StateChange(
            match keyword(tree).as_str() {
                "HIBERN" => StateOp::Hibern,
                "AWAKE" => StateOp::Awake,
                "DESTROY" => StateOp::Destroy,
                "RESUR" => StateOp::Resur,
                _ => StateOp::Purge,
            },
            operands(as_tree(&tree.children[1]))?,
        ),
        R::Design => StmtKind::Design(operands(as_tree(&tree.children[1]))?),
        R::AdtAssignment => {
            let target = as_tree(&tree.children[1]);
            let target = match target.rule {
                R::Property => LetTarget::Property {
                    adt: adt(the(target, R::Adt))?,
                    name: name(the(target, R::Name)),
                },
                _ if target.children.len() == 1 => {
                    LetTarget::Variable(name(as_tree(&target.children[0])))
                }
                _ => LetTarget::Attribute {
                    object: designation(the(target, R::Object))?,
                    attribute: adt_name(the(target, R::AdtName))?,
                },
            };
            StmtKind::Let {
                target,
                value: expr(tree.children.last().unwrap())?,
            }
        }
        R::Assertion => assertion(tree)?,
        R::Modality => modality(tree)?,
        R::Specify => specify(tree)?,
        R::Include | R::Exclude => {
            let mut lists = trees(tree).map(classes);
            let classes = lists.next().unwrap()?;
            let other = lists.next().unwrap()?;
            match tree.rule {
                R::Include => StmtKind::Include {
                    classes,
                    into: other,
                },
                _ => StmtKind::Exclude {
                    classes,
                    from: other,
                },
            }
        }
        R::Associate | R::Dissociate => {
            let mut pairs = Vec::new();
            let adts = trees_of(tree, R::Adt);
            for (a, d) in adts.zip(trees_of(tree, R::Designation)) {
                pairs.push((adt(a)?, designation(d)?));
            }
            match tree.rule {
                R::Associate => StmtKind::Associate(pairs),
                _ => StmtKind::Dissociate(pairs),
            }
        }
        R::Compose => StmtKind::Compose {
            class: class(the(tree, R::Class))?,
            with: classes(the(tree, R::ClassList))?,
        },
        R::Build => StmtKind::Build {
            classes: classes(the(tree, R::ClassList))?,
            model: class(the(tree, R::Class))?,
        },
        R::ProcessStatement => process(tree)?,
        R::Call => match keyword(tree).as_str() {
            "CHAIN" => StmtKind::Chain(module_name(the(tree, R::ModuleName))),
            "CALL" => StmtKind::Call(invocation(the(tree, R::Invocation))?),
            _ => StmtKind::Resume(invocation(the(tree, R::Invocation))?),
        },
        R::Io => match keyword(tree).as_str() {
            "READ" => StmtKind::Read(variables(tree)?),
            _ => StmtKind::Print(expression_list(the(tree, R::ExpressionList))?),
        },
        rule => panic!("{} is not a statement", rule.name()),
    };
    Ok(Some(Stmt {
        kind,
        span: tree_span(tree),
    }))
}

fn case(sections: &Sections) -> Lowered<StmtKind> {
    let mut arms = Vec::new();
    let mut values = Vec::new();
    for (key, nodes) in sections {
        match key.as_str() {
            "WHEN" => values = expression_list(as_tree(nodes[0]))?,
            "DO" => arms.push(CaseArm {
                values: std::mem::take(&mut values),
                body: statements(as_tree(nodes[0]))?,
            }),
            _ => {}
        }
    }
    Ok(StmtKind::Case {
        subject: expr_after(sections, "CASE")?.expect("a subject"),
        arms,
        others: match after(sections, "OTHERS") {
            Some(others) => Some(statements(as_tree(others))?),
            None => None,
        },
    })
}

fn variables(tree: &Tree) -> Lowered<Vec<Variable>> {
    trees_of(tree, R::Variable).map(variable).collect()
}

fn list_statement(tree: &Tree) -> Lowered<StmtKind> {
    let items = || {
        let lists = tree.children.iter();
        let lists = lists.filter(|c| matches!(c, Node::Tree(t) if t.rule == R::ListExpression));
        lists.map(expr).collect::<Lowered<Vec<_>>>()
    };
    let list = || name(the(tree, R::Name));
    Ok(match keyword(tree).as_str() {
        "INSERT" => StmtKind::Insert {
            items: items()?,
            list: list(),
            position: if has(tree, "AFTER") || has(tree, "BEFORE") {
                let position = match has(tree, "AFTER") {
                    true => Position::After,
                    false => Position::Before,
                };
                Some((position, expr(tree.children.last().unwrap())?))
            } else {
                None
            },
        },
        "EXTRACT" => StmtKind::Extract {
            items: items()?,
            list: list(),
        },
        "EMPTY" => StmtKind::Empty(list()),
        word => StmtKind::Organize {
            kind: match word {
                "LINEAR" => Organization::Linear,
                "CIRCULAR" => Organization::Circular,
                "SINGLE" => Organization::Single,
                _ => Organization::Tuple,
            },
            lists: names(tree),
        },
    })
}

fn operands(list: &Tree) -> Lowered<Operands> {
    Ok(match list.rule {
        R::DesignationList => {
            Operands::Designations(trees(list).map(designation).collect::<Lowered<_>>()?)
        }
        R::ClassList => Operands::Designations(
            trees(list)
                .map(|c| Ok(Designation::Named(class(c)?)))
                .collect::<Lowered<_>>()?,
        ),
        R::ClassAttributeList => {
            Operands::Attributes(trees(list).map(class_attribute).collect::<Lowered<_>>()?)
        }
        R::ClassLinkDeclarations => {
            Operands::Links(trees(list).map(link_declaration).collect::<Lowered<_>>()?)
        }
        R::ClassLinkList => Operands::Links(
            trees(list)
                .map(|l| {
                    Ok(LinkDeclaration {
                        link: class_link(l)?,
                        reverse: Reverse::None,
                    })
                })
                .collect::<Lowered<_>>()?,
        ),
        rule => panic!("{} is not a list of operands", rule.name()),
    })
}

fn modality(tree: &Tree) -> Lowered<StmtKind> {
    let mut objects = trees_of(tree, R::Object).map(designation);
    let relation = tree
        .children
        .iter()
        .find(|c| matches!(c, Node::Token(t) if t.kind == Kind::String))
        .expect("a relation");
    let modal = if has(tree, "MUST") {
        Modal::Must
    } else if has(tree, "CANNOT") {
        Modal::Cannot
    } else {
        Modal::May
    };
    Ok(StmtKind::Modality {
        from: objects.next().unwrap()?,
        modal,
        relation: string(relation),
        to: objects.next().unwrap()?,
    })
}

fn assertion(tree: &Tree) -> Lowered<StmtKind> {
    if child(tree, R::Object).is_none() {
        return Ok(StmtKind::Assert(Assertion::Name(name(the(tree, R::Name)))));
    }
    let mut objects = trees_of(tree, R::Object).map(designation);
    let relation = tree
        .children
        .iter()
        .find(|c| matches!(c, Node::Token(t) if t.kind == Kind::String))
        .expect("a relation");
    Ok(StmtKind::Assert(Assertion::Link {
        from: objects.next().unwrap()?,
        negated: has(tree, "NOT"),
        relation: string(relation),
        probability: child(tree, R::Real).map(real).transpose()?,
        to: objects.next().unwrap()?,
    }))
}

fn specify(tree: &Tree) -> Lowered<StmtKind> {
    let mut items = Vec::new();
    for c in trees(tree) {
        items.push(match c.rule {
            R::Class => Specified::Class(class(c)?),
            R::ClassAttribute => Specified::Attribute(class_attribute(c)?),
            R::ClassLink => Specified::Link(class_link(c)?),
            _ => continue,
        });
    }
    let spec = the(tree, R::Specification);
    let spec = if let Some(list) = child(spec, R::ListType) {
        Specification::List(scalar_in(list))
    } else if child(spec, R::CompoundType).is_some() {
        Specification::Compound
    } else if let Some(rank) = rank_in(spec) {
        Specification::Array {
            rank,
            element: scalar_in(spec),
        }
    } else if let Some(scalar) = scalar_in(spec) {
        Specification::Scalar(scalar)
    } else {
        Specification::Fuzziness(fuzziness(the(spec, R::Fuzziness)))
    };
    Ok(StmtKind::Specify {
        items,
        spec,
        unit: child(tree, R::Name).map(name),
        computed: has(tree, "COMPUTED"),
    })
}

fn process(tree: &Tree) -> Lowered<StmtKind> {
    if let Some(hold) = child(tree, R::Hold) {
        return Ok(StmtKind::Hold {
            object: child(hold, R::Object).map(designation).transpose()?,
            duration: expr(hold.children.last().unwrap())?,
        });
    }
    let mut objects = trees_of(tree, R::Object).map(designation);
    let mut next = || objects.next().expect("an object");
    let last = || expr(tree.children.last().unwrap());
    Ok(match keyword(tree).as_str() {
        "ACTIVATE" => StmtKind::Activate(next()?),
        "REACTIVATE" => StmtKind::Reactivate {
            object: next()?,
            when: if has(tree, "AT") {
                Some(When::At(last()?))
            } else if has(tree, "BEFORE") {
                Some(When::Before(next()?))
            } else if has(tree, "AFTER") {
                Some(When::After(next()?))
            } else {
                None
            },
        },
        "CANCEL" => StmtKind::Cancel(next()?),
        "KILL" => StmtKind::Kill(next()?),
        "DELAY" => StmtKind::Delay {
            object: next()?,
            duration: last()?,
        },
        "PASSIVATE" => StmtKind::Passivate,
        _ => StmtKind::Terminate,
    })
}

fn invocation(tree: &Tree) -> Lowered<Call> {
    Ok(Call {
        name: module_name(the(tree, R::ModuleName)),
        args: match child(tree, R::ExpressionList) {
            Some(list) => expression_list(list)?,
            None => Vec::new(),
        },
    })
}

// A.d.t. designations, pages 23 and 24

fn class(tree: &Tree) -> Lowered<AdtName> {
    adt_name(the(tree, R::AdtName))
}

fn classes(tree: &Tree) -> Lowered<Vec<AdtName>> {
    trees(tree).map(class).collect()
}

fn adt_name(tree: &Tree) -> Lowered<AdtName> {
    Ok(match tree.children.as_slice() {
        [Node::Tree(t)] if t.rule == R::Name => AdtName::Name(name(t)),
        [call @ Node::Tree(_)] => AdtName::Expr(Box::new(expr(call)?)),
        [string @ Node::Token(_)] => AdtName::String(self::string(string)),
        [_, inner, _] => AdtName::Expr(Box::new(expr(inner)?)),
        _ => panic!("an a.d.t. name of {} children", tree.children.len()),
    })
}

/// The grammar's `designation` and `object` alike.
fn designation(tree: &Tree) -> Lowered<Designation> {
    Ok(match tree.children.as_slice() {
        [this @ Node::Token(_)] => Designation::This(span(this)),
        [Node::Tree(t)] if t.rule == R::Name => Designation::Named(AdtName::Name(name(t))),
        [Node::Tree(t)] => Designation::Named(class(t)?),
        [n, _, c, _] => Designation::Classed {
            name: adt_name(as_tree(n))?,
            class: class(as_tree(c))?,
        },
        _ => panic!("a designation of {} children", tree.children.len()),
    })
}

fn adt(tree: &Tree) -> Lowered<Adt> {
    let designation = designation(as_tree(&tree.children[0]))?;
    let part = match tree.children.get(1) {
        None => AdtPart::None,
        Some(Node::Token(t)) if t.kind == Kind::String => AdtPart::Link(
            string(&tree.children[1]),
            self::designation(as_tree(&tree.children[2]))?,
        ),
        Some(_) => AdtPart::Attribute(adt_name(as_tree(&tree.children[2]))?),
    };
    Ok(Adt { designation, part })
}

fn class_attribute(tree: &Tree) -> Lowered<ClassAttribute> {
    Ok(ClassAttribute {
        class: class(as_tree(&tree.children[0]))?,
        attribute: adt_name(as_tree(&tree.children[2]))?,
    })
}

fn class_link(tree: &Tree) -> Lowered<ClassLink> {
    Ok(ClassLink {
        from: class(as_tree(&tree.children[0]))?,
        relation: string(&tree.children[1]),
        to: class(as_tree(&tree.children[2]))?,
    })
}

fn link_declaration(tree: &Tree) -> Lowered<LinkDeclaration> {
    Ok(LinkDeclaration {
        link: class_link(the(tree, R::ClassLink))?,
        reverse: if has(tree, "REFLEXIVE") {
            Reverse::Reflexive
        } else if has(tree, "REVERSE") {
            Reverse::Named(adt_name(the(tree, R::AdtName))?)
        } else {
            Reverse::None
        },
    })
}

// Constants, pages 25 and 26

fn constant(tree: &Tree) -> Lowered<Constant> {
    let value = &tree.children[0];
    let Node::Tree(value) = value else {
        return Ok(Constant::String(string(value).text));
    };
    if value.rule == R::BooleanConstant {
        return Ok(Constant::Boolean(keyword(value) == "TRUE"));
    }
    let c = as_tree(&value.children[0]);
    Ok(match c.rule {
        R::IntegerConstant => Constant::Integer(signed_integers(c)?[0]),
        R::FloatConstant => Constant::Float(signed_floats(c)[0]),
        R::ComplexConstant => {
            let f = signed_floats(c);
            Constant::Complex { re: f[0], im: f[1] }
        }
        R::QuaternionConstant => {
            let f = signed_floats(c);
            Constant::Quaternion([f[0], f[1], f[2], f[3]])
        }
        R::GaussConstant => {
            let i = signed_integers(c)?;
            Constant::Gauss { re: i[0], im: i[1] }
        }
        _ => {
            let numerator = signed_integers(c)?[0];
            let denominator = c.children.last().unwrap();
            Constant::Rational(numerator, unsigned(denominator)?)
        }
    })
}

/// The numbers among the children, each with whether a minus sign comes
/// just before it.
fn signed<'t, 'a>(tree: &'t Tree<'a>) -> Vec<(bool, &'t Node<'a>)> {
    let mut out = Vec::new();
    let mut negative = false;
    for c in &tree.children {
        match c {
            Node::Tree(sign) if sign.rule == R::Sign => negative = keyword(sign) == "-",
            Node::Token(t) if t.kind == Kind::Symbol => {}
            number => {
                out.push((negative, number));
                negative = false;
            }
        }
    }
    out
}

/// The digits of a number token, its imaginary unit dropped.
fn digits<'a>(node: &Node<'a>) -> &'a str {
    let token = as_token(node);
    match token.kind {
        Kind::Imaginary(_) => &token.text[..token.text.len() - 1],
        _ => token.text,
    }
}

fn unsigned(node: &Node) -> Lowered<u64> {
    digits(node)
        .parse()
        .map_err(|_| error(node, "integer too large"))
}

fn signed_integers(tree: &Tree) -> Lowered<Vec<i64>> {
    signed(tree)
        .into_iter()
        .map(|(negative, node)| {
            let value = i128::from(unsigned(node)?);
            let value = if negative { -value } else { value };
            i64::try_from(value).map_err(|_| error(node, "integer too large"))
        })
        .collect()
}

fn float(node: &Node) -> f64 {
    digits(node).parse().expect("the lexer reads floats")
}

fn signed_floats(tree: &Tree) -> Vec<f64> {
    signed(tree)
        .into_iter()
        .map(|(negative, node)| if negative { -float(node) } else { float(node) })
        .collect()
}

fn real(tree: &Tree) -> Lowered<Real> {
    let token = &tree.children[0];
    Ok(match as_token(token).kind {
        Kind::Integer => Real::Integer(unsigned(token)?),
        _ => Real::Float(float(token)),
    })
}

// Expressions, pages 26 to 28

fn expression_list(tree: &Tree) -> Lowered<Vec<Expr>> {
    tree.children
        .iter()
        .filter(|c| !is_punctuation(c))
        .map(expr)
        .collect()
}

fn binary_op(word: &str) -> BinaryOp {
    match word.to_ascii_uppercase().as_str() {
        "OR" => BinaryOp::Or,
        "NOR" => BinaryOp::Nor,
        "XOR" => BinaryOp::Xor,
        "AND" => BinaryOp::And,
        "NAND" => BinaryOp::Nand,
        "=" => BinaryOp::Eq,
        "/=" => BinaryOp::Ne,
        "<" => BinaryOp::Lt,
        "<=" => BinaryOp::Le,
        ">" => BinaryOp::Gt,
        ">=" => BinaryOp::Ge,
        "IN" => BinaryOp::In,
        "NOTIN" => BinaryOp::NotIn,
        "UNION" => BinaryOp::Union,
        "INTER" => BinaryOp::Inter,
        "COMPL" => BinaryOp::Compl,
        "&" => BinaryOp::Concat,
        "+" => BinaryOp::Add,
        "-" => BinaryOp::Sub,
        "[+]" => BinaryOp::ArrayAdd,
        "*" => BinaryOp::Mul,
        "/" => BinaryOp::Div,
        "MOD" => BinaryOp::Mod,
        "//" => BinaryOp::Ratio,
        "[*]" => BinaryOp::ArrayMul,
        "[.]" => BinaryOp::ArrayDot,
        "[^]" => BinaryOp::ArrayPow,
        "^" => BinaryOp::Pow,
        op => panic!("{op} is not an operator"),
    }
}

/// `first (op operand)*`, folded to the left.
fn chain(first: Expr, rest: &[Node]) -> Lowered<Expr> {
    let mut left = first;
    for pair in rest.chunks(2) {
        let op = binary_op(as_token(&pair[0]).text);
        let right = expr(&pair[1])?;
        let span = left.span;
        left = Expr {
            kind: ExprKind::Binary(op, Box::new(left), Box::new(right)),
            span,
        };
    }
    Ok(left)
}

fn expr(node: &Node) -> Lowered<Expr> {
    let span = span(node);
    let tree = match node {
        // A bare token is a string operand: every other one is wrapped.
        Node::Token(_) => {
            return Ok(Expr {
                kind: ExprKind::String(string(node).text),
                span,
            });
        }
        Node::Tree(tree) => tree,
    };
    let c = &tree.children;
    let kind = match tree.rule {
        R::Disjunction
        | R::Conjunction
        | R::Union
        | R::Intersection
        | R::Complement
        | R::Concatenation
        | R::Product
        | R::Power
        | R::ListUnion
        | R::ListIntersection
        | R::ListComplement => return chain(expr(&c[0])?, &c[1..]),
        R::Sum => {
            let Node::Tree(sign) = &c[0] else {
                return chain(expr(&c[0])?, &c[1..]);
            };
            if sign.rule != R::Sign {
                return chain(expr(&c[0])?, &c[1..]);
            }
            let op = match keyword(sign).as_str() {
                "-" => UnaryOp::Neg,
                _ => UnaryOp::Plus,
            };
            let first = Expr {
                kind: ExprKind::Unary(op, Box::new(expr(&c[1])?)),
                span,
            };
            return chain(first, &c[2..]);
        }
        R::Negation => ExprKind::Unary(UnaryOp::Not, Box::new(expr(&c[1])?)),
        R::Relation => match &c[1] {
            Node::Token(t) if t.kind == Kind::String => ExprKind::Link(
                Box::new(designation(as_tree(&c[0]))?),
                string(&c[1]),
                Box::new(designation(as_tree(&c[2]))?),
            ),
            Node::Tree(comparator) => ExprKind::Binary(
                binary_op(as_token(&comparator.children[0]).text),
                Box::new(expr(&c[0])?),
                Box::new(expr(&c[2])?),
            ),
            Node::Token(t) => ExprKind::Binary(
                binary_op(t.text),
                Box::new(expr(&c[0])?),
                Box::new(expr(&c[2])?),
            ),
        },
        R::Operand => return expr(&c[1]),
        R::ListExpression => return expr(&c[0]),
        R::ListTerm => return expr(&c[c.len() / 2]),
        R::FuzzyNumber => ExprKind::Number(fuzzy_number(tree)?),
        R::Multiple => {
            let mut values = Vec::new();
            for (negative, number) in signed(tree) {
                values.push((negative, fuzzy_number(as_tree(number))?));
            }
            ExprKind::Multiple(values)
        }
        R::BooleanConstant => ExprKind::Boolean(keyword(tree) == "TRUE"),
        R::This => ExprKind::This,
        R::Variable => ExprKind::Variable(variable(tree)?),
        R::FunctionCall => ExprKind::Call(Call {
            name: module_name(the(tree, R::ModuleName)),
            args: expression_list(the(tree, R::ExpressionList))?,
        }),
        R::FunctorExpression => ExprKind::Functor(
            Box::new(adt(the(tree, R::Adt))?),
            trees_of(tree, R::Invocation)
                .map(invocation)
                .collect::<Lowered<_>>()?,
        ),
        R::ObjectAttributeValue => ExprKind::Attribute(
            Box::new(designation(the(tree, R::Object))?),
            Box::new(adt_name(the(tree, R::AdtName))?),
        ),
        rule => panic!("{} is not an expression", rule.name()),
    };
    Ok(Expr { kind, span })
}

fn fuzzy_number(tree: &Tree) -> Lowered<FuzzyNumber> {
    let s = sections(tree);
    let number = as_tree(&tree.children[0]);
    let value = match &number.children[0] {
        Node::Tree(r) => Number::Real(real(r)?),
        imaginary => {
            let Kind::Imaginary(unit) = as_token(imaginary).kind else {
                panic!("a number that is not one");
            };
            let part = if digits(imaginary).contains('.') {
                Real::Float(float(imaginary))
            } else {
                Real::Integer(unsigned(imaginary)?)
            };
            Number::Imaginary(part, unit)
        }
    };
    Ok(FuzzyNumber {
        value,
        approximation: after(&s, "+-").map(|r| real(as_tree(r))).transpose()?,
        probability: after(&s, "%").map(|r| real(as_tree(r))).transpose()?,
        opinion: after(&s, "@").map(|o| string(o).text),
    })
}

fn variable(tree: &Tree) -> Lowered<Variable> {
    Ok(Variable {
        name: name(the(tree, R::Name)),
        indices: match child(tree, R::ExpressionList) {
            Some(list) => expression_list(list)?,
            None => Vec::new(),
        },
    })
}
