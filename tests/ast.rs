//! The typed tree: lowering, printing, and reading the print back.

use std::fs;
use std::path::Path;

use embryo_systems::adt81::ast::*;
use embryo_systems::adt81::{lower, print, sample};

fn fragment(source: &str) -> Fragment {
    lower::parse_fragment(source).unwrap_or_else(|e| panic!("{source}: {e}"))
}

/// The single statement of `source`.
fn stmt(source: &str) -> StmtKind {
    match fragment(source).items.as_slice() {
        [Item::Stmt(s)] => s.kind.clone(),
        items => panic!("{source}: {} items", items.len()),
    }
}

fn expr(source: &str) -> Expr {
    match stmt(&format!("x := {source}")) {
        StmtKind::Assign { value, .. } => value,
        kind => panic!("{kind:?}"),
    }
}

/// How an expression was read, every operation in parentheses.
fn read(source: &str) -> String {
    print::parenthesised(&expr(source))
}

fn constant(source: &str) -> Constant {
    match fragment(&format!("c : FLOAT INIT {source}"))
        .items
        .as_slice()
    {
        [
            Item::Local(Declaration {
                init: Some(Init::Initial(c)),
                ..
            }),
        ] => c.clone(),
        items => panic!("{source}: {items:?}"),
    }
}

/// Prints `program`, reads it back, and prints again: the same tree, the
/// same text.
fn round_trip_program(program: &Program, what: &str) {
    let text = program.to_string();
    let back = lower::parse_program(&text).unwrap_or_else(|e| panic!("{what}: {e}\n{text}"));
    assert!(
        &back == program,
        "{what} reads back otherwise, printed as:\n{text}"
    );
    assert_eq!(back.to_string(), text, "{what}: printing is not stable");
}

fn round_trip_fragment(fragment: &Fragment, what: &str) {
    let text = fragment.to_string();
    let back = lower::parse_fragment(&text).unwrap_or_else(|e| panic!("{what}: {e}\n{text}"));
    assert!(
        &back == fragment,
        "{what} reads back otherwise, printed as:\n{text}"
    );
    assert_eq!(back.to_string(), text, "{what}: printing is not stable");
}

// The corpus and random programs

#[test]
fn corpus_round_trips() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/adt81");
    for dir in ["programs", "checked", "fragments"] {
        for entry in fs::read_dir(root.join(dir)).unwrap() {
            let path = entry.unwrap().path();
            let source = fs::read_to_string(&path).unwrap();
            let what = path.display().to_string();
            if dir != "fragments" {
                let program = lower::parse_program(&source).unwrap();
                round_trip_program(&program, &what);
            } else {
                round_trip_fragment(&fragment(&source), &what);
            }
        }
    }
}

#[test]
fn random_programs_round_trip() {
    for seed in 0..1000 {
        round_trip_program(&sample::program(seed), &format!("program {seed}"));
    }
}

#[test]
fn random_fragments_round_trip() {
    for seed in 0..1000 {
        round_trip_fragment(&sample::fragment(seed), &format!("fragment {seed}"));
    }
}

// Precedence and associativity, loosest first

#[test]
fn precedence() {
    let cases = [
        ("a OR b AND c", "(a OR (b AND c))"),
        ("a AND b OR c", "((a AND b) OR c)"),
        ("a OR b XOR c NOR d", "(((a OR b) XOR c) NOR d)"),
        ("a AND b NAND c", "((a AND b) NAND c)"),
        ("NOT a AND b", "((NOT a) AND b)"),
        ("NOT a = b", "(NOT (a = b))"),
        ("NOT NOT a", "(NOT (NOT a))"),
        ("a = b UNION c", "(a = (b UNION c))"),
        ("a /= b", "(a /= b)"),
        ("a <= b & c", "(a <= (b & c))"),
        ("a IN L UNION M", "(a IN (L UNION M))"),
        ("a NOTIN L", "(a NOTIN L)"),
        ("a UNION b INTER c", "(a UNION (b INTER c))"),
        ("a UNION b UNION c", "((a UNION b) UNION c)"),
        ("a INTER b COMPL c", "(a INTER (b COMPL c))"),
        ("a COMPL b & c", "(a COMPL (b & c))"),
        ("a & b + c", "(a & (b + c))"),
        ("a & b & c", "((a & b) & c)"),
        ("a + b * c", "(a + (b * c))"),
        ("a - b - c", "((a - b) - c)"),
        ("a - b + c", "((a - b) + c)"),
        ("a / b MOD c // d", "(((a / b) MOD c) // d)"),
        ("a * b ^ c", "(a * (b ^ c))"),
        ("a [+] b [*] c", "(a [+] (b [*] c))"),
        ("a [.] b [^] c", "((a [.] b) [^] c)"),
        ("-a + b", "((-a) + b)"),
        ("-a * b", "(-(a * b))"),
        ("+a", "(+a)"),
        ("a & -b", "(a & (-b))"),
        ("a = -b", "(a = (-b))"),
        ("(a + b) * c", "((a + b) * c)"),
        ("a - (b - c)", "(a - (b - c))"),
        ("2 ^ (3 ^ 4)", "(2 ^ (3 ^ 4))"),
        ("(-a) ^ 2", "((-a) ^ 2)"),
        ("THIS \"r\" a AND b", "((THIS \"r\" a) AND b)"),
        ("NOT a \"r\" b", "(NOT (a \"r\" b))"),
        ("((a))", "a"),
    ];
    for (source, expected) in cases {
        assert_eq!(read(source), expected, "{source}");
    }
}

