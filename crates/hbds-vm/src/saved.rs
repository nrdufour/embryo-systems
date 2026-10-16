//! The kernel's fibers as text, for the daemon's journal (docs/
//! architecture.md, decision 5): each process's body and the coroutines
//! it resumed, their frames and stacks, as a pause leaves them. A module
//! is named by its unit's hash and its place there, so that the order
//! units load in, and code the store keeps compiled since, do not matter.

use std::collections::HashMap;

use hbds_core::terms::{
    Reader, Term, adt_ref, list, optional, read_terms, value, word, write_term,
};

use super::{Cell, Fiber, FiberState, Frame, Item, Owned, Role, Session, Span, Var};

fn var(v: &Var) -> Term {
    list(vec![
        optional(v.value.as_ref().map(value)),
        optional(v.initial.as_ref().map(value)),
    ])
}

fn read_var(r: &mut Reader) -> Result<Var, String> {
    let mut v = r.list()?;
    let var = Var {
        value: v.optional(|r| r.value())?,
        initial: v.optional(|r| r.value())?,
    };
    v.end()?;
    Ok(var)
}

fn item(i: &Item) -> Term {
    match i {
        Item::Value(v) => list(vec![word("value"), value(v)]),
        Item::Ref(f, t) => list(vec![word("ref"), word(f), word(t)]),
        Item::Shared(i) => list(vec![word("shared"), word(i)]),
    }
}

fn read_item(r: &mut Reader) -> Result<Item, String> {
    let mut i = r.list()?;
    let item = match i.word()? {
        "value" => Item::Value(i.value()?),
        "ref" => Item::Ref(i.parse()?, i.parse()?),
        "shared" => Item::Shared(i.parse()?),
        w => return Err(format!("{w:?} is not an operand")),
    };
    i.end()?;
    Ok(item)
}

fn cell(c: &Cell) -> Term {
    match c {
        Cell::Own(v) => list(vec![word("own"), var(v)]),
        Cell::Ref(f, t) => list(vec![word("ref"), word(f), word(t)]),
        Cell::Shared(i) => list(vec![word("shared"), word(i)]),
    }
}

fn read_cell(r: &mut Reader) -> Result<Cell, String> {
    let mut c = r.list()?;
    let cell = match c.word()? {
        "own" => Cell::Own(read_var(&mut c)?),
        "ref" => Cell::Ref(c.parse()?, c.parse()?),
        "shared" => Cell::Shared(c.parse()?),
        w => return Err(format!("{w:?} is not a cell")),
    };
    c.end()?;
    Ok(cell)
}

fn all<T>(items: &[T], f: impl Fn(&T) -> Term) -> Term {
    list(items.iter().map(f).collect())
}

fn read_all<T>(
    r: &mut Reader,
    f: impl Fn(&mut Reader) -> Result<T, String>,
) -> Result<Vec<T>, String> {
    let mut items = r.list()?;
    let mut out = Vec::new();
    while items.more() {
        out.push(f(&mut items)?);
    }
    Ok(out)
}

const STATES: [(FiberState, &str); 3] = [
    (FiberState::Running, "running"),
    (FiberState::Paused, "paused"),
    (FiberState::Ended, "ended"),
];

impl Session {
    /// [`save_fibers`](Session::save_fibers), for the VM in a turn.
    pub(super) fn save_fibers_of(o: &Owned) -> Result<String, String> {
        Saver { o }.save()
    }

    pub fn save_fibers(&self) -> Result<String, String> {
        Saver { o: &self.o }.save()
    }
}

/// The VM's own state, saved.
struct Saver<'a> {
    o: &'a Owned,
}

impl Saver<'_> {
    /// A module as a saved fiber names it.
    fn module_term(&self, m: usize) -> Result<Term, String> {
        match &self.o.origin[m] {
            Some((hash, i)) => Ok(list(vec![word(hash), word(i)])),
            None => Err("a fiber in code the store keeps, which runs to completion".into()),
        }
    }
}

fn read_module(r: &mut Reader, index: &HashMap<(String, usize), usize>) -> Result<usize, String> {
    let mut m = r.list()?;
    let key = (m.word()?.to_string(), m.parse()?);
    m.end()?;
    index
        .get(&key)
        .copied()
        .ok_or_else(|| format!("no module {} of unit {} is loaded", key.1, key.0))
}

