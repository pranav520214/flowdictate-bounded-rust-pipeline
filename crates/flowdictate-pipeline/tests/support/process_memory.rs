//! Coarse working-set measurements used only by manual Windows acceptance.
#![allow(unsafe_code)]
use std::ffi::c_void;

#[repr(C)]
#[derive(Default)]
struct Counters {
    size: u32,
    faults: u32,
    peak: usize,
    working_set: usize,
    paged_peak: usize,
    paged: usize,
    nonpaged_peak: usize,
    nonpaged: usize,
    pagefile: usize,
    pagefile_peak: usize,
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, id: u32) -> *mut c_void;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn GetProcessHandleCount(handle: *mut c_void, count: *mut u32) -> i32;
    fn CreateToolhelp32Snapshot(flags: u32, process: u32) -> *mut c_void;
    fn Thread32First(snapshot: *mut c_void, entry: *mut ThreadEntry) -> i32;
    fn Thread32Next(snapshot: *mut c_void, entry: *mut ThreadEntry) -> i32;
}

#[repr(C)]
#[derive(Default)]
struct ThreadEntry {
    size: u32,
    usage: u32,
    id: u32,
    process: u32,
    base_priority: i32,
    delta_priority: i32,
    flags: u32,
}

/// Test-only numeric snapshot; thread enumeration retains no unrelated IDs.
#[allow(dead_code)]
pub fn resources(id: u32) -> Result<(usize, u32, u32), &'static str> {
    let rss = working_set(id)?;
    // SAFETY: documented Win32 layouts, synchronous live pointers, query-only
    // rights, and exactly one close for each valid handle on all exit paths.
    unsafe {
        let process = OpenProcess(0x0400, 0, id);
        if process.is_null() {
            return Err("process query failed");
        }
        let mut handles = 0;
        let success = GetProcessHandleCount(process, &raw mut handles);
        CloseHandle(process);
        if success == 0 {
            return Err("handle query failed");
        }
        let snapshot = CreateToolhelp32Snapshot(4, 0);
        if snapshot as isize == -1 {
            return Err("thread snapshot failed");
        }
        let mut entry = ThreadEntry {
            size: 28,
            ..ThreadEntry::default()
        };
        let mut present = Thread32First(snapshot, &raw mut entry);
        let mut threads = 0;
        while present != 0 {
            threads += u32::from(entry.process == id);
            entry.size = 28;
            present = Thread32Next(snapshot, &raw mut entry);
        }
        CloseHandle(snapshot);
        if threads == 0 {
            return Err("thread enumeration failed");
        }
        Ok((rss, handles, threads))
    }
}
#[link(name = "psapi")]
unsafe extern "system" {
    fn GetProcessMemoryInfo(handle: *mut c_void, counters: *mut Counters, size: u32) -> i32;
}

pub fn working_set(id: u32) -> Result<usize, &'static str> {
    let size = u32::try_from(std::mem::size_of::<Counters>()).map_err(|_| "memory size")?;
    // SAFETY: only query/read rights; no process mutation. Counters matches
    // PROCESS_MEMORY_COUNTERS and remains live for the synchronous API call.
    // The successfully opened handle is closed exactly once before returning.
    unsafe {
        let handle = OpenProcess(0x0400 | 0x0010, 0, id);
        if handle.is_null() {
            return Err("memory query unavailable");
        }
        let mut counters = Counters {
            size,
            ..Counters::default()
        };
        let success = GetProcessMemoryInfo(handle, &raw mut counters, size);
        CloseHandle(handle);
        if success == 0 {
            Err("memory query failed")
        } else {
            Ok(counters.working_set)
        }
    }
}