#[test]
fn minimal_parentheses() {
    let cases = [
        ("(a + b) * c", "(a + b) * c"),
        ("a + (b * c)", "a + b * c"),
        ("(a - b) - c", "a - b - c"),
        ("a - (b - c)", "a - (b - c)"),
        ("(-a) ^ 2", "(-a) ^ 2"),
        ("-(a ^ 2)", "-a ^ 2"),
        ("a + (-b)", "a + (-b)"),
        ("NOT (a AND b)", "NOT (a AND b)"),
        ("(a = b) = c", "(a = b) = c"),
        ("((a))", "a"),
    ];
    for (source, expected) in cases {
        assert_eq!(expr(source).to_string(), expected, "{source}");
    }
}

/// The decision on designations: ' and . bind tighter than &, and a
/// bracket after a name is the class.
#[test]
fn designations() {
    let ExprKind::Binary(BinaryOp::Concat, left, right) = expr("\"A\" & \"B\" ' F").kind else {
        panic!("not a concatenation");
    };
    assert!(matches!(left.kind, ExprKind::String(_)));
    assert!(matches!(right.kind, ExprKind::Functor(..)));

    let ExprKind::Functor(adt, calls) = expr("(\"A\" & \"B\") ' F ' G(x)").kind else {
        panic!("not a functor");
    };
    assert!(matches!(
        &adt.designation,
        Designation::Named(AdtName::Expr(e)) if matches!(e.kind, ExprKind::Binary(BinaryOp::Concat, ..))
    ));
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].args.len(), 1);

    let ExprKind::Binary(BinaryOp::Concat, left, _) = expr("a . \"x\" & \"y\"").kind else {
        panic!("not a concatenation");
    };
    assert!(matches!(left.kind, ExprKind::Attribute(..)));

    assert!(matches!(
        expr("v[i] ' F").kind,
        ExprKind::Functor(adt, _) if matches!(adt.designation, Designation::Classed { .. })
    ));
    assert!(matches!(
        stmt("CREATE p [ \"C\" ]"),
        StmtKind::Create(Operands::Designations(d)) if matches!(d[0], Designation::Classed { .. })
    ));
    assert!(matches!(expr("v[i]").kind, ExprKind::Variable(v) if v.indices.len() == 1));
    assert!(matches!(expr("THIS \"r\" a").kind, ExprKind::Link(..)));
    assert!(matches!(
        expr("THIS \"r\" a ' F").kind,
        ExprKind::Functor(adt, _) if matches!(adt.part, AdtPart::Link(..))
    ));
    assert!(matches!(
        expr("p [ \"C\" ] . \"x\"").kind,
        ExprKind::Attribute(o, _) if matches!(*o, Designation::Classed { .. })
    ));
    // A call names an a.d.t. as the value it returns, parenthesised or not.
    assert_eq!(expr("f(x) ' F"), expr("(f(x)) ' F"));
    assert_ne!(expr("(x) ' F"), expr("x ' F"));
}

// Constants and numbers