impl Saver<'_> {
    fn frame_term(&self, f: &Frame) -> Result<Term, String> {
        Ok(list(vec![
            self.module_term(f.module)?,
            word(f.pc),
            all(&f.cells, cell),
            all(&f.args, item),
            optional(f.prefix.as_ref().map(value)),
            optional(f.this.map(adt_ref)),
            word(f.depth),
            word(f.within),
        ]))
    }

    /// The kernel's fibers as text, as a pause leaves them.
    fn save(&self) -> Result<String, String> {
        let mut fibers = Vec::new();
        for (n, f) in self.o.fibers.iter().enumerate() {
            // The current fiber's frames and stack are the VM's own.
            let (frames, stack) = match n == self.o.current {
                true => (&self.o.frames, &self.o.stack),
                false => (&f.frames, &f.stack),
            };
            let role = match f.role {
                Role::Main => list(vec![word("main")]),
                Role::Coroutine => list(vec![word("coroutine")]),
                Role::Process(p) => list(vec![word("process"), word(p)]),
            };
            let state = STATES
                .iter()
                .find(|(s, _)| *s == f.state)
                .expect("a state")
                .1;
            let frames = (frames.iter())
                .map(|fr| self.frame_term(fr))
                .collect::<Result<Vec<_>, _>>()?;
            fibers.push(list(vec![
                role,
                list(frames),
                all(stack, item),
                optional(f.resumer.map(word)),
                word(state),
            ]));
        }
        let mut coroutines = Vec::new();
        for (&m, &fiber) in &self.o.coroutines {
            coroutines.push(list(vec![self.module_term(m)?, word(fiber)]));
        }
        coroutines.sort_by_key(|t| {
            let mut s = String::new();
            write_term(t, &mut s);
            s
        });
        let mut processes: Vec<_> = self.o.processes.iter().collect();
        processes.sort();
        let terms = [
            list(fibers),
            list(coroutines),
            list(
                processes
                    .into_iter()
                    .map(|(p, f)| list(vec![word(p), word(f)]))
                    .collect(),
            ),
            all(&self.o.shared, var),
            list(vec![word(self.o.held_at.line), word(self.o.held_at.column)]),
        ];
        let mut out = String::new();
        for (i, t) in terms.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            write_term(t, &mut out);
        }
        Ok(out)
    }
}

impl Session {
    /// The kernel's fibers as `save_fibers` wrote them, its units loaded
    /// first. The kernel starts idle: the next process due runs on.
    pub fn restore_fibers(&mut self, text: &str) -> Result<(), String> {
        let index: HashMap<(String, usize), usize> = (self.o.origin.iter().enumerate())
            .filter_map(|(m, o)| o.clone().map(|key| (key, m)))
            .collect();
        let terms = read_terms(text)?;
        let mut r = Reader::new(&terms);
        let mut fibers = Vec::new();
        let mut items = r.list()?;
        while items.more() {
            let mut f = items.list()?;
            let mut role = f.list()?;
            let role = match role.word()? {
                "main" => Role::Main,
                "coroutine" => Role::Coroutine,
                "process" => Role::Process(role.id()?),
                w => return Err(format!("{w:?} is not a fiber's role")),
            };
            let mut frames = Vec::new();
            let mut fs = f.list()?;
            while fs.more() {
                let mut fr = fs.list()?;
                frames.push(Frame {
                    module: read_module(&mut fr, &index)?,
                    pc: fr.parse()?,
                    cells: read_all(&mut fr, read_cell)?,
                    args: read_all(&mut fr, read_item)?,
                    prefix: fr.optional(|r| r.value())?,
                    this: fr.optional(|r| r.adt_ref())?,
                    depth: fr.parse()?,
                    within: fr.parse()?,
                });
                fr.end()?;
            }
            let stack = read_all(&mut f, read_item)?;
            let resumer = f.optional(|r| r.parse())?;
            let w = f.word()?;
            let state = STATES
                .iter()
                .find(|(_, n)| *n == w)
                .ok_or(format!("{w:?} is not a fiber's state"))?
                .0;
            f.end()?;
            fibers.push(Fiber {
                frames,
                stack,
                role,
                resumer,
                state,
            });
        }
        let mut coroutines = HashMap::new();
        let mut cs = r.list()?;
        while cs.more() {
            let mut c = cs.list()?;
            coroutines.insert(read_module(&mut c, &index)?, c.parse()?);
            c.end()?;
        }
        let mut processes = HashMap::new();
        let mut ps = r.list()?;
        while ps.more() {
            let mut p = ps.list()?;
            processes.insert(p.id()?, p.parse()?);
            p.end()?;
        }
        let shared = read_all(&mut r, read_var)?;
        let mut at = r.list()?;
        let held_at = Span {
            line: at.parse()?,
            column: at.parse()?,
        };
        r.end()?;
        if fibers.is_empty() {
            return Err("no fibers".into());
        }
        self.o.fibers = fibers;
        // Idle: the main fiber is the kernel's own, with no frames.
        self.o.current = 0;
        self.o.frames = std::mem::take(&mut self.o.fibers[0].frames);
        self.o.stack = std::mem::take(&mut self.o.fibers[0].stack);
        self.o.coroutines = coroutines;
        self.o.processes = processes;
        self.o.shared = shared;
        self.o.held_at = held_at;
        self.o.running = None;
        Ok(())
    }
}
