//! The grouped journal and compaction (docs/architecture.md, decision 5):
//! what they write rebuilds the store; transactions that come fast share
//! a sync; a compaction keeps what came before and after it; a crash
//! loses whole transactions only.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hbds_core::AdtRef;
use hbds_core::value::Value;
use hbds_gtad::{ClassKind, Modality, State, Store};
use hbds_journal as journal;

fn directory(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hbds-journal-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

/// `n` transactions on `store`: an object each, its value set twice.
fn work(store: &mut Store, from: usize, n: usize) {
    let class = match store.find_class("TOWN") {
        Some(c) => c,
        None => store.create_class("TOWN", ClassKind::Class).unwrap(),
    };
    let a = match store.find_attribute(class, "size") {
        Some(a) => a,
        None => store.create_attribute(class, "size").unwrap(),
    };
    for i in from..from + n {
        store.begin();
        let o = store.create_object(class, &format!("t{i}")).unwrap();
        store.set(o, a, Value::integer(i as i64)).unwrap();
        store.set(o, a, Value::integer(2 * i as i64)).unwrap();
        store.end(true).unwrap();
    }
}

fn reopened(dir: &Path) -> String {
    let (store, warnings) = journal::open(dir).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    store.verify().unwrap();
    store.dump()
}

#[test]
fn grouped_transactions_share_syncs() {
    let dir = directory("grouped");
    let (mut store, durable, _) = journal::open_grouped(&dir, Duration::from_millis(5)).unwrap();
    work(&mut store, 0, 300);
    let s = durable.stats();
    durable.wait(s.submitted).unwrap();
    let s = durable.stats();
    assert_eq!((s.submitted, s.synced), (s.submitted, s.submitted));
    assert!(s.syncs * 4 < s.submitted, "{s:?}");
    let dump = store.dump();
    drop(store);
    // The journal's thread lets go of the lock once the store is gone.
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(reopened(&dir), dump);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_compaction_keeps_what_came_before_and_after() {
    let dir = directory("compaction");
    let (mut store, durable, _) = journal::open_grouped(&dir, Duration::ZERO).unwrap();
    work(&mut store, 0, 100);
    durable.wait(durable.stats().submitted).unwrap();
    let before = durable.stats().lines;
    durable.maybe_compact(&store, 10);
    // Queued behind the compaction: in the new journal.
    work(&mut store, 100, 20);
    durable.wait(durable.stats().submitted).unwrap();
    let s = durable.stats();
    assert_eq!(s.compactions, 1, "{s:?}");
    assert!(s.compacted < before, "{s:?}, {before} before");
    let dump = store.dump();
    drop(store);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(reopened(&dir), dump);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn compact_rewrites_a_store_shorter() {
    let dir = directory("compact");
    let (mut store, _) = journal::open(&dir).unwrap();
    work(&mut store, 0, 30);
    let gone = store
        .find_object(store.find_class("TOWN").unwrap(), "t3")
        .unwrap();
    store
        .change_state(AdtRef::Object(gone), State::Active, State::Destroyed)
        .unwrap();
    store.purge_adt(AdtRef::Object(gone)).unwrap();
    let sleeping = store
        .find_object(store.find_class("TOWN").unwrap(), "t4")
        .unwrap();
    store
        .change_state(AdtRef::Object(sleeping), State::Active, State::Hibernating)
        .unwrap();
    let dump = store.dump();
    drop(store);
    let (before, after) = journal::compact(&dir).unwrap();
    assert!(after * 3 < before * 2, "{before} lines, then {after}");
    assert_eq!(reopened(&dir), dump);
    assert!(dump.contains("t4 (hibernating)"), "{dump}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_uncommitted_transaction_is_dropped() {
    let dir = directory("uncommitted");
    let (mut store, _) = journal::open(&dir).unwrap();
    work(&mut store, 0, 2);
    let dump = store.dump();
    drop(store);
    // A crash in the middle of a transaction's write: its changes are
    // there, its commit is not.
    let path = dir.join("journal");
    let mut text = fs::read_to_string(&path).unwrap();
    text.push_str("object 99 1 \"half\"\n");
    fs::write(&path, &text).unwrap();
    let (store, warnings) = journal::open(&dir).unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("not committed"), "{warnings:?}");
    assert_eq!(store.dump(), dump);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn versions_older_read_newer_refused() {
    let dir = directory("versions");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("journal"),
        "class 1 \"TOWN\" class\nobject 2 1 \"Lyon\"\n",
    )
    .unwrap();
    let dump = reopened(&dir);
    assert!(dump.contains("object Lyon"), "{dump}");
    let text = fs::read_to_string(dir.join("journal")).unwrap();
    assert!(text.starts_with("journal 4\n"), "{text}");
    assert_eq!(reopened(&dir), dump);
    fs::write(dir.join("journal"), "journal 5\n").unwrap();
    let e = journal::open(&dir).err().expect("a newer journal").0;
    assert_eq!(e, "the journal's version 5 is newer than this reader's, 4");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn before_version_3_an_object_attributes_property_was_its_class_attributes() {
    let dir = directory("version-2-properties");
    let (mut store, _) = journal::open(&dir).unwrap();
    let town = store.create_class("TOWN", ClassKind::Class).unwrap();
    let size = store.create_attribute(town, "size").unwrap();
    let lyon = store.create_object(town, "Lyon").unwrap();
    store
        .set_property(
            AdtRef::ObjectAttribute(lyon, size),
            "ACTION",
            "ADT81",
            "PRINT 1",
        )
        .unwrap();
    drop(store);
    let own = reopened(&dir);
    let text = fs::read_to_string(dir.join("journal")).unwrap();
    fs::write(
        dir.join("journal"),
        text.replacen("journal 4", "journal 2", 1),
    )
    .unwrap();
    let older = reopened(&dir);
    assert_ne!(own, older);
    let (store, _) = journal::open(&dir).unwrap();
    assert!(
        store.props(AdtRef::ObjectAttribute(lyon, size)).is_none(),
        "{older}"
    );
    let class_attribute = store.props(AdtRef::ClassAttribute(size)).unwrap();
    assert!(class_attribute.properties.contains_key("ACTION"), "{older}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn modalities_are_journalled_and_refused_with_their_transaction() {
    let dir = directory("modalities");
    let (mut store, _) = journal::open(&dir).unwrap();
    let v = store.create_class("vehicle", ClassKind::Class).unwrap();
    store
        .create_class_link(v, "follow", v, Some("followed by"), false)
        .unwrap();
    let a = store.create_object(v, "a").unwrap();
    let b = store.create_object(v, "b").unwrap();
    let c = store.create_object(v, "c").unwrap();
    store.hold_to(b, "follow", a, Some(Modality::Must)).unwrap();
    store
        .hold_to(c, "follow", a, Some(Modality::Cannot))
        .unwrap();
    // A refused transaction takes its modality back.
    store.begin();
    store.hold_to(a, "follow", c, Some(Modality::Must)).unwrap();
    store.end(false).unwrap();
    let dump = store.dump();
    assert!(
        dump.ends_with(
            "b[vehicle] MUST \"follow\" a[vehicle]\nc[vehicle] CANNOT \"follow\" a[vehicle]\n"
        ),
        "{dump}"
    );
    assert!(store.assert_link(a, "followed by", c, None).is_err());
    drop(store);
    assert_eq!(reopened(&dir), dump);
    journal::compact(&dir).unwrap();
    assert_eq!(reopened(&dir), dump);
    let _ = fs::remove_dir_all(&dir);
}

/// The review of 2026-11-15's case: a hyperclass purged, the journal
/// compacted and reopened, and the next class took its id, and with it
/// the classes it had included.
#[test]
fn a_purged_id_is_not_given_again() {
    let dir = directory("purged-id");
    let (mut store, _) = journal::open(&dir).unwrap();
    let town = store.create_class("TOWN", ClassKind::Class).unwrap();
    let place = store.create_class("PLACE", ClassKind::Class).unwrap();
    let size = store.create_attribute(place, "size").unwrap();
    let paris = store.create_object(town, "Paris").unwrap();
    store.include(town, place).unwrap();
    store.set(paris, size, Value::integer(7)).unwrap();
    store
        .change_state(AdtRef::Class(place), State::Active, State::Destroyed)
        .unwrap();
    store.purge_adt(AdtRef::Class(place)).unwrap();
    // Nothing of it is left: not its members' tie, not their values.
    store.verify().unwrap();
    assert!(store.classes()[&town].hyper.is_empty());
    assert_eq!(store.get(paris, size), None);
    drop(store);
    journal::compact(&dir).unwrap();
    let (mut store, _) = journal::open(&dir).unwrap();
    let river = store.create_class("RIVER", ClassKind::Class).unwrap();
    assert!(river > size, "{river} was given before, up to {size}");
    assert!(store.classes()[&town].hyper.is_empty());
    store.verify().unwrap();
    let _ = fs::remove_dir_all(&dir);
}
