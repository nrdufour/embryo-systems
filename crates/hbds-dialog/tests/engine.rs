//! The engine on a fake language: a task's code is a word to log, or
//! `fail`; a line reads as an integer, a float with a point, TRUE or
//! FALSE.

use std::ops::DerefMut;

use hbds_core::value::{Scalar, Value};
use hbds_core::{AdtRef, Id};
use hbds_dialog::*;
use hbds_gtad::{Code, Store};

struct Fake {
    store: Store,
    out: String,
    log: Vec<String>,
}

impl Host for Fake {
    fn store(&mut self) -> Box<dyn DerefMut<Target = Store> + '_> {
        Box::new(&mut self.store)
    }

    fn run(&mut self, part: &str, owner: Id, code: &Code, _: Id) -> Result<(), String> {
        if code.text == "fail" {
            return Err(format!("the {part} fails"));
        }
        let o = self.store.objects()[&owner].name.clone();
        self.log.push(format!("{part} {o} {}", code.text));
        Ok(())
    }

    fn read(&mut self, line: &str) -> Result<Value, String> {
        let s = match line {
            "TRUE" => Scalar::Boolean(true),
            "FALSE" => Scalar::Boolean(false),
            _ if line.contains('.') => Scalar::Float(line.parse().map_err(|_| "no")?),
            _ => Scalar::Integer(line.parse().map_err(|_| "no")?),
        };
        Ok(Value::exact(s))
    }

    fn write(&mut self, text: &str) {
        self.out += text;
    }
}

fn new() -> Fake {
    let mut store = Store::memory();
    install(&mut store).unwrap();
    Fake {
        store,
        out: String::new(),
        log: Vec::new(),
    }
}

impl Fake {
    fn object(&mut self, class: &str, name: &str) -> Id {
        let c = self.store.class(class).unwrap();
        self.store.create_object(c, name).unwrap()
    }

    fn set(&mut self, o: Id, a: &str, v: Scalar) {
        let class = self.store.objects()[&o].class;
        let a = self.store.attribute(class, a).unwrap();
        self.store.set(o, a, Value::exact(v)).unwrap();
    }

    fn text(&mut self, o: Id, t: &str) {
        self.set(o, TEXT, Scalar::String(t.into()));
    }

    fn link(&mut self, from: Id, relation: &str, to: Id) {
        self.store.assert_link(from, relation, to, None).unwrap();
    }

    fn code(&mut self, o: Id, part: &str, text: &str) {
        (self.store)
            .set_property(AdtRef::Object(o), part, "fake", text)
            .unwrap();
    }

    fn sq(&mut self, name: &str, question: &str) -> Id {
        let q = self.object(SQ, name);
        self.text(q, question);
        q
    }

    fn mcq(&mut self, name: &str, question: &str, answers: &[&str]) -> (Id, Vec<Id>) {
        let q = self.object(MCQ, name);
        self.text(q, question);
        let pas = (answers.iter())
            .map(|a| {
                let pa = self.object(PA, &format!("{name} {a}"));
                self.text(pa, a);
                self.link(q, PROPOSES, pa);
                pa
            })
            .collect();
        (q, pas)
    }

    fn dialog(&mut self, initial: Id) -> Id {
        let d = self.object(DIALOG, "d");
        self.link(d, INITIAL, initial);
        d
    }

    /// Runs the dialog on these lines: the output, and whether it ended.
    fn talk(&mut self, dialog: Id, lines: &[&str]) -> Result<Id, String> {
        let run = start(&mut self.store, dialog)?;
        let mut lines = lines.iter();
        loop {
            let line = if wants(&self.store, run) {
                match lines.next() {
                    Some(l) => Some(*l),
                    None => return Ok(run),
                }
            } else {
                None
            };
            if step(self, run, line)? {
                return Ok(run);
            }
        }
    }

