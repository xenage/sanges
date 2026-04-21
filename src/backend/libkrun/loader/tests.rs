use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

use tempfile::tempdir;

use super::{KrunInitLog, call_optional_fd, init_log_once, pad_kernel_file_for_mmap};

static SUCCESS_CALLS: AtomicUsize = AtomicUsize::new(0);
static FAILURE_CALLS: AtomicUsize = AtomicUsize::new(0);

extern "C" fn fake_init_log_success(_: i32, _: u32, _: u32, _: u32) -> i32 {
    SUCCESS_CALLS.fetch_add(1, Ordering::SeqCst);
    0
}

extern "C" fn fake_init_log_failure(_: i32, _: u32, _: u32, _: u32) -> i32 {
    FAILURE_CALLS.fetch_add(1, Ordering::SeqCst);
    -7
}

#[test]
fn init_log_runs_only_once_after_success() {
    let state = std::sync::OnceLock::new();
    SUCCESS_CALLS.store(0, Ordering::SeqCst);

    init_log_once(&state, fake_init_log_success as KrunInitLog).expect("first init");
    init_log_once(&state, fake_init_log_success as KrunInitLog).expect("second init");

    assert_eq!(SUCCESS_CALLS.load(Ordering::SeqCst), 1);
}

#[test]
fn init_log_caches_failure_without_retrying() {
    let state = std::sync::OnceLock::new();
    FAILURE_CALLS.store(0, Ordering::SeqCst);

    let first = init_log_once(&state, fake_init_log_failure as KrunInitLog)
        .expect_err("first init should fail");
    let second = init_log_once(&state, fake_init_log_failure as KrunInitLog)
        .expect_err("second init should fail");

    assert_eq!(
        first.to_string(),
        "backend failure: krun_init_log failed with -7"
    );
    assert_eq!(
        second.to_string(),
        "backend failure: krun_init_log failed with -7"
    );
    assert_eq!(FAILURE_CALLS.load(Ordering::SeqCst), 1);
}

#[test]
fn optional_fd_treats_einval_as_absent_shutdown_fd() {
    assert_eq!(
        call_optional_fd(-libc::EINVAL, "krun_get_shutdown_eventfd").unwrap(),
        None
    );
}

#[test]
fn optional_fd_returns_descriptor_when_present() {
    assert_eq!(
        call_optional_fd(17, "krun_get_shutdown_eventfd").unwrap(),
        Some(17)
    );
}

#[test]
fn pad_kernel_file_for_mmap_extends_non_aligned_images() {
    let temp = tempdir().expect("tempdir");
    let kernel = temp.path().join("vmlinuz-virt");
    fs::write(&kernel, [0x4d_u8; 5]).expect("write kernel");

    let padded = pad_kernel_file_for_mmap(&kernel, temp.path(), 4).expect("pad kernel");

    assert_ne!(padded, kernel);
    assert_eq!(fs::metadata(&padded).expect("metadata").len(), 8);
    assert_eq!(&fs::read(&padded).expect("read padded")[..5], &[0x4d; 5]);
}

#[test]
fn pad_kernel_file_for_mmap_reuses_aligned_images() {
    let temp = tempdir().expect("tempdir");
    let kernel = temp.path().join("vmlinuz-virt");
    fs::write(&kernel, [0x4d_u8; 8]).expect("write kernel");

    let padded = pad_kernel_file_for_mmap(&kernel, temp.path(), 4).expect("pad kernel");

    assert_eq!(padded, kernel);
}
