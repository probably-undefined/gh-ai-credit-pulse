use iced::widget::{button, column, container, horizontal_rule, horizontal_space, progress_bar, row, text};
use iced::{Background, Border, Color, Element, Length, Shadow, Size, Subscription, Task, Theme};
use serde::Deserialize;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const BACKGROUND: Color = Color::from_rgb(0.063, 0.078, 0.114);
const SURFACE: Color = Color::from_rgb(0.090, 0.114, 0.165);
const BORDER: Color = Color::from_rgb(0.161, 0.196, 0.278);
const TEXT: Color = Color::from_rgb(0.957, 0.969, 0.992);
const MUTED: Color = Color::from_rgb(0.482, 0.537, 0.631);
const ACCENT: Color = Color::from_rgb(0.486, 0.549, 1.0);
const SUCCESS: Color = Color::from_rgb(0.455, 0.882, 0.729);
const ERROR: Color = Color::from_rgb(1.0, 0.569, 0.627);

fn main() -> iced::Result {
    if env::args().any(|argument| argument == "--version" || argument == "version") {
        println!("gh-ai-credit-pulse {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    iced::application(Dashboard::boot, Dashboard::update, Dashboard::view)
        .title("GitHub AI Credit Pulse")
        .theme(|_| Theme::Dark)
        .window_size(Size::new(920.0, 650.0))
        .subscription(Dashboard::subscription)
        .run()
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

        let status = if self.refreshing {
            status_pill("Refreshing…", MUTED)
        } else if self.error.is_some() {
            status_pill("Cached", ERROR)
        } else {
            status_pill("Live · 30s", SUCCESS)
        };

        let header = row![
            column![
                text("AI credit pulse").size(25).color(TEXT),
                text(format!(
                    "{}  ·  {}",
                    current.plan.as_deref().unwrap_or("Copilot"),
                    reset_text(current.reset_at)
                ))
                .size(12)
                .color(MUTED),
            ]
            .spacing(2),
            horizontal_space(),
            status,
            button("Refresh").on_press(Message::Refresh),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center);

        let cards = row![
            metric_card("COST USED", money(Some(used), false), format!("{} AIC", number(Some(used)))),
            metric_card(
                "TODAY",
                money(metrics.delta_today, true),
                format!("{} last hour", money(metrics.delta_1h, false)),
            ),
            metric_card(
                "CURRENT RATE",
                format!("{}/h", money(metrics.rate_per_hour, false)),
                format!("{}/day avg", money(metrics.average_per_day, false)),
            ),
            metric_card(
                "PROJECTION",
                money(metrics.projected_at_reset, false),
                "at next reset".to_owned(),
            ),
        ]
        .spacing(12);

        let history = panel(
            column![
                row![
                    kicker("RECENT USAGE"),
                    horizontal_space(),
                    text(format!("{} local samples", self.data.sample_count.unwrap_or(0)))
                        .size(11)
                        .color(MUTED),
                ]
                .align_y(iced::Alignment::Center),
                horizontal_rule(1),
                usage_row("Last hour", metrics.delta_1h),
                usage_row("Today", metrics.delta_today),
                usage_row("Last 7 days", metrics.delta_7d),
                usage_row("Last 30 days", metrics.delta_30d),
            ]
            .spacing(12),
        );

        let entitlement = current.entitlement.unwrap_or(0.0);
        let allowance_fraction = if entitlement > 0.0 {
            (used / entitlement).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let allowance = panel(
            column![
                row![
                    kicker("MONTHLY ALLOWANCE"),
                    horizontal_space(),
                    text(if entitlement > 0.0 {
                        format!("{} / {}", money(Some(used), false), money(Some(entitlement), false))
                    } else {
                        "Not reported".to_owned()
                    })
                    .size(11)
                    .color(MUTED),
                ],
                progress_bar(0.0..=1.0, allowance_fraction),
                text(format!(
                    "{} remaining  ·  {} overage",
                    money(current.remaining, false),
                    money(current.overage_count, false)
                ))
                .size(11)
                .color(MUTED),
                text(format!(
                    "Current pace: {} per day",
                    money(metrics.average_per_day, false)
                ))
                .size(11)
                .color(MUTED),
            ]
            .spacing(12),
        );

        let lower = row![history, allowance].spacing(12).height(Length::Fill);
        let mut content = column![header, cards, lower].spacing(14);
        if let Some(error) = &self.error {
            content = content.push(error_banner(error));
        }

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(22)
            .style(|_| container::Style {
                background: Some(Background::Color(BACKGROUND)),
                text_color: Some(TEXT),
                ..container::Style::default()
            })
            .into()
    }
}

fn load_dashboard() -> Task<Message> {
    Task::perform(
        async { run_collector() },
        Message::Loaded,
    )
}

fn run_collector() -> Result<DashboardData, String> {
    let collector = collector_path().ok_or_else(|| "Could not locate gh_ai_credits.py".to_owned())?;
    let python = env::var("GH_AI_CREDIT_PULSE_PYTHON").unwrap_or_else(|_| {
        if cfg!(windows) {
            "python".to_owned()
        } else {
            "/usr/bin/python3".to_owned()
        }
    });
    let output = Command::new(&python)
        .arg(&collector)
        .args(["sample", "--window", "24h"])
        .output()
        .map_err(|error| format!("Could not run {python}: {error}"))?;

    if output.stdout.is_empty() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("Invalid collector response: {error}"))
}

fn collector_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os("GH_AI_CREDIT_PULSE_COLLECTOR").map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    let executable = env::current_exe().ok()?;
    let directory = executable.parent()?;
    [
        directory.join("scripts/gh_ai_credits.py"),
        directory.join("../scripts/gh_ai_credits.py"),
        directory.join("../../scripts/gh_ai_credits.py"),
    ]
    .into_iter()
    .find(|path| Path::new(path).is_file())
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

fn metric_card<'a>(title: &'static str, value: String, detail: String) -> Element<'a, Message> {
    container(
        column![
            kicker(title),
            text(value).size(25).color(TEXT),
            text(detail).size(11).color(MUTED),
        ]
        .spacing(7),
    )
    .width(Length::FillPortion(1))
    .padding(14)
    .style(panel_style)
    .into()
}

