use super::Result;
use super::model::Snapshot;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::Path;

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub(crate) struct SampleRow {
    pub id: i64,
    pub sampled_at: i64,
    pub api_timestamp: Option<String>,
    pub credits_used: f64,
    pub entitlement: Option<f64>,
    pub remaining: Option<f64>,
    pub percent_remaining: Option<f64>,
    pub unlimited: bool,
    pub overage_count: f64,
    pub reset_at: Option<i64>,
    pub plan: Option<String>,
}

pub(crate) struct Store {
    connection: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        let store = Self { connection };
        store.configure()?;
        store.migrate()?;
        Ok(store)
    }

    #[cfg(test)]
    pub fn in_memory() -> Result<Self> {
        let store = Self {
            connection: Connection::open_in_memory()?,
        };
        store.configure()?;
        store.migrate()?;
        Ok(store)
    }

    fn configure(&self) -> Result<()> {
        self.connection.pragma_update(None, "journal_mode", "WAL")?;
        self.connection
            .pragma_update(None, "synchronous", "NORMAL")?;
        self.connection.pragma_update(None, "busy_timeout", 5_000)?;
        Ok(())
    }

    fn migrate(&self) -> Result<()> {
        self.connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS samples (
                id INTEGER PRIMARY KEY,
                sampled_at INTEGER NOT NULL,
                api_timestamp TEXT,
                credits_used REAL NOT NULL,
                entitlement REAL,
                remaining REAL,
                percent_remaining REAL,
                unlimited INTEGER NOT NULL DEFAULT 0,
                overage_count REAL NOT NULL DEFAULT 0,
                reset_at INTEGER,
                plan TEXT
            );
            CREATE INDEX IF NOT EXISTS samples_sampled_at_idx ON samples(sampled_at);
            CREATE TABLE IF NOT EXISTS metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            ",
        )?;
        self.set_metadata("schema_version", &SCHEMA_VERSION.to_string())
    }

    pub fn insert_snapshot(&self, snapshot: &Snapshot, raw_payload: &Value) -> Result<()> {
        self.connection.execute(
            "
            INSERT INTO samples(
                sampled_at, api_timestamp, credits_used, entitlement, remaining,
                percent_remaining, unlimited, overage_count, reset_at, plan
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ",
            params![
                snapshot.sampled_at,
                snapshot.api_timestamp,
                snapshot.credits_used,
                snapshot.entitlement,
                snapshot.remaining,
                snapshot.percent_remaining,
                snapshot.unlimited,
                snapshot.overage_count,
                snapshot.reset_at,
                snapshot.plan,
            ],
        )?;

        let compact = serde_json::to_string(raw_payload)?;
        let digest = format!("{:x}", Sha256::digest(compact.as_bytes()));
        self.set_metadata("last_payload", &compact)?;
        self.set_metadata("last_payload_sha256", &digest)
    }

    pub fn prune_if_due(&self, now: i64, retention_days: u32) -> Result<()> {
        let last_prune = self
            .connection
            .query_row(
                "SELECT value FROM metadata WHERE key = 'last_prune_at'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .and_then(|value| value.parse::<i64>().ok());
        if last_prune.is_some_and(|last| now - last < 24 * 60 * 60) {
            return Ok(());
        }

        let cutoff = now - i64::from(retention_days) * 24 * 60 * 60;
        self.connection
            .execute("DELETE FROM samples WHERE sampled_at < ?", [cutoff])?;
        self.set_metadata("last_prune_at", &now.to_string())
    }

    pub fn rows_since(&self, since: i64) -> Result<Vec<SampleRow>> {
        self.query_rows(
            "SELECT * FROM samples WHERE sampled_at >= ? ORDER BY sampled_at, id",
            [since],
        )
    }

    pub fn latest_rows(&self, count: i64) -> Result<Vec<SampleRow>> {
        let mut rows = self.query_rows(
            "SELECT * FROM samples ORDER BY sampled_at DESC, id DESC LIMIT ?",
            [count],
        )?;
        rows.reverse();
        Ok(rows)
    }

    pub fn value_at_or_before(&self, at: i64) -> Result<Option<SampleRow>> {
        self.connection
            .query_row(
                "SELECT * FROM samples WHERE sampled_at <= ? ORDER BY sampled_at DESC, id DESC LIMIT 1",
                [at],
                map_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn sample_count(&self) -> Result<u64> {
        self.connection
            .query_row("SELECT COUNT(*) FROM samples", [], |row| row.get(0))
            .map_err(Into::into)
    }

    pub fn export(&self, writer: impl Write) -> Result<()> {
        let rows = self.query_rows("SELECT * FROM samples ORDER BY sampled_at, id", [])?;
        let mut csv = csv::Writer::from_writer(writer);
        csv.write_record([
            "sampled_at",
            "sampled_at_iso",
            "credits_used",
            "entitlement",
            "remaining",
            "percent_remaining",
            "overage_count",
            "reset_at",
            "plan",
        ])?;
        for row in rows {
            let sampled_at_iso = DateTime::<Utc>::from_timestamp(row.sampled_at, 0)
                .map(|date| date.to_rfc3339())
                .unwrap_or_default();
            csv.write_record([
                row.sampled_at.to_string(),
                sampled_at_iso,
                row.credits_used.to_string(),
                optional_number(row.entitlement),
                optional_number(row.remaining),
                optional_number(row.percent_remaining),
                row.overage_count.to_string(),
                row.reset_at
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                row.plan.unwrap_or_default(),
            ])?;
        }
        csv.flush()?;
        Ok(())
    }

    fn set_metadata(&self, key: &str, value: &str) -> Result<()> {
        self.connection.execute(
            "INSERT OR REPLACE INTO metadata(key, value) VALUES(?, ?)",
            params![key, value],
        )?;
        Ok(())
    }

    fn query_rows<const N: usize>(
        &self,
        sql: &str,
        parameters: [i64; N],
    ) -> Result<Vec<SampleRow>> {
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement
            .query_map(params_from_iter(parameters), map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SampleRow> {
    Ok(SampleRow {
        id: row.get("id")?,
        sampled_at: row.get("sampled_at")?,
        api_timestamp: row.get("api_timestamp")?,
        credits_used: row.get("credits_used")?,
        entitlement: row.get("entitlement")?,
        remaining: row.get("remaining")?,
        percent_remaining: row.get("percent_remaining")?,
        unlimited: row.get("unlimited")?,
        overage_count: row.get("overage_count")?,
        reset_at: row.get("reset_at")?,
        plan: row.get("plan")?,
    })
}

fn optional_number(value: Option<f64>) -> String {
    value.map(|number| number.to_string()).unwrap_or_default()
}
