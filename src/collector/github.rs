use super::model::Snapshot;
use super::{Error, Result};
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) fn fetch_payload(timeout: Duration) -> Result<Value> {
    if let Some(fixture) = env::var_os("GH_AI_CREDITS_FIXTURE") {
        let content = fs::read_to_string(&fixture).map_err(|error| {
            Error::Usage(format!(
                "cannot read fixture {}: {error}",
                PathBuf::from(fixture).display()
            ))
        })?;
        return serde_json::from_str(&content)
            .map_err(|error| Error::Usage(format!("cannot read fixture: {error}")));
    }

    let gh = resolve_gh_executable()?;
    let mut child = Command::new(&gh)
        .args(["api", "/copilot_internal/user"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| Error::Usage(format!("could not start {}: {error}", gh.display())))?;

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Usage(format!(
                    "GitHub API request timed out after {}s",
                    timeout.as_secs_f64()
                )));
            }
            Err(error) => {
                return Err(Error::Usage(format!(
                    "could not wait for {}: {error}",
                    gh.display()
                )));
            }
        }
    }

    let output = child.wait_with_output().map_err(|error| {
        Error::Usage(format!(
            "could not read output from {}: {error}",
            gh.display()
        ))
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let message = stderr
            .trim()
            .lines()
            .last()
            .or_else(|| stdout.trim().lines().last());
        return Err(Error::Usage(
            message
                .unwrap_or("unknown gh error")
                .chars()
                .take(400)
                .collect(),
        ));
    }

    let payload: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| Error::Usage("GitHub CLI returned invalid JSON".to_owned()))?;
    if !payload.is_object() {
        return Err(Error::Usage(
            "GitHub API response is not a JSON object".to_owned(),
        ));
    }
    Ok(payload)
}

pub(crate) fn parse_snapshot(payload: &Value, sampled_at: i64) -> Result<Snapshot> {
    let premium = payload
        .get("quota_snapshots")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Usage("response has no quota_snapshots object".to_owned()))?
        .get("premium_interactions")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Usage("response has no premium_interactions quota".to_owned()))?;

    let entitlement = premium.get("entitlement").and_then(number);
    let remaining = premium
        .get("quota_remaining")
        .and_then(number)
        .or_else(|| premium.get("remaining").and_then(number));
    let credits_used = premium
        .get("credits_used")
        .and_then(number)
        .or_else(|| {
            entitlement
                .zip(remaining)
                .map(|(total, left)| (total - left).max(0.0))
        })
        .ok_or_else(|| {
            Error::Usage(
                "premium_interactions contains neither credits_used nor usable quota totals"
                    .to_owned(),
            )
        })?;

    let reset_at = payload
        .get("quota_reset_date_utc")
        .or_else(|| payload.get("quota_reset_date"))
        .and_then(Value::as_str)
        .and_then(iso_to_epoch);

    Ok(Snapshot {
        sampled_at,
        api_timestamp: premium
            .get("timestamp_utc")
            .and_then(Value::as_str)
            .map(str::to_owned),
        credits_used,
        entitlement,
        remaining,
        percent_remaining: premium.get("percent_remaining").and_then(number),
        unlimited: premium
            .get("unlimited")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        overage_count: premium.get("overage_count").and_then(number).unwrap_or(0.0),
        reset_at,
        plan: payload
            .get("copilot_plan")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn resolve_gh_executable() -> Result<PathBuf> {
    if let Some(override_path) = env::var_os("GH_AI_CREDITS_GH") {
        let candidate = expand_home(PathBuf::from(override_path));
        return is_executable(&candidate)
            .then_some(candidate.clone())
            .ok_or_else(|| {
                Error::Usage(format!(
                    "GH_AI_CREDITS_GH is not executable: {}",
                    candidate.display()
                ))
            });
    }

    if let Some(path) = env::var_os("PATH") {
        for directory in env::split_paths(&path) {
            let candidate = directory.join("gh");
            if is_executable(&candidate) {
                return Ok(candidate);
            }
        }
    }

    let home = env::var_os("HOME").map(PathBuf::from);
    let mut candidates = Vec::new();
    if let Some(home) = &home {
        candidates.extend([
            home.join(".local/bin/gh"),
            home.join(".linuxbrew/bin/gh"),
            home.join(".local/share/gh/bin/gh"),
        ]);
    }
    candidates.extend([
        PathBuf::from("/home/linuxbrew/.linuxbrew/bin/gh"),
        PathBuf::from("/usr/local/bin/gh"),
        PathBuf::from("/usr/bin/gh"),
        PathBuf::from("/snap/bin/gh"),
    ]);
    if let Some(candidate) = candidates.iter().find(|path| is_executable(path)) {
        return Ok(candidate.clone());
    }

    let searched = candidates
        .iter()
        .filter_map(|path| path.parent())
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Err(Error::Usage(format!(
        "GitHub CLI not found in PATH or common install locations ({searched})"
    )))
}

fn expand_home(path: PathBuf) -> PathBuf {
    let Some(raw) = path.to_str() else {
        return path;
    };
    if raw == "~" {
        return env::var_os("HOME").map(PathBuf::from).unwrap_or(path);
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    path
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
    .filter(|value| value.is_finite())
}

fn iso_to_epoch(value: &str) -> Option<i64> {
    let value = value.trim();
    if value.len() == 10 {
        return NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .ok()?
            .and_hms_opt(0, 0, 0)
            .map(|date| date.and_utc().timestamp());
    }
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.with_timezone(&Utc).timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_credits_used_as_source_of_truth() {
        let fixture = include_str!("../../tests/fixtures/copilot_user.json");
        let payload: Value = serde_json::from_str(fixture).unwrap();
        let snapshot = parse_snapshot(&payload, 123).unwrap();
        assert_eq!(snapshot.credits_used, 3027.0);
        assert_eq!(snapshot.remaining, Some(6973.0));
        assert_eq!(snapshot.sampled_at, 123);
    }

    #[test]
    fn falls_back_to_entitlement_minus_remaining() {
        let payload = json!({
            "copilot_plan": "business",
            "quota_snapshots": {
                "premium_interactions": {
                    "entitlement": 300,
                    "quota_remaining": 287.5
                }
            }
        });
        let snapshot = parse_snapshot(&payload, 123).unwrap();
        assert_eq!(snapshot.credits_used, 12.5);
    }

    #[test]
    fn rejects_missing_premium_quota() {
        let error = parse_snapshot(&serde_json::json!({"quota_snapshots": {}}), 123)
            .unwrap_err()
            .to_string();
        assert!(error.contains("premium_interactions"));
    }

    #[test]
    fn parses_iso_dates() {
        assert_eq!(iso_to_epoch("1970-01-02"), Some(86_400));
        assert_eq!(iso_to_epoch("1970-01-01T01:00:00+01:00"), Some(0));
    }
}
