use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
};

thread_local! { static IN_WORKER: Cell<bool> = const { Cell::new(false) }; }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerError {
    InvalidCount,
    Start,
    Busy,
    Nested,
    Disconnected,
    Panicked,
}

#[derive(Clone, Copy)]
struct Task {
    data: usize,
    invoke: unsafe fn(usize, usize),
}

struct Worker {
    commands: SyncSender<Option<Task>>,
    completed: Receiver<bool>,
    thread: Option<JoinHandle<()>>,
}

pub struct Workers {
    workers: Vec<Worker>,
    active: AtomicBool,
}

struct Batch<'a> {
    pool: &'a Workers,
    pending: usize,
    received: usize,
    error: Option<WorkerError>,
}

impl Batch<'_> {
    fn finish(&mut self) {
        while self.received < self.pending {
            match self.pool.workers[self.received].completed.recv() {
                Ok(true) => {}
                Ok(false) => {
                    self.error.get_or_insert(WorkerError::Panicked);
                }
                Err(_) => {
                    self.error.get_or_insert(WorkerError::Disconnected);
                }
            }
            self.received += 1;
        }
    }
}

impl Drop for Batch<'_> {
    fn drop(&mut self) {
        self.finish();
        self.pool.active.store(false, Ordering::Release);
    }
}

impl Workers {
    pub fn new(count: usize) -> Result<Self, WorkerError> {
        if !(1..=16).contains(&count) {
            return Err(WorkerError::InvalidCount);
        }
        let mut pool = Self {
            workers: Vec::with_capacity(count),
            active: AtomicBool::new(false),
        };
        for index in 0..count {
            let (commands, incoming) = mpsc::sync_channel::<Option<Task>>(1);
            let (completed, outgoing) = mpsc::sync_channel(1);
            let thread = thread::Builder::new()
                .name(format!("Search Probes {index}"))
                .spawn(move || {
                    IN_WORKER.set(true);
                    while let Ok(Some(task)) = incoming.recv() {
                        let ok = catch_unwind(AssertUnwindSafe(|| unsafe {
                            (task.invoke)(task.data, index)
                        }))
                        .is_ok();
                        if completed.send(ok).is_err() {
                            break;
                        }
                    }
                })
                .map_err(|_| WorkerError::Start)?;
            pool.workers.push(Worker {
                commands,
                completed: outgoing,
                thread: Some(thread),
            });
        }
        Ok(pool)
    }

    pub fn count(&self) -> usize {
        self.workers.len()
    }

    pub fn run<F: Fn(usize) + Sync>(&self, task: F) -> Result<(), WorkerError> {
        self.run_count(self.count(), task)
    }

    pub fn run_count<F: Fn(usize) + Sync>(&self, count: usize, task: F) -> Result<(), WorkerError> {
        if count == 0 || count > self.count() {
            return Err(WorkerError::InvalidCount);
        }
        if IN_WORKER.get() {
            return Err(WorkerError::Nested);
        }
        if self
            .active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(WorkerError::Busy);
        }
        unsafe fn invoke<F: Fn(usize) + Sync>(data: usize, index: usize) {
            unsafe { (&*(data as *const F))(index) };
        }
        let mut batch = Batch {
            pool: self,
            pending: 0,
            received: 0,
            error: None,
        };
        let dispatch = Task {
            data: (&task as *const F) as usize,
            invoke: invoke::<F>,
        };
        // Batch joins every dispatched borrow before task drops, including error and unwind paths.
        for worker in self.workers.iter().take(count) {
            if worker.commands.send(Some(dispatch)).is_err() {
                batch.error = Some(WorkerError::Disconnected);
                break;
            }
            batch.pending += 1;
        }
        batch.finish();
        batch.error.map_or(Ok(()), Err)
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        for worker in &self.workers {
            let _ = worker.commands.send(None);
        }
        for worker in &mut self.workers {
            if let Some(thread) = worker.thread.take() {
                let _ = thread.join();
            }
        }
    }
}
