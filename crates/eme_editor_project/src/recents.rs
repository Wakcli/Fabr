use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;

use eme_editor_core::HubState;
use ron::ser::{to_string_pretty, PrettyConfig};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::ProjectError;

const USER_FILE: &str = "user.ron";
const SESSION_FILE: &str = "session.ron";

/// Directory holding editor state files.
///
/// `EMBERRLY_CONFIG_DIR` wins over `XDG_CONFIG_HOME/emberrly`, which wins over
/// `$HOME/.config/emberrly`.
pub fn user_config_dir() -> PathBuf {
    if let Some(dir) = non_empty("EMBERRLY_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    if let Some(dir) = non_empty("XDG_CONFIG_HOME") {
        return PathBuf::from(dir).join("emberrly");
    }
    match non_empty("HOME") {
        Some(home) => PathBuf::from(home).join(".config").join("emberrly"),
        None => PathBuf::from(".config").join("emberrly"),
    }
}

/// Persisted layout and theme paths.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionState {
    /// Layout file last used.
    pub layout: Option<PathBuf>,
    /// Theme file last used.
    pub theme: Option<PathBuf>,
}

/// Writes the hub state to `user.ron` in [`user_config_dir`].
pub fn save_user_state(state: &HubState) -> Result<(), ProjectError> {
    save(&user_config_dir().join(USER_FILE), state)
}

/// Reads the hub state, returning an empty hub when missing or corrupt.
pub fn load_user_state() -> HubState {
    load(&user_config_dir().join(USER_FILE))
}

/// Writes the session state to `session.ron` in [`user_config_dir`].
pub fn save_session_state(state: &SessionState) -> Result<(), ProjectError> {
    save(&user_config_dir().join(SESSION_FILE), state)
}

/// Reads the session state, returning defaults when missing or corrupt.
pub fn load_session_state() -> SessionState {
    load(&user_config_dir().join(SESSION_FILE))
}

fn non_empty(key: &str) -> Option<OsString> {
    std::env::var_os(key).filter(|value| !value.is_empty())
}

fn save<T: Serialize>(path: &Path, value: &T) -> Result<(), ProjectError> {
    let dir = path.parent().ok_or_else(|| {
        ProjectError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "state path has no parent",
        ))
    })?;
    fs::create_dir_all(dir)?;
    let body = to_string_pretty(value, PrettyConfig::default())
        .map_err(|e| ProjectError::Parse(e.to_string()))?;

    let tmp = dir.join(tmp_name(path));
    fs::write(&tmp, body)?;
    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(err.into());
    }
    Ok(())
}

fn tmp_name(path: &Path) -> String {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("state");
    format!(".{name}.tmp-{}", process::id())
}

fn load<T: DeserializeOwned + Default>(path: &Path) -> T {
    let src = match fs::read_to_string(path) {
        Ok(src) => src,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return T::default(),
        Err(err) => {
            report(path, &err.to_string());
            return T::default();
        }
    };
    match ron::from_str(&src) {
        Ok(value) => value,
        Err(err) => {
            report(path, &err.to_string());
            T::default()
        }
    }
}

#[expect(
    clippy::print_stderr,
    reason = "corrupt editor state is reported once instead of failing"
)]
fn report(path: &Path, err: &str) {
    eprintln!(
        "ignoring unreadable editor state at {}: {err}",
        path.display()
    );
}
