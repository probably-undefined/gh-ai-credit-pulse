#!/usr/bin/env python3
"""Collect and summarize GitHub Copilot AI credit usage.

The only external runtime dependency is the authenticated GitHub CLI (`gh`).
All history is stored locally in SQLite.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
import sqlite3
import subprocess
import sys
import time
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable, Sequence


SCHEMA_VERSION = 1
DEFAULT_RETENTION_DAYS = 180
WINDOWS = {
    "1h": 60 * 60,
    "6h": 6 * 60 * 60,
    "24h": 24 * 60 * 60,
    "7d": 7 * 24 * 60 * 60,
    "30d": 30 * 24 * 60 * 60,
}


class UsageError(RuntimeError):
    """An expected fetch or payload error."""


@dataclass(frozen=True)
class Snapshot:
    sampled_at: int
    api_timestamp: str | None
    credits_used: float
    entitlement: float | None
    remaining: float | None
    percent_remaining: float | None
    unlimited: bool
    overage_count: float
    reset_at: int | None
    plan: str | None


def default_db_path() -> Path:
    override = os.environ.get("GH_AI_CREDITS_DB")
    if override:
        return Path(override).expanduser()
    state_home = Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state"))
    return state_home / "gh-ai-credits" / "history.sqlite3"


def iso_to_epoch(value: str | None) -> int | None:
    if not value:
        return None
    normalized = value.strip()
    if len(normalized) == 10:
        normalized += "T00:00:00+00:00"
    elif normalized.endswith("Z"):
        normalized = normalized[:-1] + "+00:00"
    try:
        parsed = datetime.fromisoformat(normalized)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=timezone.utc)
    return int(parsed.timestamp())


def number(value: Any) -> float | None:
    if value is None or isinstance(value, bool):
        return None
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def parse_snapshot(payload: dict[str, Any], sampled_at: int | None = None) -> Snapshot:
    snapshots = payload.get("quota_snapshots")
    if not isinstance(snapshots, dict):
        raise UsageError("response has no quota_snapshots object")
    premium = snapshots.get("premium_interactions")
    if not isinstance(premium, dict):
        raise UsageError("response has no premium_interactions quota")

    entitlement = number(premium.get("entitlement"))
    remaining = number(premium.get("quota_remaining"))
    if remaining is None:
        remaining = number(premium.get("remaining"))

    credits_used = number(premium.get("credits_used"))
    if credits_used is None and entitlement is not None and remaining is not None:
        credits_used = max(0.0, entitlement - remaining)
    if credits_used is None:
        raise UsageError("premium_interactions contains neither credits_used nor usable quota totals")

    reset_value = payload.get("quota_reset_date_utc") or payload.get("quota_reset_date")
    return Snapshot(
        sampled_at=int(sampled_at if sampled_at is not None else time.time()),
        api_timestamp=premium.get("timestamp_utc"),
        credits_used=credits_used,
        entitlement=entitlement,
        remaining=remaining,
        percent_remaining=number(premium.get("percent_remaining")),
        unlimited=bool(premium.get("unlimited", False)),
        overage_count=number(premium.get("overage_count")) or 0.0,
        reset_at=iso_to_epoch(reset_value if isinstance(reset_value, str) else None),
        plan=payload.get("copilot_plan") if isinstance(payload.get("copilot_plan"), str) else None,
    )


def fetch_payload(timeout: float = 20.0) -> dict[str, Any]:
    fixture = os.environ.get("GH_AI_CREDITS_FIXTURE")
    if fixture:
        try:
            return json.loads(Path(fixture).read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            raise UsageError(f"cannot read fixture: {exc}") from exc

    try:
        completed = subprocess.run(
            ["gh", "api", "/copilot_internal/user"],
            check=False,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except FileNotFoundError as exc:
        raise UsageError("GitHub CLI not found; install `gh` and run `gh auth login`") from exc
    except subprocess.TimeoutExpired as exc:
        raise UsageError(f"GitHub API request timed out after {timeout:g}s") from exc

    if completed.returncode != 0:
        message = completed.stderr.strip() or completed.stdout.strip() or "unknown gh error"
        raise UsageError(message.splitlines()[-1][:400])
    try:
        payload = json.loads(completed.stdout)
    except json.JSONDecodeError as exc:
        raise UsageError("GitHub CLI returned invalid JSON") from exc
    if not isinstance(payload, dict):
        raise UsageError("GitHub API response is not a JSON object")
    return payload


def connect(db_path: Path) -> sqlite3.Connection:
    db_path.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(db_path, timeout=5.0)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("PRAGMA synchronous=NORMAL")
    conn.execute("PRAGMA busy_timeout=5000")
    migrate(conn)
    return conn


def migrate(conn: sqlite3.Connection) -> None:
    conn.executescript(
        """
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
        """
    )
    conn.execute(
        "INSERT OR REPLACE INTO metadata(key, value) VALUES('schema_version', ?)",
        (str(SCHEMA_VERSION),),
    )
    conn.commit()


def insert_snapshot(conn: sqlite3.Connection, snapshot: Snapshot, raw_payload: dict[str, Any]) -> None:
    values = asdict(snapshot)
    conn.execute(
        """
        INSERT INTO samples(
            sampled_at, api_timestamp, credits_used, entitlement, remaining,
            percent_remaining, unlimited, overage_count, reset_at, plan
        ) VALUES(
            :sampled_at, :api_timestamp, :credits_used, :entitlement, :remaining,
            :percent_remaining, :unlimited, :overage_count, :reset_at, :plan
        )
        """,
        values,
    )
    compact = json.dumps(raw_payload, separators=(",", ":"), sort_keys=True)
    conn.execute("INSERT OR REPLACE INTO metadata(key, value) VALUES('last_payload', ?)", (compact,))
    conn.execute(
        "INSERT OR REPLACE INTO metadata(key, value) VALUES('last_payload_sha256', ?)",
        (hashlib.sha256(compact.encode()).hexdigest(),),
    )
    conn.commit()


def prune_if_due(conn: sqlite3.Connection, now: int, retention_days: int) -> None:
    row = conn.execute("SELECT value FROM metadata WHERE key='last_prune_at'").fetchone()
    if row and now - int(row["value"]) < 24 * 60 * 60:
        return
    cutoff = now - retention_days * 24 * 60 * 60
    conn.execute("DELETE FROM samples WHERE sampled_at < ?", (cutoff,))
    conn.execute(
        "INSERT OR REPLACE INTO metadata(key, value) VALUES('last_prune_at', ?)",
        (str(now),),
    )
    conn.commit()


def rows_since(conn: sqlite3.Connection, since: int) -> list[sqlite3.Row]:
    return list(
        conn.execute(
            "SELECT * FROM samples WHERE sampled_at >= ? ORDER BY sampled_at, id",
            (since,),
        )
    )


def latest_rows(conn: sqlite3.Connection, count: int = 2) -> list[sqlite3.Row]:
    rows = list(conn.execute("SELECT * FROM samples ORDER BY sampled_at DESC, id DESC LIMIT ?", (count,)))
    rows.reverse()
    return rows


def delta_between(rows: Sequence[sqlite3.Row]) -> float:
    """Accumulate positive deltas while treating counter decreases as resets."""
    if len(rows) < 2:
        return 0.0
    total = 0.0
    previous = float(rows[0]["credits_used"])
    for row in rows[1:]:
        current = float(row["credits_used"])
        if current >= previous:
            total += current - previous
        else:
            total += max(0.0, current)
        previous = current
    return total


def value_at_or_before(conn: sqlite3.Connection, at: int) -> sqlite3.Row | None:
    return conn.execute(
        "SELECT * FROM samples WHERE sampled_at <= ? ORDER BY sampled_at DESC, id DESC LIMIT 1",
        (at,),
    ).fetchone()


def usage_since(conn: sqlite3.Connection, since: int, now: int) -> float:
    rows = rows_since(conn, since)
    anchor = value_at_or_before(conn, since)
    if anchor is not None and (not rows or anchor["id"] != rows[0]["id"]):
        rows.insert(0, anchor)
    if rows and rows[-1]["sampled_at"] > now:
        rows = [row for row in rows if row["sampled_at"] <= now]
    return delta_between(rows)


def local_midnight_epoch(now: int) -> int:
    local = datetime.fromtimestamp(now).astimezone()
    midnight = local.replace(hour=0, minute=0, second=0, microsecond=0)
    return int(midnight.timestamp())


def downsample(rows: Sequence[sqlite3.Row], max_points: int = 180) -> list[dict[str, float | int]]:
    if not rows:
        return []
    if len(rows) <= max_points:
        selected = list(rows)
    else:
        step = (len(rows) - 1) / (max_points - 1)
        indices = sorted({round(index * step) for index in range(max_points)})
        selected = [rows[index] for index in indices]

    result: list[dict[str, float | int]] = []
    previous: float | None = None
    for row in selected:
        used = float(row["credits_used"])
        delta = 0.0 if previous is None or used < previous else used - previous
        result.append({"t": int(row["sampled_at"]), "used": round(used, 3), "delta": round(delta, 3)})
        previous = used
    return result


def daily_usage(conn: sqlite3.Connection, now: int, days: int = 14) -> list[dict[str, Any]]:
    local_now = datetime.fromtimestamp(now).astimezone()
    result: list[dict[str, Any]] = []
    for days_ago in range(days - 1, -1, -1):
        day = local_now.date().fromordinal(local_now.date().toordinal() - days_ago)
        start_dt = datetime.combine(day, datetime.min.time(), tzinfo=local_now.tzinfo)
        end_dt = datetime.combine(
            day.fromordinal(day.toordinal() + 1), datetime.min.time(), tzinfo=local_now.tzinfo
        )
        result.append(
            {
                "date": day.isoformat(),
                "label": day.strftime("%a")[0],
                "credits": round(usage_since(conn, int(start_dt.timestamp()), min(now, int(end_dt.timestamp()))), 3),
            }
        )
    return result


def cycle_start(reset_at: int | None, now: int) -> int:
    if reset_at:
        reset_dt = datetime.fromtimestamp(reset_at, timezone.utc)
        month = reset_dt.month - 1
        year = reset_dt.year
        if month == 0:
            month = 12
            year -= 1
        return int(datetime(year, month, 1, tzinfo=timezone.utc).timestamp())
    local = datetime.fromtimestamp(now).astimezone()
    return int(local.replace(day=1, hour=0, minute=0, second=0, microsecond=0).timestamp())


def current_dict(row: sqlite3.Row) -> dict[str, Any]:
    return {
        "sampled_at": int(row["sampled_at"]),
        "api_timestamp": row["api_timestamp"],
        "credits_used": round(float(row["credits_used"]), 3),
        "entitlement": row["entitlement"],
        "remaining": row["remaining"],
        "percent_remaining": row["percent_remaining"],
        "unlimited": bool(row["unlimited"]),
        "overage_count": round(float(row["overage_count"]), 3),
        "reset_at": row["reset_at"],
        "plan": row["plan"],
    }


def build_dashboard(conn: sqlite3.Connection, window: str, now: int | None = None) -> dict[str, Any]:
    now = int(now if now is not None else time.time())
    latest = latest_rows(conn, 2)
    if not latest:
        return {
            "status": "empty",
            "generated_at": now,
            "window": window,
            "message": "No samples yet",
            "series": [],
            "daily": [],
        }

    current = latest[-1]
    current_used = float(current["credits_used"])
    selected_rows = rows_since(conn, now - WINDOWS[window])
    anchor = value_at_or_before(conn, now - WINDOWS[window])
    if anchor is not None and (not selected_rows or anchor["id"] != selected_rows[0]["id"]):
        selected_rows.insert(0, anchor)

    last_delta = delta_between(latest)
    one_hour = usage_since(conn, now - 60 * 60, now)
    six_hour_rows = rows_since(conn, now - 6 * 60 * 60)
    six_hour_anchor = value_at_or_before(conn, now - 6 * 60 * 60)
    if six_hour_anchor is not None and (not six_hour_rows or six_hour_anchor["id"] != six_hour_rows[0]["id"]):
        six_hour_rows.insert(0, six_hour_anchor)
    rate_window_hours = 0.0
    if len(six_hour_rows) >= 2:
        rate_window_hours = max(1 / 120, (six_hour_rows[-1]["sampled_at"] - six_hour_rows[0]["sampled_at"]) / 3600)
    rate_per_hour = delta_between(six_hour_rows) / rate_window_hours if rate_window_hours else 0.0

    start = cycle_start(current["reset_at"], now)
    elapsed_days = max(1 / 24, (now - start) / 86400)
    average_per_day = current_used / elapsed_days
    reset_at = int(current["reset_at"]) if current["reset_at"] is not None else None
    projected_at_reset = None
    pace_delta = None
    if reset_at and reset_at > now:
        projected_at_reset = current_used + average_per_day * ((reset_at - now) / 86400)
        entitlement = number(current["entitlement"])
        if entitlement is not None and reset_at > start:
            expected = entitlement * min(1.0, max(0.0, (now - start) / (reset_at - start)))
            pace_delta = expected - current_used

    return {
        "status": "ok",
        "generated_at": now,
        "window": window,
        "sample_count": conn.execute("SELECT COUNT(*) FROM samples").fetchone()[0],
        "current": current_dict(current),
        "metrics": {
            "delta_last_sample": round(last_delta, 3),
            "delta_1h": round(one_hour, 3),
            "delta_today": round(usage_since(conn, local_midnight_epoch(now), now), 3),
            "delta_7d": round(usage_since(conn, now - 7 * 86400, now), 3),
            "delta_30d": round(usage_since(conn, now - 30 * 86400, now), 3),
            "rate_per_hour": round(rate_per_hour, 3),
            "average_per_day": round(average_per_day, 3),
            "projected_at_reset": round(projected_at_reset, 1) if projected_at_reset is not None else None,
            "pace_delta": round(pace_delta, 1) if pace_delta is not None else None,
        },
        "series": downsample(selected_rows),
        "daily": daily_usage(conn, now),
    }


def emit(payload: dict[str, Any]) -> None:
    print(json.dumps(payload, separators=(",", ":"), allow_nan=False))


def command_sample(args: argparse.Namespace) -> int:
    now = int(time.time())
    with connect(args.db) as conn:
        try:
            payload = fetch_payload(args.timeout)
            snapshot = parse_snapshot(payload, sampled_at=now)
            insert_snapshot(conn, snapshot, payload)
            prune_if_due(conn, now, args.retention_days)
            result = build_dashboard(conn, args.window, now)
            result["fresh"] = True
            emit(result)
            return 0
        except UsageError as exc:
            result = build_dashboard(conn, args.window, now)
            result.update({"status": "error", "fresh": False, "error": str(exc)})
            emit(result)
            return 2


def command_dashboard(args: argparse.Namespace) -> int:
    with connect(args.db) as conn:
        result = build_dashboard(conn, args.window)
        result["fresh"] = False
        emit(result)
    return 0


def command_export(args: argparse.Namespace) -> int:
    with connect(args.db) as conn:
        rows = conn.execute("SELECT * FROM samples ORDER BY sampled_at, id")
        output = sys.stdout if args.output == "-" else open(args.output, "w", newline="", encoding="utf-8")
        try:
            writer = csv.writer(output)
            writer.writerow(
                [
                    "sampled_at",
                    "sampled_at_iso",
                    "credits_used",
                    "entitlement",
                    "remaining",
                    "percent_remaining",
                    "overage_count",
                    "reset_at",
                    "plan",
                ]
            )
            for row in rows:
                writer.writerow(
                    [
                        row["sampled_at"],
                        datetime.fromtimestamp(row["sampled_at"], timezone.utc).isoformat(),
                        row["credits_used"],
                        row["entitlement"],
                        row["remaining"],
                        row["percent_remaining"],
                        row["overage_count"],
                        row["reset_at"],
                        row["plan"],
                    ]
                )
        finally:
            if output is not sys.stdout:
                output.close()
    return 0


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--db", type=Path, default=default_db_path(), help="SQLite history path")
    sub = result.add_subparsers(dest="command", required=True)

    sample = sub.add_parser("sample", help="fetch, persist, and print dashboard JSON")
    sample.add_argument("--window", choices=WINDOWS, default="24h")
    sample.add_argument("--timeout", type=float, default=20.0)
    sample.add_argument("--retention-days", type=int, default=DEFAULT_RETENTION_DAYS)
    sample.set_defaults(func=command_sample)

    dashboard = sub.add_parser("dashboard", help="print dashboard JSON without fetching")
    dashboard.add_argument("--window", choices=WINDOWS, default="24h")
    dashboard.set_defaults(func=command_dashboard)

    export = sub.add_parser("export", help="export all normalized samples as CSV")
    export.add_argument("--output", "-o", default="-", help="CSV path or - for stdout")
    export.set_defaults(func=command_export)
    return result


def main(argv: Sequence[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        return int(args.func(args))
    except (OSError, sqlite3.Error) as exc:
        emit({"status": "error", "fresh": False, "error": str(exc)})
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
