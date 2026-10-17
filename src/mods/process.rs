//! Own complete process trees, including native workers spawned outside the SDK.
use super::types::Result;
use tokio::process::Command;

pub fn configure(command: &mut Command) {
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    #[cfg(unix)]
    command.process_group(0);
}

#[cfg(windows)]
mod platform {
    use super::*;
    type Handle = isize;
    #[repr(C)]
    #[derive(Default)]
    struct Basic {
        process_time: i64,
        job_time: i64,
        flags: u32,
        min: usize,
        max: usize,
        active: u32,
        affinity: usize,
        priority: u32,
        scheduling: u32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct Io {
        read_ops: u64,
        write_ops: u64,
        other_ops: u64,
        read_bytes: u64,
        write_bytes: u64,
        other_bytes: u64,
    }
    #[repr(C)]
    #[derive(Default)]
    struct Extended {
        basic: Basic,
        io: Io,
        process_memory: usize,
        job_memory: usize,
        peak_process: usize,
        peak_job: usize,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateJobObjectW(attributes: *const std::ffi::c_void, name: *const u16) -> Handle;
        fn SetInformationJobObject(
            job: Handle,
            class: i32,
            data: *const std::ffi::c_void,
            length: u32,
        ) -> i32;
        fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
        fn OpenProcess(access: u32, inherit: i32, id: u32) -> Handle;
        fn TerminateJobObject(job: Handle, code: u32) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }
    pub struct ProcessGuard {
        job: Handle,
    }
    impl ProcessGuard {
        pub fn attach(pid: u32) -> Result<Self> {
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job == 0 {
                    return Err(std::io::Error::last_os_error().to_string());
                }
                let guard = Self { job };
                let mut info = Extended::default();
                info.basic.flags = 0x2000; // KILL_ON_JOB_CLOSE
                if SetInformationJobObject(
                    job,
                    9,
                    &info as *const _ as _,
                    std::mem::size_of::<Extended>() as u32,
                ) == 0
                {
                    return Err(std::io::Error::last_os_error().to_string());
                }
                let process = OpenProcess(0x0100 | 0x0001, 0, pid);
                if process == 0 {
                    return Err(std::io::Error::last_os_error().to_string());
                }
                let assigned = AssignProcessToJobObject(job, process);
                let error = std::io::Error::last_os_error();
                CloseHandle(process);
                if assigned == 0 {
                    return Err(error.to_string());
                }
                Ok(guard)
            }
        }
        pub fn terminate(&self) {
            unsafe {
                TerminateJobObject(self.job, 1);
            }
        }
    }
    impl Drop for ProcessGuard {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.job);
            }
        }
    }
}
#[cfg(unix)]
mod platform {
    use super::*;
    extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    pub struct ProcessGuard {
        pid: u32,
    }
    impl ProcessGuard {
        pub fn attach(pid: u32) -> Result<Self> {
            Ok(Self { pid })
        }
        pub fn terminate(&self) {
            unsafe {
                kill(-(self.pid as i32), 9);
            }
        }
    }
    impl Drop for ProcessGuard {
        fn drop(&mut self) {
            self.terminate();
        }
    }
}
pub use platform::ProcessGuard;
