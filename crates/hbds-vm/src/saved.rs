//! The kernel's fibers as text, for the daemon's journal (docs/
//! architecture.md, decision 5): each process's body and the coroutines
//! it resumed, their frames and stacks, as a pause leaves them. A module
//! is named by its unit's hash and its place there, so that the order
//! units load in, and code the store keeps compiled since, do not matter.

use std::collections::HashMap;

use hbds_core::terms::{
    Reader, Term, adt_ref, list, optional, read_terms, text, value, word, write_term,
};

use std::rc::Rc;

use super::{
    Cell, Fiber, FiberState, Frame, Item, Language, Origin, Owned, Role, Session, Span, Var,
    relocate,
};

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
    /// A module as a saved fiber names it: a unit's by hash and place,
    /// code the store keeps (a reflex's reaction in time) by what it is.
    fn module_term(&self, m: usize) -> Result<Term, String> {
        Ok(match &self.o.origin[m] {
            Origin::Unit(hash, i) => list(vec![word(hash), word(i)]),
            Origin::Code(part, language, code) => list(vec![
                word("code"),
                text(part),
                text(language),
                text(code),
                word(&self.o.hashes[self.o.home[m]]),
            ]),
        })
    }
}

impl Session {
    /// A module a saved fiber names: a unit's, loaded; code, compiled
    /// again if it is not yet.
    fn read_module(
        &mut self,
        r: &mut Reader,
        index: &HashMap<(String, usize), usize>,
        language: &dyn Language,
    ) -> Result<usize, String> {
        let mut m = r.list()?;
        let first = m.word()?.to_string();
        if first == "code" {
            let (part, lang, code) = (m.text()?, m.text()?, m.text()?);
            // The unit it was compiled against; fibers saved before it
            // was said took the last.
            let home = match m.more() {
                true => {
                    let hash = m.word()?;
                    let home = self.o.hashes.iter().position(|h| h == hash);
                    home.ok_or_else(|| format!("no unit {hash} is loaded"))?
                }
                false => self.units.len().saturating_sub(1),
            };
            m.end()?;
            return self.compile_code(&part, &lang, &code, home, language);
        }
        let key = (first, m.parse()?);
        m.end()?;
        index
            .get(&key)
            .copied()
            .ok_or_else(|| format!("no module {} of unit {} is loaded", key.1, key.0))
    }

    /// Code the store keeps, compiled against the unit at `home`, as a
    /// turn compiles it.
    fn compile_code(
        &mut self,
        part: &str,
        lang: &str,
        code: &str,
        home: usize,
        language: &dyn Language,
    ) -> Result<usize, String> {
        if lang != language.name() {
            return Err(format!(
                "code in {lang}, which {} does not run",
                language.name()
            ));
        }
        let key = (home, part.to_string(), code.to_string());
        if let Some(&m) = self.o.compiled.get(&key) {
            return Ok(m);
        }
        let (unit, offset) = self.units.get(home).ok_or("no unit loaded")?;
        let mut module = language.compile(part, code, "the store's code", unit)?;
        relocate(&mut module, *offset);
        self.o.modules.push(Rc::new(module));
        self.o
            .origin
            .push(Origin::Code(part.into(), lang.into(), code.into()));
        self.o.home.push(home);
        let m = self.o.modules.len() - 1;
        self.o.compiled.insert(key, m);
        Ok(m)
    }
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
                Role::Reflex(n) => list(vec![word("reflex"), word(n)]),
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
        let mut activations: Vec<_> = self.o.activations.iter().collect();
        activations.sort();
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
            list(
                activations
                    .into_iter()
                    .map(|(n, f)| list(vec![word(n), word(f)]))
                    .collect(),
            ),
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
    /// A fiber the journal gives, against the code it runs: taken as
    /// written, a damaged one would index out of its module, its frames
    /// or its stack later, in the kernel's loop. Its frames name
    /// instructions and slots that are there, its references variables
    /// that are, and its stack holds what the verifier counts before
    /// each frame's instruction.
    fn check_fiber(&self, f: &Fiber, shared: usize) -> Result<(), String> {
        let modules = |c: usize| self.o.modules.get(c).map(|m| &**m);
        let place = |frames: &[Frame], i: &Item| match *i {
            Item::Value(_) => true,
            Item::Ref(g, t) => frames.get(g).is_some_and(|fr| t < fr.cells.len()),
            Item::Shared(i) => i < shared,
        };
        let mut depth = 0;
        for (j, fr) in f.frames.iter().enumerate() {
            let m = modules(fr.module).ok_or("a frame of no module")?;
            let at = |e: &str| format!("frame {j}, in \"{}\" at {}: {e}", m.spelling, fr.pc);
            if fr.cells.len() != m.slots.len() {
                return Err(at("as many variables as the module's are expected"));
            }
            let below = &f.frames[..j];
            let cells = fr.cells.iter().all(|c| match *c {
                Cell::Own(_) => true,
                Cell::Ref(g, t) => below.get(g).is_some_and(|fr| t < fr.cells.len()),
                Cell::Shared(i) => i < shared,
            });
            if !cells || !fr.args.iter().all(|i| place(below, i)) {
                return Err(at("a reference to a variable that is not there"));
            }
            let before = hbds_acode::depth_at(m, fr.pc, &modules)
                .ok_or_else(|| at("no path leads to this instruction"))?;
            // A frame that called waits for its callee's result, which
            // the count before its next instruction includes.
            let awaited = match (j + 1 < f.frames.len(), fr.pc.checked_sub(1)) {
                (true, Some(pc)) => match m.code[pc] {
                    hbds_acode::Instr::Call(c, _) => modules(c).is_some_and(|callee| {
                        matches!(
                            callee.kind,
                            hbds_acode::Kind::Function | hbds_acode::Kind::Functor
                        )
                    }),
                    _ => false,
                },
                _ => false,
            };
            depth += before
                .checked_sub(awaited as usize)
                .ok_or_else(|| at("a result is awaited and no operand is"))?;
        }
        if !f.stack.iter().all(|i| place(&f.frames, i)) {
            return Err("an operand refers to a variable that is not there".into());
        }
        match f.stack.len() == depth {
            true => Ok(()),
            false => Err(format!(
                "{} operand(s), and its frames hold {depth}",
                f.stack.len()
            )),
        }
    }

