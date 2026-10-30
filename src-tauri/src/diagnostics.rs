//! Best-effort local JSONL diagnostics. The UI/inference threads never wait on disk.
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, SyncSender},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_BYTES: u64 = 2 * 1024 * 1024;
const BACKUPS: usize = 3;
static LOGGER: OnceLock<Logger> = OnceLock::new();
struct Logger {
    sender: SyncSender<Message>,
    dropped: AtomicU64,
}
enum Message {
    Line(Vec<u8>),
    Flush(SyncSender<()>),
}

pub fn init() {
    LOGGER.get_or_init(|| {
        let (sender, receiver) = mpsc::sync_channel::<Message>(256);
        let directory = crate::settings::data_dir().join("logs");
        let _ = std::thread::Builder::new()
            .name("local-diagnostics".into())
            .spawn(move || {
                let mut writer = RollingLog::new(directory, MAX_BYTES, BACKUPS);
                while let Ok(message) = receiver.recv() {
                    match message {
                        Message::Line(line) => {
                            let _ = writer.write(&line);
                        }
                        Message::Flush(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            });
        Logger {
            sender,
            dropped: AtomicU64::new(0),
        }
    });
    event(
        "info",
        "app.start",
        json!({
            "version": env!("CARGO_PKG_VERSION"), "debug": cfg!(debug_assertions),
            "executable": std::env::current_exe().ok(),
            "logDirectory": crate::settings::data_dir().join("logs")
        }),
    );
}

pub fn event(level: &str, event: &str, fields: Value) {
    let Some(logger) = LOGGER.get() else {
        return;
    };
    let dropped = logger.dropped.swap(0, Ordering::Relaxed);
    let record = json!({
        "timeUnixMs": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
        "pid": std::process::id(), "level": level, "event": event,
        "droppedBefore": dropped, "fields": fields
    });
    if let Ok(mut line) = serde_json::to_vec(&record) {
        // Every caller passes bounded metadata, never screenshots or OCR/translation bodies.
        if line.len() > 16 * 1024 {
            logger
                .dropped
                .fetch_add(dropped.saturating_add(1), Ordering::Relaxed);
            return;
        }
        line.push(b'\n');
        if logger.sender.try_send(Message::Line(line)).is_err() {
            logger
                .dropped
                .fetch_add(dropped.saturating_add(1), Ordering::Relaxed);
        }
    }
}

pub fn flush() {
    if let Some(logger) = LOGGER.get() {
        let (sender, receiver) = mpsc::sync_channel(1);
        if logger.sender.try_send(Message::Flush(sender)).is_ok() {
            let _ = receiver.recv_timeout(Duration::from_millis(500));
        }
    }
}

pub fn short_error(error: impl std::fmt::Display) -> String {
    error.to_string().chars().take(1000).collect()
}

struct RollingLog {
    directory: PathBuf,
    limit: u64,
    backups: usize,
    file: Option<File>,
    bytes: u64,
}
impl RollingLog {
    fn new(directory: PathBuf, limit: u64, backups: usize) -> Self {
        Self {
            directory,
            limit,
            backups,
            file: None,
            bytes: 0,
        }
    }
    fn path(&self, index: usize) -> PathBuf {
        self.directory.join(if index == 0 {
            "gameocr.log".into()
        } else {
            format!("gameocr.{index}.log")
        })
    }
    fn write(&mut self, line: &[u8]) -> io::Result<()> {
        if self.file.is_none() {
            std::fs::create_dir_all(&self.directory)?;
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.path(0))?;
            self.bytes = file.metadata()?.len();
            self.file = Some(file);
        }
        if self.bytes > 0 && self.bytes.saturating_add(line.len() as u64) > self.limit {
            self.file = None; // Close before moving on Windows.
            remove_if_exists(&self.path(self.backups))?;
            for index in (0..self.backups).rev() {
                match std::fs::rename(self.path(index), self.path(index + 1)) {
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    result => result?,
                }
            }
            self.file = Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(self.path(0))?,
            );
            self.bytes = 0;
        }
        let result = self.file.as_mut().unwrap().write_all(line);
        if result.is_ok() {
            self.bytes += line.len() as u64;
        } else {
            self.file = None;
        }
        result
    }
}
fn remove_if_exists(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotation_bounds_storage_preserves_json_lines_and_appends_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = RollingLog::new(dir.path().into(), 128, 3);
        for index in 0..100 {
            log.write(format!("{{\"index\":{index}}}\n").as_bytes())
                .unwrap();
        }
        drop(log);
        let mut log = RollingLog::new(dir.path().into(), 128, 3);
        log.write(b"{\"index\":100}\n").unwrap();
        drop(log);
        let paths: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(paths.len(), 4);
        for path in paths {
            let path = path.unwrap().path();
            assert!(std::fs::metadata(&path).unwrap().len() <= 128);
            for line in std::fs::read_to_string(path).unwrap().lines() {
                serde_json::from_str::<Value>(line).unwrap();
            }
        }
        assert!(
            std::fs::read_to_string(dir.path().join("gameocr.log"))
                .unwrap()
                .contains("100")
        );
    }
    #[test]
    fn unavailable_directory_is_recoverable_and_errors_are_utf8_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs");
        std::fs::write(&path, "blocked").unwrap();
        let mut log = RollingLog::new(path.clone(), 128, 3);
        assert!(log.write(b"{}\n").is_err());
        std::fs::remove_file(path).unwrap();
        log.write(b"{}\n").unwrap();
        assert_eq!(short_error("译".repeat(2000)).chars().count(), 1000);
    }
}
