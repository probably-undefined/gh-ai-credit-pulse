use chrono::{Datelike, NaiveDate, Weekday};
use clap::Parser;
use gh_ai_credit_pulse::collector::{
    Collector, Current, DailyUsage, DashboardData, Metrics, UsageSample, Window, default_db_path,
};
use iced::widget::{Row, Space, button, column, container, progress_bar, row, text};
use iced::{
    Alignment, Background, Border, Color, Degrees, Element, Gradient, Length, Size, Subscription,
    Task, Theme, gradient, window,
};
use std::time::Duration;

// Palette. The GNOME extension stylesheet mirrors these values so both
// surfaces read as one product: violet is the brand accent, mint marks
// "live", "remaining", and "today", amber warns, and the error tone is
// reserved for failures.
const BACKGROUND: Color = Color::from_rgb(0.027, 0.035, 0.063);
const SURFACE: Color = Color::from_rgb(0.059, 0.078, 0.125);
const SURFACE_HIGH: Color = Color::from_rgb(0.075, 0.094, 0.145);
const HERO_TINT: Color = Color::from_rgb(0.114, 0.082, 0.216);
const BORDER: Color = Color::from_rgb(0.157, 0.196, 0.294);
const TEXT: Color = Color::from_rgb(0.973, 0.980, 1.0);
const MUTED: Color = Color::from_rgb(0.620, 0.663, 0.753);
const VIOLET: Color = Color::from_rgb(0.545, 0.361, 0.965);
const MINT: Color = Color::from_rgb(0.204, 0.827, 0.600);
const AMBER: Color = Color::from_rgb(0.984, 0.749, 0.141);
const ERROR: Color = Color::from_rgb(0.984, 0.443, 0.522);

// Type scale. Nothing renders below 11px so labels stay legible on a
// 1080p desktop without display scaling.
const TYPE_DISPLAY: f32 = 56.0;
const TYPE_STAT: f32 = 30.0;
const TYPE_CARD: f32 = 28.0;
const TYPE_TITLE: f32 = 18.0;
const TYPE_BODY: f32 = 13.0;
const TYPE_LABEL: f32 = 12.0;
const TYPE_KICKER: f32 = 11.0;

const CARD_RADIUS: f32 = 16.0;
const INSET_RADIUS: f32 = 12.0;
const CONTROL_RADIUS: f32 = 10.0;
const BAR_RADIUS: f32 = 5.0;
const CHART_DAYS: usize = 14;
const ALLOWANCE_WARNING: f32 = 0.75;
const ALLOWANCE_CRITICAL: f32 = 0.90;
const APPLICATION_ID: &str = "io.github.probably_undefined.GhAiCreditPulse";

#[derive(Debug, Parser)]
#[command(
    name = "gh-ai-credit-pulse",
    version,
    about = "Open the GitHub Copilot AI credit dashboard"
)]
struct GuiArgs {}

fn main() -> iced::Result {
    GuiArgs::parse();
    run_gui()
}

fn run_gui() -> iced::Result {
    iced::application(Dashboard::boot, Dashboard::update, Dashboard::view)
        .title("Copilot Usage")
        .theme(app_theme)
        .window(window::Settings {
            size: Size::new(1120.0, 760.0),
            min_size: Some(Size::new(880.0, 640.0)),
            platform_specific: window::settings::PlatformSpecific {
                application_id: APPLICATION_ID.to_owned(),
                ..Default::default()
            },
            ..Default::default()
        })
        .subscription(Dashboard::subscription)
        .run()
}

fn app_theme(_: &Dashboard) -> Theme {
    Theme::Dark
}

#[derive(Debug, Clone)]
enum Message {
    Refresh,
    Tick,
    Loaded(Result<DashboardData, String>),
}

#[derive(Debug, Default)]
struct Dashboard {
    data: DashboardData,
    refreshing: bool,
    error: Option<String>,
}

