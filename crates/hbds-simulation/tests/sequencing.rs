//! The sequencing set and figure 52's transitions, legal and not.

use hbds_simulation::*;

fn d(a: Agent) -> String {
    match a {
        Agent::Main | Agent::Client(_) => "the main program".into(),
        Agent::Reflex(n) => format!("reflex {n}"),
        Agent::Process(id) => format!("p{id}"),
    }
}

const P: Agent = Agent::Process(1);
const Q: Agent = Agent::Process(2);
const R: Agent = Agent::Process(3);

fn names(s: &Sequencing) -> Vec<(f64, String)> {
    s.due().into_iter().map(|(t, a)| (t, d(a))).collect()
}

#[test]
fn first_in_first_out_among_equal_times() {
    let mut s = Sequencing::new();
    for p in [P, Q, R] {
        s.reactivate(p, When::Now, &d).unwrap();
    }
    s.hold(0.0).unwrap();
    let order: Vec<Agent> = std::iter::from_fn(|| s.advance()).collect();
    assert_eq!(order, [P, Q, R, Agent::Main]);
}

#[test]
fn hold_moves_time_on() {
    let mut s = Sequencing::new();
    s.reactivate(P, When::Now, &d).unwrap();
    s.hold(5.0).unwrap();
    assert_eq!(s.advance(), Some(P));
    assert_eq!(s.now(), 0.0);
    s.hold(2.5).unwrap();
    assert_eq!(s.state(P), State::Hold);
    assert_eq!(s.clock(P), 2.5);
    assert_eq!(s.advance(), Some(P));
    assert_eq!(s.now(), 2.5);
    assert_eq!(s.state(P), State::Active);
    assert_eq!(s.advance(), Some(Agent::Main));
    assert_eq!(s.now(), 5.0);
    assert_eq!(
        s.hold(-1.0).unwrap_err(),
        "HOLD for -1: a duration is not negative"
    );
}

#[test]
fn reactivate_at_before_and_after() {
    let mut s = Sequencing::new();
    s.reactivate(P, When::At(4.0), &d).unwrap();
    s.reactivate(Q, When::Before(P), &d).unwrap();
    s.reactivate(R, When::After(P), &d).unwrap();
    assert_eq!(
        names(&s),
        [(4.0, "p2".into()), (4.0, "p1".into()), (4.0, "p3".into())]
    );
    assert_eq!(s.clock(Q), 4.0);
    s.hold(10.0).unwrap();
    assert_eq!(s.advance(), Some(Q));
    let e = s
        .reactivate(Agent::Process(9), When::At(1.0), &d)
        .unwrap_err();
    assert_eq!(e, "1 is past: the time is 4");
    let e = s
        .reactivate(Agent::Process(9), When::Before(Q), &d)
        .unwrap_err();
    assert_eq!(e, "p2 is ACTIVE, not HOLD");
}

#[test]
fn cancel_delay_and_kill() {
    let mut s = Sequencing::new();
    s.reactivate(P, When::At(1.0), &d).unwrap();
    s.reactivate(Q, When::At(2.0), &d).unwrap();
    s.reactivate(R, When::At(3.0), &d).unwrap();
    s.cancel(Q, &d).unwrap();
    assert_eq!(s.state(Q), State::Passive);
    s.delay(P, 5.0, &d).unwrap();
    assert_eq!(names(&s), [(3.0, "p3".into()), (6.0, "p1".into())]);
    s.kill(R, &d).unwrap();
    s.kill(Q, &d).unwrap();
    assert_eq!(s.state(Q), State::Terminated);
    assert_eq!(names(&s), [(6.0, "p1".into())]);
    assert_eq!(s.cancel(Q, &d).unwrap_err(), "p2 is TERMINATED, not HOLD");
    assert_eq!(
        s.kill(R, &d).unwrap_err(),
        "p3 is TERMINATED, not HOLD or PASSIVE"
    );
    assert_eq!(
        s.reactivate(R, When::Now, &d).unwrap_err(),
        "p3 is TERMINATED, not PASSIVE"
    );
    assert_eq!(
        s.delay(P, -7.0, &d).unwrap_err(),
        "p1 would be due at -1, which is past"
    );
}

