//! The journal, GTAD's persistence: every [`Change`] of a store, one line
//! each, appended to `DIR/journal`, each transaction closed by a `commit`
//! line, as the print's exchange zone forces its writes (page 103) and
//! its file system keeps a DAYFILE (page 86). Opening a store replays its
//! journal into a fresh one, then records what follows. The store holds
//! `DIR/lock` while it is open, so that a second one, in this process or
//! another, is refused rather than writing the same journal. The print's
//! own persistence, SFSI and ZE (project 13), would be another
//! [`Recorder`].
//!
//! Two ways to record (docs/architecture.md, decision 5): [`open`] syncs
//! each transaction before the statement goes on, for a standalone run;
//! [`open_grouped`] hands them to a thread of their own, which writes and
//! syncs what has queued up in one go, and says how far it has got
//! ([`Durable`]): the daemon's group commit. [`compact`] rewrites a
//! journal as the shortest one that rebuilds its store; the grouped
//! journal compacts itself as it grows.
//!
//! A line is a sequence of terms: words, numbers, quoted strings, and
//! parenthesised lists of terms. `class 1 "TOWN" class`, `set 7 3 (f 2.5)`.
//! Version 3 starts with `journal 3`, and is read and written. Version 2,
//! the same format, set an object attribute's properties on its class
//! attribute; version 1 had no header or commits, each line its own
//! transaction. Both are read as they meant, and rewritten as 3.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use hbds_core::AdtRef;
use hbds_core::terms::{
    Reader, Term, adt_ref, float, list, optional, read_terms, text, value, word, write_term,
};
use hbds_gtad::{Change, ClassKind, Error, Image, Modality, Recorder, State, Store, state_name};

/// The journal's format, decision 10: older ones read, newer refused.
/// Version 3 gives object attributes properties of their own; before it,
/// a property set on one was its class attribute's.
pub const VERSION: u32 = 3;

const COMMIT: &str = "commit";
/// The changes a compacted journal commits together.
const CHUNK: u64 = 10_000;

/// A transaction's lines: its changes, then `commit`.
fn transaction(changes: &[Change]) -> (String, u64) {
    let mut text: String = changes.iter().map(|c| encode(c) + "\n").collect();
    text.push_str(COMMIT);
    text.push('\n');
    (text, changes.len() as u64 + 1)
}

/// A journal's file, appended to.
struct Writer {
    dir: PathBuf,
    file: File,
}

impl Writer {
    fn append(&mut self, text: &str, sync: bool) -> std::io::Result<()> {
        self.file.write_all(text.as_bytes())?;
        if sync {
            self.file.sync_data()?;
        }
        Ok(())
    }

    /// Replaces the journal, atomically, by one rebuilding the world in
    /// `image`: written beside it, synced, renamed over it, the directory
    /// synced. Its lines.
    fn rewrite(&mut self, image: &Image) -> std::io::Result<u64> {
        let new = self.dir.join("journal.new");
        // Each change written as it comes, a commit every CHUNK: replay
        // holds a transaction until its commit, and the file is whole
        // once renamed, so neither side holds the world twice.
        let mut file = BufWriter::new(File::create(&new)?);
        writeln!(file, "journal {VERSION}")?;
        let (mut lines, mut open, mut failed) = (1, 0, None);
        image.each_change(&mut |c| {
            if failed.is_some() {
                return;
            }
            let mut put = || {
                writeln!(file, "{}", encode(&c))?;
                open += 1;
                if open == CHUNK {
                    writeln!(file, "{COMMIT}")?;
                    lines += open + 1;
                    open = 0;
                }
                Ok(())
            };
            failed = put().err();
        });
        if let Some(e) = failed {
            return Err(e);
        }
        if open > 0 {
            writeln!(file, "{COMMIT}")?;
            lines += open + 1;
        }
        file.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        fs::rename(&new, self.dir.join("journal"))?;
        File::open(&self.dir)?.sync_all()?;
        self.file = OpenOptions::new()
            .append(true)
            .open(self.dir.join("journal"))?;
        Ok(lines)
    }
}

/// A sync per transaction: a standalone run's journal.
struct Journal {
    writer: Writer,
    /// Locked while the store is open; the system lets go when the
    /// process ends, however it ends. Not the journal itself, which
    /// compaction replaces by another file.
    _lock: File,
}