impl Dashboard {
    fn boot() -> (Self, Task<Message>) {
        (
            Self {
                refreshing: true,
                ..Self::default()
            },
            load_dashboard(false),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Refresh if !self.refreshing => {
                self.refreshing = true;
                load_dashboard(true)
            }
            Message::Tick if !self.refreshing => {
                self.refreshing = true;
                load_dashboard(false)
            }
            Message::Loaded(Ok(data)) => {
                self.refreshing = false;
                self.error = data.error.clone();
                self.data = data;
                Task::none()
            }
            Message::Loaded(Err(error)) => {
                self.refreshing = false;
                self.error = Some(error);
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        iced::time::every(Duration::from_secs(30)).map(|_| Message::Tick)
    }

    fn view(&self) -> Element<'_, Message> {
        let current = &self.data.current;
        let metrics = &self.data.metrics;
        let used = current.credits_used.unwrap_or(0.0);
        let entitlement = current.entitlement.filter(|value| *value > 0.0);

        let status = if self.refreshing {
            status_pill("SYNCING", MUTED)
        } else if self.error.is_some() {
            status_pill("CACHED", AMBER)
        } else {
            status_pill("● LIVE", MINT)
        };

        let brand = column![
            text("COPILOT USAGE").size(TYPE_TITLE).color(TEXT),
            text("LOCAL COST HISTORY").size(TYPE_KICKER).color(MUTED),
        ]
        .spacing(3);

        let header = row![
            brand,
            Space::new().width(Length::Fill),
            status,
            button(
                text(if self.refreshing {
                    "Syncing…"
                } else {
                    "↻  Refresh"
                })
                .size(TYPE_LABEL)
            )
            .on_press_maybe((!self.refreshing).then_some(Message::Refresh))
            .padding([9, 14])
            .style(refresh_button_style),
        ]
        .spacing(12)
        .align_y(Alignment::Center);

        let hero_copy = column![
            kicker("CURRENT BILLING CYCLE", VIOLET),
            text(money(current.credits_used, false))
                .size(TYPE_DISPLAY)
                .color(TEXT),
            row![
                text(format!("{} AI credits", number(current.credits_used)))
                    .size(TYPE_BODY)
                    .color(MUTED),
                dot(),
                text("100 AIC = $1.00").size(TYPE_BODY).color(MUTED),
            ]
            .spacing(9)
            .align_y(Alignment::Center),
        ]
        .spacing(7)
        .width(Length::FillPortion(3));

        let allowance_note = if entitlement.is_some() {
            let (label, color) = pace_label(metrics.pace_delta);
            text(label).size(TYPE_LABEL).color(color)
        } else {
            text("Allowance not reported by GitHub")
                .size(TYPE_LABEL)
                .color(MUTED)
        };
        let cycle_summary = column![
            kicker("PROJECTED TOTAL", MUTED),
            text(money(metrics.projected_at_reset, false))
                .size(TYPE_STAT)
                .color(TEXT),
            text(format!(
                "{}/day cycle average",
                money(metrics.average_per_day, false)
            ))
            .size(TYPE_LABEL)
            .color(MUTED),
            text("Forecast counts weekdays 06:00–19:00")
                .size(TYPE_LABEL)
                .color(MUTED),
            text(format!(
                "{}  ·  {} samples",
                reset_text(current.reset_at),
                self.data.sample_count.unwrap_or(0)
            ))
            .size(TYPE_LABEL)
            .color(MUTED),
            allowance_note,
        ]
        .spacing(7)
        .width(Length::FillPortion(2));

        let hero = container(
            row![hero_copy, vertical_divider(), cycle_summary]
                .spacing(28)
                .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([22, 26])
        .style(|_| hero_style());

        let metrics_row = row![
            metric_card(
                "TODAY",
                money(metrics.delta_today, true),
                "Since local midnight".to_owned(),
            ),
            metric_card(
                "LAST HOUR",
                money(metrics.delta_1h, true),
                "Recorded usage".to_owned(),
            ),
            metric_card(
                "6-HOUR RATE",
                format!("{}/h", money(metrics.rate_per_hour, false)),
                "Average over the last six hours".to_owned(),
            ),
        ]
        .spacing(14);

        let chart = usage_chart(&self.data.daily, &self.data.series, self.data.sample_count);
        let lower: Element<'_, Message> = if let Some(entitlement) = entitlement {
            let fraction = (used / entitlement).clamp(0.0, 1.0) as f32;
            row![
                chart,
                allowance_panel(current, metrics, used, entitlement, fraction)
            ]
            .spacing(14)
            .height(Length::Fill)
            .into()
        } else {
            chart
        };

        let footer = text(format!(
            "{} · history stored locally · updates every 30 seconds",
            current.plan.as_deref().unwrap_or("GitHub Copilot")
        ))
        .size(TYPE_KICKER)
        .color(MUTED);

        // Problems surface directly under the header, where the eye lands
        // first, instead of trailing the footer.
        let mut content = column![header].spacing(16);
        if let Some(error) = &self.error {
            content = content.push(error_banner(error));
        }
        content = content
            .push(hero)
            .push(metrics_row)
            .push(lower)
            .push(footer);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding([22, 28])
            .style(|_| container::Style {
                background: Some(Background::Color(BACKGROUND)),
                text_color: Some(TEXT),
                ..container::Style::default()
            })
            .into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DayKind {
    Weekday,
    Weekend,
    Today,
}

impl DayKind {
    fn color(self) -> Color {
        match self {
            Self::Weekday => VIOLET,
            Self::Weekend => MUTED,
            Self::Today => MINT,
        }
    }
}

fn usage_chart<'a>(
    days: &[DailyUsage],
    series: &[UsageSample],
    sample_count: Option<u64>,
) -> Element<'a, Message> {
    let start = days.len().saturating_sub(CHART_DAYS);
    let visible = &days[start..];
    let active_days = visible.iter().filter(|day| day.credits > 0.0).count();
    let daily_ready = active_days >= 2;
    let mut bars: Row<'a, Message> = Row::new()
        .spacing(7)
        .height(Length::Fill)
        .align_y(Alignment::End);
    let mut legend: Option<Element<'a, Message>> = None;

    let (title, subtitle, summary) = if daily_ready {
        let max_credits = visible
            .iter()
            .map(|day| day.credits)
            .fold(0.0_f64, f64::max)
            .max(1.0);
        for (index, day) in visible.iter().enumerate() {
            let today = index + 1 == visible.len();
            let kind = day_kind(&day.date, today);
            let label = if today {
                "Today".to_owned()
            } else {
                weekday_initial(day)
            };
            bars = bars.push(chart_bar(
                (day.credits / max_credits).clamp(0.0, 1.0) as f32,
                kind.color(),
                label,
                today,
            ));
        }
        legend = Some(chart_legend());
        (
            "USAGE HISTORY".to_owned(),
            date_range(visible).unwrap_or_else(|| format!("Last {CHART_DAYS} days")),
            money(Some(visible.iter().map(|day| day.credits).sum()), false),
        )
    } else if series.len() >= 2 {
        let buckets = recent_buckets(series, 24);
        let maximum = buckets.iter().copied().fold(0.0_f64, f64::max).max(1.0);
        for (index, credits) in buckets.iter().enumerate() {
            let last = index + 1 == buckets.len();
            let label = if index == 0 {
                "Earlier".to_owned()
            } else if last {
                "Now".to_owned()
            } else {
                " ".to_owned()
            };
            let color = if last { MINT } else { VIOLET };
            bars = bars.push(chart_bar(
                (*credits / maximum).clamp(0.0, 1.0) as f32,
                color,
                label,
                last,
            ));
        }
        (
            "RECENT ACTIVITY".to_owned(),
            "Recorded samples; the daily view starts after a second day".to_owned(),
            format!("{} samples", sample_count.unwrap_or(series.len() as u64)),
        )
    } else {
        bars = bars.push(
            container(
                column![
                    text("Waiting for usage samples")
                        .size(TYPE_TITLE)
                        .color(TEXT),
                    text("The chart appears after the next refresh.")
                        .size(TYPE_LABEL)
                        .color(MUTED),
                ]
                .spacing(7)
                .align_x(Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
        );
        (
            "USAGE HISTORY".to_owned(),
            "No recorded changes yet".to_owned(),
            "—".to_owned(),
        )
    };

    let mut heading = row![
        column![
            text(title).size(TYPE_KICKER).color(MUTED),
            text(subtitle).size(TYPE_LABEL).color(MUTED),
        ]
        .spacing(3),
        Space::new().width(Length::Fill),
    ]
    .spacing(18)
    .align_y(Alignment::Center);
    if let Some(legend) = legend {
        heading = heading.push(legend);
    }
    heading = heading.push(text(summary).size(TYPE_BODY).color(TEXT));

    container(column![heading, bars].spacing(15).height(Length::Fill))
        .width(Length::FillPortion(3))
        .height(Length::Fill)
        .padding(18)
        .style(|_| elevated_style())
        .into()
}

fn day_kind(date: &str, today: bool) -> DayKind {
    if today {
        return DayKind::Today;
    }
    match parse_date(date).map(|date| date.weekday()) {
        Some(Weekday::Sat | Weekday::Sun) => DayKind::Weekend,
        _ => DayKind::Weekday,
    }
}

fn parse_date(date: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()
}

fn weekday_initial(day: &DailyUsage) -> String {
    let initial = day.label.trim();
    if initial.is_empty() {
        parse_date(&day.date).map_or_else(
            || "·".to_owned(),
            |date| date.format("%a").to_string().chars().take(1).collect(),
        )
    } else {
        initial.to_owned()
    }
}

fn date_range(days: &[DailyUsage]) -> Option<String> {
    let first = parse_date(&days.first()?.date)?;
    let last = parse_date(&days.last()?.date)?;
    Some(format!("{} – {}", short_date(first), short_date(last)))
}

fn short_date(date: NaiveDate) -> String {
    format!("{} {}", date.format("%b"), date.day())
}

fn recent_buckets(series: &[UsageSample], maximum: usize) -> Vec<f64> {
    let chunk_size = series.len().div_ceil(maximum);
    series
        .chunks(chunk_size.max(1))
        .map(|chunk| chunk.iter().map(|sample| sample.delta).sum())
        .collect()
}

/// One column of the bar chart. `share` is the bar's height as a fraction of
/// the tallest bar; the column fills whatever height the chart card has, so
/// the chart scales with the window instead of using fixed pixel heights.
fn chart_bar<'a>(share: f32, color: Color, label: String, current: bool) -> Element<'a, Message> {
    const SCALE: u16 = 1000;
    let mut column = column![]
        .width(Length::FillPortion(1))
        .height(Length::Fill)
        .spacing(6)
        .align_x(Alignment::Center);

    if share > 0.0 {
        let portion =
            ((share.clamp(0.0, 1.0) * f32::from(SCALE - 2)).round() as u16 + 1).min(SCALE - 1);
        column = column
            .push(Space::new().height(Length::FillPortion(SCALE - portion)))
            .push(
                container(Space::new().width(Length::Fill))
                    .width(Length::Fill)
                    .height(Length::FillPortion(portion))
                    .style(move |_| bar_style(color, 0.92)),
            );
    } else {
        column = column.push(Space::new().height(Length::Fill)).push(
            container(Space::new().width(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fixed(3.0))
                .style(move |_| bar_style(color, 0.35)),
        );
    }

    column
        .push(
            text(label)
                .size(TYPE_KICKER)
                .color(if current { MINT } else { MUTED }),
        )
        .into()
}

fn chart_legend<'a>() -> Element<'a, Message> {
    row![
        legend_item(DayKind::Weekday.color(), "Weekday"),
        legend_item(DayKind::Weekend.color(), "Weekend"),
        legend_item(DayKind::Today.color(), "Today"),
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .into()
}

fn legend_item<'a>(color: Color, label: &'static str) -> Element<'a, Message> {
    row![
        container(Space::new())
            .width(Length::Fixed(9.0))
            .height(Length::Fixed(9.0))
            .style(move |_| bar_style(color, 0.92)),
        text(label).size(TYPE_KICKER).color(MUTED),
    ]
    .spacing(5)
    .align_y(Alignment::Center)
    .into()
}

fn allowance_panel<'a>(
    current: &Current,
    metrics: &Metrics,
    used: f64,
    entitlement: f64,
    fraction: f32,
) -> Element<'a, Message> {
    let percent = fraction * 100.0;
    let tone = allowance_tone(fraction);
    container(
        column![
            row![
                kicker("ALLOWANCE", tone),
                Space::new().width(Length::Fill),
                text("Monthly cap").size(TYPE_KICKER).color(MUTED),
            ]
            .align_y(Alignment::Center),
            text(format!("{percent:.0}% used"))
                .size(TYPE_STAT)
                .color(TEXT),
            progress_bar(0.0..=1.0, fraction)
                .girth(10)
                .style(move |_| progress_bar::Style {
                    background: Background::Color(Color { a: 0.85, ..BORDER }),
                    bar: Background::Color(tone),
                    border: Border {
                        radius: BAR_RADIUS.into(),
                        ..Border::default()
                    },
                }),
            row![
                column![
                    text("REMAINING").size(TYPE_KICKER).color(MUTED),
                    text(money(current.remaining, false))
                        .size(TYPE_TITLE)
                        .color(tone),
                ]
                .spacing(3),
                Space::new().width(Length::Fill),
                column![
                    text("DAILY AVG").size(TYPE_KICKER).color(MUTED),
                    text(money(metrics.average_per_day, false))
                        .size(TYPE_TITLE)
                        .color(TEXT),
                ]
                .spacing(3),
            ],
            container(
                text(format!(
                    "{} of {} consumed",
                    money(Some(used), false),
                    money(Some(entitlement), false)
                ))
                .size(TYPE_KICKER)
                .color(MUTED),
            )
            .width(Length::Fill)
            .padding(10)
            .style(|_| glass_style()),
        ]
        .spacing(14),
    )
    .width(Length::FillPortion(2))
    .height(Length::Fill)
    .padding(18)
    .style(|_| elevated_style())
    .into()
}

/// Mint while comfortably inside the cap, amber once three quarters are
/// gone, and the error tone when the cycle is nearly exhausted.
fn allowance_tone(fraction: f32) -> Color {
    if fraction >= ALLOWANCE_CRITICAL {
        ERROR
    } else if fraction >= ALLOWANCE_WARNING {
        AMBER
    } else {
        MINT
    }
}

fn load_dashboard(force: bool) -> Task<Message> {
    Task::perform(async move { run_collector(force) }, Message::Loaded)
}

fn run_collector(force: bool) -> Result<DashboardData, String> {
    Collector::open(default_db_path())
        .and_then(|collector| {
            if force {
                collector.sample_force(Window::OneDay, Duration::from_secs(20), 180)
            } else {
                collector.sample(Window::OneDay, Duration::from_secs(20), 180)
            }
        })
        .map_err(|error| error.to_string())
}

fn money(value: Option<f64>, signed: bool) -> String {
    let Some(credits) = value.filter(|value| value.is_finite()) else {
        return "—".to_owned();
    };
    let dollars = credits / 100.0;
    let prefix = if signed && dollars > 0.0 { "+" } else { "" };
    format!("{prefix}${dollars:.2}")
}

fn number(value: Option<f64>) -> String {
    let Some(value) = value.filter(|value| value.is_finite()) else {
        return "—".to_owned();
    };
    if value.fract().abs() < 0.001 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

fn reset_text(epoch: Option<i64>) -> String {
    let Some(reset) = epoch else {
        return "No reset reported".to_owned();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64);
    let days = ((reset - now).max(0) + 86_399) / 86_400;
    match days {
        0 => "Resets today".to_owned(),
        1 => "Resets tomorrow".to_owned(),
        _ => format!("Resets in {days} days"),
    }
}

fn pace_label(value: Option<f64>) -> (String, Color) {
    match value {
        Some(credits) if credits >= 0.0 => (
            format!("{} below expected spend", money(Some(credits), false)),
            MINT,
        ),
        Some(credits) => (
            format!(
                "+{} above expected spend",
                money(Some(credits.abs()), false)
            ),
            AMBER,
        ),
        None => ("Not enough data to compare".to_owned(), MUTED),
    }
}

fn metric_card<'a>(title: &'static str, value: String, detail: String) -> Element<'a, Message> {
    container(
        column![
            kicker(title, MUTED),
            text(value).size(TYPE_CARD).color(TEXT),
            text(detail).size(TYPE_LABEL).color(MUTED),
        ]
        .spacing(8),
    )
    .width(Length::FillPortion(1))
    .padding([15, 17])
    .style(|_| elevated_style())
    .into()
}

