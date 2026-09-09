use super::Result;
use super::model::{Current, DailyUsage, DashboardData, Metrics, UsageSample, Window};
use super::store::{SampleRow, Store};
use chrono::{Datelike, Local, LocalResult, NaiveDate, TimeZone, Utc, Weekday};

const WORKDAY_START_HOUR: u32 = 6;
const WORKDAY_END_HOUR: u32 = 19;
const MINIMUM_PACE_SECONDS: f64 = 60.0 * 60.0;

pub(crate) fn build_dashboard(store: &Store, window: Window, now: i64) -> Result<DashboardData> {
    let latest = store.latest_rows(2)?;
    if latest.is_empty() {
        return Ok(DashboardData {
            status: "empty".to_owned(),
            generated_at: now,
            window: window.as_str().to_owned(),
            message: Some("No samples yet".to_owned()),
            ..DashboardData::default()
        });
    }

    let current_row = latest.last().expect("latest is not empty");
    let current_used = current_row.credits_used;
    let selected_rows = rows_with_anchor(store, now - window.seconds())?;
    let six_hour_rows = rows_with_anchor(store, now - 6 * 60 * 60)?;

    let rate_window_hours = match (six_hour_rows.first(), six_hour_rows.last()) {
        (Some(first), Some(last)) if six_hour_rows.len() >= 2 => {
            ((last.sampled_at - first.sampled_at) as f64 / 3_600.0).max(1.0 / 120.0)
        }
        _ => 0.0,
    };
    let rate_per_hour =
        (rate_window_hours > 0.0).then(|| delta_between(&six_hour_rows) / rate_window_hours);

    let cycle_start = cycle_start(current_row.reset_at, now);
    let elapsed_days = ((now - cycle_start) as f64 / 86_400.0).max(1.0 / 24.0);
    let average_per_day = current_used / elapsed_days;
    let (projected_at_reset, pace_delta) = projection(current_row, now, cycle_start);
    let daily = daily_usage(store, now, 14)?;
    let last_seven_days = daily.iter().rev().take(7).map(|day| day.credits).sum();

    Ok(DashboardData {
        status: "ok".to_owned(),
        generated_at: now,
        window: window.as_str().to_owned(),
        sample_count: Some(store.sample_count()?),
        current: current_from_row(current_row),
        metrics: Metrics {
            delta_last_sample: Some(round(delta_between(&latest), 3)),
            delta_1h: Some(round(usage_since(store, now - 60 * 60, now)?, 3)),
            delta_today: daily.last().map(|day| day.credits),
            delta_7d: Some(round(last_seven_days, 3)),
            delta_30d: Some(round(usage_since(store, now - 30 * 86_400, now)?, 3)),
            rate_per_hour: Some(round(rate_per_hour.unwrap_or(0.0), 3)),
            average_per_day: Some(round(average_per_day, 3)),
            projected_at_reset: projected_at_reset.map(|value| round(value, 1)),
            pace_delta: pace_delta.map(|value| round(value, 1)),
        },
        series: downsample(&selected_rows, 180),
        daily,
        ..DashboardData::default()
    })
}

fn rows_with_anchor(store: &Store, since: i64) -> Result<Vec<SampleRow>> {
    let mut rows = store.rows_since(since)?;
    // A strict predecessor preserves order even with multiple samples at `since`.
    if let Some(anchor) = store.value_before(since)? {
        rows.insert(0, anchor);
    }
    Ok(rows)
}

fn usage_since(store: &Store, since: i64, now: i64) -> Result<f64> {
    let rows = rows_with_anchor(store, since)?
        .into_iter()
        .filter(|row| row.sampled_at <= now)
        .collect::<Vec<_>>();
    Ok(delta_between(&rows))
}

fn delta_between(rows: &[SampleRow]) -> f64 {
    observed_deltas(rows).map(|(_, delta)| delta).sum()
}