#[test]
fn constants() {
    assert_eq!(constant("-3"), Constant::Integer(-3));
    assert_eq!(constant("+3"), Constant::Integer(3));
    assert_eq!(constant("2.5"), Constant::Float(2.5));
    assert_eq!(constant("-0."), Constant::Float(-0.0));
    assert_eq!(
        constant("1.0 - 2.0i"),
        Constant::Complex { re: 1.0, im: -2.0 }
    );
    assert_eq!(constant("-1 + 2i"), Constant::Gauss { re: -1, im: 2 });
    assert_eq!(
        constant("1.0 + 2.0i - 3.0j + 4.0k"),
        Constant::Quaternion([1.0, 2.0, -3.0, 4.0])
    );
    assert_eq!(constant("-3 // 4"), Constant::Rational(-3, 4));
    assert_eq!(constant("TRUE"), Constant::Boolean(true));
    assert_eq!(constant("\"s t\""), Constant::String("s t".into()));
    assert_eq!(
        constant("-9223372036854775808"),
        Constant::Integer(i64::MIN)
    );
}

#[test]
fn integers_too_large() {
    let error = |source| lower::parse_fragment(source).unwrap_err().to_string();
    assert_eq!(
        error("c : INTEGER INIT 9223372036854775808"),
        "1:18: integer too large"
    );
    assert_eq!(error("x := 18446744073709551616"), "1:6: integer too large");
    assert_eq!(
        error("x := 1 // 99999999999999999999 "),
        "1:11: integer too large"
    );
    assert!(lower::parse_fragment("x := 18446744073709551615").is_ok());
}

#[test]
fn fuzzy_numbers() {
    let ExprKind::Number(n) = expr("291.4 +- 0.5 % 95 @\"by eye\"").kind else {
        panic!("not a number");
    };
    assert_eq!(n.value, Number::Real(Real::Float(291.4)));
    assert_eq!(n.approximation, Some(Real::Float(0.5)));
    assert_eq!(n.probability, Some(Real::Integer(95)));
    assert_eq!(n.opinion.as_deref(), Some("by eye"));

    let number = |source| match expr(source).kind {
        ExprKind::Number(n) => n.value,
        kind => panic!("{kind:?}"),
    };
    assert_eq!(number("2i"), Number::Imaginary(Real::Integer(2), Unit::I));
    assert_eq!(number("3.5j"), Number::Imaginary(Real::Float(3.5), Unit::J));
    assert_eq!(number("1.k"), Number::Imaginary(Real::Float(1.0), Unit::K));

    let ExprKind::Multiple(values) = expr("{ 25.3 +- 0.37, -25.7, +1 }").kind else {
        panic!("not a multiple value");
    };
    let signs: Vec<bool> = values.iter().map(|v| v.0).collect();
    assert_eq!(signs, [false, true, false]);
}

// Names, positions, empty statements

#[test]
fn names_fold() {
    assert_eq!(fragment("x := total"), fragment("X := TOTAL"));
    assert_ne!(fragment("x := \"total\""), fragment("x := \"TOTAL\""));
    let ExprKind::Variable(v) = expr("Total").kind else {
        panic!("not a variable");
    };
    assert_eq!(
        (v.name.spelling.as_str(), v.name.key.as_str()),
        ("Total", "TOTAL")
    );
}

#[test]
fn positions() {
    let f = fragment("\n  x := a +\n    b");
    let Item::Stmt(s) = &f.items[0] else {
        panic!("not a statement");
    };
    assert_eq!((s.span.line, s.span.column), (2, 3));
    let StmtKind::Assign { value, .. } = &s.kind else {
        panic!("not an assignment");
    };
    let ExprKind::Binary(_, _, right) = &value.kind else {
        panic!("not a sum");
    };
    assert_eq!((right.span.line, right.span.column), (3, 5));
}

#[test]
fn empty_statements_vanish() {
    assert_eq!(fragment("; x := 1 ; ; ;").items.len(), 1);
    assert!(matches!(
        stmt("IF a THEN ; ; ELSE ENDIF"),
        StmtKind::If { then, otherwise: Some(o), .. } if then.is_empty() && o.is_empty()
    ));
    assert!(matches!(
        stmt("IF a THEN ENDIF"),
        StmtKind::If {
            otherwise: None,
            ..
        }
    ));
}

// One shape per statement