fn vertical_divider<'a>() -> Element<'a, Message> {
    container(Space::new())
        .width(Length::Fixed(1.0))
        .height(Length::Fixed(104.0))
        .style(|_| container::Style {
            background: Some(Background::Color(Color { a: 0.70, ..BORDER })),
            ..container::Style::default()
        })
        .into()
}

fn kicker<'a>(value: &'static str, color: Color) -> iced::widget::Text<'a> {
    text(value).size(TYPE_KICKER).color(color)
}

fn dot<'a>() -> Element<'a, Message> {
    text("•").size(TYPE_LABEL).color(BORDER).into()
}

fn status_pill<'a>(value: &str, color: Color) -> Element<'a, Message> {
    container(text(value.to_owned()).size(TYPE_KICKER).color(color))
        .padding([5, 10])
        .style(move |_| container::Style {
            background: Some(Background::Color(Color { a: 0.12, ..color })),
            border: Border {
                color: Color { a: 0.38, ..color },
                width: 1.0,
                radius: CONTROL_RADIUS.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn refresh_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = button::secondary(theme, status);
    style.border.radius = CONTROL_RADIUS.into();
    style
}

fn error_banner<'a>(message: &'a str) -> Element<'a, Message> {
    container(text(message).size(TYPE_LABEL).color(ERROR))
        .width(Length::Fill)
        .padding(10)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgb(0.20, 0.07, 0.12))),
            border: Border {
                color: Color { a: 0.55, ..ERROR },
                width: 1.0,
                radius: CONTROL_RADIUS.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// The hero carries a soft violet wash so the headline figure sits apart
/// from the plain metric cards below it.
fn hero_style() -> container::Style {
    let wash = gradient::Linear::new(Degrees(155.0))
        .add_stop(0.0, HERO_TINT)
        .add_stop(1.0, SURFACE);
    container::Style {
        background: Some(Background::Gradient(Gradient::Linear(wash))),
        border: Border {
            color: Color { a: 0.55, ..VIOLET },
            width: 1.0,
            radius: CARD_RADIUS.into(),
        },
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

fn elevated_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE)),
        border: Border {
            color: Color { a: 0.72, ..BORDER },
            width: 1.0,
            radius: CARD_RADIUS.into(),
        },
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

fn glass_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE_HIGH)),
        border: Border {
            color: Color { a: 0.66, ..BORDER },
            width: 1.0,
            radius: INSET_RADIUS.into(),
        },
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

