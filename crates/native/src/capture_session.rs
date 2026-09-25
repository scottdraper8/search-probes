use crate::capture::{self, Facts};
use std::{
    cell::Cell,
    marker::PhantomData,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

thread_local! {
    static ACTIVE: Cell<*const Session> = const { Cell::new(std::ptr::null()) };
}

struct Session {
    callback: unsafe fn(*mut u8, *const u8) -> Result<(), &'static str>,
    data: *mut u8,
    error: Cell<Option<&'static str>>,
    observed: Cell<usize>,
    busy: Cell<bool>,
}

struct Reset(PhantomData<Rc<()>>);

impl Drop for Reset {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.set(std::ptr::null()));
    }
}

struct ExitCallback<'a>(&'a Cell<bool>);

pub fn is_active() -> bool {
    ACTIVE.with(|active| !active.get().is_null())
}

impl Drop for ExitCallback<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// # Safety
/// The destructor hook supplies only this query's live planets, on the calling thread. The visitor
/// may read them until it returns, but must never retain their pointers or unwind through NMS.
pub unsafe fn visit<T, F>(mut visitor: F, operation: impl FnOnce() -> T) -> Result<T, &'static str>
where
    F: FnMut(*const u8) -> Result<(), &'static str>,
{
    unsafe fn dispatch<F: FnMut(*const u8) -> Result<(), &'static str>>(
        data: *mut u8,
        planet: *const u8,
    ) -> Result<(), &'static str> {
        unsafe { (&mut *data.cast::<F>())(planet) }
    }
    let session = Session {
        callback: dispatch::<F>,
        data: (&mut visitor as *mut F).cast(),
        error: Cell::new(None),
        observed: Cell::new(0),
        busy: Cell::new(false),
    };
    ACTIVE.with(|active| {
        if !active.get().is_null() {
            return Err("nested planet capture");
        }
        active.set(&session);
        Ok(())
    })?;
    let reset = Reset(PhantomData);
    let value = operation();
    drop(reset);
    session.error.get().map_or(Ok(value), Err)
}

/// # Safety
/// Generator resources remain valid and immutable throughout the query; the visit contract applies.
pub unsafe fn collect<T>(
    generator: *const u8,
    wanted: u32,
    operation: impl FnOnce() -> T,
) -> Result<(T, Vec<Facts>), &'static str> {
    if wanted & !capture::ALL != 0 {
        return Err("unknown capture requirement");
    }
    let mut facts = Vec::with_capacity(8);
    let result = unsafe {
        visit(
            |planet| {
                facts.push(capture::capture(planet, generator, wanted)?);
                Ok(())
            },
            operation,
        )
    }?;
    Ok((result, facts))
}

/// # Safety
/// During an active visit this is the query's live planet, before its destructor runs. Otherwise the
/// pointer is ignored. Recursive destructor observations fail without aliasing the active visitor.
pub unsafe fn observe(planet: *const u8) -> Result<bool, &'static str> {
    ACTIVE.with(|active| {
        let Some(session) = (unsafe { active.get().as_ref() }) else {
            return Ok(false);
        };
        if let Some(error) = session.error.get() {
            return Err(error);
        }
        let error = if session.busy.get() {
            Some("recursive planet destructor capture")
        } else if session.observed.get() == 8 {
            Some("more than eight planets captured in one query")
        } else {
            None
        };
        if let Some(error) = error {
            session.error.set(Some(error));
            return Err(error);
        }
        session.observed.set(session.observed.get() + 1);
        session.busy.set(true);
        let _exit = ExitCallback(&session.busy);
        let result = catch_unwind(AssertUnwindSafe(|| unsafe {
            (session.callback)(session.data, planet)
        }))
        .unwrap_or(Err("planet capture callback panicked"));
        match result {
            Ok(()) => Ok(true),
            Err(error) => {
                session.error.set(Some(error));
                Err(error)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_panic_is_caught_before_returning_through_native_code() {
        let mut native_returned = false;
        let result = unsafe {
            visit(
                |_| panic!("capture failure"),
                || {
                    assert_eq!(
                        observe(std::ptr::null()),
                        Err("planet capture callback panicked")
                    );
                    native_returned = true;
                },
            )
        };
        assert_eq!(result, Err("planet capture callback panicked"));
        assert!(native_returned);
        assert!(!is_active());
    }

    #[test]
    fn recursive_destructor_does_not_reenter_mutable_visitor() {
        let mut calls = 0;
        let result = unsafe {
            visit(
                |planet| {
                    calls += 1;
                    assert_eq!(observe(planet), Err("recursive planet destructor capture"));
                    Ok(())
                },
                || observe(std::ptr::null()),
            )
        };
        assert!(result.is_err());
        assert_eq!(calls, 1);
        assert_eq!(unsafe { observe(std::ptr::null()) }, Ok(false));
    }

    #[test]
    fn inactive_capture_ignores_pointer() {
        assert_eq!(unsafe { observe(std::ptr::null()) }, Ok(false));
    }

    #[test]
    fn sessions_are_bounded_and_reset_after_failure() {
        let result = unsafe {
            collect(std::ptr::null(), 0, || {
                for _ in 0..8 {
                    assert_eq!(observe(std::ptr::null()), Ok(true));
                }
                assert!(observe(std::ptr::null()).is_err());
            })
        };
        assert!(result.is_err());
        assert_eq!(unsafe { observe(std::ptr::null()) }, Ok(false));
    }

    #[test]
    fn nested_capture_cannot_replace_outer_session() {
        let (_, facts) = unsafe {
            collect(std::ptr::null(), 0, || {
                assert!(collect(std::ptr::null(), 0, || ()).is_err());
                assert_eq!(observe(std::ptr::null()), Ok(true));
            })
        }
        .unwrap();
        assert_eq!(facts.len(), 1);
    }

    #[test]
    fn concurrent_sessions_have_separate_capture_storage() {
        std::thread::scope(|scope| {
            let workers: Vec<_> = (1..=8)
                .map(|count| {
                    scope.spawn(move || unsafe {
                        let (_, facts) = collect(std::ptr::null(), 0, || {
                            for _ in 0..count {
                                observe(std::ptr::null()).unwrap();
                            }
                        })
                        .unwrap();
                        assert_eq!(facts.len(), count);
                    })
                })
                .collect();
            for worker in workers {
                worker.join().unwrap();
            }
        });
    }
}
