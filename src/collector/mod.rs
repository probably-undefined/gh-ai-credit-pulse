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
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub const DEFAULT_RETENTION_DAYS: u32 = 180;

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
        let now = now_epoch();
        match fetch_payload(timeout).and_then(|payload| self.persist(payload, now)) {
            Ok(()) => {
                self.store.prune_if_due(now, retention_days)?;
                let mut dashboard = build_dashboard(&self.store, window, now)?;
                dashboard.fresh = true;
                Ok(dashboard)
            }
            Err(Error::Usage(message)) => {
                let mut dashboard = build_dashboard(&self.store, window, now)?;
                dashboard.status = "error".to_owned();
                dashboard.fresh = false;
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
        return PathBuf::from(home)
            .join(".local/state/gh-ai-credits/history.sqlite3");
    }
    PathBuf::from(".local/state/gh-ai-credits/history.sqlite3")
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}
