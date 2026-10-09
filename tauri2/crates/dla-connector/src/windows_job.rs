//! Ties `cloudflared` to the app on Windows. The connector starts suspended,
//! joins a job object that kills its members when the last handle closes,
//! and only then runs, so even a crash of the app can never leave it behind.

use std::{io, mem, os::windows::io::AsRawHandle, process::Child, ptr};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
        },
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        },
        Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME},
    },
};

/// Passed with `CREATE_NO_WINDOW` when spawning the connector.
pub const CREATE_SUSPENDED: u32 = 0x0000_0004;

/// A job object whose members die with it. The app holds the only handle.
pub struct Job(HANDLE);

// SAFETY: a job handle is a kernel object reference usable from any thread.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    pub fn new() -> io::Result<Self> {
        // SAFETY: plain Win32 calls with valid arguments; the handle is owned
        // by the returned value and closed in Drop.
        unsafe {
            let handle = CreateJobObjectW(ptr::null(), ptr::null());
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = Self(handle);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let set = SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                ptr::from_ref(&limits).cast(),
                mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if set == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }
    }
}

/// Lets a child spawned with `CREATE_SUSPENDED` run, after adding it to `job`
/// when there is one. Returns whether it joined; an error means it is still
/// suspended and should be killed.
pub fn release(job: Option<&Job>, child: &Child) -> io::Result<bool> {
    // SAFETY: the child's process handle is valid while `child` lives.
    let joined = job.is_some_and(|job| unsafe {
        AssignProcessToJobObject(job.0, child.as_raw_handle() as HANDLE) != 0
    });
    resume(child.id())?;
    Ok(joined)
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle was created by this value and is closed once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// Resumes every thread of a process created suspended (only its main one).
fn resume(pid: u32) -> io::Result<()> {
    // SAFETY: the snapshot and thread handles are checked and closed here;
    // THREADENTRY32 is plain data initialised with its size as required.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut entry: THREADENTRY32 = mem::zeroed();
        entry.dwSize = mem::size_of::<THREADENTRY32>() as u32;
        let mut resumed = false;
        let mut more = Thread32First(snapshot, &mut entry) != 0;
        while more {
            if entry.th32OwnerProcessID == pid {
                let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                if !thread.is_null() {
                    resumed |= ResumeThread(thread) != u32::MAX;
                    CloseHandle(thread);
                }
            }
            more = Thread32Next(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        if resumed {
            Ok(())
        } else {
            Err(io::Error::other(
                "the private connector's thread could not be resumed",
            ))
        }
    }
}
