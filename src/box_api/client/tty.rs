use std::io::Read;

use tokio::sync::mpsc;

pub(super) fn spawn_tty_stdin_reader() -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel(64);
    std::thread::spawn(move || {
        #[cfg(unix)]
        let mut input: Box<dyn Read + Send> =
            match std::fs::OpenOptions::new().read(true).open("/dev/tty") {
                Ok(tty) => Box::new(tty),
                Err(_) => Box::new(std::io::stdin()),
            };
        #[cfg(not(unix))]
        let mut input: Box<dyn Read + Send> = Box::new(std::io::stdin());
        let mut buffer = [0_u8; 1024];
        loop {
            match input.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => {
                    if tx.blocking_send(buffer[..size].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    rx
}
