use std::{marker::PhantomData, rc::Rc};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, HANDLE, STILL_ACTIVE},
    System::{
        Diagnostics::Debug::{CONTEXT, CONTEXT_CONTROL_AMD64, GetThreadContext},
        LibraryLoader::{GetModuleHandleW, GetProcAddress},
        Threading::{
            GetCurrentProcess, GetCurrentThreadId, GetExitCodeThread, GetThreadId, ResumeThread,
            SuspendThread, THREAD_GET_CONTEXT, THREAD_QUERY_LIMITED_INFORMATION,
            THREAD_SUSPEND_RESUME,
        },
    },
};

const THREAD_LIMIT: usize = 4096;
type NextThread = unsafe extern "system" fn(HANDLE, HANDLE, u32, u32, u32, *mut HANDLE) -> i32;

#[derive(Clone, Copy, Debug)]
pub enum ThreadError {
    Enumerate(u32),
    Suspend(u32),
    Context(u32),
    Resume(u32),
    Busy(u32),
    Limit,
    Unstable,
}
struct Suspended {
    id: u32,
    handle: HANDLE,
}
pub struct FrozenThreads {
    threads: Vec<Suspended>,
    _current_thread: PhantomData<Rc<()>>,
}
fn exited(handle: HANDLE) -> bool {
    let mut code = STILL_ACTIVE as u32;
    unsafe { GetExitCodeThread(handle, &mut code) != 0 && code != STILL_ACTIVE as u32 }
}
impl FrozenThreads {
    pub fn outside(target: usize, length: usize) -> Result<Self, ThreadError> {
        let module = unsafe { GetModuleHandleW(windows_sys::w!("ntdll.dll")) };
        let next = unsafe { GetProcAddress(module, c"NtGetNextThread".as_ptr().cast()) }
            .ok_or(ThreadError::Enumerate(unsafe { GetLastError() }))?;
        let next = unsafe {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, NextThread>(next)
        };
        let mut frozen = Self {
            threads: Vec::with_capacity(THREAD_LIMIT),
            _current_thread: PhantomData,
        };
        let current = unsafe { GetCurrentThreadId() };
        // Toolhelp allocates from the process heap. Once a thread is suspended it may own that heap's lock.
        for _ in 0..4 {
            let before = frozen.threads.len();
            frozen.freeze_pass(next, current, target, length)?;
            if frozen.threads.len() == before {
                return Ok(frozen);
            }
        }
        Err(ThreadError::Unstable)
    }
    fn freeze_pass(
        &mut self,
        next: NextThread,
        current: u32,
        target: usize,
        length: usize,
    ) -> Result<(), ThreadError> {
        let mut cursor = std::ptr::null_mut();
        let mut retained = false;
        loop {
            let mut handle = std::ptr::null_mut();
            let status = unsafe {
                next(
                    GetCurrentProcess(),
                    cursor,
                    THREAD_GET_CONTEXT | THREAD_QUERY_LIMITED_INFORMATION | THREAD_SUSPEND_RESUME,
                    0,
                    0,
                    &mut handle,
                )
            };
            if !cursor.is_null() && !retained {
                unsafe {
                    CloseHandle(cursor);
                }
            }
            if status as u32 == 0x8000001a {
                return Ok(());
            }
            if status != 0 {
                return Err(ThreadError::Enumerate(status as u32));
            }
            cursor = handle;
            retained = false;
            let id = unsafe { GetThreadId(handle) };
            if id == current || self.threads.iter().any(|thread| thread.id == id) {
                continue;
            }
            if self.threads.len() == THREAD_LIMIT {
                unsafe {
                    CloseHandle(handle);
                }
                return Err(ThreadError::Limit);
            }
            if unsafe { SuspendThread(handle) } == u32::MAX {
                let error = unsafe { GetLastError() };
                if exited(handle) {
                    continue;
                }
                unsafe {
                    CloseHandle(handle);
                }
                return Err(ThreadError::Suspend(error));
            }
            self.threads.push(Suspended { id, handle });
            retained = true;
            let mut context: CONTEXT = unsafe { std::mem::zeroed() };
            context.ContextFlags = CONTEXT_CONTROL_AMD64;
            if unsafe { GetThreadContext(handle, &mut context) } == 0 && !exited(handle) {
                return Err(ThreadError::Context(unsafe { GetLastError() }));
            }
            if (target..target + length).contains(&(context.Rip as usize)) {
                return Err(ThreadError::Busy(id));
            }
        }
    }
    pub fn resume(mut self) -> Result<(), ThreadError> {
        self.resume_all()
    }
    fn resume_all(&mut self) -> Result<(), ThreadError> {
        let mut failure = None;
        for thread in self.threads.iter_mut().rev() {
            if thread.handle.is_null() {
                continue;
            }
            if unsafe { ResumeThread(thread.handle) } == u32::MAX && !exited(thread.handle) {
                failure = Some(ThreadError::Resume(unsafe { GetLastError() }));
                continue;
            }
            unsafe {
                CloseHandle(thread.handle);
            }
            thread.handle = std::ptr::null_mut();
        }
        failure.map_or(Ok(()), Err)
    }
}
impl Drop for FrozenThreads {
    fn drop(&mut self) {
        let _ = self.resume_all();
    }
}
