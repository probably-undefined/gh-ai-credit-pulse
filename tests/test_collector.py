import importlib.util
import json
import sqlite3
import sys
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "scripts" / "gh_ai_credits.py"
SPEC = importlib.util.spec_from_file_location("gh_ai_credits", MODULE_PATH)
assert SPEC and SPEC.loader
collector = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = collector
SPEC.loader.exec_module(collector)


class CollectorTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.db = Path(self.temp.name) / "history.sqlite3"

    def tearDown(self):
        self.temp.cleanup()

    def payload(self, used, *, entitlement=10000, remaining=None, reset="2026-09-01T00:00:00Z"):
        if remaining is None:
            remaining = entitlement - used
        return {
            "copilot_plan": "business",
            "quota_reset_date_utc": reset,
            "quota_snapshots": {
                "premium_interactions": {
                    "credits_used": used,
                    "entitlement": entitlement,
                    "quota_remaining": remaining,
                    "percent_remaining": remaining / entitlement * 100,
                    "overage_count": 0,
                    "unlimited": False,
                    "timestamp_utc": "2026-08-04T06:00:00Z",
                }
            },
        }

    def insert(self, conn, used, at):
        payload = self.payload(used)
        snapshot = collector.parse_snapshot(payload, sampled_at=at)
        collector.insert_snapshot(conn, snapshot, payload)

    def test_parses_credits_used_as_source_of_truth(self):
        fixture = json.loads((ROOT / "tests/fixtures/copilot_user.json").read_text())
        snapshot = collector.parse_snapshot(fixture, sampled_at=123)
        self.assertEqual(snapshot.credits_used, 3027)
        self.assertEqual(snapshot.remaining, 6973)
        self.assertEqual(snapshot.sampled_at, 123)

    def test_falls_back_to_entitlement_minus_remaining(self):
        payload = self.payload(12.5, entitlement=300, remaining=287.5)
        del payload["quota_snapshots"]["premium_interactions"]["credits_used"]
        snapshot = collector.parse_snapshot(payload)
        self.assertEqual(snapshot.credits_used, 12.5)

    def test_rejects_missing_premium_quota(self):
        with self.assertRaises(collector.UsageError):
            collector.parse_snapshot({"quota_snapshots": {}})

    def test_dashboard_aggregates_usage_and_rate(self):
        now = int(datetime(2026, 8, 4, 12, 0, tzinfo=timezone.utc).timestamp())
        with collector.connect(self.db) as conn:
            self.insert(conn, 100, now - 3600)
            self.insert(conn, 104, now - 1800)
            self.insert(conn, 110, now)
            dashboard = collector.build_dashboard(conn, "24h", now)

        self.assertEqual(dashboard["current"]["credits_used"], 110)
        self.assertEqual(dashboard["metrics"]["delta_1h"], 10)
        self.assertEqual(dashboard["metrics"]["rate_per_hour"], 10)
        self.assertEqual(len(dashboard["series"]), 3)

    def test_counter_reset_is_not_a_negative_usage_spike(self):
        now = 1_800_000_000
        with collector.connect(self.db) as conn:
            self.insert(conn, 299, now - 120)
            self.insert(conn, 1, now - 60)
            self.insert(conn, 3, now)
            rows = collector.rows_since(conn, now - 120)
        self.assertEqual(collector.delta_between(rows), 3)

    def test_downsampling_keeps_first_and_last(self):
        with collector.connect(self.db) as conn:
            for index in range(500):
                self.insert(conn, index, 10_000 + index)
            rows = collector.rows_since(conn, 0)
            series = collector.downsample(rows, max_points=40)
        self.assertLessEqual(len(series), 40)
        self.assertEqual(series[0]["used"], 0)
        self.assertEqual(series[-1]["used"], 499)


if __name__ == "__main__":
    unittest.main()
