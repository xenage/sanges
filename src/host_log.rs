use std::collections::VecDeque;
use std::io::{self, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn emit(component: &str, message: impl AsRef<str>) {
    let timestamp_ms = now_ms();
    let mut stderr = io::stderr().lock();
    let mut wrote_line = false;
    for line in message.as_ref().lines() {
        let _ = writeln!(stderr, "[{timestamp_ms}] [{component}] {line}");
        wrote_line = true;
    }
    if !wrote_line {
        let _ = writeln!(stderr, "[{timestamp_ms}] [{component}]");
    }
    let _ = stderr.flush();
}

pub(crate) fn emit_file_excerpt(component: &str, label: &str, path: &Path, max_lines: usize) {
    match read_file_tail_lossy(path, max_lines) {
        Ok(text) if text.trim().is_empty() => emit(
            component,
            format!("{label} log is empty path={}", path.display()),
        ),
        Ok(text) => {
            emit(
                component,
                format!(
                    "{label} log excerpt path={} tail_lines={max_lines}",
                    path.display()
                ),
            );
            for line in text.lines() {
                emit(component, format!("{label} | {line}"));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => emit(
            component,
            format!("{label} log is missing path={}", path.display()),
        ),
        Err(error) => emit(
            component,
            format!(
                "failed to read {label} log path={} error={error}",
                path.display()
            ),
        ),
    }
}

pub(crate) fn read_file_lossy(path: &Path) -> io::Result<String> {
    std::fs::read(path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

pub(crate) fn read_file_tail_lossy(path: &Path, max_lines: usize) -> io::Result<String> {
    if max_lines == 0 {
        return Ok(String::new());
    }
    let text = read_file_lossy(path)?;
    let mut tail = VecDeque::with_capacity(max_lines);
    for line in text.lines() {
        if tail.len() == max_lines {
            tail.pop_front();
        }
        tail.push_back(line.to_string());
    }
    Ok(tail.into_iter().collect::<Vec<_>>().join("\n"))
}

fn now_ms() -> u128 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis(),
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::read_file_tail_lossy;

    #[test]
    fn reads_only_requested_tail_lines() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("daemon.log");
        fs::write(&path, "one\ntwo\nthree\nfour\n").expect("write");

        let tail = read_file_tail_lossy(&path, 2).expect("tail");

        assert_eq!(tail, "three\nfour");
    }

    #[test]
    fn returns_empty_tail_for_zero_lines() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("daemon.log");
        fs::write(&path, "one\ntwo\n").expect("write");

        let tail = read_file_tail_lossy(&path, 0).expect("tail");

        assert!(tail.is_empty());
    }
}
