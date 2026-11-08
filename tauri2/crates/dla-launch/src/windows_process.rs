//! The game Locale Remulator started. LRProc starts it and ends at once, so
//! the launcher finds the process LRProc left behind and follows that one,
//! as it follows a game it started itself.

use std::{mem, path::Path};

use dla_application::launch::{LaunchError, LaunchProcessExit, ManagedLaunchProcess};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        Threading::{
            GetExitCodeProcess, INFINITE, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
        },
    },
};

pub struct GameProcess {
    handle: HANDLE,
    id: u32,
}

// SAFETY: a process handle is a kernel object reference usable from any thread.
unsafe impl Send for GameProcess {}

impl GameProcess {
    /// The process `parent` started, preferring one running `target`.
    pub fn started_by(parent: u32, target: &Path) -> Result<Self, LaunchError> {
        let file_name = target
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let id = child_of(parent, &file_name)
            .ok_or_else(|| LaunchError::adapter("Locale Remulator did not start the game"))?;
        // SAFETY: a plain Win32 call; the handle is checked and owned by the
        // returned value, which closes it in Drop.
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
                0,
                id,
            )
        };
        if handle.is_null() {
            return Err(LaunchError::adapter(format!(
                "could not follow the game Locale Remulator started: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(Self { handle, id })
    }

    fn exit(&self) -> Result<LaunchProcessExit, LaunchError> {
        let mut code = 0u32;
        // SAFETY: the handle is valid while `self` lives.
        if unsafe { GetExitCodeProcess(self.handle, &mut code) } == 0 {
            return Err(LaunchError::adapter(format!(
                "could not read how the game ended: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(LaunchProcessExit {
            exit_code: Some(code as i32),
        })
    }
}

impl ManagedLaunchProcess for GameProcess {
    fn process_id(&self) -> u32 {
        self.id
    }

    fn try_wait(&mut self) -> Result<Option<LaunchProcessExit>, LaunchError> {
        // SAFETY: the handle is valid while `self` lives.
        match unsafe { WaitForSingleObject(self.handle, 0) } {
            WAIT_OBJECT_0 => self.exit().map(Some),
            WAIT_TIMEOUT => Ok(None),
            _ => Err(LaunchError::adapter(format!(
                "could not monitor process: {}",
                std::io::Error::last_os_error()
            ))),
        }
    }

    fn terminate(&mut self) -> Result<(), LaunchError> {
        if self.try_wait()?.is_some() {
            return Ok(());
        }
        // SAFETY: the handle is valid and was opened with PROCESS_TERMINATE.
        if unsafe { TerminateProcess(self.handle, 1) } == 0 {
            return Err(LaunchError::adapter(format!(
                "could not stop process: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(())
    }

    fn wait(&mut self) -> Result<LaunchProcessExit, LaunchError> {
        // SAFETY: the handle is valid and was opened with PROCESS_SYNCHRONIZE.
        unsafe { WaitForSingleObject(self.handle, INFINITE) };
        self.exit()
    }
}

impl Drop for GameProcess {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by this value and is closed once.
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

/// A process whose parent is `parent`: the one named `file_name` if there is
/// one, else any.
fn child_of(parent: u32, file_name: &str) -> Option<u32> {
    // SAFETY: the snapshot handle is checked and closed here; PROCESSENTRY32W
    // is plain data initialised with its size as required.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = mem::zeroed();
        entry.dwSize = mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = None;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more {
            if entry.th32ParentProcessID == parent {
                let length = entry
                    .szExeFile
                    .iter()
                    .position(|&unit| unit == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..length]);
                if found.is_none() || name.eq_ignore_ascii_case(file_name) {
                    found = Some(entry.th32ProcessID);
                }
            }
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        found
    }
}
