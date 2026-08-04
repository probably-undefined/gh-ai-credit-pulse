use iced::widget::{button, column, container, progress_bar, row, text, Row, Space};
use iced::{
    window, Alignment, Background, Border, Color, Element, Length, Size, Subscription, Task, Theme,
};
use gh_ai_credit_pulse::collector::{
    Collector, Current, DailyUsage, DashboardData, Metrics, UsageSample, Window, default_db_path,
};
use std::env;
use std::time::Duration;

const BACKGROUND: Color = Color::from_rgb(0.027, 0.035, 0.063);
const SURFACE: Color = Color::from_rgb(0.059, 0.078, 0.125);
const SURFACE_HIGH: Color = Color::from_rgb(0.075, 0.094, 0.145);
const HERO: Color = SURFACE;
const BORDER: Color = Color::from_rgb(0.157, 0.196, 0.294);
const TEXT: Color = Color::from_rgb(0.973, 0.980, 1.0);
const MUTED: Color = Color::from_rgb(0.620, 0.663, 0.753);
const VIOLET: Color = Color::from_rgb(0.545, 0.361, 0.965);
const MINT: Color = Color::from_rgb(0.204, 0.827, 0.600);
const AMBER: Color = Color::from_rgb(0.984, 0.749, 0.141);
const ERROR: Color = Color::from_rgb(0.984, 0.443, 0.522);
const CARD_RADIUS: f32 = 16.0;
const INSET_RADIUS: f32 = 12.0;
const CONTROL_RADIUS: f32 = 10.0;
const BAR_RADIUS: f32 = 5.0;
const APPLICATION_ID: &str = "io.github.probably_undefined.GhAiCreditPulse";