fn observed_deltas(rows: &[SampleRow]) -> impl Iterator<Item = (&SampleRow, f64)> {
    let mut high_water = rows.first().map_or(0.0, |row| row.credits_used);
    rows.windows(2).map(move |pair| {
        let previous = &pair[0];
        let current = &pair[1];
        // A corrected/stale counter is not evidence of a new billing cycle.
        // Keep the high-water mark so its recovery is not counted twice either.
        let new_cycle = match (previous.reset_at, current.reset_at) {
            (Some(before), Some(after)) => after > before,
            (Some(before), None) => previous.sampled_at < before && current.sampled_at >= before,
            _ => false,
        };
        if new_cycle {
            high_water = 0.0;
        }
        let delta = (current.credits_used - high_water).max(0.0);
        high_water = high_water.max(current.credits_used);
        (current, delta)
    })
}

fn downsample(rows: &[SampleRow], max_points: usize) -> Vec<UsageSample> {
    if rows.is_empty() || max_points == 0 {
        return Vec::new();
    }
    let selected = if rows.len() <= max_points {
        rows.iter().collect::<Vec<_>>()
    } else {
        (0..max_points)
            .map(|index| {
                let position = index as f64 * (rows.len() - 1) as f64 / (max_points - 1) as f64;
                &rows[position.round() as usize]
            })
            .collect::<Vec<_>>()
    };

    let mut previous = None;
    selected
        .into_iter()
        .map(|row| {
            let delta = previous.map_or(0.0, |value| {
                if row.credits_used >= value {
                    row.credits_used - value
                } else {
                    0.0
                }
            });
            previous = Some(row.credits_used);
            UsageSample {
                t: row.sampled_at,
                used: round(row.credits_used, 3),
                delta: round(delta, 3),
            }
        })
        .collect()
}

fn daily_usage(store: &Store, now: i64, days: usize) -> Result<Vec<DailyUsage>> {
    let local_now = Local
        .timestamp_opt(now, 0)
        .single()
        .unwrap_or_else(Local::now);
    let today = local_now.date_naive();
    let mut daily = (0..days)
        .rev()
        .map(|days_ago| {
            let date = today - chrono::Days::new(days_ago as u64);
            DailyUsage {
                date: date.format("%Y-%m-%d").to_string(),
                label: date.format("%a").to_string(),
                credits: 0.0,
            }
        })
        .collect::<Vec<_>>();
    if days == 0 {
        return Ok(daily);
    }
    let first_date = today - chrono::Days::new((days - 1) as u64);
    let rows = rows_with_anchor(store, local_epoch(first_date))?;
    for (row, delta) in observed_deltas(&rows) {
        if row.sampled_at > now {
            break;
        }
        let Some(local) = Local.timestamp_opt(row.sampled_at, 0).single() else {
            continue;
        };
        let index = (local.date_naive() - first_date).num_days();
        if index >= 0 {
            if let Some(day) = daily.get_mut(index as usize) {
                // Attribute each observed change once, including midnight samples.
                day.credits += delta;
            }
        }
    }
    for day in &mut daily {
        day.credits = round(day.credits, 3);
    }
    Ok(daily)
}

fn local_epoch(date: NaiveDate) -> i64 {
    let naive = date.and_hms_opt(0, 0, 0).expect("midnight is valid");
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(value) => value.timestamp(),
        LocalResult::Ambiguous(first, _) => first.timestamp(),
        LocalResult::None => naive.and_utc().timestamp(),
    }
}

fn cycle_start(reset_at: Option<i64>, now: i64) -> i64 {
    if let Some(reset_at) = reset_at.and_then(|value| Utc.timestamp_opt(value, 0).single()) {
        let (year, month) = if reset_at.month() == 1 {
            (reset_at.year() - 1, 12)
        } else {
            (reset_at.year(), reset_at.month() - 1)
        };
        return Utc
            .with_ymd_and_hms(year, month, 1, 0, 0, 0)
            .single()
            .map_or(now, |value| value.timestamp());
    }

    let local_now = Local
        .timestamp_opt(now, 0)
        .single()
        .unwrap_or_else(Local::now);
    local_epoch(
        NaiveDate::from_ymd_opt(local_now.year(), local_now.month(), 1)
            .expect("current year and month are valid"),
    )
}

