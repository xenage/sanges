use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};

use tokio::fs::File as TokioFile;

use crate::{Result, SandboxError};

pub struct ShellPty {
    pub master_reader: TokioFile,
    pub master_writer: TokioFile,
    pub slave: File,
    pub slave_fd: RawFd,
}

pub fn open_shell_pty(cols: u16, rows: u16) -> Result<ShellPty> {
    let mut master = 0;
    let mut slave = 0;
    let winsize = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let result = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            &winsize,
        )
    };
    if result != 0 {
        return Err(SandboxError::io(
            "opening guest pty",
            std::io::Error::last_os_error(),
        ));
    }
    let master = unsafe { File::from_raw_fd(master) };
    let master_reader = master
        .try_clone()
        .map_err(|error| SandboxError::io("cloning guest pty master for reader", error))?;
    let slave = unsafe { File::from_raw_fd(slave) };
    let slave_fd = slave.as_raw_fd();
    Ok(ShellPty {
        master_reader: TokioFile::from_std(master_reader),
        master_writer: TokioFile::from_std(master),
        slave,
        slave_fd,
    })
}

pub fn resize_pty(master: &TokioFile, cols: u16, rows: u16) -> Result<()> {
    let winsize = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let result = unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &winsize) };
    if result == 0 {
        Ok(())
    } else {
        Err(SandboxError::io(
            "resizing guest pty",
            std::io::Error::last_os_error(),
        ))
    }
}
