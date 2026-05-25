use std::os::fd::{AsRawFd, OwnedFd};
use std::path::Path;

use crate::host_hardening;
use crate::{Result, SandboxError};

use super::sys::{
    bpf_jump, bpf_stmt, path_cstring, seccomp_audit_arch, syscall_ok, syscall_owned_fd,
};

const BLOCKED_SYSCALLS: &[u32] = &[
    libc::SYS_mount as u32,
    libc::SYS_umount2 as u32,
    libc::SYS_unshare as u32,
    libc::SYS_setns as u32,
    libc::SYS_execve as u32,
    libc::SYS_execveat as u32,
    libc::SYS_ptrace as u32,
    libc::SYS_bpf as u32,
    libc::SYS_perf_event_open as u32,
    libc::SYS_kexec_load as u32,
    libc::SYS_kexec_file_load as u32,
    libc::SYS_reboot as u32,
    libc::SYS_swapon as u32,
    libc::SYS_swapoff as u32,
    libc::SYS_init_module as u32,
    libc::SYS_finit_module as u32,
    libc::SYS_delete_module as u32,
    libc::SYS_open_by_handle_at as u32,
    libc::SYS_userfaultfd as u32,
];

pub(super) fn apply_landlock_policy() -> Result<()> {
    let abi = host_hardening::landlock_abi()?;
    let ruleset_attr = LandlockRulesetAttr {
        handled_access_fs: landlock_handled_access_fs(abi),
        handled_access_net: 0,
        scoped: 0,
    };
    let ruleset_fd = syscall_owned_fd(
        unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                &ruleset_attr as *const LandlockRulesetAttr,
                std::mem::size_of::<LandlockRulesetAttr>(),
                0,
            )
        },
        "creating Landlock ruleset",
    )?;
    add_landlock_rule(&ruleset_fd, Path::new("/"), landlock_runtime_access(abi))?;
    syscall_ok(
        unsafe {
            libc::syscall(libc::SYS_landlock_restrict_self, ruleset_fd.as_raw_fd(), 0)
                as libc::c_int
        },
        "restricting secure runner with Landlock",
    )
}