#[test]
fn control_statements() {
    assert!(matches!(
        stmt("LOOP FOR i : 1 TO n STEP 2 WHILE c DO x := 1 UNTIL d ENDLOOP"),
        StmtKind::Loop { range: Some(Range { step: Some(_), .. }), condition: Some(_), until: Some(_), body }
            if body.len() == 1
    ));
    assert!(matches!(
        stmt("LOOP DO ENDLOOP"),
        StmtKind::Loop {
            range: None,
            condition: None,
            until: None,
            ..
        }
    ));
    assert!(matches!(
        stmt("EACH o OF L UNION M / o . \"X\" > 0 DO ENDEACH"),
        StmtKind::Each {
            of: Some(Expr {
                kind: ExprKind::Binary(BinaryOp::Union, ..),
                ..
            }),
            filter: Some(_),
            ..
        }
    ));
    assert!(matches!(
        stmt("EACH o DO ENDEACH"),
        StmtKind::Each {
            of: None,
            filter: None,
            ..
        }
    ));
    assert!(matches!(
        stmt("VECT FOR j : 1 TO 10 DO v[j] := 0 ENDVECT"),
        StmtKind::Vect {
            range: Range { step: None, .. },
            ..
        }
    ));
    assert!(matches!(
        stmt("PARBEGIN a := 1 PAR PAR b := 2 ENDPAR"),
        StmtKind::Parallel(branches) if branches.len() == 3 && branches[1].is_empty()
    ));
    let StmtKind::Case { arms, others, .. } =
        stmt("CASE x WHEN 1, 2 DO y := 1 WHEN 3 DO OTHERS y := 0 ENDCASE")
    else {
        panic!("not a case");
    };
    assert_eq!(arms.len(), 2);
    assert_eq!((arms[0].values.len(), arms[0].body.len()), (2, 1));
    assert!(arms[1].body.is_empty());
    assert_eq!(others.map(|o| o.len()), Some(1));
    assert!(matches!(
        stmt("EXIT"),
        StmtKind::Exit {
            kind: ExitKind::Exit,
            count: None
        }
    ));
    assert!(matches!(
        stmt("NEXT 2"),
        StmtKind::Exit {
            kind: ExitKind::Next,
            count: Some(_)
        }
    ));
    assert!(matches!(
        stmt("reloop"),
        StmtKind::Exit {
            kind: ExitKind::Reloop,
            ..
        }
    ));
    for (source, kind) in [
        ("RETURN", StmtKind::Return),
        ("REPROC", StmtKind::Reproc),
        ("PAUSE", StmtKind::Pause),
        ("STOP", StmtKind::Stop),
        ("RESTART", StmtKind::Restart),
        ("PASSIVATE", StmtKind::Passivate),
        ("TERMINATE", StmtKind::Terminate),
    ] {
        assert_eq!(stmt(source), kind);
    }
}

#[test]
fn simple_statements() {
    assert!(matches!(
        stmt("v[i, j] := 1"),
        StmtKind::Assign { target: Variable { indices, .. }, .. } if indices.len() == 2
    ));
    assert!(matches!(stmt("RESET a, v[1]"), StmtKind::Reset(vs) if vs.len() == 2));
    assert!(matches!(stmt("RESULT a"), StmtKind::Result(_)));
    assert!(matches!(
        stmt("INSERT A, B INTO L AFTER e"),
        StmtKind::Insert { items, position: Some((Position::After, _)), .. } if items.len() == 2
    ));
    assert!(matches!(
        stmt("INSERT (a + 1) INTO L BEFORE 2"),
        StmtKind::Insert {
            position: Some((Position::Before, _)),
            ..
        }
    ));
    assert!(matches!(
        stmt("INSERT C INTO L"),
        StmtKind::Insert { position: None, .. }
    ));
    assert!(matches!(
        stmt("EXTRACT A ' F OUTOF L"),
        StmtKind::Extract { .. }
    ));
    assert!(matches!(stmt("EMPTY L"), StmtKind::Empty(_)));
    assert!(matches!(
        stmt("CIRCULAR L, M"),
        StmtKind::Organize { kind: Organization::Circular, lists } if lists.len() == 2
    ));
    assert!(matches!(stmt("CHAIN Q"), StmtKind::Chain(_)));
    assert!(matches!(stmt("CALL S"), StmtKind::Call(c) if c.args.is_empty()));
    assert!(matches!(stmt("CALL S (a, 1)"), StmtKind::Call(c) if c.args.len() == 2));
    assert!(matches!(stmt("RESUME K"), StmtKind::Resume(_)));
    assert!(matches!(stmt("READ a, v[2]"), StmtKind::Read(vs) if vs.len() == 2));
    assert!(matches!(stmt("PRINT a, \"x\""), StmtKind::Print(es) if es.len() == 2));
}