fn usage_row<'a>(label: &'static str, value: Option<f64>) -> Element<'a, Message> {
    row![
        text(label).size(13).color(MUTED),
        horizontal_space(),
        text(money(value, true)).size(17).color(TEXT),
    ]
    .align_y(iced::Alignment::Center)
    .into()
}

fn panel<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .width(Length::FillPortion(1))
        .height(Length::Fill)
        .padding(16)
        .style(panel_style)
        .into()
}

fn kicker<'a>(value: &'static str) -> iced::widget::Text<'a> {
    text(value).size(11).color(MUTED)
}

fn status_pill<'a>(value: &'static str, color: Color) -> Element<'a, Message> {
    container(text(value).size(11).color(color))
        .padding([6, 11])
        .style(move |_| container::Style {
            background: Some(Background::Color(Color { a: 0.16, ..color })),
            border: Border {
                color: Color { a: 0.45, ..color },
                width: 1.0,
                radius: 14.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn error_banner<'a>(message: &'a str) -> Element<'a, Message> {
    container(text(message).size(11).color(ERROR))
        .width(Length::Fill)
        .padding(10)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgb(0.20, 0.12, 0.16))),
            border: Border {
                color: Color::from_rgb(0.40, 0.20, 0.26),
                width: 1.0,
                radius: 10.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn panel_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE)),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 15.0.into(),
        },
        shadow: Shadow::default(),
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct DashboardData {
    status: String,
    error: Option<String>,
    sample_count: Option<u64>,
    current: Current,
    metrics: Metrics,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Current {
    credits_used: Option<f64>,
    entitlement: Option<f64>,
    remaining: Option<f64>,
    overage_count: Option<f64>,
    reset_at: Option<i64>,
    plan: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Metrics {
    delta_1h: Option<f64>,
    delta_today: Option<f64>,
    delta_7d: Option<f64>,
    delta_30d: Option<f64>,
    rate_per_hour: Option<f64>,
    average_per_day: Option<f64>,
    projected_at_reset: Option<f64>,
}
