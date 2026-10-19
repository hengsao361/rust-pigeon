use crate::{config::AppConfig, receipt::format_channel};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs::{self, OpenOptions},
    hash::{DefaultHasher, Hash, Hasher},
    io::{self, Write},
    path::{Path, PathBuf},
    process::{self, Command},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const HISTORY_FILE_PREFIX: &str = "print-history-";
const HISTORY_FILE_SUFFIX: &str = ".csv";
const CLIENT_ID_FILE_NAME: &str = "client-id.txt";
const UPLOAD_STATUS_FILE_NAME: &str = "upload-status.log";
const UPLOAD_STATE_FILE_NAME: &str = "upload-state.json";
const UPLOAD_URL_ENV: &str = "RUST_PIGEON_LOG_UPLOAD_URL";
const UPLOAD_TOKEN_ENV: &str = "RUST_PIGEON_LOG_UPLOAD_TOKEN";
const BUILT_IN_UPLOAD_URL: &str = "https://1329186100-0ogxl1fc2j.ap-nanjing.tencentscf.com";
const BUILT_IN_UPLOAD_TOKEN: &str = match option_env!("RUST_PIGEON_LOG_UPLOAD_TOKEN") {
    Some(value) => value,
    None => "",
};
const COS_UPLOAD_URL_PREFIX: &str = "https://pg-1329186100.cos.ap-nanjing.myqcloud.com/";

#[derive(Debug, Clone, Copy)]
pub enum PrintAction {
    Automatic,
    Reprint,
    Test,
}

