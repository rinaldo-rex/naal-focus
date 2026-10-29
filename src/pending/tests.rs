use super::Pending;
use std::{
    sync::mpsc::{self, SyncSender},
    thread,
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(3);
const STOPPED: &str = "Test worker stopped.";

pub(crate) fn wait<T>(mut poll: impl FnMut() -> Option<Result<T, String>>) -> Result<T, String> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(result) = poll() {
            return result;
        }
        assert!(Instant::now() < deadline, "worker result timed out");
        thread::sleep(Duration::from_millis(5));
    }
}

fn channel<T>() -> (SyncSender<Result<T, String>>, Pending<T>) {
    let (tx, result) = mpsc::sync_channel(1);
    (
        tx,
        Pending {
            result,
            done: false,
        },
    )
}

#[test]
fn poll_once_returns_owned_success_or_failure_only_once() {
    for expected in [Ok(vec![1, 2, 3]), Err("Job failed.".into())] {
        let (tx, mut pending) = channel::<Vec<u8>>();
        assert_eq!(pending.poll_once(STOPPED), None);
        tx.send(expected.clone()).unwrap();
        assert_eq!(pending.poll_once(STOPPED), Some(expected));
        assert_eq!(pending.poll_once(STOPPED), None);
        drop(tx);
        assert_eq!(pending.poll_once(STOPPED), None);
    }
}

#[test]
fn poll_once_reports_a_disconnected_worker_only_once() {
    let (tx, mut pending) = channel::<()>();
    drop(tx);
    assert_eq!(pending.poll_once(STOPPED), Some(Err(STOPPED.into())));
    assert_eq!(pending.poll_once(STOPPED), None);
}

#[test]
fn raw_poll_preserves_repeated_disconnects_after_a_result() {
    for expected in [Ok(vec![4, 5]), Err("Read failed.".into())] {
        let (tx, pending) = channel::<Vec<u8>>();
        assert_eq!(pending.poll(STOPPED), None);
        tx.send(expected.clone()).unwrap();
        drop(tx);
        assert_eq!(pending.poll(STOPPED), Some(expected));
        assert_eq!(pending.poll(STOPPED), Some(Err(STOPPED.into())));
        assert_eq!(pending.poll(STOPPED), Some(Err(STOPPED.into())));
    }
}

#[test]
fn named_worker_starts_separately_and_stalled_work_does_not_block_polling() {
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (go_tx, go_rx) = mpsc::sync_channel(1);
    let mut pending = Pending::start("pending-name-test", move || {
        ready_tx.send(()).unwrap();
        go_rx.recv().unwrap();
        Ok(thread::current().name().unwrap().to_owned())
    })
    .unwrap();
    ready_rx.recv_timeout(TIMEOUT).unwrap();

    let (checked_tx, checked_rx) = mpsc::sync_channel(1);
    let checker = thread::spawn(move || {
        checked_tx.send(pending.poll_once(STOPPED)).unwrap();
        pending
    });
    let checked = checked_rx.recv_timeout(TIMEOUT);
    // Release work even if polling regresses, so the test can fail without hanging.
    go_tx.send(()).unwrap();
    let mut pending = checker.join().unwrap();
    assert_eq!(checked.unwrap(), None);
    assert_eq!(
        wait(|| pending.poll_once(STOPPED)),
        Ok("pending-name-test".into())
    );
    assert_eq!(pending.poll_once(STOPPED), None);
}

#[test]
fn a_panicked_worker_reports_the_configured_stopped_error() {
    let mut pending = Pending::<()>::start("pending-panic-test", || {
        panic!("simulated worker panic");
    })
    .unwrap();
    assert_eq!(wait(|| pending.poll_once(STOPPED)), Err(STOPPED.into()));
    assert_eq!(pending.poll_once(STOPPED), None);
}

#[test]
fn dropping_pending_neither_waits_for_nor_cancels_the_job() {
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (go_tx, go_rx) = mpsc::sync_channel(1);
    let (finished_tx, finished_rx) = mpsc::sync_channel(1);
    let pending = Pending::start("pending-drop-test", move || {
        ready_tx.send(()).unwrap();
        go_rx.recv().unwrap();
        finished_tx.send(()).unwrap();
        Ok(())
    })
    .unwrap();
    ready_rx.recv_timeout(TIMEOUT).unwrap();

    let (dropped_tx, dropped_rx) = mpsc::sync_channel(1);
    let dropper = thread::spawn(move || {
        drop(pending);
        dropped_tx.send(()).unwrap();
    });
    let dropped = dropped_rx.recv_timeout(TIMEOUT);
    go_tx.send(()).unwrap();
    let finished = finished_rx.recv_timeout(TIMEOUT);
    dropper.join().unwrap();
    assert!(dropped.is_ok(), "dropping pending waited for the job");
    finished.unwrap();
}
