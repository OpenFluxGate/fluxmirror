use crate::store::{Event, EventStore};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_millis(100);
const MAX_BATCH: usize = 100;

pub struct WriterHandle {
    pub thread: thread::JoinHandle<()>,
}

/// Spawn the background writer. Drains the channel until it disconnects
/// (i.e. the producer side dropped); also flushes any pending events on
/// exit so we never lose buffered data on shutdown.
pub fn spawn(mut store: EventStore, rx: Receiver<Event>) -> WriterHandle {
    let thread = thread::Builder::new()
        .name("event-writer".to_string())
        .spawn(move || run(&mut store, rx))
        .expect("spawn writer thread");
    WriterHandle { thread }
}

fn run(store: &mut EventStore, rx: Receiver<Event>) {
    eprintln!("[fluxmirror-proxy] event writer started");
    loop {
        match rx.recv_timeout(POLL_INTERVAL) {
            Ok(first) => {
                let mut batch: Vec<Event> = Vec::with_capacity(MAX_BATCH);
                batch.push(first);
                while batch.len() < MAX_BATCH {
                    match rx.try_recv() {
                        Ok(ev) => batch.push(ev),
                        Err(_) => break,
                    }
                }
                store.insert_batch(&batch);
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    // Final drain — any events still in flight after disconnect.
    let mut remainder: Vec<Event> = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        remainder.push(ev);
    }
    if !remainder.is_empty() {
        eprintln!(
            "[fluxmirror-proxy] flushing {} remaining events on shutdown",
            remainder.len()
        );
        store.insert_batch(&remainder);
    }
    eprintln!("[fluxmirror-proxy] event writer stopped");
}