fn projection(current: &SampleRow, now: i64, start: i64) -> (Option<f64>, Option<f64>) {
    projection_in(current, now, start, &Local)
}

fn projection_in<Tz: TimeZone>(
    current: &SampleRow,
    now: i64,
    start: i64,
    timezone: &Tz,
) -> (Option<f64>, Option<f64>) {
    let Some(reset_at) = current.reset_at.filter(|reset| *reset > now) else {
        return (None, None);
    };

    let elapsed_work_seconds = working_seconds_between_in(start, now, timezone);
    let remaining_work_seconds = working_seconds_between_in(now, reset_at, timezone);
    let projected = if elapsed_work_seconds > 0.0 {
        let rate_per_work_second =
            current.credits_used / elapsed_work_seconds.max(MINIMUM_PACE_SECONDS);
        current.credits_used + rate_per_work_second * remaining_work_seconds
    } else {
        current.credits_used
    };

    let pace = current
        .entitlement
        .filter(|_| elapsed_work_seconds + remaining_work_seconds > 0.0)
        .map(|entitlement| {
            let elapsed = elapsed_work_seconds / (elapsed_work_seconds + remaining_work_seconds);
            entitlement * elapsed - current.credits_used
        });
    (Some(projected), pace)
}

fn working_seconds_between_in<Tz: TimeZone>(start: i64, end: i64, timezone: &Tz) -> f64 {
    if end <= start {
        return 0.0;
    }

    let Some(mut date) = timezone
        .timestamp_opt(start, 0)
        .earliest()
        .map(|value| value.date_naive())
    else {
        return 0.0;
    };
    let Some(end_date) = timezone
        .timestamp_opt(end, 0)
        .earliest()
        .map(|value| value.date_naive())
    else {
        return 0.0;
    };

    let mut seconds = 0_i64;
    while date <= end_date {
        if !matches!(date.weekday(), Weekday::Sat | Weekday::Sun) {
            let work_start = local_hour_epoch(timezone, date, WORKDAY_START_HOUR);
            let work_end = local_hour_epoch(timezone, date, WORKDAY_END_HOUR);
            if let (Some(work_start), Some(work_end)) = (work_start, work_end) {
                seconds += (end.min(work_end) - start.max(work_start)).max(0);
            }
        }
        let Some(next_date) = date.succ_opt() else {
            break;
        };
        date = next_date;
    }

    seconds as f64
}

fn local_hour_epoch<Tz: TimeZone>(timezone: &Tz, date: NaiveDate, hour: u32) -> Option<i64> {
    let naive = date.and_hms_opt(hour, 0, 0)?;
    timezone
        .from_local_datetime(&naive)
        .earliest()
        .map(|value| value.timestamp())
}

fn current_from_row(row: &SampleRow) -> Current {
    Current {
        sampled_at: Some(row.sampled_at),
        api_timestamp: row.api_timestamp.clone(),
        credits_used: Some(round(row.credits_used, 3)),
        entitlement: row.entitlement,
        remaining: row.remaining,
        percent_remaining: row.percent_remaining,
        unlimited: row.unlimited,
        overage_count: Some(round(row.overage_count, 3)),
        reset_at: row.reset_at,
        plan: row.plan.clone(),
    }
}

