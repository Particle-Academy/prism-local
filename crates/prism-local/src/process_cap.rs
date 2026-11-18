//! Platform process limit. The job handle must live until process exit.
use std::alloc::{GlobalAlloc, Layout, System};

pub const MEMORY_EXIT: i32 = 125;
pub const HEADROOM_MIB: u64 = 256;
const MIB: u64 = 1024 * 1024;

pub fn limit_bytes(payload_mib: u64) -> Option<usize> {
    usize::try_from(payload_mib.checked_add(HEADROOM_MIB)?.checked_mul(MIB)?).ok()
}

/// Allocation failure must not use Rust's platform-dependent abort status.
pub struct ExitOnAllocationFailure;

// SAFETY: All operations delegate their original arguments to System. On a
// failed allocation we terminate without allocating, unwinding, or returning.
unsafe impl GlobalAlloc for ExitOnAllocationFailure {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: GlobalAlloc forwards the caller's valid layout unchanged.
        terminate_if_null(unsafe { System.alloc(layout) })
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: GlobalAlloc forwards the caller's valid layout unchanged.
        terminate_if_null(unsafe { System.alloc_zeroed(layout) })
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: GlobalAlloc forwards the caller's valid pointer and layouts.
        terminate_if_null(unsafe { System.realloc(ptr, layout, size) })
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: The pointer and layout came from this System-backed allocator.
        unsafe { System.dealloc(ptr, layout) }
    }
}

fn terminate_if_null(ptr: *mut u8) -> *mut u8 {
    if ptr.is_null() {
        platform::exit_memory();
    }
    ptr
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod platform {
    use super::MEMORY_EXIT;
    use std::ffi::c_int;

    const RLIMIT_AS: c_int = 9;
    #[repr(C)]
    struct Rlimit {
        current: u64,
        maximum: u64,
    }
    const _: () = assert!(std::mem::size_of::<Rlimit>() == 16);
    unsafe extern "C" {
        fn getrlimit(resource: c_int, limit: *mut Rlimit) -> c_int;
        fn setrlimit(resource: c_int, limit: *const Rlimit) -> c_int;
        fn _exit(status: c_int) -> !;
    }
    pub struct Guard;
    pub fn install(bytes: usize) -> Result<Guard, ()> {
        let requested = u64::try_from(bytes).map_err(|_| ())?;
        let mut old = Rlimit {
            current: 0,
            maximum: 0,
        };
        // SAFETY: old points to writable memory with the Linux x86-64 rlimit layout.
        if unsafe { getrlimit(RLIMIT_AS, &mut old) } != 0 {
            return Err(());
        }
        // Never weaken an inherited sandbox, and fail closed if it is stricter.
        if old.current < requested || old.maximum < requested {
            return Err(());
        }
        let new = Rlimit {
            current: requested,
            maximum: requested,
        };
        // SAFETY: new points to an initialized Linux x86-64 rlimit.
        if unsafe { setrlimit(RLIMIT_AS, &new) } != 0 {
            return Err(());
        }
        let mut actual = Rlimit {
            current: 0,
            maximum: 0,
        };
        // SAFETY: actual is a writable Linux x86-64 rlimit.
        if unsafe { getrlimit(RLIMIT_AS, &mut actual) } != 0
            || actual.current != requested
            || actual.maximum != requested
        {
            return Err(());
        }
        Ok(Guard)
    }
    pub fn exit_memory() -> ! {
        // SAFETY: _exit terminates immediately and does not touch Rust state.
        unsafe { _exit(MEMORY_EXIT) }
    }
}

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
mod platform {
    use super::MEMORY_EXIT;
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};

    type Handle = *mut c_void;
    const EXTENDED_LIMIT: i32 = 9;
    const PROCESS_MEMORY: u32 = 0x100;
    #[repr(C)]
    #[derive(Default)]
    struct Basic {
        process_time: i64,
        job_time: i64,
        flags: u32,
        min_working: usize,
        max_working: usize,
        active: u32,
        affinity: usize,
        priority: u32,
        scheduling: u32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct IoCounters {
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
        io: IoCounters,
        process_memory: usize,
        job_memory: usize,
        peak_process: usize,
        peak_job: usize,
    }
    // The Windows x64 ABI aligns SIZE_T fields to eight bytes.
    const _: () = assert!(std::mem::size_of::<Basic>() == 64);
    const _: () = assert!(std::mem::size_of::<IoCounters>() == 48);
    const _: () = assert!(std::mem::size_of::<Extended>() == 144);
    const _: () = assert!(std::mem::offset_of!(Extended, process_memory) == 112);
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> Handle;
        fn SetInformationJobObject(
            job: Handle,
            class: i32,
            info: *const c_void,
            length: u32,
        ) -> i32;
        fn QueryInformationJobObject(
            job: Handle,
            class: i32,
            info: *mut c_void,
            length: u32,
            returned: *mut u32,
        ) -> i32;
        fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
        fn GetCurrentProcess() -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
        fn TerminateProcess(process: Handle, code: u32) -> i32;
        fn ExitProcess(code: u32) -> !;
    }
    pub struct Guard(Handle);
    impl Drop for Guard {
        fn drop(&mut self) {
            // SAFETY: Guard uniquely owns the valid job handle.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    pub fn install(bytes: usize) -> Result<Guard, ()> {
        // SAFETY: null attributes and name request a private unnamed job.
        let handle = unsafe { CreateJobObjectW(null(), null()) };
        if handle.is_null() {
            return Err(());
        }
        let guard = Guard(handle);
        let mut info = Extended::default();
        info.basic.flags = PROCESS_MEMORY;
        info.process_memory = bytes;
        let size = u32::try_from(std::mem::size_of::<Extended>()).map_err(|_| ())?;
        // SAFETY: info has the documented C layout and remains live for the call.
        if unsafe {
            SetInformationJobObject(handle, EXTENDED_LIMIT, (&raw const info).cast(), size)
        } == 0
        {
            return Err(());
        }
        // SAFETY: both handles are valid; assignment failure is handled closed.
        if unsafe { AssignProcessToJobObject(handle, GetCurrentProcess()) } == 0 {
            return Err(());
        }
        let mut actual = Extended::default();
        // SAFETY: actual is writable and sized to the requested information class.
        if unsafe {
            QueryInformationJobObject(
                handle,
                EXTENDED_LIMIT,
                (&raw mut actual).cast(),
                size,
                null_mut(),
            )
        } == 0
            || actual.basic.flags & PROCESS_MEMORY == 0
            || actual.process_memory != bytes
        {
            return Err(());
        }
        Ok(guard)
    }
    pub fn exit_memory() -> ! {
        // SAFETY: GetCurrentProcess returns a valid pseudo-handle. Terminating
        // ourselves avoids DLL-detach deadlocks with the watchdog thread. If
        // the system unexpectedly rejects it, ExitProcess is the last resort.
        unsafe {
            TerminateProcess(GetCurrentProcess(), MEMORY_EXIT as u32);
            ExitProcess(MEMORY_EXIT as u32)
        }
    }
}

#[cfg(not(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "windows", target_arch = "x86_64")
)))]
compile_error!("process memory limits currently support x86-64 Windows and Linux only");

pub use platform::install;