    fn answers(&self, run: Id) -> Vec<String> {
        let mut out = Vec::new();
        for l in self.store.links_from(run) {
            let link = &self.store.object_links()[&l];
            let rel = &self.store.class_links()[&link.class_link].relation;
            if rel.as_deref() == Some(GAVE) {
                let o = &self.store.objects()[&link.to];
                let a = self.store.find_attribute(o.class, TEXT).unwrap();
                out.push(self.store.show(self.store.get(link.to, a).unwrap()));
            }
        }
        out
    }

    fn phase(&self, run: Id) -> String {
        let class = self.store.objects()[&run].class;
        let a = self.store.find_attribute(class, PHASE).unwrap();
        self.store.show(self.store.get(run, a).unwrap())
    }
}

#[test]
fn simple_questions_chain_and_type_their_answers() {
    let mut f = new();
    let a = f.sq("a", "Depth");
    let b = f.sq("b", "Name");
    f.set(a, TYPE, Scalar::String("INTEGER".into()));
    f.set(a, MEMORIZE, Scalar::Boolean(true));
    f.set(b, MEMORIZE, Scalar::Boolean(true));
    f.link(a, NEXT, b);
    f.code(a, PROLOG, "pa");
    f.code(a, EPILOG, "ea");
    let d = f.dialog(a);
    let run = f.talk(d, &["deep", "12", "Loire"]).unwrap();
    assert_eq!(f.out, "Depth ? INTEGER expected\nDepth ? Name ? ");
    assert_eq!(f.answers(run), ["12", "Loire"]);
    assert_eq!(f.log, ["PROLOG a pa", "EPILOG a ea"]);
    assert_eq!(f.phase(run), "done");
}

#[test]
fn an_mcq_branches_by_number_or_text() {
    let mut f = new();
    let (q, pas) = f.mcq("q", "Theme", &["roads", "rivers"]);
    let roads = f.sq("roads", "Road");
    let rivers = f.sq("rivers", "River");
    f.link(pas[0], NEXT, roads);
    f.link(pas[1], NEXT, rivers);
    f.code(pas[1], TASK, "t");
    let d = f.dialog(q);
    f.talk(d, &["3", "Rivers", "Loire"]).unwrap();
    assert_eq!(
        f.out,
        "Theme\n1. roads\n2. rivers\n? choose 1 to 2\nTheme\n1. roads\n2. rivers\n? River ? "
    );
    assert_eq!(f.log, ["TASK q rivers t"]);
}

#[test]
fn an_answer_may_be_facultative() {
    let mut f = new();
    let a = f.sq("a", "Comment");
    let b = f.sq("b", "Next");
    let c = f.sq("c", "Skipped");
    f.link(a, NEXT, b);
    f.link(a, IF_BLANK, c);
    f.code(a, BLANK, "e");
    f.code(a, EPILOG, "never");
    let d = f.dialog(a);
    f.talk(d, &[""]).unwrap();
    assert_eq!(f.out, "Comment ? an answer is required\nComment ? ");
    f.set(a, FACULTATIVE, Scalar::Boolean(true));
    f.out.clear();
    f.talk(d, &[""]).unwrap();
    assert_eq!(f.out, "Comment ? Skipped ? ");
    assert_eq!(f.log, ["BLANK a e"]);
}

/// Page 139's example: a theme chosen, then four questions shared by
/// every theme, and the branch the choice made only after them.
#[test]
fn a_delayed_choice_branches_a_later_simple_question() {
    let mut f = new();
    let (theme, pas) = f.mcq("theme", "Theme", &["roads", "rivers"]);
    let x = f.sq("x", "X");
    let y = f.sq("y", "Y");
    let roads = f.sq("roads", "Road");
    let rivers = f.sq("rivers", "River");
    f.link(theme, NEXT, x);
    f.link(x, NEXT, y);
    f.link(y, CONTROLLED_BY, theme);
    f.link(pas[0], DELAYED, roads);
    f.link(pas[1], DELAYED, rivers);
    let d = f.dialog(theme);
    f.talk(d, &["2", "1", "2"]).unwrap();
    assert!(f.out.ends_with("X ? Y ? River ? "), "{}", f.out);
}

