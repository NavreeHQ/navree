//! `read_logs` and `last_error`: the log file the app writes (`log.file`).
//!
//! navree-log writes `log.file.path` with the date inserted before the
//! extension when the file rotates (`storage/logs/navree.2026-10-27.log`
//! daily, `navree.2026-10-27-09.log` hourly, `navree.log` never), in the
//! app's log format: JSON (one object per line with `timestamp`, `level`,
//! `target`, `fields` and the current `span`) or pretty
//! (`timestamp LEVEL spans: target: message key=value`). Both are read.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration as TimeDelta, Utc};
use navree_config::Config;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How much of each file is read, from its end.
const TAIL_BYTES: u64 = 8 * 1024 * 1024;

/// How many files (newest first) are read at most.
const MAX_FILES: usize = 3;

/// The default and the largest `limit` of `read_logs`.
pub const DEFAULT_LIMIT: usize = 100;
/// The largest `limit` of `read_logs`.
pub const MAX_LIMIT: usize = 1_000;

/// What `read_logs` and `last_error` say when the app writes no log file.
pub const NO_LOG_FILE: &str = "The app doesn't write a log file, so there are no logs to read. \
Turn one on for development in config/app.toml:\n\n[log]\nfile = { path = \"storage/logs/navree.log\", rotation = \"daily\", keep_days = 7 }\n\n\
Then restart the app (`navree dev`).";

/// The `log.file` section, as navree-log reads it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LogFile {
    /// The path of the file, relative to the app.
    pub path: PathBuf,
    /// `daily` (default), `hourly` or `never`.
    pub rotation: String,
}

impl Default for LogFile {
    fn default() -> Self {
        Self {
            path: PathBuf::from("storage/logs/navree.log"),
            rotation: "daily".to_owned(),
        }
    }
}

impl LogFile {
    /// The `log.file` section of `config`, if the app writes a log file.
    pub fn from_config(config: &Config) -> Option<LogFile> {
        config.section::<LogFile>("log.file").ok()
    }