pub(super) fn apply_seccomp_filter() -> Result<()> {
    let filter = build_seccomp_filter();
    let mut program = libc::sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_ptr().cast_mut(),
    };
    let rc = unsafe {
        libc::prctl(
            libc::PR_SET_SECCOMP,
            libc::SECCOMP_MODE_FILTER as libc::c_ulong,
            &mut program as *mut libc::sock_fprog,
        )
    };
    if rc != 0 {
        return Err(SandboxError::io(
            "installing secure runner seccomp filter",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(())
}

fn add_landlock_rule(ruleset_fd: &OwnedFd, path: &Path, allowed_access: u64) -> Result<()> {
    let parent_fd = open_path_fd(path, &format!("opening Landlock path {}", path.display()))?;
    let rule = LandlockPathBeneathAttr {
        allowed_access,
        parent_fd: parent_fd.as_raw_fd(),
    };
    syscall_ok(
        unsafe {
            libc::syscall(
                libc::SYS_landlock_add_rule,
                ruleset_fd.as_raw_fd(),
                LANDLOCK_RULE_PATH_BENEATH,
                &rule as *const LandlockPathBeneathAttr,
                0,
            ) as libc::c_int
        },
        &format!("adding Landlock rule for {}", path.display()),
    )
}

fn open_path_fd(path: &Path, context: &str) -> Result<OwnedFd> {
    let path = path_cstring(path, context)?;
    syscall_owned_fd(
        unsafe {
            libc::open(
                path.as_ptr(),
                libc::O_PATH | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            ) as libc::c_long
        },
        context.to_owned(),
    )
}

fn landlock_handled_access_fs(abi: u32) -> u64 {
    let mut access = LANDLOCK_ACCESS_FS_EXECUTE
        | LANDLOCK_ACCESS_FS_WRITE_FILE
        | LANDLOCK_ACCESS_FS_READ_FILE
        | LANDLOCK_ACCESS_FS_READ_DIR
        | LANDLOCK_ACCESS_FS_REMOVE_DIR
        | LANDLOCK_ACCESS_FS_REMOVE_FILE
        | LANDLOCK_ACCESS_FS_MAKE_DIR
        | LANDLOCK_ACCESS_FS_MAKE_REG
        | LANDLOCK_ACCESS_FS_MAKE_SOCK
        | LANDLOCK_ACCESS_FS_MAKE_FIFO
        | LANDLOCK_ACCESS_FS_MAKE_SYM;
    if abi >= 2 {
        access |= LANDLOCK_ACCESS_FS_REFER;
    }
    if abi >= 3 {
        access |= LANDLOCK_ACCESS_FS_TRUNCATE;
    }
    access
}

fn landlock_runtime_access(abi: u32) -> u64 {
    landlock_handled_access_fs(abi)
}

fn build_seccomp_filter() -> Vec<libc::sock_filter> {
    let mut filter = vec![
        bpf_stmt(
            (libc::BPF_LD | libc::BPF_W | libc::BPF_ABS) as u16,
            SECCOMP_DATA_ARCH_OFFSET,
        ),
        bpf_jump(
            (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16,
            seccomp_audit_arch(),
            1,
            0,
        ),
        bpf_stmt(
            (libc::BPF_RET | libc::BPF_K) as u16,
            libc::SECCOMP_RET_KILL_PROCESS,
        ),
        bpf_stmt(
            (libc::BPF_LD | libc::BPF_W | libc::BPF_ABS) as u16,
            SECCOMP_DATA_NR_OFFSET,
        ),
    ];
    filter.extend(socket_family_filter());
    for syscall in BLOCKED_SYSCALLS {
        filter.push(bpf_jump(
            (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16,
            *syscall,
            0,
            1,
        ));
        filter.push(bpf_stmt(
            (libc::BPF_RET | libc::BPF_K) as u16,
            libc::SECCOMP_RET_ERRNO | libc::EPERM as u32,
        ));
    }
    filter.push(bpf_stmt(
        (libc::BPF_RET | libc::BPF_K) as u16,
        libc::SECCOMP_RET_ALLOW,
    ));
    filter
}

fn socket_family_filter() -> Vec<libc::sock_filter> {
    vec![
        bpf_jump(
            (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16,
            libc::SYS_socket as u32,
            0,
            4,
        ),
        bpf_stmt(
            (libc::BPF_LD | libc::BPF_W | libc::BPF_ABS) as u16,
            SECCOMP_DATA_ARG0_OFFSET,
        ),
        bpf_jump(
            (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16,
            libc::AF_UNIX as u32,
            1,
            0,
        ),
        bpf_stmt(
            (libc::BPF_RET | libc::BPF_K) as u16,
            libc::SECCOMP_RET_ERRNO | libc::EPERM as u32,
        ),
        bpf_stmt(
            (libc::BPF_RET | libc::BPF_K) as u16,
            libc::SECCOMP_RET_ALLOW,
        ),
    ]
}

#[repr(C)]
struct LandlockRulesetAttr {
    handled_access_fs: u64,
    handled_access_net: u64,
    scoped: u64,
}

#[repr(C, packed)]
struct LandlockPathBeneathAttr {
    allowed_access: u64,
    parent_fd: i32,
}

const LANDLOCK_RULE_PATH_BENEATH: libc::c_int = 1;
const LANDLOCK_ACCESS_FS_EXECUTE: u64 = 1 << 0;
const LANDLOCK_ACCESS_FS_WRITE_FILE: u64 = 1 << 1;
const LANDLOCK_ACCESS_FS_READ_FILE: u64 = 1 << 2;
const LANDLOCK_ACCESS_FS_READ_DIR: u64 = 1 << 3;
const LANDLOCK_ACCESS_FS_REMOVE_DIR: u64 = 1 << 4;
const LANDLOCK_ACCESS_FS_REMOVE_FILE: u64 = 1 << 5;
const LANDLOCK_ACCESS_FS_MAKE_DIR: u64 = 1 << 7;
const LANDLOCK_ACCESS_FS_MAKE_REG: u64 = 1 << 8;
const LANDLOCK_ACCESS_FS_MAKE_SOCK: u64 = 1 << 9;
const LANDLOCK_ACCESS_FS_MAKE_FIFO: u64 = 1 << 10;
const LANDLOCK_ACCESS_FS_MAKE_SYM: u64 = 1 << 12;
const LANDLOCK_ACCESS_FS_REFER: u64 = 1 << 13;
const LANDLOCK_ACCESS_FS_TRUNCATE: u64 = 1 << 14;
const SECCOMP_DATA_NR_OFFSET: u32 = 0;
const SECCOMP_DATA_ARCH_OFFSET: u32 = 4;
const SECCOMP_DATA_ARG0_OFFSET: u32 = 16;
