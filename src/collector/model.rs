use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window {
    OneHour,
    SixHours,
    OneDay,
    SevenDays,
    ThirtyDays,
}

impl Window {
    pub const VALUES: [&'static str; 5] = ["1h", "6h", "24h", "7d", "30d"];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OneHour => "1h",
            Self::SixHours => "6h",
            Self::OneDay => "24h",
            Self::SevenDays => "7d",
            Self::ThirtyDays => "30d",
        }
    }

    pub fn seconds(self) -> i64 {
        match self {
            Self::OneHour => 60 * 60,
            Self::SixHours => 6 * 60 * 60,
            Self::OneDay => 24 * 60 * 60,
            Self::SevenDays => 7 * 24 * 60 * 60,
            Self::ThirtyDays => 30 * 24 * 60 * 60,
        }
    }
}

impl FromStr for Window {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "1h" => Ok(Self::OneHour),
            "6h" => Ok(Self::SixHours),
            "24h" => Ok(Self::OneDay),
            "7d" => Ok(Self::SevenDays),
            "30d" => Ok(Self::ThirtyDays),
            other => Err(format!(
                "invalid window {other:?}; expected one of {}",
                Self::VALUES.join(", ")
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Snapshot {
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DashboardData {
    pub status: String,
    pub generated_at: i64,
    pub window: String,
    pub fresh: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_count: Option<u64>,
    pub current: Current,
    pub metrics: Metrics,
    pub daily: Vec<DailyUsage>,
    pub series: Vec<UsageSample>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Current {
    pub sampled_at: Option<i64>,
    pub api_timestamp: Option<String>,
    pub credits_used: Option<f64>,
    pub entitlement: Option<f64>,
    pub remaining: Option<f64>,
    pub percent_remaining: Option<f64>,
    pub unlimited: bool,
    pub overage_count: Option<f64>,
    pub reset_at: Option<i64>,
    pub plan: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Metrics {
    pub delta_last_sample: Option<f64>,
    pub delta_1h: Option<f64>,
    pub delta_today: Option<f64>,
    pub delta_7d: Option<f64>,
    pub delta_30d: Option<f64>,
    pub rate_per_hour: Option<f64>,
    pub average_per_day: Option<f64>,
    pub projected_at_reset: Option<f64>,
    pub pace_delta: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DailyUsage {
    pub date: String,
    pub label: String,
    pub credits: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UsageSample {
    pub t: i64,
    pub used: f64,
    pub delta: f64,
}
