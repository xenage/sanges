use crate::{Result, SandboxError};

pub struct TerminalMode {
    is_tty: bool,
    size: Option<(u16, u16)>,
}

#[cfg(unix)]
pub struct RawTerminalGuard {
    fd: std::os::fd::RawFd,
    original: libc::termios,
}

impl TerminalMode {
    pub fn capture() -> Result<Self> {
        #[cfg(unix)]
        {
            if let Some(size) = current_tty_size()? {
                return Ok(Self {
                    is_tty: true,
                    size: Some(size),
                });
            }
            let fd = libc::STDIN_FILENO;
            if unsafe { libc::isatty(fd) } != 1 {
                return Ok(Self {
                    is_tty: false,
                    size: None,
                });
            }
            Ok(Self {
                is_tty: true,
                size: current_size(fd)?,
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {
                is_tty: false,
                size: None,
            })
        }
    }

    pub fn is_tty(&self) -> bool {
        self.is_tty
    }

    pub fn size(&self) -> Option<(u16, u16)> {
        self.size
    }

    #[cfg(unix)]
    pub fn enter_raw_mode(&self) -> Result<Option<RawTerminalGuard>> {
        if !self.is_tty {
            return Ok(None);
        }
        let tty = match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
        {
            Ok(tty) => tty,
            Err(_) => return Ok(None),
        };
        let fd = std::os::fd::AsRawFd::as_raw_fd(&tty);
        let mut termios = unsafe { std::mem::zeroed::<libc::termios>() };
        if unsafe { libc::tcgetattr(fd, &mut termios) } != 0 {
            return Err(SandboxError::io(
                "reading host terminal mode",
                std::io::Error::last_os_error(),
            ));
        }
        let original = termios;
        unsafe { libc::cfmakeraw(&mut termios) };
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &termios) } != 0 {
            return Err(SandboxError::io(
                "enabling host raw terminal mode",
                std::io::Error::last_os_error(),
            ));
        }
        std::mem::forget(tty);
        Ok(Some(RawTerminalGuard { fd, original }))
    }
}

#[cfg(unix)]
impl Drop for RawTerminalGuard {
    fn drop(&mut self) {
        let _ = unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &self.original) };
        let _ = unsafe { libc::close(self.fd) };
    }
}

#[cfg(unix)]
fn current_tty_size() -> Result<Option<(u16, u16)>> {
    let tty = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
    {
        Ok(tty) => tty,
        Err(_) => return Ok(None),
    };
    current_size(std::os::fd::AsRawFd::as_raw_fd(&tty))
}

#[cfg(unix)]
fn current_size(fd: i32) -> Result<Option<(u16, u16)>> {
    let mut winsize = unsafe { std::mem::zeroed::<libc::winsize>() };
    if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut winsize) } != 0 {
        return Err(SandboxError::io(
            "reading host terminal size",
            std::io::Error::last_os_error(),
        ));
    }
    if winsize.ws_col == 0 || winsize.ws_row == 0 {
        return Ok(None);
    }
    Ok(Some((winsize.ws_col, winsize.ws_row)))
}