#[test]
fn adt_statements() {
    let operands = |source| match stmt(source) {
        StmtKind::Create(o) | StmtKind::Design(o) | StmtKind::StateChange(_, o) => o,
        kind => panic!("{kind:?}"),
    };
    assert!(matches!(operands("CREATE \"A\", x, THIS"), Operands::Designations(d) if d.len() == 3));
    assert!(matches!(
        operands("CREATE \"A\" . \"x\""),
        Operands::Attributes(_)
    ));
    assert!(matches!(
        operands("CREATE \"A\" \"r\" \"B\" REVERSE \"s\", \"A\" \"n\" \"A\" REFLEXIVE"),
        Operands::Links(l) if matches!(l[0].reverse, Reverse::Named(_)) && l[1].reverse == Reverse::Reflexive
    ));
    assert!(matches!(
        operands("PURGE \"A\" \"r\" \"B\""),
        Operands::Links(l) if l[0].reverse == Reverse::None
    ));
    assert!(matches!(
        stmt("HIBERN \"A\""),
        StmtKind::StateChange(StateOp::Hibern, _)
    ));
    assert!(matches!(
        operands("DESIGN \"E\", \"F\""),
        Operands::Designations(d) if d.iter().all(|d| matches!(d, Designation::Named(_)))
    ));

    assert!(matches!(
        stmt("LET x := 1"),
        StmtKind::Let {
            target: LetTarget::Variable(_),
            ..
        }
    ));
    assert!(matches!(
        stmt("LET o . \"X\" := 1"),
        StmtKind::Let {
            target: LetTarget::Attribute { .. },
            ..
        }
    ));
    assert!(matches!(
        stmt("LET \"A\" . \"x\" ' COND := \"x > 0\""),
        StmtKind::Let {
            target: LetTarget::Property {
                adt: Adt {
                    part: AdtPart::Attribute(_),
                    ..
                },
                ..
            },
            ..
        }
    ));

    assert!(matches!(
        stmt("ASSERT \"o\" [ \"C\" ] NOT \"r\" % 0.5 p"),
        StmtKind::Assert(Assertion::Link {
            negated: true,
            probability: Some(_),
            ..
        })
    ));
    assert!(matches!(
        stmt("ASSERT a \"r\" b"),
        StmtKind::Assert(Assertion::Link {
            negated: false,
            probability: None,
            ..
        })
    ));
    assert!(matches!(
        stmt("ASSERT r"),
        StmtKind::Assert(Assertion::Name(_))
    ));

    let spec = |source| match stmt(source) {
        StmtKind::Specify { spec, .. } => spec,
        kind => panic!("{kind:?}"),
    };
    assert!(matches!(
        spec("SPECIFY \"A\" FLOAT APPR"),
        Specification::Scalar(_)
    ));
    assert!(matches!(
        spec("SPECIFY \"A\" VECTOR"),
        Specification::Array {
            rank: Rank::Vector,
            element: None
        }
    ));
    assert!(matches!(
        spec("SPECIFY \"A\" LIST OF OBJECT"),
        Specification::List(Some(_))
    ));
    assert!(matches!(
        spec("SPECIFY \"A\" COMPOUND"),
        Specification::Compound
    ));
    assert!(matches!(
        spec("SPECIFY \"A\" PROB"),
        Specification::Fuzziness(_)
    ));
    assert!(matches!(
        spec("SPECIFY \"A\""),
        Specification::Fuzziness(f) if f == Fuzziness::NONE
    ));
    assert!(matches!(
        stmt("SPECIFY \"A\" . \"t\", \"A\" \"r\" \"B\", \"C\" : FLOAT UNIT (celsius) COMPUTED"),
        StmtKind::Specify { items, unit: Some(_), computed: true, .. }
            if matches!(items[..], [Specified::Attribute(_), Specified::Link(_), Specified::Class(_)])
    ));

    assert!(matches!(
        stmt("INCLUDE \"A\", \"B\" INTO \"H\""),
        StmtKind::Include { classes, into } if classes.len() == 2 && into.len() == 1
    ));
    assert!(matches!(
        stmt("EXCLUDE \"A\" OUTOF \"H\""),
        StmtKind::Exclude { .. }
    ));
    assert!(matches!(
        stmt("ASSOC \"A\" . \"x\" WITH \"B\", x WITH y"),
        StmtKind::Associate(pairs) if pairs.len() == 2
    ));
    assert!(matches!(
        stmt("DISSOC \"A\" \"r\" \"B\" FROM \"C\""),
        StmtKind::Dissociate(_)
    ));
    assert!(
        matches!(stmt("COMPOSE \"G\" WITH \"E\", \"F\""), StmtKind::Compose { with, .. } if with.len() == 2)
    );
    assert!(
        matches!(stmt("BUILD \"R1\", \"R2\" AS \"G\""), StmtKind::Build { classes, .. } if classes.len() == 2)
    );
}