    /// Every file this setting writes, newest first: the rotated files by
    /// their date, then the unrotated path.
    pub fn files(&self, root: &Path) -> Vec<PathBuf> {
        let path = if self.path.is_absolute() {
            self.path.clone()
        } else {
            root.join(&self.path)
        };
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let extension = path.extension().map(|e| e.to_string_lossy().into_owned());
        let mut rotated: Vec<(String, PathBuf)> = std::fs::read_dir(&dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|entry| {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        let date = rotated_date(&name, &stem, extension.as_deref())?;
                        Some((date, entry.path()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        rotated.sort();
        let mut files: Vec<PathBuf> = rotated.into_iter().rev().map(|(_, p)| p).collect();
        if path.is_file() {
            files.push(path);
        }
        files
    }
}

/// The date part of `stem.<date>.ext` (`navree.2026-10-27.log`,
/// `navree.2026-10-27-09.log`), if `name` is a rotated file of `stem`.
fn rotated_date(name: &str, stem: &str, extension: Option<&str>) -> Option<String> {
    let rest = name.strip_prefix(stem)?.strip_prefix('.')?;
    let date = match extension {
        Some(ext) => rest.strip_suffix(ext)?.strip_suffix('.')?,
        None => rest,
    };
    let looks_like_date = date.len() >= 10
        && date
            .chars()
            .all(|c| c.is_ascii_digit() || c == '-' || c == '_');
    looks_like_date.then(|| date.to_owned())
}

/// One log line.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LogLine {
    /// When it was written (RFC 3339), if the line has a timestamp.
    pub timestamp: Option<String>,
    /// `ERROR`, `WARN`, `INFO`, `DEBUG` or `TRACE`.
    pub level: Option<String>,
    /// The target (`navree::request`).
    pub target: Option<String>,
    /// The message (pretty lines: the message and its fields).
    pub message: String,
    /// The request id, from the line's fields or spans.
    pub request_id: Option<String>,
    /// The event's fields (JSON lines only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<Value>,
    /// The spans the line was written in (JSON lines only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spans: Option<Value>,
}

/// Parses one line of either format.
pub fn parse_line(line: &str) -> LogLine {
    let line = line.trim_end();
    if line.starts_with('{')
        && let Ok(Value::Object(object)) = serde_json::from_str::<Value>(line)
    {
        return json_line(&Value::Object(object));
    }
    pretty_line(line)
}

fn json_line(object: &Value) -> LogLine {
    let fields = object.get("fields").cloned();
    let message = fields
        .as_ref()
        .and_then(|f| f.get("message"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let string = |v: Option<&Value>| match v {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Null) | None => None,
        Some(other) => Some(other.to_string()),
    };
    let mut request_id = string(fields.as_ref().and_then(|f| f.get("request_id")));
    if request_id.is_none()
        && let Some(Value::Array(spans)) = object.get("spans")
    {
        request_id = spans
            .iter()
            .rev()
            .find_map(|span| string(span.get("request_id")));
    }
    LogLine {
        timestamp: string(object.get("timestamp")),
        level: string(object.get("level")).map(|l| l.to_ascii_uppercase()),
        target: string(object.get("target")),
        message,
        request_id,
        fields,
        spans: object.get("spans").cloned(),
    }
}

fn pretty_line(line: &str) -> LogLine {
    let mut words = line.splitn(2, char::is_whitespace);
    let first = words.next().unwrap_or_default();
    let timestamp = DateTime::parse_from_rfc3339(first)
        .ok()
        .map(|_| first.to_owned());
    let rest = if timestamp.is_some() {
        words.next().unwrap_or_default().trim_start()
    } else {
        line
    };
    let (level, rest) = match rest.split_once(char::is_whitespace) {
        Some((word, after)) if ["ERROR", "WARN", "INFO", "DEBUG", "TRACE"].contains(&word) => {
            (Some(word.to_owned()), after.trim_start())
        }
        _ => (None, rest),
    };
    let (target, message) = split_target(rest);
    LogLine {
        timestamp,
        level,
        target,
        message: message.to_owned(),
        request_id: field_value(line, "request_id"),
        fields: None,
        spans: None,
    }
}

/// Splits `spans: target: message` (spans optional) into the target and
/// the message.
fn split_target(rest: &str) -> (Option<String>, &str) {
    let pieces = split_top_level(rest, 3);
    let is_path = |s: &str| {
        !s.is_empty()
            && s.split("::").all(|part| {
                !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
    };
    let first_is_target = |first: &str| -> (Option<String>, &str) {
        let start = first.len() + 2;
        (
            Some(first.to_owned()),
            rest.get(start..).unwrap_or_default(),
        )
    };
    match pieces.as_slice() {
        // `request{...}: target: message`
        [spans, target, message] if spans.contains('{') => (Some((*target).to_owned()), message),
        // `navree::http: message` (a module path is a target, not a span)
        [first, ..] if first.contains("::") && is_path(first) => first_is_target(first),
        // `span: crm::jobs: message` (a span without fields)
        [_, target, message] if target.contains("::") && is_path(target) => {
            (Some((*target).to_owned()), message)
        }
        // `crm_api: message`
        [first, _, ..] if is_path(first) => first_is_target(first),
        _ => (None, rest),
    }
}

/// Splits on `": "` outside braces, into at most `n` pieces.
fn split_top_level(s: &str, n: usize) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b':' if depth == 0 && bytes.get(i + 1) == Some(&b' ') && pieces.len() + 1 < n => {
                pieces.push(&s[start..i]);
                start = i + 2;
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    pieces.push(&s[start..]);
    pieces
}

/// The value of `name=value` in a pretty line (quotes removed).
fn field_value(line: &str, name: &str) -> Option<String> {
    let pattern = format!("{name}=");
    let mut search = line;
    while let Some(at) = search.find(&pattern) {
        let before = search[..at].chars().last();
        let after = &search[at + pattern.len()..];
        if before.is_none_or(|c| c == ' ' || c == '{' || c == ',') {
            let value = if let Some(quoted) = after.strip_prefix('"') {
                quoted.split('"').next().unwrap_or_default()
            } else {
                after
                    .split(|c: char| c.is_whitespace() || c == '}' || c == ',')
                    .next()
                    .unwrap_or_default()
            };
            return (!value.is_empty()).then(|| value.to_owned());
        }
        search = after;
    }
    None
}

/// The arguments of `read_logs`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct LogFilter {
    /// The lowest level: `warn` returns warnings and errors.
    pub level: Option<String>,
    /// A target or target prefix (`navree::job`).
    pub target: Option<String>,
    /// One request's lines.
    pub request_id: Option<String>,
    /// Lines written at or after this time: RFC 3339, or a duration back
    /// from now (`30s`, `15m`, `2h`, `1d`).
    pub since: Option<String>,
    /// How many lines (newest last); default 100, at most 1000.
    pub limit: Option<usize>,
}

fn severity(level: &str) -> u8 {
    match level.to_ascii_uppercase().as_str() {
        "ERROR" => 5,
        "WARN" | "WARNING" => 4,
        "INFO" => 3,
        "DEBUG" => 2,
        "TRACE" => 1,
        _ => 0,
    }
}

impl LogFilter {
    /// Checks the arguments; the `since` cut-off as a time.
    fn since_time(&self, now: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, String> {
        let Some(since) = self.since.as_deref().map(str::trim) else {
            return Ok(None);
        };
        if let Ok(time) = DateTime::parse_from_rfc3339(since) {
            return Ok(Some(time.with_timezone(&Utc)));
        }
        let (number, unit) = since.split_at(since.len().saturating_sub(1));
        let amount: i64 = number.parse().map_err(|_| {
            format!("`since` must be an RFC 3339 time or a duration such as 15m, got `{since}`")
        })?;
        let delta = match unit {
            "s" => TimeDelta::seconds(amount),
            "m" => TimeDelta::minutes(amount),
            "h" => TimeDelta::hours(amount),
            "d" => TimeDelta::days(amount),
            _ => return Err(format!("`since` must end in s, m, h or d, got `{since}`")),
        };
        Ok(Some(now - delta))
    }

    fn matches(&self, line: &LogLine, since: Option<DateTime<Utc>>) -> bool {
        if let Some(level) = &self.level
            && line
                .level
                .as_deref()
                .is_none_or(|l| severity(l) < severity(level))
        {
            return false;
        }
        if let Some(target) = &self.target
            && line
                .target
                .as_deref()
                .is_none_or(|t| t != target && !t.starts_with(&format!("{target}::")))
        {
            return false;
        }
        if let Some(id) = &self.request_id
            && line.request_id.as_deref() != Some(id.as_str())
        {
            return false;
        }
        if let Some(since) = since {
            let at = line
                .timestamp
                .as_deref()
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok());
            if at.is_none_or(|at| at.with_timezone(&Utc) < since) {
                return false;
            }
        }
        true
    }
}

/// The last `TAIL_BYTES` of a file, as whole lines.
fn tail_lines(path: &Path) -> std::io::Result<Vec<String>> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0); // a partial line
    }
    lines.retain(|l| !l.trim().is_empty());
    Ok(lines)
}

/// Every line of the newest files (at most [`MAX_FILES`]), oldest first.
fn recent_lines(files: &[PathBuf]) -> Vec<LogLine> {
    let mut lines = Vec::new();
    for path in files.iter().take(MAX_FILES).rev() {
        if let Ok(file_lines) = tail_lines(path) {
            lines.extend(file_lines.iter().map(|l| parse_line(l)));
        }
    }
    lines
}

/// `read_logs`: the newest lines that match `filter`, newest last.
pub fn read_logs(files: &[PathBuf], filter: &LogFilter) -> Result<Vec<LogLine>, String> {
    if let Some(level) = &filter.level
        && severity(level) == 0
    {
        return Err(format!(
            "`level` must be error, warn, info, debug or trace, got `{level}`"
        ));
    }
    let since = filter.since_time(Utc::now())?;
    let limit = filter.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let mut matched: Vec<LogLine> = recent_lines(files)
        .into_iter()
        .filter(|line| filter.matches(line, since))
        .collect();
    let skip = matched.len().saturating_sub(limit);
    Ok(matched.split_off(skip))
}

/// The answer of `last_error`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LastError {
    /// The most recent `ERROR` line.
    pub error: LogLine,
    /// Its request id, if it was written inside a request.
    pub request_id: Option<String>,
    /// The request's method, route and status (from its `request
    /// completed` line), when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<Value>,
    /// Every line of the same request; without a request id, the lines
    /// around the error.
    pub lines: Vec<LogLine>,
}

/// `last_error`: the most recent error with its request's lines.
pub fn last_error(files: &[PathBuf]) -> Option<LastError> {
    let lines = recent_lines(files);
    let at = lines
        .iter()
        .rposition(|l| l.level.as_deref() == Some("ERROR"))?;
    let error = lines[at].clone();
    let request_id = error.request_id.clone();
    let related: Vec<LogLine> = match &request_id {
        Some(id) => lines
            .iter()
            .filter(|l| l.request_id.as_deref() == Some(id.as_str()))
            .cloned()
            .collect(),
        None => lines[at.saturating_sub(10)..(at + 6).min(lines.len())].to_vec(),
    };
    let request = related
        .iter()
        .rev()
        .find(|l| l.target.as_deref() == Some("navree::request"))
        .map(request_summary);
    Some(LastError {
        error,
        request_id,
        request,
        lines: related,
    })
}

/// `{method, route, status}` of a `navree::request` line.
fn request_summary(line: &LogLine) -> Value {
    let pick = |name: &str| -> Value {
        if let Some(fields) = &line.fields {
            return fields.get(name).cloned().unwrap_or(Value::Null);
        }
        field_value(&line.message, name).map_or(Value::Null, |v| {
            v.parse::<i64>().map_or(Value::from(v), Value::from)
        })
    };
    serde_json::json!({
        "method": pick("method"),
        "route": pick("route"),
        "status": pick("status"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON_LINE: &str = r#"{"timestamp":"2026-10-27T09:12:01.123456Z","level":"ERROR","target":"navree::http","fields":{"message":"handler failed","error":"no such column: deals.expected_close_date"},"span":{"method":"GET","path":"/api/v1/deals","request_id":"0199c4b1","name":"request"},"spans":[{"method":"GET","path":"/api/v1/deals","request_id":"0199c4b1","name":"request"}]}"#;

    const PRETTY_LINE: &str = "2026-10-27T09:12:01.123456Z ERROR request{method=GET path=/api/v1/deals request_id=0199c4b1}: navree::http: handler failed error=no such column";

    #[test]
    fn json_lines_keep_every_field() {
        let line = parse_line(JSON_LINE);
        assert_eq!(
            line.timestamp.as_deref(),
            Some("2026-10-27T09:12:01.123456Z")
        );
        assert_eq!(line.level.as_deref(), Some("ERROR"));
        assert_eq!(line.target.as_deref(), Some("navree::http"));
        assert_eq!(line.message, "handler failed");
        assert_eq!(line.request_id.as_deref(), Some("0199c4b1"));
        assert_eq!(
            line.fields.unwrap()["error"],
            "no such column: deals.expected_close_date"
        );
    }

    #[test]
    fn pretty_lines_are_split_into_parts() {
        let line = parse_line(PRETTY_LINE);
        assert_eq!(line.level.as_deref(), Some("ERROR"));
        assert_eq!(line.target.as_deref(), Some("navree::http"));
        assert_eq!(line.message, "handler failed error=no such column");
        assert_eq!(line.request_id.as_deref(), Some("0199c4b1"));

        let plain = parse_line(
            "2026-10-20T09:00:00Z  INFO navree::http: server listening addr=127.0.0.1:8080",
        );
        assert_eq!(plain.level.as_deref(), Some("INFO"));
        assert_eq!(plain.target.as_deref(), Some("navree::http"));
        assert_eq!(plain.message, "server listening addr=127.0.0.1:8080");
        assert_eq!(plain.request_id, None);

        let quoted = parse_line(
            r#"2026-10-20T09:00:00Z  WARN job{request_id="r-9"}:inner: crm::jobs: slow"#,
        );
        assert_eq!(quoted.request_id.as_deref(), Some("r-9"));
        assert_eq!(quoted.target.as_deref(), Some("crm::jobs"));
        assert_eq!(quoted.message, "slow");

        let odd = parse_line("not a log line: at all");
        assert_eq!(odd.level, None);
        assert_eq!(odd.message, "not a log line: at all");
    }

    #[test]
    fn rotated_files_are_found_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("storage/logs");
        std::fs::create_dir_all(&logs).unwrap();
        for name in [
            "navree.2026-10-25.log",
            "navree.2026-10-27.log",
            "navree.2026-10-26.log",
            "other.2026-10-27.log",
            "navree.notes.log",
        ] {
            std::fs::write(logs.join(name), "x\n").unwrap();
        }
        let files = LogFile::default().files(dir.path());
        let names: Vec<String> = files
            .iter()
            .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "navree.2026-10-27.log",
                "navree.2026-10-26.log",
                "navree.2026-10-25.log"
            ]
        );
        std::fs::write(logs.join("navree.log"), "x\n").unwrap();
        assert!(
            LogFile::default()
                .files(dir.path())
                .last()
                .unwrap()
                .ends_with("navree.log")
        );
    }

    #[test]
    fn the_log_file_comes_from_the_configuration() {
        use navree_config::{Config, ConfigSources};
        let config = Config::load(&ConfigSources::new()).unwrap();
        assert_eq!(LogFile::from_config(&config), None);
        let config = Config::load(&ConfigSources::new().set(
            "log.file",
            serde_json::json!({"path": "var/app.log", "rotation": "never"}),
        ))
        .unwrap();
        assert_eq!(
            LogFile::from_config(&config),
            Some(LogFile {
                path: PathBuf::from("var/app.log"),
                rotation: "never".to_owned()
            })
        );
    }

    fn write_log(lines: &[&str]) -> (tempfile::TempDir, Vec<PathBuf>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("navree.log");
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        (dir, vec![path])
    }

    #[test]
    fn filters_apply_and_the_newest_lines_come_last() {
        let (_dir, files) = write_log(&[
            "2026-10-20T09:00:00Z  INFO navree::http: first",
            "2026-10-20T09:00:01Z  WARN navree::job::worker: slow job",
            "2026-10-20T09:00:02Z ERROR navree::job: failed request_id=r-1",
            "2026-10-20T09:00:03Z DEBUG crm::deals: noisy",
            "2026-10-20T09:00:04Z  INFO navree::http: last",
        ]);
        let all = read_logs(&files, &LogFilter::default()).unwrap();
        assert_eq!(all.len(), 5);
        assert_eq!(all.last().unwrap().message, "last");

        let warn = read_logs(
            &files,
            &LogFilter {
                level: Some("warn".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            warn.iter().map(|l| l.message.as_str()).collect::<Vec<_>>(),
            ["slow job", "failed request_id=r-1"]
        );

        let job = read_logs(
            &files,
            &LogFilter {
                target: Some("navree::job".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(job.len(), 2);

        let request = read_logs(
            &files,
            &LogFilter {
                request_id: Some("r-1".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(request.len(), 1);

        let since = read_logs(
            &files,
            &LogFilter {
                since: Some("2026-10-20T09:00:03Z".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(since.len(), 2);

        let limited = read_logs(
            &files,
            &LogFilter {
                limit: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            limited
                .iter()
                .map(|l| l.message.as_str())
                .collect::<Vec<_>>(),
            ["noisy", "last"]
        );

        assert!(
            read_logs(
                &files,
                &LogFilter {
                    since: Some("yesterday".into()),
                    ..Default::default()
                }
            )
            .is_err()
        );
        let recent = read_logs(
            &files,
            &LogFilter {
                since: Some("15m".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(recent.is_empty(), "these lines are older than 15 minutes");
    }

    #[test]
    fn last_error_brings_the_lines_of_its_request() {
        let (_dir, files) = write_log(&[
            r#"{"timestamp":"2026-10-27T09:12:00Z","level":"INFO","target":"navree::http","fields":{"message":"other"},"spans":[{"name":"request","request_id":"other"}]}"#,
            r#"{"timestamp":"2026-10-27T09:12:01Z","level":"DEBUG","target":"navree_db","fields":{"message":"slow query","sql":"SELECT ..."},"spans":[{"name":"request","request_id":"0199c4b1"}]}"#,
            JSON_LINE,
            r#"{"timestamp":"2026-10-27T09:12:01.2Z","level":"INFO","target":"navree::request","fields":{"message":"request completed","method":"GET","route":"/api/v1/deals","status":500,"request_id":"0199c4b1"}}"#,
            r#"{"timestamp":"2026-10-27T09:12:02Z","level":"INFO","target":"navree::http","fields":{"message":"later"}}"#,
        ]);
        let last = last_error(&files).expect("an error");
        assert_eq!(last.error.message, "handler failed");
        assert_eq!(last.request_id.as_deref(), Some("0199c4b1"));
        assert_eq!(last.lines.len(), 3);
        assert_eq!(
            last.request,
            Some(serde_json::json!({"method": "GET", "route": "/api/v1/deals", "status": 500}))
        );

        let (_dir, files) = write_log(&["2026-10-20T09:00:00Z  INFO navree::http: fine"]);
        assert_eq!(last_error(&files), None);
    }
}