impl PrintAction {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Reprint => "reprint",
            Self::Test => "test",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadOutcome {
    NoLog,
    UpToDate,
    Uploaded,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct UploadState {
    files: BTreeMap<String, FileFingerprint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct FileFingerprint {
    size_bytes: u64,
    modified_unix_ms: u128,
}

struct PendingUpload {
    path: PathBuf,
    file_name: String,
    fingerprint: FileFingerprint,
}

#[derive(Serialize)]
struct UploadUrlRequest<'a> {
    client_id: &'a str,
    file_name: &'a str,
}

#[derive(Deserialize)]
struct UploadUrlResponse {
    upload_url: String,
    method: String,
}

pub fn directory() -> PathBuf {
    AppConfig::data_dir().join("logs")
}

fn client_id() -> io::Result<String> {
    load_or_create_client_id_at(&AppConfig::data_dir())
}

fn load_or_create_client_id_at(app_directory: &Path) -> io::Result<String> {
    let path = app_directory.join(CLIENT_ID_FILE_NAME);
    if let Ok(value) = fs::read_to_string(&path) {
        let value = value.trim();
        if is_valid_client_id(value) {
            return Ok(value.to_owned());
        }
    }

    fs::create_dir_all(app_directory)?;
    let value = generate_client_id();
    fs::write(path, format!("{value}\n"))?;
    Ok(value)
}

fn generate_client_id() -> String {
    let mut hasher = DefaultHasher::new();
    for name in ["COMPUTERNAME", "HOSTNAME"] {
        env::var_os(name).hash(&mut hasher);
    }
    process::id().hash(&mut hasher);
    let marker = 0_u8;
    (&marker as *const u8 as usize).hash(&mut hasher);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let value = timestamp ^ (u128::from(hasher.finish()) << 64);
    format!("{value:032x}")
}

fn is_valid_client_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn append_success(ring_number: &str, channel: &str, action: PrintAction) -> io::Result<()> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    append_success_at(
        &directory(),
        duration,
        &client_id()?,
        ring_number,
        channel,
        action,
    )
}

fn append_success_at(
    directory: &Path,
    duration: Duration,
    client_id: &str,
    ring_number: &str,
    channel: &str,
    action: PrintAction,
) -> io::Result<()> {
    fs::create_dir_all(directory)?;
    let timestamp_shanghai = format_shanghai(duration.as_secs(), duration.subsec_millis());
    let channel = format_channel(channel);
    let path = history_path_at(directory, client_id, &timestamp_shanghai);

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    if file.metadata()?.len() == 0 {
        file.write_all(b"channel,timestamp_shanghai,ring_number,action\n")?;
    }
    writeln!(
        file,
        "{},{},{},{}",
        csv_field(&channel),
        csv_field(&timestamp_shanghai),
        csv_field(ring_number),
        csv_field(action.as_str())
    )
}

fn history_path_at(directory: &Path, client_id: &str, timestamp_shanghai: &str) -> PathBuf {
    let date = timestamp_shanghai.get(..10).unwrap_or("unknown-date");
    directory.join(format!(
        "{HISTORY_FILE_PREFIX}{client_id}-{date}{HISTORY_FILE_SUFFIX}"
    ))
}

fn daily_history_paths(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry
                .file_name()
                .to_str()
                .is_some_and(is_daily_history_file_name)
        {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

fn is_daily_history_file_name(name: &str) -> bool {
    history_file_identity(name).is_some()
}

fn history_file_identity(name: &str) -> Option<(&str, &str)> {
    let value = name
        .strip_prefix(HISTORY_FILE_PREFIX)
        .and_then(|value| value.strip_suffix(HISTORY_FILE_SUFFIX))?;
    let date_start = value.len().checked_sub(10)?;
    let (client_with_separator, date) = value.split_at(date_start);
    let client_id = client_with_separator.strip_suffix('-')?;
    let bytes = date.as_bytes();
    let valid_date = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit());
    (is_valid_client_id(client_id) && valid_date).then_some((client_id, date))
}

fn load_upload_state(directory: &Path) -> io::Result<UploadState> {
    match fs::read(directory.join(UPLOAD_STATE_FILE_NAME)) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_default()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(UploadState::default()),
        Err(error) => Err(error),
    }
}

fn save_upload_state(directory: &Path, state: &UploadState) -> io::Result<()> {
    fs::create_dir_all(directory)?;
    let bytes = serde_json::to_vec_pretty(state).map_err(io::Error::other)?;
    fs::write(directory.join(UPLOAD_STATE_FILE_NAME), bytes)
}

fn file_fingerprint(path: &Path) -> io::Result<FileFingerprint> {
    let metadata = fs::metadata(path)?;
    let modified_unix_ms = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    Ok(FileFingerprint {
        size_bytes: metadata.len(),
        modified_unix_ms,
    })
}

fn pending_uploads(histories: Vec<PathBuf>, state: &UploadState) -> io::Result<Vec<PendingUpload>> {
    let mut pending = Vec::new();
    for path in histories {
        let Some(file_name) = path
            .file_name()
            .and_then(|value| value.to_str())
            .map(str::to_owned)
        else {
            continue;
        };
        let fingerprint = file_fingerprint(&path)?;
        if state.files.get(&file_name) != Some(&fingerprint) {
            pending.push(PendingUpload {
                path,
                file_name,
                fingerprint,
            });
        }
    }
    Ok(pending)
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
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
    let runtime_url = std::env::var(UPLOAD_URL_ENV).ok();
    let url = runtime_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(BUILT_IN_UPLOAD_URL);
    if !url.starts_with("https://") || url.contains(['\r', '\n']) {
        return Err(format!("{UPLOAD_URL_ENV} 必须是有效的 HTTPS 地址"));
    }

    let log_directory = directory();
    let histories = daily_history_paths(&log_directory)
        .map_err(|error| format!("读取每日打印日志失败：{error}"))?;
    if histories.is_empty() {
        return Ok(UploadOutcome::NoLog);
    }
    let mut state = load_upload_state(&log_directory)
        .map_err(|error| format!("读取日志上传状态失败：{error}"))?;
    let pending = pending_uploads(histories, &state)
        .map_err(|error| format!("检查每日打印日志失败：{error}"))?;
    if pending.is_empty() {
        return Ok(UploadOutcome::UpToDate);
    }

    let runtime_token = std::env::var(UPLOAD_TOKEN_ENV).ok();
    let token = runtime_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            let value = BUILT_IN_UPLOAD_TOKEN.trim();
            (!value.is_empty()).then_some(value)
        })
        .ok_or_else(|| "当前构建未内置日志上传 Token".to_owned())?;
    if token.contains(['\r', '\n']) {
        return Err(format!("{UPLOAD_TOKEN_ENV} 不能包含换行符"));
    }

    let mut uploaded = 0;
    for item in pending {
        if let Err(error) = upload_with_curl(&item.path, url, Some(token)) {
            let error = format!("{}：{error}", item.file_name);
            let _ = append_upload_status(&format!("上传失败：{error}"));
            return Err(error);
        }
        state.files.insert(item.file_name, item.fingerprint);
        if let Err(error) = save_upload_state(&log_directory, &state) {
            let error = format!("保存日志上传状态失败：{error}");
            let _ = append_upload_status(&error);
            return Err(error);
        }
        uploaded += 1;
    }

    let _ = append_upload_status(&format!("上传成功：{uploaded} 个新增或变更的日志文件"));
    Ok(UploadOutcome::Uploaded)
}