fn round(value: f64, digits: i32) -> f64 {
    let factor = 10_f64.powi(digits);
    (value * factor).round() / factor
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::model::Snapshot;
    use serde_json::json;

    fn snapshot(used: f64, sampled_at: i64) -> Snapshot {
        Snapshot {
            sampled_at,
            api_timestamp: None,
            credits_used: used,
            entitlement: Some(10_000.0),
            remaining: Some(10_000.0 - used),
            percent_remaining: Some((10_000.0 - used) / 100.0),
            unlimited: false,
            overage_count: 0.0,
            reset_at: None,
            plan: Some("business".to_owned()),
        }
    }

    fn insert(store: &Store, used: f64, sampled_at: i64) {
        store
            .insert_snapshot(&snapshot(used, sampled_at), &json!({"used": used}))
            .unwrap();
    }

    #[test]
    fn aggregates_usage_and_rate() {
        let store = Store::in_memory().unwrap();
        let now = 1_800_000_000;
        insert(&store, 100.0, now - 3_600);
        insert(&store, 104.0, now - 1_800);
        insert(&store, 110.0, now);
        let dashboard = build_dashboard(&store, Window::OneDay, now).unwrap();

        assert_eq!(dashboard.current.credits_used, Some(110.0));
        assert_eq!(dashboard.metrics.delta_1h, Some(10.0));
        assert_eq!(dashboard.metrics.rate_per_hour, Some(10.0));
        assert_eq!(dashboard.series.len(), 3);
    }

    #[test]
    fn reports_zero_rate_with_only_one_sample() {
        let store = Store::in_memory().unwrap();
        let now = 1_800_000_000;
        insert(&store, 100.0, now);
        let dashboard = build_dashboard(&store, Window::OneDay, now).unwrap();

        assert_eq!(dashboard.metrics.rate_per_hour, Some(0.0));
    }

    #[test]
    fn counter_reset_is_not_a_negative_usage_spike() {
        let rows = [
            SampleRow {
                credits_used: 299.0,
                reset_at: Some(2),
                ..sample_row(1)
            },
            SampleRow {
                credits_used: 1.0,
                reset_at: Some(100),
                ..sample_row(2)
            },
            SampleRow {
                credits_used: 3.0,
                reset_at: Some(100),
                ..sample_row(3)
            },
        ];
        assert_eq!(delta_between(&rows), 3.0);
    }

    #[test]
    fn counter_corrections_and_recovery_do_not_add_usage() {
        let rows = [100.0, 95.0, 100.0, 103.0].map(|credits_used| SampleRow {
            credits_used,
            reset_at: Some(100),
            ..sample_row(1)
        });
        assert_eq!(delta_between(&rows), 3.0);
    }

    #[test]
    fn new_cycle_counts_usage_even_when_counter_exceeds_previous_cycle() {
        let rows = [
            SampleRow {
                credits_used: 10.0,
                reset_at: Some(2),
                ..sample_row(1)
            },
            SampleRow {
                credits_used: 20.0,
                reset_at: Some(100),
                ..sample_row(3)
            },
        ];
        assert_eq!(delta_between(&rows), 20.0);
    }

    #[test]
    fn midnight_samples_belong_to_the_new_day_once() {
        let store = Store::in_memory().unwrap();
        let midnight = local_epoch(NaiveDate::from_ymd_opt(2026, 9, 9).unwrap());
        insert(&store, 100.0, midnight - 60);
        insert(&store, 110.0, midnight);
        insert(&store, 112.0, midnight);
        insert(&store, 115.0, midnight + 60);
        insert(&store, 999.0, midnight + 120); // Future sample must not enter the chart.

        let daily = daily_usage(&store, midnight + 60, 2).unwrap();
        assert_eq!(daily[0].credits, 0.0);
        assert_eq!(daily[1].credits, 15.0);
        assert_eq!(usage_since(&store, midnight, midnight + 60).unwrap(), 15.0);
    }

    #[test]
    fn seven_day_total_matches_calendar_bars_and_today() {
        let store = Store::in_memory().unwrap();
        let first = NaiveDate::from_ymd_opt(2026, 9, 3).unwrap();
        insert(&store, 50.0, local_epoch(first) - 3_600);
        for offset in 0..7 {
            let date = first + chrono::Days::new(offset);
            insert(&store, 60.0 + offset as f64 * 10.0, local_epoch(date));
        }
        let now = local_epoch(first + chrono::Days::new(6)) + 3_600;
        let data = build_dashboard(&store, Window::SevenDays, now).unwrap();
        assert_eq!(data.metrics.delta_7d, Some(70.0));
        assert_eq!(data.metrics.delta_today, Some(10.0));
        assert_eq!(
            data.daily
                .iter()
                .rev()
                .take(7)
                .map(|day| day.credits)
                .sum::<f64>(),
            70.0
        );
    }

    #[test]
    fn calendar_days_survive_dst_and_missing_samples() {
        // Run in Europe/Berlin as well as UTC: these ranges cross both DST changes.
        for (month, day) in [(3, 29), (10, 25)] {
            let store = Store::in_memory().unwrap();
            let date = NaiveDate::from_ymd_opt(2026, month, day).unwrap();
            insert(&store, 100.0, local_epoch(date - chrono::Days::new(2)));
            insert(&store, 120.0, local_epoch(date));
            insert(&store, 130.0, local_epoch(date + chrono::Days::new(1)));
            let daily = daily_usage(&store, local_epoch(date + chrono::Days::new(1)), 3).unwrap();
            assert_eq!(
                daily.iter().map(|day| day.credits).collect::<Vec<_>>(),
                vec![0.0, 20.0, 10.0]
            );
            assert_eq!(daily[1].date, date.to_string());
        }
    }

    #[test]
    fn first_sample_is_a_baseline_not_a_day_of_spending() {
        let store = Store::in_memory().unwrap();
        let now = local_epoch(NaiveDate::from_ymd_opt(2026, 9, 9).unwrap());
        insert(&store, 22_982.0, now);
        let data = build_dashboard(&store, Window::SevenDays, now).unwrap();
        assert_eq!(data.metrics.delta_7d, Some(0.0));
        assert_eq!(data.metrics.delta_today, Some(0.0));
    }

    #[test]
    fn downsampling_keeps_first_and_last() {
        let rows = (0..500)
            .map(|index| SampleRow {
                sampled_at: 10_000 + index,
                credits_used: index as f64,
                ..sample_row(index)
            })
            .collect::<Vec<_>>();
        let series = downsample(&rows, 40);
        assert!(series.len() <= 40);
        assert_eq!(series.first().unwrap().used, 0.0);
        assert_eq!(series.last().unwrap().used, 499.0);
    }

    #[test]
    fn working_time_skips_weekends_and_clips_to_workday_hours() {
        let friday_at_18 = timestamp(2026, 9, 4, 18);
        let monday_at_7 = timestamp(2026, 9, 7, 7);

        assert_eq!(
            working_seconds_between_in(friday_at_18, monday_at_7, &Utc),
            2.0 * 60.0 * 60.0
        );

        let monday_midnight = timestamp(2026, 9, 7, 0);
        let tuesday_at_23 = timestamp(2026, 9, 8, 23);
        assert_eq!(
            working_seconds_between_in(monday_midnight, tuesday_at_23, &Utc),
            2.0 * 13.0 * 60.0 * 60.0
        );
    }

    #[test]
    fn projection_uses_only_elapsed_and_remaining_work_time() {
        let start = timestamp(2026, 9, 4, 6);
        let now = timestamp(2026, 9, 4, 19);
        let reset_at = timestamp(2026, 9, 7, 19);
        let current = SampleRow {
            credits_used: 40.0,
            entitlement: Some(100.0),
            reset_at: Some(reset_at),
            ..sample_row(1)
        };

        let (projected, pace) = projection_in(&current, now, start, &Utc);

        assert_eq!(projected, Some(80.0));
        assert_eq!(pace, Some(10.0));
    }

    #[test]
    fn projection_adds_nothing_during_a_weekend() {
        let start = timestamp(2026, 9, 1, 6);
        let friday_at_19 = timestamp(2026, 9, 4, 19);
        let monday_at_6 = timestamp(2026, 9, 7, 6);
        let current = SampleRow {
            credits_used: 120.0,
            reset_at: Some(monday_at_6),
            ..sample_row(1)
        };

        let (projected, _) = projection_in(&current, friday_at_19, start, &Utc);

        assert_eq!(projected, Some(120.0));
    }

    fn timestamp(year: i32, month: u32, day: u32, hour: u32) -> i64 {
        Utc.with_ymd_and_hms(year, month, day, hour, 0, 0)
            .single()
            .unwrap()
            .timestamp()
    }

    fn sample_row(sampled_at: i64) -> SampleRow {
        SampleRow {
            sampled_at,
            api_timestamp: None,
            credits_used: 0.0,
            entitlement: None,
            remaining: None,
            percent_remaining: None,
            unlimited: false,
            overage_count: 0.0,
            reset_at: None,
            plan: None,
        }
    }
}
