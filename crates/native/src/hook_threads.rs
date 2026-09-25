use std::{marker::PhantomData, rc::Rc};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES, GetLastError, HANDLE,
        INVALID_HANDLE_VALUE, STILL_ACTIVE,
    },
    System::{
        Diagnostics::{
            Debug::{CONTEXT, CONTEXT_CONTROL_AMD64, GetThreadContext},
            ToolHelp::{
                CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                Thread32Next,
            },
        },
        Threading::{
            GetCurrentProcessId, GetCurrentThreadId, GetExitCodeThread, OpenThread, ResumeThread,
            SuspendThread, THREAD_GET_CONTEXT, THREAD_QUERY_LIMITED_INFORMATION,
            THREAD_SUSPEND_RESUME,
        },
    },
};

const THREAD_LIMIT: usize = 4096;

#[derive(Clone, Copy, Debug)]
pub enum ThreadError {
    Snapshot(u32),
    Open(u32),
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
        let mut frozen = Self {
            threads: Vec::with_capacity(THREAD_LIMIT),
            _current_thread: PhantomData,
        };
        let process = unsafe { GetCurrentProcessId() };
        let current = unsafe { GetCurrentThreadId() };
        for _ in 0..4 {
            let before = frozen.threads.len();
            let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(ThreadError::Snapshot(unsafe { GetLastError() }));
            }
            let result = frozen.freeze_snapshot(snapshot, process, current, target, length);
            unsafe {
                CloseHandle(snapshot);
            }
            result?;
            if frozen.threads.len() == before {
                return Ok(frozen);
            }
        }
        Err(ThreadError::Unstable)
    }

    fn freeze_snapshot(
        &mut self,
        snapshot: HANDLE,
        process: u32,
        current: u32,
        target: usize,
        length: usize,
    ) -> Result<(), ThreadError> {
        let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut found = unsafe { Thread32First(snapshot, &mut entry) };
        while found != 0 {
            if entry.th32OwnerProcessID == process
                && entry.th32ThreadID != current
                && !self
                    .threads
                    .iter()
                    .any(|thread| thread.id == entry.th32ThreadID)
            {
                self.freeze_one(entry.th32ThreadID, target, length)?;
            }
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            found = unsafe { Thread32Next(snapshot, &mut entry) };
        }
        let error = unsafe { GetLastError() };
        if error != ERROR_NO_MORE_FILES {
            return Err(ThreadError::Snapshot(error));
        }
        Ok(())
    }

    fn freeze_one(&mut self, id: u32, target: usize, length: usize) -> Result<(), ThreadError> {
        if self.threads.len() == THREAD_LIMIT {
            return Err(ThreadError::Limit);
        }
        let handle = unsafe {
            OpenThread(
                THREAD_GET_CONTEXT | THREAD_QUERY_LIMITED_INFORMATION | THREAD_SUSPEND_RESUME,
                0,
                id,
            )
        };
        if handle.is_null() {
            let error = unsafe { GetLastError() };
            return if error == ERROR_INVALID_PARAMETER {
                Ok(())
            } else {
                Err(ThreadError::Open(error))
            };
        }
        if unsafe { SuspendThread(handle) } == u32::MAX {
            let error = unsafe { GetLastError() };
            let terminated = exited(handle);
            unsafe {
                CloseHandle(handle);
            }
            return if terminated {
                Ok(())
            } else {
                Err(ThreadError::Suspend(error))
            };
        }
        self.threads.push(Suspended { id, handle });
        let mut context: CONTEXT = unsafe { std::mem::zeroed() };
        context.ContextFlags = CONTEXT_CONTROL_AMD64;
        if unsafe { GetThreadContext(handle, &mut context) } == 0 {
            return if exited(handle) {
                Ok(())
            } else {
                Err(ThreadError::Context(unsafe { GetLastError() }))
            };
        }
        if (target..target + length).contains(&(context.Rip as usize)) {
            return Err(ThreadError::Busy(id));
        }
        Ok(())
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