fn upload_with_curl(path: &Path, url: &str, token: Option<&str>) -> Result<(), String> {
    let executable = if cfg!(windows) { "curl.exe" } else { "curl" };
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "每日日志文件名无效".to_owned())?;
    let (client_id, _) = history_file_identity(file_name)
        .ok_or_else(|| "每日日志文件名不包含有效客户端标识".to_owned())?;
    let upload_url = request_upload_url(executable, url, token, client_id, file_name)?;

    upload_to_signed_url(executable, path, &upload_url)
}

fn request_upload_url(
    executable: &str,
    signer_url: &str,
    token: Option<&str>,
    client_id: &str,
    file_name: &str,
) -> Result<String, String> {
    let request_body = serde_json::to_string(&UploadUrlRequest {
        client_id,
        file_name,
    })
    .map_err(|error| format!("生成签名请求失败：{error}"))?;
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
        "Content-Type: application/json",
        "--data-binary",
        &request_body,
    ]);
    if let Some(token) = token {
        command.args(["--header", &format!("Authorization: Bearer {token}")]);
    }
    command.arg(signer_url);

    configure_hidden_window(&mut command);

    let output = command
        .output()
        .map_err(|error| format!("无法启动 {executable} 请求上传地址：{error}"))?;
    if !output.status.success() {
        return Err(curl_failure("请求上传地址失败", &output));
    }

    parse_upload_url_response(&output.stdout)
}

fn parse_upload_url_response(bytes: &[u8]) -> Result<String, String> {
    let response: UploadUrlResponse = serde_json::from_slice(bytes)
        .map_err(|error| format!("签名服务返回了无效 JSON：{error}"))?;
    if !response.method.eq_ignore_ascii_case("PUT") {
        return Err(format!(
            "签名服务返回了不支持的上传方法：{}",
            response.method
        ));
    }
    let upload_url = response.upload_url.trim();
    if !upload_url.starts_with(COS_UPLOAD_URL_PREFIX) || upload_url.contains(['\r', '\n']) {
        return Err("签名服务返回的地址不属于指定的腾讯云 COS Bucket".to_owned());
    }
    Ok(upload_url.to_owned())
}

fn upload_to_signed_url(executable: &str, path: &Path, upload_url: &str) -> Result<(), String> {
    let mut command = Command::new(executable);
    command.args([
        "--silent",
        "--show-error",
        "--fail-with-body",
        "--connect-timeout",
        "3",
        "--max-time",
        "30",
        "--request",
        "PUT",
        "--header",
        "Content-Type: text/csv; charset=utf-8",
        "--data-binary",
    ]);
    let mut file_argument = OsString::from("@");
    file_argument.push(path);
    command.arg(file_argument);
    command.arg(upload_url);

    configure_hidden_window(&mut command);

    let output = command
        .output()
        .map_err(|error| format!("无法启动 {executable} 上传日志：{error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(curl_failure("上传 COS 失败", &output))
    }
}

fn curl_failure(context: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let details = if !stderr.is_empty() { stderr } else { stdout };
    if details.is_empty() {
        format!("{context}，curl 退出码：{}", output.status)
    } else {
        format!("{context}：{details}")
    }
}

fn configure_hidden_window(_command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        _command.creation_flags(CREATE_NO_WINDOW);
    }
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
        format_shanghai(duration.as_secs(), duration.subsec_millis())
    )
}