impl Recorder for Journal {
    fn record(&mut self, change: &Change) -> Result<(), String> {
        self.record_all(std::slice::from_ref(change))
    }

    /// A transaction's changes in one write, then one sync.
    fn record_all(&mut self, changes: &[Change]) -> Result<(), String> {
        let (text, _) = transaction(changes);
        (self.writer.append(&text, true)).map_err(|e| format!("the journal cannot be written: {e}"))
    }
}

/// A store's directory, locked, its journal replayed: the store, the
/// journal's file and lines, and what was dropped on the way.
struct Opened {
    store: Store,
    writer: Writer,
    lock: File,
    lines: u64,
    warnings: Vec<String>,
}

fn opened(dir: &Path) -> Result<Opened, Error> {
    let io = |e: std::io::Error| Error(format!("{}: {e}", dir.display()));
    fs::create_dir_all(dir).map_err(io)?;
    let lock = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(dir.join("lock"))
        .map_err(io)?;
    lock.try_lock().map_err(|e| match e {
        TryLockError::WouldBlock => Error(format!(
            "{}: the store is in use by another run",
            dir.display()
        )),
        TryLockError::Error(e) => io(e),
    })?;
    let path = dir.join("journal");
    // Read a line at a time: the whole journal as one string would sit
    // beside the store it rebuilds.
    let mut reader = match File::open(&path) {
        Ok(f) => Some(BufReader::new(f)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(io(e)),
    };
    let mut warnings = Vec::new();
    let mut store = Store::memory();
    let mut pending = Vec::new();
    let (mut version, mut empty, mut buf) = (1, true, String::new());
    // Where the last whole transaction ends, in bytes, and in lines.
    let (mut offset, mut kept, mut count) = (0, 0, 1);
    for n in 0.. {
        let Some(r) = &mut reader else { break };
        buf.clear();
        let read = r.read_line(&mut buf).map_err(io)?;
        if read == 0 {
            break;
        }
        empty = false;
        let Some(line) = buf.strip_suffix('\n') else {
            warnings.push(format!(
                "the journal's last line was cut short, and is dropped: {buf:?}"
            ));
            break;
        };
        let line = line.strip_suffix('\r').unwrap_or(line);
        offset += read;
        if n == 0
            && let Some(v) = line.strip_prefix("journal ")
        {
            let v = v.trim();
            let v: u32 = v
                .parse()
                .map_err(|_| Error(format!("journal line 1: a version, not {v:?}")))?;
            if v > VERSION {
                return Err(Error(format!(
                    "the journal's version {v} is newer than this reader's, {VERSION}"
                )));
            }
            version = v;
            if v >= 2 {
                kept = offset;
            }
            continue;
        }
        if version >= 2 && line == COMMIT {
            for c in pending.drain(..) {
                store.replay(c);
            }
            (kept, count) = (offset, n as u64 + 1);
            continue;
        }
        let mut change = decode(line).map_err(|e| Error(format!("journal line {}: {e}", n + 1)))?;
        if version < 3
            && let Change::Property { target, .. } = &mut change
            && let AdtRef::ObjectAttribute(_, a) = *target
        {
            *target = AdtRef::ClassAttribute(a);
        }
        match version {
            1 => {
                store.replay(change);
                (kept, count) = (offset, n as u64 + 1);
            }
            _ => pending.push(change),
        }
    }
    drop(reader);
    if !pending.is_empty() {
        warnings.push(format!(
            "the journal's last transaction was not committed, and is dropped: {} change(s)",
            pending.len()
        ));
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(io)?;
    file.set_len(kept as u64).map_err(io)?;
    let mut writer = Writer {
        dir: dir.to_path_buf(),
        file,
    };
    // A new journal, or an older one, is written anew in this one's.
    if version < VERSION || empty {
        count = writer.rewrite(&store.image()).map_err(io)?;
    }
    Ok(Opened {
        store,
        writer,
        lock,
        lines: count,
        warnings,
    })
}

/// Opens, or creates, the store in `dir`, replaying its journal; each
/// transaction is synced as it ends. A last line cut short by a crash, or
/// a last transaction never committed, is dropped, with a warning. A
/// store already open is refused.
pub fn open(dir: &Path) -> Result<(Store, Vec<String>), Error> {
    let o = opened(dir)?;
    let mut store = o.store;
    store.record_to(Box::new(Journal {
        writer: o.writer,
        _lock: o.lock,
    }));
    Ok((store, o.warnings))
}

/// Rewrites the journal of the store in `dir` as the shortest one that
/// rebuilds it: its lines before and after.
pub fn compact(dir: &Path) -> Result<(u64, u64), Error> {
    let io = |e: std::io::Error| Error(format!("{}: {e}", dir.display()));
    let mut o = opened(dir)?;
    let after = o.writer.rewrite(&o.store.image()).map_err(io)?;
    Ok((o.lines, after))
}

/// How far the grouped journal has got.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// Transactions handed to the journal, and those on disk.
    pub submitted: u64,
    pub synced: u64,
    /// Syncs done: far fewer than transactions, when they come fast.
    pub syncs: u64,
    pub compactions: u64,
    /// The journal's lines, and how many the last compaction wrote.
    pub lines: u64,
    pub compacted: u64,
}

#[derive(Default)]
struct Progress {
    stats: Stats,
    /// What the kernel holds back: kept transactions' changes, in order.
    held: Option<Vec<Change>>,
    /// Why the journal stopped, if it did: nothing after is kept.
    failed: Option<String>,
    /// A compaction asked for and not yet done.
    compacting: bool,
}

struct Shared {
    progress: Mutex<Progress>,
    changed: Condvar,
}

enum Job {
    Write(String, u64),
    Compact(Box<Image>),
}

/// The grouped journal's progress, shared: what a client waits on before
/// it hears that its statements are done.
#[derive(Clone)]
pub struct Durable {
    shared: Arc<Shared>,
    jobs: Sender<Job>,
}

impl Durable {
    pub fn stats(&self) -> Stats {
        self.shared.progress.lock().unwrap().stats
    }

    /// What a client waits for before it hears of what it did: the
    /// transactions submitted, and the one held back, if one is.
    pub fn stamp(&self) -> u64 {
        let p = self.shared.progress.lock().unwrap();
        p.stats.submitted + p.held.is_some() as u64
    }

    /// Waits until `submitted` transactions are on disk; the journal's
    /// failure if it stopped first.
    pub fn wait(&self, submitted: u64) -> Result<(), String> {
        let mut p = self.shared.progress.lock().unwrap();
        loop {
            if p.stats.synced >= submitted {
                return Ok(());
            }
            if let Some(e) = &p.failed {
                return Err(e.clone());
            }
            p = self.shared.changed.wait(p).unwrap();
        }
    }

    /// Compacts the journal if it has grown past twice what the last
    /// compaction wrote, and past `at` lines: `store` is the world as the
    /// transactions submitted so far leave it. The journal's thread builds
    /// its changes from an image of it, so the store goes on meanwhile.
    pub fn maybe_compact(&self, store: &Store, at: u64) {
        {
            let mut p = self.shared.progress.lock().unwrap();
            let s = p.stats;
            // Not while the kernel holds a step back: compaction would
            // keep half of it.
            if p.compacting || p.held.is_some() || s.lines < at || s.lines < 2 * s.compacted {
                return;
            }
            p.compacting = true;
        }
        let _ = self.jobs.send(Job::Compact(Box::new(store.image())));
    }
}

/// The grouped journal's side of the store.
struct Grouped {
    shared: Arc<Shared>,
    jobs: Sender<Job>,
    _lock: File,
}

impl Recorder for Grouped {
    fn record(&mut self, change: &Change) -> Result<(), String> {
        self.record_all(std::slice::from_ref(change))
    }

    /// Queued, not written: the journal's thread writes and syncs it with
    /// what else is queued. A journal that stopped refuses it.
    fn record_all(&mut self, changes: &[Change]) -> Result<(), String> {
        let mut p = self.shared.progress.lock().unwrap();
        if let Some(e) = &p.failed {
            return Err(e.clone());
        }
        if let Some(held) = &mut p.held {
            held.extend_from_slice(changes);
            return Ok(());
        }
        let (text, lines) = transaction(changes);
        p.stats.submitted += 1;
        self.jobs
            .send(Job::Write(text, lines))
            .map_err(|_| "the journal's thread is gone".to_string())
    }

    fn hold(&mut self) {
        let mut p = self.shared.progress.lock().unwrap();
        p.held.get_or_insert_with(Vec::new);
    }

    /// Under one lock, so that no stamp falls between the held write
    /// and its submission.
    fn release(&mut self, extra: &[Change]) -> Result<(), String> {
        let mut p = self.shared.progress.lock().unwrap();
        let mut changes = p.held.take().unwrap_or_default();
        changes.extend_from_slice(extra);
        if changes.is_empty() {
            return Ok(());
        }
        if let Some(e) = &p.failed {
            return Err(e.clone());
        }
        let (text, lines) = transaction(&changes);
        p.stats.submitted += 1;
        self.jobs
            .send(Job::Write(text, lines))
            .map_err(|_| "the journal's thread is gone".to_string())
    }
}

/// Opens the store in `dir` as [`open`] does, its transactions recorded
/// by a thread of their own, which syncs together those that queue up
/// while it syncs. `delay` slows each sync, for tests.
pub fn open_grouped(dir: &Path, delay: Duration) -> Result<(Store, Durable, Vec<String>), Error> {
    let o = opened(dir)?;
    let shared = Arc::new(Shared {
        progress: Mutex::new(Progress {
            stats: Stats {
                lines: o.lines,
                ..Stats::default()
            },
            ..Progress::default()
        }),
        changed: Condvar::new(),
    });
    let (jobs, queue) = mpsc::channel();
    let writer = o.writer;
    let thread = shared.clone();
    std::thread::Builder::new()
        .name("journal".into())
        .spawn(move || journal_thread(writer, queue, thread, delay))
        .map_err(|e| Error(format!("no thread for the journal: {e}")))?;
    let mut store = o.store;
    store.record_to(Box::new(Grouped {
        shared: shared.clone(),
        jobs: jobs.clone(),
        _lock: o.lock,
    }));
    Ok((store, Durable { shared, jobs }, o.warnings))
}

fn journal_thread(mut writer: Writer, queue: Receiver<Job>, shared: Arc<Shared>, delay: Duration) {
    let fail = |e: std::io::Error| {
        let mut p = shared.progress.lock().unwrap();
        p.failed = Some(format!("the journal cannot be written: {e}"));
        shared.changed.notify_all();
    };
    while let Ok(first) = queue.recv() {
        let mut jobs = vec![first];
        jobs.extend(queue.try_iter());
        let (mut batch, mut count, mut lines) = (String::new(), 0, 0);
        let flush = |batch: &mut String, count: &mut u64, lines: &mut u64, w: &mut Writer| {
            if *count == 0 {
                return true;
            }
            std::thread::sleep(delay);
            if let Err(e) = w.append(batch, true) {
                fail(e);
                return false;
            }
            let mut p = shared.progress.lock().unwrap();
            p.stats.synced += *count;
            p.stats.syncs += 1;
            p.stats.lines += *lines;
            shared.changed.notify_all();
            batch.clear();
            *count = 0;
            *lines = 0;
            true
        };
        for job in jobs {
            match job {
                Job::Write(text, n) => {
                    batch.push_str(&text);
                    count += 1;
                    lines += n;
                }
                // In the order it came: what was queued before is in
                // `changes`, what comes after goes to the new file.
                Job::Compact(image) => {
                    if !flush(&mut batch, &mut count, &mut lines, &mut writer) {
                        return;
                    }
                    match writer.rewrite(&image) {
                        Ok(n) => {
                            let mut p = shared.progress.lock().unwrap();
                            p.stats.compactions += 1;
                            p.stats.lines = n;
                            p.stats.compacted = n;
                            p.compacting = false;
                        }
                        Err(e) => return fail(e),
                    }
                }
            }
        }
        if !flush(&mut batch, &mut count, &mut lines, &mut writer) {
            return;
        }
    }
}

fn state_word(s: State) -> Term {
    word(state_name(s))
}

/// A change as the journal writes it.
pub fn line(change: &Change) -> String {
    encode(change)
}

fn encode(change: &Change) -> String {
    let terms = match change {
        Change::Class { id, name, kind } => {
            let kind = match kind {
                ClassKind::Class => word("class"),
                ClassKind::Embryo => word("embryo"),
                ClassKind::Prototype(e) => {
                    let mut items = vec![word("prototype")];
                    items.extend(e.iter().map(word));
                    list(items)
                }
                ClassKind::Structure(p) => list(vec![word("structure"), word(p)]),
            };
            vec![word("class"), word(id), text(name), kind]
        }
        Change::Object { id, class, name } => {
            vec![word("object"), word(id), word(class), text(name)]
        }
        Change::Attribute { id, class, name } => {
            vec![word("attribute"), word(id), word(class), text(name)]
        }
        Change::ClassLink {
            id,
            reverse,
            from,
            relation,
            to,
            reverse_relation,
        } => vec![
            word("classlink"),
            word(id),
            word(reverse),
            word(from),
            text(relation),
            word(to),
            optional(reverse_relation.as_deref().map(text)),
        ],
        Change::ObjectLink {
            id,
            reverse,
            class_link,
            from,
            to,
            probability,
        } => vec![
            word("objectlink"),
            word(id),
            word(reverse),
            word(class_link),
            word(from),
            word(to),
            optional(probability.map(float)),
        ],
        Change::Unlink { id } => vec![word("unlink"), word(id)],
        Change::Set {
            object,
            attribute,
            value: v,
        } => vec![word("set"), word(object), word(attribute), value(v)],
        Change::Specify {
            target,
            spec,
            unit,
            computed,
        } => vec![
            word("specify"),
            adt_ref(*target),
            optional(spec.map(|s| text(&s.to_string()))),
            optional(unit.as_deref().map(text)),
            word(computed),
        ],
        Change::State { target, state } => {
            vec![word("state"), adt_ref(*target), state_word(*state)]
        }
        Change::Purge { target } => vec![word("purge"), adt_ref(*target)],
        Change::Include { class, hyper } => vec![word("include"), word(class), word(hyper)],
        Change::Exclude { class, hyper } => vec![word("exclude"), word(class), word(hyper)],
        Change::Assoc { a, b } => vec![word("assoc"), adt_ref(*a), adt_ref(*b)],
        Change::Dissoc { a, b } => vec![word("dissoc"), adt_ref(*a), adt_ref(*b)],
        Change::Modality {
            class_link,
            from,
            to,
            modality,
        } => vec![
            word("modality"),
            word(class_link),
            word(from),
            word(to),
            word(modality.map_or("may", |m| match m {
                Modality::Must => "must",
                Modality::Cannot => "cannot",
            })),
        ],
        Change::Property {
            target,
            name,
            language,
            text: t,
        } => vec![
            word("property"),
            adt_ref(*target),
            text(name),
            text(language),
            text(t),
        ],
        Change::Module { hash, text: t } => vec![word("module"), text(hash), text(t)],
        Change::Fibers { text: t } => vec![word("fibers"), text(t)],
        Change::Sequencing { text: t } => vec![word("sequencing"), text(t)],
    };
    let mut out = String::new();
    for (i, t) in terms.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        write_term(t, &mut out);
    }
    out
}