/// Figure 70: an MCQ's own answer kept, its branch another's.
#[test]
fn a_delayed_choice_branches_a_later_mcq() {
    let mut f = new();
    let (theme, pas) = f.mcq("theme", "Theme", &["roads", "rivers"]);
    let (scale, scales) = f.mcq("scale", "Scale", &["small", "large"]);
    let roads = f.sq("roads", "Road");
    let rivers = f.sq("rivers", "River");
    let small = f.sq("small", "Small");
    f.link(theme, NEXT, scale);
    f.link(scales[0], NEXT, small);
    f.link(scale, CONTROLLED_BY, theme);
    f.link(pas[0], DELAYED, roads);
    f.link(pas[1], DELAYED, rivers);
    f.set(scale, MEMORIZE, Scalar::Boolean(true));
    let d = f.dialog(theme);
    let run = f.talk(d, &["1", "small"]).unwrap();
    assert!(f.out.ends_with("? Road ? "), "{}", f.out);
    assert_eq!(f.answers(run), ["small"]);
}

#[test]
fn a_delayed_branch_needs_its_choice_made() {
    let mut f = new();
    let (theme, _) = f.mcq("theme", "Theme", &["roads"]);
    let x = f.sq("x", "X");
    f.link(x, CONTROLLED_BY, theme);
    let d = f.dialog(x);
    let e = f.talk(d, &["1"]).unwrap_err();
    assert_eq!(
        e,
        "x[SQ] branches as theme[MCQ] was answered, and it was not"
    );
}

#[test]
fn going_back_asks_again() {
    let mut f = new();
    let a = f.sq("a", "A");
    let b = f.sq("b", "B");
    f.link(a, NEXT, b);
    f.code(a, PROLOG, "p");
    let d = f.dialog(a);
    let run = f.talk(d, &["/back", "1", "/back", "2", "/quit"]).unwrap();
    assert_eq!(f.out, "A ? nothing to go back to\nA ? B ? A ? B ? ");
    assert_eq!(f.log, ["PROLOG a p", "PROLOG a p"]);
    assert_eq!(f.phase(run), "done");
}

#[test]
fn a_learning_mcq_takes_a_new_answer() {
    let mut f = new();
    let (q, _) = f.mcq("q", "Furniture", &["bench"]);
    f.set(q, LEARNING, Scalar::Boolean(true));
    f.set(q, DISPLAY, Scalar::Boolean(false));
    f.set(q, MEMORIZE, Scalar::Boolean(true));
    let d = f.dialog(q);
    let run = f.talk(d, &["lamp"]).unwrap();
    assert_eq!(f.out, "Furniture\n? ");
    assert_eq!(f.answers(run), ["lamp"]);
    let pa = f.store.class(PA).unwrap();
    assert!(f.store.find_object(pa, "q 1").is_some());
}

#[test]
fn a_slash_is_escaped_by_another() {
    let mut f = new();
    let a = f.sq("a", "Path");
    f.set(a, MEMORIZE, Scalar::Boolean(true));
    let d = f.dialog(a);
    let run = f.talk(d, &["//back"]).unwrap();
    assert_eq!(f.answers(run), ["/back"]);
}

#[test]
fn a_failing_task_and_a_dialog_without_start_are_errors() {
    let mut f = new();
    let a = f.sq("a", "A");
    f.code(a, PROLOG, "fail");
    let d = f.dialog(a);
    assert_eq!(f.talk(d, &[]).unwrap_err(), "the PROLOG fails");
    let lone = f.object(DIALOG, "lone");
    assert_eq!(
        f.talk(lone, &[]).unwrap_err(),
        "lone[DIALOG] has no \"initial\" place"
    );
}