fn format_shanghai(seconds: u64, millis: u32) -> String {
    const SHANGHAI_OFFSET_SECONDS: u64 = 8 * 60 * 60;
    let local_seconds = seconds.saturating_add(SHANGHAI_OFFSET_SECONDS);
    let days = (local_seconds / 86_400) as i64;
    let seconds_in_day = local_seconds % 86_400;
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

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}+08:00")
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_CLIENT_ID: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn shanghai_format_matches_unix_epoch() {
        assert_eq!(format_shanghai(0, 0), "1970-01-01T08:00:00.000+08:00");
        assert_eq!(
            format_shanghai(1_767_225_600, 123),
            "2026-01-01T08:00:00.123+08:00"
        );
    }

    #[test]
    fn client_id_is_created_once_and_reused() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rust-pigeon-client-id-{unique}"));

        let first = load_or_create_client_id_at(&root).unwrap();
        let second = load_or_create_client_id_at(&root).unwrap();
        assert!(is_valid_client_id(&first));
        assert_eq!(first, second);
        assert_eq!(
            fs::read_to_string(root.join(CLIENT_ID_FILE_NAME))
                .unwrap()
                .trim(),
            first
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn appends_csv_print_history_with_one_header() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rust-pigeon-log-{unique}"));
        let path = root.join(format!("print-history-{TEST_CLIENT_ID}-1970-01-01.csv"));

        append_success_at(
            &root,
            Duration::from_secs(0),
            TEST_CLIENT_ID,
            "2026-03-0498274",
            "5",
            PrintAction::Automatic,
        )
        .unwrap();
        append_success_at(
            &root,
            Duration::from_secs(1),
            TEST_CLIENT_ID,
            "0237849",
            "12",
            PrintAction::Reprint,
        )
        .unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = content.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "channel,timestamp_shanghai,ring_number,action");

        let first: Vec<_> = lines[1].split(',').collect();
        assert_eq!(first.len(), 4);
        assert_eq!(first[0], "05");
        assert!(first[1].ends_with("+08:00"));
        assert_eq!(first[2], "2026-03-0498274");
        assert_eq!(first[3], "automatic");

        let second: Vec<_> = lines[2].split(',').collect();
        assert_eq!(second[0], "12");
        assert_eq!(second[2], "0237849");
        assert_eq!(second[3], "reprint");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn splits_csv_history_by_shanghai_day() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rust-pigeon-daily-log-{unique}"));

        append_success_at(
            &root,
            Duration::from_secs(57_599),
            TEST_CLIENT_ID,
            "1234567",
            "1",
            PrintAction::Automatic,
        )
        .unwrap();
        append_success_at(
            &root,
            Duration::from_secs(57_600),
            TEST_CLIENT_ID,
            "7654321",
            "2",
            PrintAction::Automatic,
        )
        .unwrap();

        let paths = daily_history_paths(&root).unwrap();
        let names: Vec<_> = paths
            .iter()
            .filter_map(|path| path.file_name()?.to_str())
            .collect();
        assert_eq!(
            names,
            [
                "print-history-0123456789abcdef0123456789abcdef-1970-01-01.csv",
                "print-history-0123456789abcdef0123456789abcdef-1970-01-02.csv"
            ]
        );
        for path in paths {
            let content = fs::read_to_string(path).unwrap();
            assert_eq!(
                content
                    .matches("channel,timestamp_shanghai,ring_number,action")
                    .count(),
                1
            );
            assert_eq!(content.lines().count(), 2);
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn uploads_only_new_or_changed_daily_files() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rust-pigeon-upload-state-{unique}"));

        append_success_at(
            &root,
            Duration::from_secs(0),
            TEST_CLIENT_ID,
            "1234567",
            "1",
            PrintAction::Automatic,
        )
        .unwrap();
        let histories = daily_history_paths(&root).unwrap();
        let mut state = UploadState::default();
        let pending = pending_uploads(histories.clone(), &state).unwrap();
        assert_eq!(pending.len(), 1);

        state
            .files
            .insert(pending[0].file_name.clone(), pending[0].fingerprint);
        save_upload_state(&root, &state).unwrap();
        let state = load_upload_state(&root).unwrap();
        assert!(pending_uploads(histories, &state).unwrap().is_empty());

        append_success_at(
            &root,
            Duration::from_secs(1),
            TEST_CLIENT_ID,
            "7654321",
            "1",
            PrintAction::Automatic,
        )
        .unwrap();
        let changed = pending_uploads(daily_history_paths(&root).unwrap(), &state).unwrap();
        assert_eq!(changed.len(), 1);

        fs::write(root.join(UPLOAD_STATE_FILE_NAME), b"invalid json").unwrap();
        assert!(load_upload_state(&root).unwrap().files.is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn escapes_csv_fields() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("a\"b"), "\"a\"\"b\"");
        assert_eq!(csv_field("a\nb"), "\"a\nb\"");
    }

    #[test]
    fn accepts_only_https_put_upload_urls() {
        let response = br#"{
            "upload_url":"https://pg-1329186100.cos.ap-nanjing.myqcloud.com/log/test.csv?q-signature=abc",
            "method":"PUT"
        }"#;
        assert_eq!(
            parse_upload_url_response(response).unwrap(),
            "https://pg-1329186100.cos.ap-nanjing.myqcloud.com/log/test.csv?q-signature=abc"
        );

        assert!(
            parse_upload_url_response(
                br#"{"upload_url":"https://other.example.com/a","method":"PUT"}"#
            )
            .is_err()
        );
        assert!(
            parse_upload_url_response(br#"{"upload_url":"https://example.com/a","method":"POST"}"#)
                .is_err()
        );
    }
}
