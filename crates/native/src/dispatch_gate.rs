//! Exclusive gameplay ownership without holding a mutex across native calls. Closure drains the
//! current owner before shutdown; stale admissions cannot enter a reopened generation.
use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
    rc::Rc,
    sync::{
        Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, ThreadId},
};
type Result<T> = std::result::Result<T, &'static str>;

struct State<T> {
    value: Option<T>,
    owner: Option<ThreadId>,
    poisoned: bool,
}
pub struct Gate<T> {
    state: Mutex<State<T>>,
    idle: Condvar,
    generation: AtomicU64,
}
pub struct Admission<'a, T> {
    gate: &'a Gate<T>,
    generation: u64,
}
/// Thread-bound exclusive value. The gate's short state lock is released before native work.
pub struct Owner<'a, T> {
    gate: &'a Gate<T>,
    value: Option<T>,
    _thread: PhantomData<Rc<()>>,
}
/// Dropping an incomplete closure leaves dispatch closed. Reopening is explicit after teardown.
pub struct Closed<'a, T> {
    gate: &'a Gate<T>,
    generation: u64,
}
impl<T> Gate<T> {
    pub fn new(value: T) -> Self {
        Self {
            state: Mutex::new(State {
                value: Some(value),
                owner: None,
                poisoned: false,
            }),
            idle: Condvar::new(),
            generation: AtomicU64::new(0),
        }
    }
    fn take<'a>(&'a self, state: &mut State<T>) -> Result<Owner<'a, T>> {
        let value = state.value.take().ok_or("runtime value is already owned")?;
        state.owner = Some(thread::current().id());
        Ok(Owner {
            gate: self,
            value: Some(value),
            _thread: PhantomData,
        })
    }
    /// Control/cleanup access remains available while dispatch is closed or a frame has failed.
    /// Same-thread reentry refuses instead of waiting on the value its caller already owns.
    pub fn lock(&self) -> Result<Owner<'_, T>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "runtime gate state poisoned")?;
        while let Some(owner) = state.owner {
            if owner == thread::current().id() {
                return Err("runtime control reentered its active owner");
            }
            state = self
                .idle
                .wait(state)
                .map_err(|_| "runtime gate state poisoned")?;
        }
        self.take(&mut state)
    }
    pub fn admission(&self) -> Option<Admission<'_, T>> {
        let generation = self.generation.load(Ordering::Acquire);
        (generation & 1 == 0).then_some(Admission {
            gate: self,
            generation,
        })
    }
    pub fn close(&self) -> Result<Closed<'_, T>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "runtime gate state poisoned")?;
        if state.owner == Some(thread::current().id()) {
            return Err("runtime shutdown reentered its active owner");
        }
        let old = self.generation.load(Ordering::Acquire);
        if old & 1 != 0 {
            return Err("gameplay dispatch is already closing");
        }
        let generation = old
            .checked_add(1)
            .ok_or("gameplay dispatch generation exhausted")?;
        self.generation
            .compare_exchange(old, generation, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "gameplay dispatch closure raced")?;
        // New admissions are closed before waiting. Existing native work and borrowed workers
        // finish before Owner restores the value and wakes us. No state mutex crosses teardown.
        while state.owner.is_some() {
            state = self
                .idle
                .wait(state)
                .map_err(|_| "runtime gate state poisoned")?;
        }
        Ok(Closed {
            gate: self,
            generation,
        })
    }
}
impl<'a, T> Admission<'a, T> {
    pub fn lock(self) -> Result<Option<Owner<'a, T>>> {
        let mut state = self
            .gate
            .state
            .lock()
            .map_err(|_| "runtime gate state poisoned")?;
        if self.gate.generation.load(Ordering::Acquire) != self.generation || state.owner.is_some()
        {
            return Ok(None);
        }
        if state.poisoned {
            return Err("runtime owner failed during dispatch");
        }
        self.gate.take(&mut state).map(Some)
    }
}
impl<T> Deref for Owner<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.value.as_ref().unwrap()
    }
}
impl<T> DerefMut for Owner<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.value.as_mut().unwrap()
    }
}
impl<T> Drop for Owner<'_, T> {
    fn drop(&mut self) {
        let mut state = self
            .gate
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.poisoned |= thread::panicking();
        state.value = self.value.take();
        state.owner = None;
        self.gate.idle.notify_all();
    }
}
impl<T> Closed<'_, T> {
    pub fn complete(self) -> Result<()> {
        let state = self
            .gate
            .state
            .lock()
            .map_err(|_| "runtime gate state poisoned")?;
        if state.owner.is_some() {
            return Err("runtime cleanup must finish before reopening dispatch");
        }
        let next = self
            .generation
            .checked_add(1)
            .ok_or("gameplay dispatch generation exhausted")?;
        self.gate
            .generation
            .compare_exchange(self.generation, next, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "gameplay dispatch closure ownership changed")?;
        Ok(())
    }
}