#[test]
fn passivate_and_terminate_the_running_process() {
    let mut s = Sequencing::new();
    assert!(
        s.passivate()
            .unwrap_err()
            .starts_with("the main program is not passivated")
    );
    assert!(
        s.terminate()
            .unwrap_err()
            .starts_with("the main program is not terminated")
    );
    s.reactivate(P, When::Now, &d).unwrap();
    s.reactivate(Q, When::Now, &d).unwrap();
    s.hold(1.0).unwrap();
    assert_eq!(s.advance(), Some(P));
    s.passivate().unwrap();
    assert_eq!(s.state(P), State::Passive);
    assert_eq!(s.advance(), Some(Q));
    assert_eq!(
        s.reactivate(Q, When::Now, &d).unwrap_err(),
        "p2 is ACTIVE, not PASSIVE"
    );
    s.terminate().unwrap();
    assert_eq!(s.state(Q), State::Terminated);
    // P, passive, waits for a REACTIVATE: the main program is next.
    assert_eq!(s.advance(), Some(Agent::Main));
    assert_eq!(s.advance(), None);
}

#[test]
fn processes_known_and_recorded() {
    let mut store = hbds_gtad::Store::memory();
    install(&mut store).unwrap();
    install(&mut store).unwrap();
    let process = store.find_class(PROCESS).unwrap();
    let pump = store
        .create_class("PUMP", hbds_gtad::ClassKind::Class)
        .unwrap();
    store.include(pump, process).unwrap();
    let p = store.create_object(pump, "p").unwrap();
    let town = store
        .create_class("TOWN", hbds_gtad::ClassKind::Class)
        .unwrap();
    let t = store.create_object(town, "t").unwrap();
    assert!(is_process(&store, p));
    assert!(!is_process(&store, t));
    record(&mut store, p, State::Hold, 7.5).unwrap();
    assert_eq!(stored(&store, p, STATE).unwrap().to_string(), "PASSIVE");
    assert_eq!(stored(&store, p, CLOCK).unwrap().to_string(), "7.5");
}

#[test]
fn a_set_saves_and_restores_without_its_clients() {
    let mut s = Sequencing::kernel();
    s.run_as(Agent::Client(7));
    s.reactivate(Agent::Process(3), When::At(2.5), &d).unwrap();
    s.reactivate(Agent::Process(4), When::Now, &d).unwrap();
    s.hold(1.0).unwrap();
    assert_eq!(s.advance(), Some(Agent::Process(4)));
    s.hold(4.0).unwrap();
    let text = s.save();
    let back = Sequencing::restore(&text).unwrap();
    assert_eq!(back.now(), s.now());
    // The client's agent is gone; the processes are due as they were.
    let due: Vec<_> = s
        .due()
        .into_iter()
        .filter(|(_, a)| !matches!(a, Agent::Client(_)))
        .collect();
    assert_eq!(back.due(), due);
    assert_eq!(back.clock(Agent::Process(3)), 2.5);
    assert_eq!(back.owner(Agent::Process(4)), None);
    assert!(!back.knows(Agent::Client(7)));
    assert_eq!(
        Sequencing::restore(&back.save()).unwrap().save(),
        back.save()
    );
}

#[test]
fn a_reflex_in_time_is_due_and_kept() {
    let mut s = Sequencing::new();
    let a = s.launch(7, 9, 2.5).unwrap();
    s.reactivate(Agent::Process(3), When::At(1.0), &d).unwrap();
    s.hold(5.0).unwrap();
    assert_eq!(s.advance(), Some(Agent::Process(3)));
    s.terminate().unwrap();
    assert_eq!(s.advance(), Some(a));
    assert_eq!(s.now(), 2.5);
    assert_eq!(s.reflex(1), Some((7, 9)));
    let back = Sequencing::restore(&s.save()).unwrap();
    assert_eq!(back.reflex(1), Some((7, 9)));
    assert_eq!(back.save(), s.save());
    assert!(s.launch(7, 9, -1.0).is_err());
    s.forget(a);
    assert_eq!(s.reflex(1), None);
}

/// `p` activated, as a statement does it: the set told first.
fn activate(s: &mut Sequencing, store: &mut hbds_gtad::Store, p: Agent) {
    s.changing(store);
    s.reactivate(p, When::Now, &d).unwrap();
}