fn main() -> iced::Result {
    if env::args().any(|argument| argument == "--version" || argument == "version") {
        println!("gh-ai-credit-pulse {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    iced::application(Dashboard::boot, Dashboard::update, Dashboard::view)
        .title("Copilot Usage")
        .theme(app_theme)
        .window(window::Settings {
            size: Size::new(1120.0, 760.0),
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
            load_dashboard(),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Refresh | Message::Tick if !self.refreshing => {
                self.refreshing = true;
                load_dashboard()
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
        let (pace_label, pace_color) = if entitlement.is_some() {
            pace_label(metrics.pace_delta)
        } else {
            ("No allowance data".to_owned(), MUTED)
        };

        let status = if self.refreshing {
            status_pill("SYNCING", MUTED)
        } else if self.error.is_some() {
            status_pill("CACHED", ERROR)
        } else {
            status_pill("● LIVE", MINT)
        };

        let brand = column![
            text("COPILOT USAGE").size(18).color(TEXT),
            text("LOCAL COST HISTORY").size(10).color(MUTED),
        ]
        .spacing(3);

        let header = row![
            brand,
            Space::new().width(Length::Fill),
            status,
            button(text(if self.refreshing { "Syncing…" } else { "↻  Refresh" }).size(12))
                .on_press(Message::Refresh)
                .padding([9, 14])
                .style(refresh_button_style),
        ]
        .spacing(12)
        .align_y(Alignment::Center);

        let hero_copy = column![
            kicker("CURRENT BILLING CYCLE", VIOLET),
            text(money(current.credits_used, false)).size(58).color(TEXT),
            row![
                text(format!("{} AI credits", number(current.credits_used)))
                    .size(13)
                    .color(MUTED),
                dot(),
                text("100 AIC = $1.00").size(13).color(MUTED),
            ]
            .spacing(9)
            .align_y(Alignment::Center),
        ]
        .spacing(7)
        .width(Length::FillPortion(3));

        let allowance_note = if entitlement.is_some() {
            text(pace_label).size(11).color(pace_color)
        } else {
            text("Allowance not reported by GitHub").size(11).color(MUTED)
        };
        let cycle_summary = column![
            kicker("ESTIMATED TOTAL", MUTED),
            text(money(metrics.projected_at_reset, false))
                .size(30)
                .color(TEXT),
            text(format!(
                "{}/day cycle average",
                money(metrics.average_per_day, false)
            ))
            .size(11)
            .color(MUTED),
            text(format!(
                "{}  ·  {} samples",
                reset_text(current.reset_at),
                self.data.sample_count.unwrap_or(0)
            ))
            .size(11)
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
                "CURRENT RATE",
                format!("{}/h", money(metrics.rate_per_hour, false)),
                "Based on the last six hours".to_owned(),
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
        .size(10)
        .color(MUTED);

        let mut content = column![header, hero, metrics_row, lower, footer].spacing(16);
        if let Some(error) = &self.error {
            content = content.push(error_banner(error));
        }

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

fn usage_chart<'a>(
    days: &[DailyUsage],
    series: &[UsageSample],
    sample_count: Option<u64>,
) -> Element<'a, Message> {
    let start = days.len().saturating_sub(14);
    let visible = &days[start..];
    let active_days = visible.iter().filter(|day| day.credits > 0.0).count();
    let daily_ready = active_days >= 2;
    let mut bars: Row<'a, Message> = Row::new()
        .spacing(7)
        .height(Length::Fill)
        .align_y(Alignment::End);

    let (title, subtitle, summary) = if daily_ready {
        let max_credits = visible
            .iter()
            .map(|day| day.credits)
            .fold(0.0_f64, f64::max)
            .max(1.0);
        for (index, day) in visible.iter().enumerate() {
            let intensity = (day.credits / max_credits).clamp(0.0, 1.0) as f32;
            let height = if day.credits > 0.0 {
                10.0 + intensity * 190.0
            } else {
                3.0
            };
            let color = VIOLET;
            let label = if index + 1 == visible.len() {
                "Today".to_owned()
            } else if index % 2 == 0 {
                short_date(&day.date)
            } else {
                " ".to_owned()
            };
            bars = bars.push(chart_bar(
                height,
                intensity,
                color,
                label,
                index + 1 == visible.len(),
            ));
        }
        (
            "USAGE HISTORY".to_owned(),
            "Last 14 days".to_owned(),
            money(Some(visible.iter().map(|day| day.credits).sum()), false),
        )
    } else if series.len() >= 2 {
        let buckets = recent_buckets(series, 24);
        let maximum = buckets.iter().copied().fold(0.0_f64, f64::max).max(1.0);
        for (index, credits) in buckets.iter().enumerate() {
            let intensity = (*credits / maximum).clamp(0.0, 1.0) as f32;
            let height = if *credits > 0.0 {
                8.0 + intensity * 182.0
            } else {
                3.0
            };
            let last = index + 1 == buckets.len();
            let label = if index == 0 {
                "Earlier".to_owned()
            } else if last {
                "Now".to_owned()
            } else {
                " ".to_owned()
            };
            bars = bars.push(chart_bar(
                height,
                intensity,
                VIOLET,
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
                    text("Waiting for usage samples").size(17).color(TEXT),
                    text("The chart appears after the next refresh.").size(11).color(MUTED),
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

    container(
        column![
            row![
                column![
                    text(title).size(10).color(MUTED),
                    text(subtitle).size(11).color(MUTED),
                ]
                .spacing(3),
                Space::new().width(Length::Fill),
                text(summary).size(13).color(MUTED),
            ]
            .align_y(Alignment::Center),
            bars,
        ]
        .spacing(15)
        .height(Length::Fill),
    )
    .width(Length::FillPortion(3))
    .height(Length::Fill)
    .padding(18)
    .style(|_| elevated_style())
    .into()
}

fn recent_buckets(series: &[UsageSample], maximum: usize) -> Vec<f64> {
    let chunk_size = (series.len() + maximum - 1) / maximum;
    series
        .chunks(chunk_size.max(1))
        .map(|chunk| chunk.iter().map(|sample| sample.delta).sum())
        .collect()
}

fn chart_bar<'a>(
    height: f32,
    intensity: f32,
    color: Color,
    label: String,
    current: bool,
) -> Element<'a, Message> {
    let bar = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .style(move |_| bar_style(color, 0.38 + intensity * 0.62));
    column![
        Space::new().height(Length::Fill),
        bar,
        text(label).size(9).color(if current { VIOLET } else { MUTED }),
    ]
    .width(Length::FillPortion(1))
    .height(Length::Fill)
    .spacing(6)
    .align_x(Alignment::Center)
    .into()
}

fn short_date(date: &str) -> String {
    let mut parts = date.split('-');
    let _year = parts.next();
    let month = parts.next().and_then(|value| value.parse::<u8>().ok());
    let day = parts.next().and_then(|value| value.parse::<u8>().ok());
    match (month, day) {
        (Some(month), Some(day)) => format!("{month}/{day}"),
        _ => "·".to_owned(),
    }
}

fn allowance_panel<'a>(
    current: &Current,
    metrics: &Metrics,
    used: f64,
    entitlement: f64,
    fraction: f32,
) -> Element<'a, Message> {
    let percent = fraction * 100.0;
    container(
        column![
            row![
                kicker("ALLOWANCE", MINT),
                Space::new().width(Length::Fill),
                text("Monthly cap").size(10).color(MUTED),
            ]
            .align_y(Alignment::Center),
            text(format!("{percent:.0}% used")).size(30).color(TEXT),
            progress_bar(0.0..=1.0, fraction),
            row![
                column![
                    text("REMAINING").size(9).color(MUTED),
                    text(money(current.remaining, false)).size(18).color(MINT),
                ]
                .spacing(3),
                Space::new().width(Length::Fill),
                column![
                    text("DAILY AVG").size(9).color(MUTED),
                    text(money(metrics.average_per_day, false)).size(18).color(TEXT),
                ]
                .spacing(3),
            ],
            container(
                text(format!(
                    "{} of {} consumed",
                    money(Some(used), false),
                    money(Some(entitlement), false)
                ))
                .size(10)
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

fn load_dashboard() -> Task<Message> {
    Task::perform(async { run_collector() }, Message::Loaded)
}

fn run_collector() -> Result<DashboardData, String> {
    Collector::open(default_db_path())
        .and_then(|collector| {
            collector.sample(Window::OneDay, Duration::from_secs(20), 180)
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
        1 => "Resets tomorrow".to_owned(),
        _ => format!("Resets in {days} days"),
    }
}

fn pace_label(value: Option<f64>) -> (String, Color) {
    match value {
        Some(credits) if credits >= 0.0 => {
            (format!("{} below expected spend", money(Some(credits), false)), MINT)
        }
        Some(credits) => (
            format!("+{} above expected spend", money(Some(credits.abs()), false)),
            AMBER,
        ),
        None => ("Not enough data to compare".to_owned(), MUTED),
    }
}

fn metric_card<'a>(
    title: &'static str,
    value: String,
    detail: String,
) -> Element<'a, Message> {
    container(
        column![
            kicker(title, MUTED),
            text(value).size(29).color(TEXT),
            text(detail).size(11).color(MUTED),
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
    text(value).size(10).color(color)
}

fn dot<'a>() -> Element<'a, Message> {
    text("•").size(12).color(BORDER).into()
}

fn status_pill<'a>(value: &str, color: Color) -> Element<'a, Message> {
    container(text(value.to_owned()).size(9).color(color))
        .padding([5, 9])
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
    container(text(message).size(11).color(ERROR))
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

fn hero_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(HERO)),
        border: Border {
            color: Color { a: 0.72, ..BORDER },
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
