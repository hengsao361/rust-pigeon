use crate::config::AppConfig;
use serde::Serialize;
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const HISTORY_FILE_NAME: &str = "print-history.jsonl";
const UPLOAD_STATUS_FILE_NAME: &str = "upload-status.log";
const UPLOAD_URL_ENV: &str = "RUST_PIGEON_LOG_UPLOAD_URL";
const UPLOAD_TOKEN_ENV: &str = "RUST_PIGEON_LOG_UPLOAD_TOKEN";

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintAction {
    Automatic,
    Reprint,
    Test,
}

#[derive(Debug, Serialize)]
struct PrintLogEntry<'a> {
    timestamp_utc: String,
    timestamp_unix_ms: u128,
    ring_number: &'a str,
    printer_name: &'a str,
    action: PrintAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadOutcome {
    Disabled,
    NoLog,
    Uploaded,
}

pub fn directory() -> PathBuf {
    AppConfig::data_dir().join("logs")
}

pub fn history_path() -> PathBuf {
    directory().join(HISTORY_FILE_NAME)
}

pub fn append_success(
    ring_number: &str,
    printer_name: &str,
    action: PrintAction,
) -> io::Result<()> {
    append_success_at(&history_path(), ring_number, printer_name, action)
}

fn append_success_at(
    path: &Path,
    ring_number: &str,
    printer_name: &str,
    action: PrintAction,
) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let entry = PrintLogEntry {
        timestamp_utc: format_utc(duration.as_secs(), duration.subsec_millis()),
        timestamp_unix_ms: duration.as_millis(),
        ring_number,
        printer_name,
        action,
    };

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, &entry).map_err(io::Error::other)?;
    file.write_all(b"\n")
}

pub fn open_directory() -> Result<(), String> {
    let directory = directory();
    fs::create_dir_all(&directory).map_err(|error| format!("创建日志目录失败：{error}"))?;

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        Command::new("explorer.exe")
            .arg(&directory)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|error| format!("打开日志目录失败：{error}"))?;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(&directory)
            .spawn()
            .map_err(|error| format!("打开日志目录失败：{error}"))?;
        Ok(())
    }

    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(&directory)
            .spawn()
            .map_err(|error| format!("打开日志目录失败：{error}"))?;
        Ok(())
    }
}

pub fn upload_on_exit() -> Result<UploadOutcome, String> {
    let Ok(url) = std::env::var(UPLOAD_URL_ENV) else {
        return Ok(UploadOutcome::Disabled);
    };
    let url = url.trim();
    if url.is_empty() {
        return Ok(UploadOutcome::Disabled);
    }
    if !(url.starts_with("https://") || url.starts_with("http://")) || url.contains(['\r', '\n']) {
        return Err(format!("{UPLOAD_URL_ENV} 不是有效的 HTTP(S) 地址"));
    }

    let history = history_path();
    if !history.is_file() {
        return Ok(UploadOutcome::NoLog);
    }

    let token = std::env::var(UPLOAD_TOKEN_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty());
    if token
        .as_deref()
        .is_some_and(|value| value.contains(['\r', '\n']))
    {
        return Err(format!("{UPLOAD_TOKEN_ENV} 不能包含换行符"));
    }

    let result = upload_with_curl(&history, url, token.as_deref());
    let status = match &result {
        Ok(()) => "上传成功".to_owned(),
        Err(error) => format!("上传失败：{error}"),
    };
    let _ = append_upload_status(&status);
    result.map(|()| UploadOutcome::Uploaded)
}

fn upload_with_curl(path: &Path, url: &str, token: Option<&str>) -> Result<(), String> {
    let executable = if cfg!(windows) { "curl.exe" } else { "curl" };
    let mut command = Command::new(executable);
    command.args([
        "--silent",
        "--show-error",
        "--fail-with-body",
        "--connect-timeout",
        "3",
        "--max-time",
        "10",
        "--request",
        "POST",
        "--header",
        "Content-Type: application/x-ndjson",
        "--header",
        "X-Rust-Pigeon-Upload-Mode: full-snapshot",
        "--data-binary",
    ]);
    let mut file_argument = OsString::from("@");
    file_argument.push(path);
    command.arg(file_argument);
    if let Some(token) = token {
        command.args(["--header", &format!("Authorization: Bearer {token}")]);
    }
    command.arg(url);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let output = command
        .output()
        .map_err(|error| format!("无法启动 {executable}：{error}"))?;
    if output.status.success() {
        return Ok(());
    }

    let details = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(if details.is_empty() {
        format!("{executable} 退出码：{}", output.status)
    } else {
        details
    })
}

fn append_upload_status(status: &str) -> io::Result<()> {
    let directory = directory();
    fs::create_dir_all(&directory)?;
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join(UPLOAD_STATUS_FILE_NAME))?;
    writeln!(
        file,
        "{} {status}",
        format_utc(duration.as_secs(), duration.subsec_millis())
    )
}

fn format_utc(seconds: u64, millis: u32) -> String {
    let days = (seconds / 86_400) as i64;
    let seconds_in_day = seconds % 86_400;
    let hour = seconds_in_day / 3_600;
    let minute = seconds_in_day % 3_600 / 60;
    let second = seconds_in_day % 60;

    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_format_matches_unix_epoch() {
        assert_eq!(format_utc(0, 0), "1970-01-01T00:00:00.000Z");
        assert_eq!(format_utc(1_767_225_600, 123), "2026-01-01T00:00:00.123Z");
    }

    #[test]
    fn appends_machine_readable_print_history() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rust-pigeon-log-{unique}"));
        let path = root.join(HISTORY_FILE_NAME);

        append_success_at(&path, "2026-03-0498274", "GP-801CE", PrintAction::Automatic).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let value: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(value["ring_number"], "2026-03-0498274");
        assert_eq!(value["printer_name"], "GP-801CE");
        assert_eq!(value["action"], "automatic");
        assert!(
            value["timestamp_utc"]
                .as_str()
                .is_some_and(|value| value.ends_with('Z'))
        );

        let _ = fs::remove_dir_all(root);
    }
}
