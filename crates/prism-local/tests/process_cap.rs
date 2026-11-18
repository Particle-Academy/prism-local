#![deny(unsafe_code)]

#[allow(unsafe_code, dead_code)]
#[path = "../src/process_cap.rs"]
mod process_cap;

use std::io::Write;
use std::process::Command;

#[global_allocator]
static ALLOCATOR: process_cap::ExitOnAllocationFailure = process_cap::ExitOnAllocationFailure;

#[test]
fn process_cap_refuses_large_allocation_with_stable_status() {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "cap_probe_child", "--nocapture"])
        .env("PRISM_TEST_CAP_CHILD", "1")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("CAP_INSTALLED"));
    assert_eq!(output.status.code(), Some(process_cap::MEMORY_EXIT));
}

#[test]
fn cap_probe_child() {
    let Some(level) = std::env::var_os("PRISM_TEST_CAP_CHILD") else {
        return;
    };
    let limit = process_cap::limit_bytes(4).unwrap();
    let _guard = process_cap::install(limit).expect("install hard cap");
    let mut small = Vec::<u8>::new();
    small.try_reserve_exact(1024).unwrap();
    println!("CAP_INSTALLED");
    std::io::stdout().flush().unwrap();
    if level == "1" {
        let nested = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "cap_probe_child", "--nocapture"])
            .env("PRISM_TEST_CAP_CHILD", "2")
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&nested.stdout).contains("CAP_INSTALLED"));
        assert_eq!(nested.status.code(), Some(process_cap::MEMORY_EXIT));
    }
    let mut oversized = Vec::<u8>::new();
    let _ = std::hint::black_box(&mut oversized).try_reserve_exact(limit + 32 * 1024 * 1024);
    panic!("allocation above the installed limit unexpectedly returned");
}