// Decoding

fn state(r: &mut Reader) -> Result<State, String> {
    Ok(match r.word()? {
        "active" => State::Active,
        "hibernating" => State::Hibernating,
        "destroyed" => State::Destroyed,
        w => return Err(format!("{w:?} is not a state")),
    })
}

fn decode(line: &str) -> Result<Change, String> {
    let terms = read_terms(line)?;
    let mut r = Reader::new(&terms);
    let change = match r.word()? {
        "class" => {
            let id = r.id()?;
            let name = r.text()?;
            let kind = match r.term()? {
                Term::Word(w) if w == "class" => ClassKind::Class,
                Term::Word(w) if w == "embryo" => ClassKind::Embryo,
                Term::List(items) => {
                    let mut k = Reader::new(items);
                    match k.word()? {
                        "prototype" => ClassKind::Prototype(k.rest(|k| k.id())?),
                        "structure" => ClassKind::Structure(k.id()?),
                        w => return Err(format!("{w:?} is not a kind of class")),
                    }
                }
                t => return Err(format!("{t:?} is not a kind of class")),
            };
            Change::Class { id, name, kind }
        }
        "object" => Change::Object {
            id: r.id()?,
            class: r.id()?,
            name: r.text()?,
        },
        "attribute" => Change::Attribute {
            id: r.id()?,
            class: r.id()?,
            name: r.text()?,
        },
        "classlink" => Change::ClassLink {
            id: r.id()?,
            reverse: r.id()?,
            from: r.id()?,
            relation: r.text()?,
            to: r.id()?,
            reverse_relation: r.optional(|r| r.text())?,
        },
        "objectlink" => Change::ObjectLink {
            id: r.id()?,
            reverse: r.id()?,
            class_link: r.id()?,
            from: r.id()?,
            to: r.id()?,
            probability: r.optional(|r| r.parse())?,
        },
        "unlink" => Change::Unlink { id: r.id()? },
        "set" => Change::Set {
            object: r.id()?,
            attribute: r.id()?,
            value: r.value()?,
        },
        "specify" => Change::Specify {
            target: r.adt_ref()?,
            spec: r.optional(|r| r.text()?.parse())?,
            unit: r.optional(|r| r.text())?,
            computed: r.parse()?,
        },
        "state" => Change::State {
            target: r.adt_ref()?,
            state: state(&mut r)?,
        },
        "purge" => Change::Purge {
            target: r.adt_ref()?,
        },
        "include" => Change::Include {
            class: r.id()?,
            hyper: r.id()?,
        },
        "exclude" => Change::Exclude {
            class: r.id()?,
            hyper: r.id()?,
        },
        "assoc" => Change::Assoc {
            a: r.adt_ref()?,
            b: r.adt_ref()?,
        },
        "dissoc" => Change::Dissoc {
            a: r.adt_ref()?,
            b: r.adt_ref()?,
        },
        "modality" => Change::Modality {
            class_link: r.parse()?,
            from: r.parse()?,
            to: r.parse()?,
            modality: match r.word()? {
                "may" => None,
                "must" => Some(Modality::Must),
                "cannot" => Some(Modality::Cannot),
                w => return Err(format!("no modality {w}")),
            },
        },
        "property" => Change::Property {
            target: r.adt_ref()?,
            name: r.text()?,
            language: r.text()?,
            text: r.text()?,
        },
        "module" => Change::Module {
            hash: r.text()?,
            text: r.text()?,
        },
        "fibers" => Change::Fibers { text: r.text()? },
        "sequencing" => Change::Sequencing { text: r.text()? },
        w => return Err(format!("{w:?} is not a change")),
    };
    r.end()?;
    Ok(change)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hbds_core::AdtRef;
    use hbds_core::value::{Array, Fz, List, Rank, Scalar, Value};

    fn values() -> Vec<Value> {
        let fz = |x| Fz::exact(x);
        vec![
            Value::integer(-42),
            Value::exact(Scalar::Rational(-3, 4)),
            Value::exact(Scalar::Float(0.1)),
            Value::exact(Scalar::Float(-0.0)),
            Value::exact(Scalar::Float(1e300)),
            Value::exact(Scalar::Gauss(1, -2)),
            Value::exact(Scalar::Complex(1.5, -2.25)),
            Value::exact(Scalar::Quaternion(Box::new([1.0, 2.0, -3.0, 4.5]))),
            Value::boolean(true),
            Value::exact(Scalar::Character('"')),
            Value::exact(Scalar::String("a \"quoted\" \\ line\nand (parens)".into())),
            Value::One(Fz::new(
                Scalar::Float(25.3),
                Some(0.37),
                Some(0.73),
                Some("conform; unexpected".into()),
            )),
            Value::One(Fz::new(Scalar::Integer(1), None, Some(0.5), None)),
            Value::Many(vec![fz(Scalar::Integer(1)), fz(Scalar::Float(2.5))]),
            Value::List(List {
                items: vec![Value::integer(1), Value::Adt(AdtRef::ObjectAttribute(3, 4))].into(),
                cursor: 1,
                circular: true,
            }),
            Value::Array(Box::new(Array {
                rank: Rank::Matrix,
                lows: vec![0, -1],
                dims: vec![1, 2],
                cells: vec![Value::integer(1), Value::integer(2)].into(),
            })),
            Value::Adt(AdtRef::ClassLink(9)),
        ]
    }

    #[test]
    fn values_round_trip() {
        for v in values() {
            let change = Change::Set {
                object: 1,
                attribute: 2,
                value: v,
            };
            let line = encode(&change);
            assert_eq!(decode(&line), Ok(change), "{line}");
        }
    }

    /// Values of every kind, nested, at random: each round-trips.
    #[test]
    fn random_values_round_trip() {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        fn value(next: &mut dyn FnMut(u64) -> u64, depth: u32) -> Value {
            let float = |next: &mut dyn FnMut(u64) -> u64| {
                // Any finite float, signs and tiny and huge ones included.
                let x = f64::from_bits(next(u64::MAX));
                if x.is_finite() {
                    x
                } else {
                    next(10_000) as f64 / 7.0
                }
            };
            let text = |next: &mut dyn FnMut(u64) -> u64| {
                let pool = ["", "a", "\"", "\\", "(x)", "line\nbreak", "é ü"];
                pool[next(pool.len() as u64) as usize].to_string()
            };
            let scalar = |next: &mut dyn FnMut(u64) -> u64| match next(9) {
                0 => Scalar::Integer(next(u64::MAX) as i64),
                1 => Scalar::Rational(next(1000) as i64 - 500, next(1000) as i64 + 1),
                2 => Scalar::Float(float(next)),
                3 => Scalar::Gauss(next(100) as i64 - 50, next(100) as i64 - 50),
                4 => Scalar::Complex(float(next), float(next)),
                5 => Scalar::Quaternion(Box::new([
                    float(next),
                    float(next),
                    float(next),
                    float(next),
                ])),
                6 => Scalar::Boolean(next(2) == 0),
                7 => Scalar::Character(['a', '"', '\\', ' '][next(4) as usize]),
                _ => Scalar::String(text(next).into()),
            };
            let fz = |next: &mut dyn FnMut(u64) -> u64| {
                Fz::new(
                    scalar(next),
                    (next(3) == 0).then(|| float(next)),
                    (next(3) == 0).then(|| next(100) as f64 / 100.0),
                    (next(3) == 0).then(|| text(next)),
                )
            };
            match next(if depth == 0 { 3 } else { 6 }) {
                0 | 1 => Value::One(fz(next)),
                2 => Value::Adt(AdtRef::ObjectAttribute(next(99), next(99))),
                3 => Value::Many((0..next(3)).map(|_| fz(next)).collect()),
                4 => Value::List(List {
                    items: (0..next(4))
                        .map(|_| value(next, depth - 1))
                        .collect::<Vec<_>>()
                        .into(),
                    cursor: next(4) as usize,
                    circular: next(2) == 0,
                }),
                _ => Value::Array(Box::new(Array {
                    rank: Rank::Vector,
                    lows: vec![next(5) as i64 - 2],
                    dims: vec![2],
                    cells: (0..2)
                        .map(|_| value(next, depth - 1))
                        .collect::<Vec<_>>()
                        .into(),
                })),
            }
        }
        for _ in 0..2000 {
            let v = value(&mut next, 3);
            let change = Change::Set {
                object: 1,
                attribute: 2,
                value: v,
            };
            let line = encode(&change);
            assert_eq!(decode(&line), Ok(change), "{line}");
        }
    }

    #[test]
    fn changes_round_trip() {
        let changes = [
            Change::Class {
                id: 1,
                name: "TOWN".into(),
                kind: ClassKind::Class,
            },
            Change::Class {
                id: 2,
                name: "NET".into(),
                kind: ClassKind::Prototype(vec![3, 4]),
            },
            Change::Class {
                id: 5,
                name: "R1".into(),
                kind: ClassKind::Structure(2),
            },
            Change::ClassLink {
                id: 6,
                reverse: 7,
                from: 1,
                relation: "a b".into(),
                to: 1,
                reverse_relation: None,
            },
            Change::ObjectLink {
                id: 8,
                reverse: 9,
                class_link: 6,
                from: 10,
                to: 11,
                probability: Some(0.9),
            },
            Change::Specify {
                target: AdtRef::ObjectAttribute(1, 2),
                spec: Some("FLOAT APPR".parse().unwrap()),
                unit: None,
                computed: true,
            },
            Change::State {
                target: AdtRef::Object(3),
                state: State::Hibernating,
            },
            Change::Property {
                target: AdtRef::ClassAttribute(2),
                name: "COND".into(),
                language: "ADT'81".into(),
                text: "THIS . \"temp\" > 0.".into(),
            },
            Change::Module {
                hash: "a9993e36".into(),
                text: "acode 1\nlanguage \"ADT'81\"\n".into(),
            },
        ];
        for c in changes {
            let line = encode(&c);
            assert_eq!(decode(&line), Ok(c), "{line}");
        }
    }

    #[test]
    fn bad_lines() {
        assert!(decode("class 1").is_err());
        assert!(decode("class 1 \"A\" class extra").is_err());
        assert!(decode("set 1 2 (f").is_err());
        assert!(decode("teleport 1").is_err());
    }
}