fn bar_style(color: Color, alpha: f32) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color { a: alpha, ..color })),
        border: Border {
            color: Color { a: 0.52, ..color },
            width: 1.0,
            radius: BAR_RADIUS.into(),
        },
        ..container::Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(date: &str, credits: f64) -> DailyUsage {
        DailyUsage {
            date: date.to_owned(),
            label: String::new(),
            credits,
        }
    }

    #[test]
    fn weekends_and_today_are_encoded_separately() {
        assert_eq!(day_kind("2026-09-05", false), DayKind::Weekend);
        assert_eq!(day_kind("2026-09-06", false), DayKind::Weekend);
        assert_eq!(day_kind("2026-09-07", false), DayKind::Weekday);
        assert_eq!(day_kind("2026-09-06", true), DayKind::Today);
        assert_eq!(day_kind("not a date", false), DayKind::Weekday);
    }

    #[test]
    fn chart_heading_shows_the_visible_date_range() {
        let days = [day("2026-08-20", 1.0), day("2026-09-02", 2.0)];
        assert_eq!(date_range(&days).as_deref(), Some("Aug 20 – Sep 2"));
        assert_eq!(date_range(&[]), None);
    }

    #[test]
    fn weekday_initial_falls_back_to_the_date() {
        assert_eq!(weekday_initial(&day("2026-09-02", 0.0)), "W");
        let labelled = DailyUsage {
            label: "F".to_owned(),
            ..day("2026-09-02", 0.0)
        };
        assert_eq!(weekday_initial(&labelled), "F");
    }

    #[test]
    fn allowance_tone_escalates_with_consumption() {
        assert_eq!(allowance_tone(0.2), MINT);
        assert_eq!(allowance_tone(0.75), AMBER);
        assert_eq!(allowance_tone(0.95), ERROR);
    }

    #[test]
    fn money_and_reset_copy_stay_readable() {
        assert_eq!(money(Some(3027.0), false), "$30.27");
        assert_eq!(money(Some(12.0), true), "+$0.12");
        assert_eq!(money(None, false), "—");
        assert_eq!(reset_text(None), "No reset reported");
    }
}