    /// The kernel's fibers as `save_fibers` wrote them, its units loaded
    /// first. The kernel starts idle: the next process due runs on.
    pub fn restore_fibers(&mut self, text: &str, language: &dyn Language) -> Result<(), String> {
        let index: HashMap<(String, usize), usize> = (self.o.origin.iter().enumerate())
            .filter_map(|(m, o)| match o {
                Origin::Unit(hash, i) => Some(((hash.clone(), *i), m)),
                Origin::Code(..) => None,
            })
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
                "reflex" => Role::Reflex(role.parse()?),
                w => return Err(format!("{w:?} is not a fiber's role")),
            };
            let mut frames = Vec::new();
            let mut fs = f.list()?;
            while fs.more() {
                let mut fr = fs.list()?;
                frames.push(Frame {
                    module: self.read_module(&mut fr, &index, language)?,
                    pc: fr.parse()?,
                    cells: read_all(&mut fr, read_cell)?,
                    args: read_all(&mut fr, read_item)?,
                    prefix: fr.optional(|r| r.value())?,
                    this: fr.optional(|r| r.adt_ref())?,
                    depth: fr.parse()?,
                    // A fiber is saved between statements, outside any
                    // callback: each frame above its first is a call.
                    calls: frames.len(),
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
            coroutines.insert(self.read_module(&mut c, &index, language)?, c.parse()?);
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
        // The reflexes in time, which fibers saved before them lack.
        let mut activations = HashMap::new();
        if r.more() {
            let mut xs = r.list()?;
            while xs.more() {
                let mut x = xs.list()?;
                activations.insert(x.parse()?, x.parse()?);
                x.end()?;
            }
        }
        r.end()?;
        if fibers.is_empty() {
            return Err("no fibers".into());
        }
        let count = fibers.len();
        let known = |what: &str, f: usize| match f < count {
            true => Ok(()),
            false => Err(format!("{what} is fiber {f}, of {count}")),
        };
        for (n, f) in fibers.iter().enumerate() {
            self.check_fiber(f, shared.len())
                .map_err(|e| format!("fiber {n}: {e}"))?;
            f.resumer.map_or(Ok(()), |r| known("a resumer", r))?;
        }
        for &f in (coroutines.values())
            .chain(processes.values())
            .chain(activations.values())
        {
            known("a coroutine's, a process's or a reflex's", f)?;
        }
        self.o.fibers = fibers;
        // Idle: the main fiber is the kernel's own, with no frames.
        self.o.current = 0;
        self.o.frames = std::mem::take(&mut self.o.fibers[0].frames);
        self.o.stack = std::mem::take(&mut self.o.fibers[0].stack);
        self.o.coroutines = coroutines;
        self.o.processes = processes;
        self.o.activations = activations;
        self.o.shared = shared;
        self.o.held_at = held_at;
        self.o.running = None;
        Ok(())
    }
}
