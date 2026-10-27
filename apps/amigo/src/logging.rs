use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const RECENT_LIMIT: usize = 500;
const MAX_FILE_BYTES: u64 = 1_000_000;

struct Logger {
    file: Mutex<Option<File>>,
    recent: Mutex<VecDeque<String>>,
    verbose: AtomicBool,
    path: PathBuf,
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        match metadata.level() {
            log::Level::Error | log::Level::Warn | log::Level::Info => true,
            log::Level::Debug => self.verbose.load(Ordering::Relaxed),
            log::Level::Trace => false,
        }
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let line = format!("[{}] {:<5} {}", timestamp(), record.level(), record.args());

        if let Ok(mut recent) = self.recent.lock() {
            if recent.len() >= RECENT_LIMIT {
                recent.pop_front();
            }
            recent.push_back(line.clone());
        }

        if let Ok(mut guard) = self.file.lock() {
            if let Some(file) = guard.as_mut() {
                let _ = writeln!(file, "{line}");
                let _ = file.flush();
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut guard) = self.file.lock() {
            if let Some(file) = guard.as_mut() {
                let _ = file.flush();
            }
        }
    }
}

impl Logger {
    fn new() -> Self {
        let path = default_log_path();

        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        if path
            .metadata()
            .map(|metadata| metadata.len() > MAX_FILE_BYTES)
            .unwrap_or(false)
        {
            let old = path.with_extension("log.old");
            let _ = std::fs::remove_file(&old);
            let _ = std::fs::rename(&path, &old);
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok();

        Self {
            file: Mutex::new(file),
            recent: Mutex::new(VecDeque::new()),
            verbose: AtomicBool::new(false),
            path,
        }
    }
}

pub fn init() {
    let logger = LOGGER.get_or_init(Logger::new);
    let _ = log::set_logger(logger);
    log::set_max_level(log::LevelFilter::Debug);

    log::info!("Amigo {} starting", env!("CARGO_PKG_VERSION"));
    log::info!("log file: {}", logger.path.display());
}

pub fn set_verbose(value: bool) {
    if let Some(logger) = LOGGER.get() {
        logger.verbose.store(value, Ordering::Relaxed);
    }
    log::info!(
        "verbose logging {}",
        if value { "enabled" } else { "disabled" }
    );
}

pub fn log_path() -> PathBuf {
    LOGGER
        .get()
        .map(|logger| logger.path.clone())
        .unwrap_or_else(default_log_path)
}

pub fn recent_log() -> String {
    LOGGER
        .get()
        .and_then(|logger| logger.recent.lock().ok())
        .map(|lines| lines.iter().cloned().collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
}

fn default_log_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join("Library/Logs/Amigo/amigo.log");
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(base) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(base).join("Amigo/logs/amigo.log");
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(base) = std::env::var_os("XDG_STATE_HOME") {
            return PathBuf::from(base).join("amigo/amigo.log");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".local/state/amigo/amigo.log");
        }
    }

    std::env::temp_dir().join("amigo.log")
}

fn timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);

    let days = seconds.div_euclid(86_400);
    let remainder = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);

    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}Z",
        remainder / 3_600,
        (remainder % 3_600) / 60,
        remainder % 60
    )
}

/// Howard Hinnant's days-to-civil-date algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = (z - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u32;
    let year = if month <= 2 { year + 1 } else { year };

    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_epoch_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(1), (1970, 1, 2));
    }
}
