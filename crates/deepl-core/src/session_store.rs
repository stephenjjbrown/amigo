//! Local credential storage for the DeepL OAuth session and client
//! registration.
//!
//! This prototype stores them in files under the user's application-support
//! directory with owner-only permissions (0600 on Unix). That keeps QA usable
//! across ad-hoc rebuilds, where the macOS Keychain would invalidate the item
//! every time the code signature changes. A release build should move this back
//! behind the Keychain (or another OS credential store) with a stable signing
//! identity.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use crate::error::ApiError;

const MCP_SESSION_FILE: &str = "mcp-session.json";
const MCP_CLIENT_FILE: &str = "mcp-client.json";

/// Directory under the platform's support folder. eframe derives its own
/// preferences directory from the application name, so the two must agree.
const APP_DIR: &str = "Amigo";

fn store_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library/Application Support")
                .join(APP_DIR);
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(base) = std::env::var_os("APPDATA") {
            return PathBuf::from(base).join(APP_DIR);
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(base) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(base).join(APP_DIR);
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".config").join(APP_DIR);
        }
    }

    std::env::temp_dir().join(APP_DIR)
}

fn ensure_dir() -> Result<PathBuf, ApiError> {
    let dir = store_dir();
    fs::create_dir_all(&dir).map_err(|error| {
        ApiError::storage(format!(
            "Could not create the session directory {}: {error}",
            dir.display()
        ))
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    }

    Ok(dir)
}

fn path_for(file_name: &str) -> Result<PathBuf, ApiError> {
    Ok(ensure_dir()?.join(file_name))
}

fn load_value(file_name: &str) -> Result<Option<String>, ApiError> {
    let path = path_for(file_name)?;

    match fs::read_to_string(&path) {
        Ok(value) if value.trim().is_empty() => Ok(None),
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ApiError::storage(format!(
            "Could not read the session file {}: {error}",
            path.display()
        ))),
    }
}

fn save_value(file_name: &str, value: &str) -> Result<(), ApiError> {
    let path = path_for(file_name)?;

    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(&path).map_err(|error| {
        ApiError::storage(format!(
            "Could not open the session file {}: {error}",
            path.display()
        ))
    })?;

    file.write_all(value.as_bytes()).map_err(|error| {
        ApiError::storage(format!(
            "Could not write the session file {}: {error}",
            path.display()
        ))
    })?;

    file.sync_all().map_err(|error| {
        ApiError::storage(format!(
            "Could not flush the session file {}: {error}",
            path.display()
        ))
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }

    Ok(())
}

fn delete_value(file_name: &str) -> Result<(), ApiError> {
    let path = path_for(file_name)?;

    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ApiError::storage(format!(
            "Could not remove the session file {}: {error}",
            path.display()
        ))),
    }
}

pub fn save_mcp_session(value: &str) -> Result<(), ApiError> {
    save_value(MCP_SESSION_FILE, value)
}

pub fn load_mcp_session() -> Result<Option<String>, ApiError> {
    load_value(MCP_SESSION_FILE)
}

pub fn delete_mcp_session() -> Result<(), ApiError> {
    delete_value(MCP_SESSION_FILE)
}

pub fn save_mcp_client(value: &str) -> Result<(), ApiError> {
    save_value(MCP_CLIENT_FILE, value)
}

pub fn load_mcp_client() -> Result<Option<String>, ApiError> {
    load_value(MCP_CLIENT_FILE)
}

pub fn delete_mcp_client() -> Result<(), ApiError> {
    delete_value(MCP_CLIENT_FILE)
}

/// Best-effort path for diagnostics.
pub fn session_file_path() -> PathBuf {
    store_dir().join(MCP_SESSION_FILE)
}
