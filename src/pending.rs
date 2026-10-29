//! Single-result jobs; polling and dropping never wait for the worker.
use std::{
    io,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

pub(crate) struct Pending<T> {
    result: Receiver<Result<T, String>>,
    done: bool,
}

impl<T: Send + 'static> Pending<T> {
    pub(crate) fn start(
        name: &str,
        work: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> io::Result<Self> {
        let (tx, result) = mpsc::sync_channel(1);
        thread::Builder::new().name(name.into()).spawn(move || {
            let _ = tx.send(work());
        })?;
        Ok(Self {
            result,
            done: false,
        })
    }

    pub(crate) fn poll(&self, stopped: &str) -> Option<Result<T, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(stopped.into())),
        }
    }

    pub(crate) fn poll_once(&mut self, stopped: &str) -> Option<Result<T, String>> {
        if self.done {
            return None;
        }
        let result = self.poll(stopped)?;
        self.done = true;
        Some(result)
    }
}

#[cfg(test)]
pub(crate) mod tests;