#[test]
fn the_set_goes_back_with_the_store() {
    let mut store = hbds_gtad::Store::memory();
    let mut s = Sequencing::new();
    // Outside a transaction, and in one that is kept: it stays.
    activate(&mut s, &mut store, P);
    store.begin();
    activate(&mut s, &mut store, Q);
    store.end(true).unwrap();
    s.follow(&mut store);
    assert_eq!(names(&s).len(), 2);
    // Refused: what it did is undone, and only that.
    store.begin();
    activate(&mut s, &mut store, R);
    s.cancel(P, &d).unwrap();
    store.end(false).unwrap();
    s.follow(&mut store);
    assert_eq!(
        names(&s),
        [(0.0, "p1".to_string()), (0.0, "p2".to_string())]
    );
    // A transaction refused that did not touch the set leaves it alone.
    store.begin();
    store.end(false).unwrap();
    activate(&mut s, &mut store, R);
    assert_eq!(names(&s).len(), 3);
}

#[test]
fn a_rollback_takes_the_set_to_its_savepoint() {
    let mut store = hbds_gtad::Store::memory();
    let mut s = Sequencing::new();
    store.begin();
    activate(&mut s, &mut store, P);
    let point = store.savepoint().unwrap();
    activate(&mut s, &mut store, Q);
    let inner = store.savepoint().unwrap();
    activate(&mut s, &mut store, R);
    store.rollback(inner);
    s.follow(&mut store);
    assert_eq!(names(&s).len(), 2);
    store.rollback(point);
    // Not asked in between: the rollback is not lost.
    activate(&mut s, &mut store, R);
    assert_eq!(
        names(&s),
        [(0.0, "p1".to_string()), (0.0, "p3".to_string())]
    );
    // And the whole of it refused takes the rest.
    store.end(false).unwrap();
    s.follow(&mut store);
    assert_eq!(names(&s), []);
}

/// A launch has no name of its own: killed, nothing can act on it, and
/// it stayed in the set a daemon saves, for as long as the daemon lived.
#[test]
fn a_killed_launch_is_forgotten() {
    let mut s = Sequencing::new();
    let empty = s.save();
    let due = s.launch(7, 9, 2.5).unwrap();
    let canceled = s.launch(7, 9, 3.0).unwrap();
    s.cancel(canceled, &d).unwrap();
    s.kill(due, &d).unwrap();
    s.kill(canceled, &d).unwrap();
    assert_eq!(s.launches(7, &STATES), []);
    assert!(!s.knows(due) && !s.knows(canceled));
    assert_eq!(s.reflex(1), None);
    // But for the number the next launch takes, as if none had been.
    assert_eq!(s.save(), empty.replace(" 1", " 3"));
    // A process killed is still one, terminated.
    s.reactivate(P, When::Now, &d).unwrap();
    s.kill(P, &d).unwrap();
    assert_eq!(s.state(P), State::Terminated);
}

const STATES: [State; 4] = [
    State::Active,
    State::Hold,
    State::Passive,
    State::Terminated,
];

/// A canceled launch waits to be reactivated, by its reflex's name: its
/// reflex purged, nothing names it.
#[test]
fn a_canceled_launch_is_forgotten_with_its_reflex() {
    let mut store = hbds_gtad::Store::memory();
    let class = (store.create_class("watch", hbds_gtad::ClassKind::Class)).unwrap();
    let kept = store.create_object(class, "kept").unwrap();
    let gone = store.create_object(class, "gone").unwrap();
    let mut s = Sequencing::new();
    let launches = [kept, gone, gone].map(|r| s.launch(r, r, 2.5).unwrap());
    s.cancel(launches[0], &d).unwrap();
    s.cancel(launches[1], &d).unwrap();
    s.purged(&mut store);
    assert_eq!(s.launches(gone, &STATES).len(), 2);
    let target = hbds_core::AdtRef::Object(gone);
    (store.change_state(
        target,
        hbds_gtad::State::Active,
        hbds_gtad::State::Destroyed,
    ))
    .unwrap();
    store.purge_adt(target).unwrap();
    s.purged(&mut store);
    // The one still due is forgotten when its time comes.
    assert_eq!(s.launches(gone, &STATES), [launches[2]]);
    assert_eq!(s.launches(kept, &STATES), [launches[0]]);
}