#[test]
fn process_statements() {
    assert!(matches!(stmt("ACTIVATE p"), StmtKind::Activate(_)));
    assert!(matches!(
        stmt("REACTIVATE p"),
        StmtKind::Reactivate { when: None, .. }
    ));
    assert!(matches!(
        stmt("REACTIVATE p AT t + 1"),
        StmtKind::Reactivate {
            when: Some(When::At(_)),
            ..
        }
    ));
    assert!(matches!(
        stmt("REACTIVATE p BEFORE q"),
        StmtKind::Reactivate {
            when: Some(When::Before(_)),
            ..
        }
    ));
    assert!(matches!(
        stmt("REACTIVATE p AFTER THIS"),
        StmtKind::Reactivate {
            when: Some(When::After(Designation::This(_))),
            ..
        }
    ));
    assert!(matches!(stmt("CANCEL p"), StmtKind::Cancel(_)));
    assert!(matches!(
        stmt("KILL \"P\" [ \"C\" ]"),
        StmtKind::Kill(Designation::Classed { .. })
    ));
    assert!(matches!(stmt("DELAY p FOR 5"), StmtKind::Delay { .. }));
    assert!(matches!(stmt("HOLD p FOR 2"), StmtKind::Hold { .. }));
}

#[test]
fn declarations_and_modules() {
    let f = fragment(
        "v : SPARSE MATRIX [1:n, -1:9] OF LONG INTEGER APPR INIT 0 ;
         y : FLOAT CONSTANTE = 2.5 ;
         L : LIST ;
         u : VECTOR OF FLOAT FUZZY INOUT",
    );
    let [
        Item::Local(v),
        Item::Local(y),
        Item::Local(l),
        Item::Parameter(u),
    ] = f.items.as_slice()
    else {
        panic!("{:?}", f.items);
    };
    let DataType::Array {
        sparse: true,
        shape,
        element,
    } = &v.ty
    else {
        panic!("{:?}", v.ty);
    };
    assert_eq!(shape.rank, Rank::Matrix);
    assert_eq!(shape.bounds[1].low, Bound::Integer(-1));
    assert!(matches!(&shape.bounds[0].high, Bound::Name(n) if n.key == "N"));
    assert_eq!(
        element.base,
        BaseType::Numeric {
            kind: NumericKind::Integer,
            long: true
        }
    );
    assert!(matches!(v.init, Some(Init::Initial(Constant::Integer(0)))));
    assert!(matches!(y.init, Some(Init::Constant(_))));
    assert_eq!(l.ty, DataType::List(None));
    assert_eq!(u.mode, Mode::InOut);
    assert!(
        matches!(&u.ty, DataType::Array { element, .. } if element.fuzziness == Fuzziness::Fuzzy)
    );

    let program = lower::parse_program(
        "FUNCTION \"TOWN\" . \"DENSITY\" RETURNS FLOAT ; BEGIN END ;
         FUNCTOR C ' SIB (x) RETURNS SPARSE VECTOR OF CLASS ;
            x : INTEGER IN ;
         LOCAL a : INTEGER ;
         EXTERNAL F, G : FUNCTION RETURNS FLOAT ; H : FUNCTOR RETURNS LIST ;
         BEGIN RETURN END ;",
    )
    .unwrap();
    let [function, functor] = program.modules.as_slice() else {
        panic!("{} modules", program.modules.len());
    };
    assert!(matches!(
        &function.name,
        ModuleName::Attribute { class, attribute } if class.text == "TOWN" && attribute.text == "DENSITY"
    ));
    assert!(lower::parse_program("FUNCTION \"A\" . p (x) RETURNS FLOAT ; BEGIN END ;").is_err());
    assert!(function.body.is_empty());
    let ModuleKind::Functor {
        prefix: Some(prefix),
        returns,
    } = &functor.kind
    else {
        panic!("{:?}", functor.kind);
    };
    assert_eq!(prefix.key, "C");
    assert!(matches!(returns, DataType::Array { sparse: true, .. }));
    assert_eq!(functor.parameters.len(), 1);
    assert_eq!(functor.specifications.len(), 1);
    assert_eq!(functor.locals.len(), 1);
    assert!(matches!(&functor.externals[..], [
        External { kind: ExternalKind::Function(_), names },
        External { kind: ExternalKind::Functor(DataType::List(None)), .. },
    ] if names.len() == 2));
}
