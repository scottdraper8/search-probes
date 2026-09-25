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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::HashSet,
        sync::{Mutex, atomic::AtomicUsize},
    };

    #[test]
    fn selected_workers_leave_the_rest_idle() {
        let pool = Workers::new(4).unwrap();
        let visited = AtomicUsize::new(0);
        pool.run_count(2, |index| {
            visited.fetch_or(1 << index, Ordering::Relaxed);
        })
        .unwrap();
        assert_eq!(visited.load(Ordering::Relaxed), 3);
        assert_eq!(pool.run_count(0, |_| {}), Err(WorkerError::InvalidCount));
        assert_eq!(pool.run_count(5, |_| {}), Err(WorkerError::InvalidCount));
    }

    #[test]
    fn disconnected_worker_still_joins_already_dispatched_borrows() {
        let mut pool = Workers::new(2).unwrap();
        pool.workers[1].commands.send(None).unwrap();
        pool.workers[1].thread.take().unwrap().join().unwrap();
        let completed = AtomicBool::new(false);
        assert_eq!(
            pool.run(|_| completed.store(true, Ordering::Release)),
            Err(WorkerError::Disconnected)
        );
        assert!(completed.load(Ordering::Acquire));
        assert!(!pool.active.load(Ordering::Acquire));
    }

    #[test]
    fn workers_reuse_threads_and_complete_borrowed_batches() {
        let pool = Workers::new(3).unwrap();
        let seen = Mutex::new(Vec::new());
        let values = [
            AtomicUsize::new(0),
            AtomicUsize::new(0),
            AtomicUsize::new(0),
        ];
        for _ in 0..100 {
            pool.run(|index| {
                values[index].fetch_add(1, Ordering::Relaxed);
                seen.lock().unwrap().push(thread::current().id());
            })
            .unwrap();
        }
        assert_eq!(seen.lock().unwrap().iter().collect::<HashSet<_>>().len(), 3);
        assert!(
            values
                .iter()
                .all(|value| value.load(Ordering::Relaxed) == 100)
        );
    }

    #[test]
    fn panicking_worker_completes_other_borrows_and_remains_usable() {
        let pool = Workers::new(2).unwrap();
        let completed = AtomicBool::new(false);
        assert_eq!(
            pool.run(|index| {
                if index == 0 {
                    panic!("worker failure");
                }
                completed.store(true, Ordering::Release);
            }),
            Err(WorkerError::Panicked)
        );
        assert!(completed.load(Ordering::Acquire));
        pool.run(|_| {}).unwrap();
    }

    #[test]
    fn recursive_pool_creation_is_not_a_recursive_dispatch() {
        Workers::new(1)
            .unwrap()
            .run(|_| {
                let inner = Workers::new(1).unwrap();
                assert_eq!(inner.run(|_| {}), Err(WorkerError::Nested));
            })
            .unwrap();
    }
}
