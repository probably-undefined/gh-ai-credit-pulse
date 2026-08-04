mod github;
mod metrics;
mod model;
mod store;

pub use model::{Current, DailyUsage, DashboardData, Metrics, UsageSample, Window};

use crate::collector::github::{fetch_payload, parse_snapshot};
use crate::collector::metrics::build_dashboard;
use crate::collector::store::Store;
use serde_json::Value;
use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub const DEFAULT_RETENTION_DAYS: u32 = 180;
pub const DEFAULT_MIN_SAMPLE_INTERVAL: Duration = Duration::from_secs(25);

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Usage(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("{0}")]
    Csv(#[from] csv::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Collector {
    store: Store,
}

impl Collector {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            store: Store::open(path.as_ref())?,
        })
    }

    pub fn sample(
        &self,
        window: Window,
        timeout: Duration,
        retention_days: u32,
    ) -> Result<DashboardData> {
        self.sample_with_policy(window, timeout, retention_days, false)
    }

    pub fn sample_force(
        &self,
        window: Window,
        timeout: Duration,
        retention_days: u32,
    ) -> Result<DashboardData> {
        self.sample_with_policy(window, timeout, retention_days, true)
    }

    fn sample_with_policy(
        &self,
        window: Window,
        timeout: Duration,
        retention_days: u32,
        force: bool,
    ) -> Result<DashboardData> {
        let now = now_epoch();
        let baseline_count = self.store.sample_count()?;
        if !force && self.sample_is_recent(now)? {
            return self.build_dashboard(window, now, true);
        }

        let deadline = Instant::now() + timeout;
        let lease_seconds = timeout.as_secs().saturating_add(5).max(5);
        let owner = format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        );

        loop {
            let attempt_time = now_epoch();
            let lease_until = attempt_time.saturating_add(lease_seconds as i64);
            if self
                .store
                .try_acquire_sample_lease(&owner, attempt_time, lease_until)?
            {
                return self.fetch_under_lease(
                    window,
                    timeout,
                    retention_days,
                    attempt_time,
                    &owner,
                    lease_until,
                );
            }

            if self.store.sample_count()? > baseline_count || self.sample_is_recent(attempt_time)? {
                return self.build_dashboard(window, attempt_time, true);
            }
            if Instant::now() >= deadline {
                let mut dashboard = self.build_dashboard(window, attempt_time, false)?;
                if dashboard.current.sampled_at.is_none() {
                    dashboard.status = "error".to_owned();
                    dashboard.error = Some("another collector is still refreshing".to_owned());
                }
                return Ok(dashboard);
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn fetch_under_lease(
        &self,
        window: Window,
        timeout: Duration,
        retention_days: u32,
        now: i64,
        owner: &str,
        lease_until: i64,
    ) -> Result<DashboardData> {
        let result = fetch_payload(timeout).and_then(|payload| self.persist(payload, now));
        self.store.release_sample_lease(owner, lease_until)?;
        match result {
            Ok(()) => {
                self.store.prune_if_due(now, retention_days)?;
                self.build_dashboard(window, now, true)
            }
            Err(Error::Usage(message)) => {
                let mut dashboard = self.build_dashboard(window, now, false)?;
                dashboard.status = "error".to_owned();
                dashboard.error = Some(message);
                Ok(dashboard)
            }
            Err(error) => Err(error),
        }
    }

    pub fn dashboard(&self, window: Window) -> Result<DashboardData> {
        let mut dashboard = build_dashboard(&self.store, window, now_epoch())?;
        dashboard.fresh = false;
        Ok(dashboard)
    }

    pub fn export(&self, writer: impl Write) -> Result<()> {
        self.store.export(writer)
    }

    fn persist(&self, payload: Value, sampled_at: i64) -> Result<()> {
        let snapshot = parse_snapshot(&payload, sampled_at)?;
        self.store.insert_snapshot(&snapshot, &payload)
    }

    fn sample_is_recent(&self, now: i64) -> Result<bool> {
        Ok(self.store.latest_sample_at()?.is_some_and(|sampled_at| {
            now.saturating_sub(sampled_at) < DEFAULT_MIN_SAMPLE_INTERVAL.as_secs() as i64
        }))
    }

    fn build_dashboard(&self, window: Window, now: i64, fresh: bool) -> Result<DashboardData> {
        let mut dashboard = build_dashboard(&self.store, window, now)?;
        dashboard.fresh = fresh;
        Ok(dashboard)
    }
}

pub fn default_db_path() -> PathBuf {
    if let Some(path) = env::var_os("GH_AI_CREDITS_DB") {
        return PathBuf::from(path);
    }
    if let Some(path) = env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(path)
            .join("gh-ai-credits")
            .join("history.sqlite3");
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home).join(".local/state/gh-ai-credits/history.sqlite3");
    }
    PathBuf::from(".local/state/gh-ai-credits/history.sqlite3")
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::model::Snapshot;
    use serde_json::json;

    #[test]
    fn freshness_window_has_an_exact_upper_boundary() {
        let store = Store::in_memory().unwrap();
        let collector = Collector { store };
        assert!(!collector.sample_is_recent(100).unwrap());

        collector
            .store
            .insert_snapshot(
                &Snapshot {
                    sampled_at: 100,
                    api_timestamp: None,
                    credits_used: 42.0,
                    entitlement: None,
                    remaining: None,
                    percent_remaining: None,
                    unlimited: false,
                    overage_count: 0.0,
                    reset_at: None,
                    plan: None,
                },
                &json!({"credits_used": 42}),
            )
            .unwrap();

        assert!(collector.sample_is_recent(124).unwrap());
        assert!(!collector.sample_is_recent(125).unwrap());
    }
}
