use iced::widget::{button, column, container, progress_bar, row, text, Row, Space};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Shadow, Size, Subscription, Task,
    Theme, Vector,
};
use serde::Deserialize;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const BACKGROUND: Color = Color::from_rgb(0.027, 0.035, 0.063);
const SURFACE: Color = Color::from_rgb(0.059, 0.078, 0.125);
const SURFACE_HIGH: Color = Color::from_rgb(0.082, 0.102, 0.165);
const HERO: Color = Color::from_rgb(0.094, 0.063, 0.180);
const BORDER: Color = Color::from_rgb(0.157, 0.196, 0.294);
const TEXT: Color = Color::from_rgb(0.973, 0.980, 1.0);
const MUTED: Color = Color::from_rgb(0.545, 0.592, 0.690);
const VIOLET: Color = Color::from_rgb(0.545, 0.361, 0.965);
const CYAN: Color = Color::from_rgb(0.133, 0.827, 0.933);
const MINT: Color = Color::from_rgb(0.204, 0.827, 0.600);
const PINK: Color = Color::from_rgb(0.957, 0.447, 0.714);
const AMBER: Color = Color::from_rgb(0.984, 0.749, 0.141);
const ERROR: Color = Color::from_rgb(0.984, 0.443, 0.522);

fn main() -> iced::Result {
    if env::args().any(|argument| argument == "--version" || argument == "version") {
        println!("gh-ai-credit-pulse {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    iced::application(Dashboard::boot, Dashboard::update, Dashboard::view)
        .title("GitHub AI Credit Pulse")
        .theme(app_theme)
        .window_size(Size::new(1120.0, 760.0))
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
        let entitlement = current.entitlement.unwrap_or(0.0);
        let allowance_fraction = if entitlement > 0.0 {
            (used / entitlement).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let (pace_label, pace_color) = pace_label(metrics.pace_delta);

        let status = if self.refreshing {
            status_pill("SYNCING", MUTED)
        } else if self.error.is_some() {
            status_pill("CACHED", ERROR)
        } else {
            status_pill("● LIVE", MINT)
        };

        let brand = row![
            container(text("✦").size(21).color(TEXT))
                .width(Length::Fixed(44.0))
                .height(Length::Fixed(44.0))
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .style(|_| accent_mark_style()),
            column![
                text("AI CREDIT PULSE").size(18).color(TEXT),
                text("COPILOT SPEND INTELLIGENCE").size(10).color(MUTED),
            ]
            .spacing(3),
        ]
        .spacing(12)
        .align_y(Alignment::Center);

        let header = row![
            brand,
            Space::new().width(Length::Fill),
            status,
            button(text(if self.refreshing { "Syncing…" } else { "↻  Refresh" }).size(12))
                .on_press(Message::Refresh)
                .padding([9, 14]),
        ]
        .spacing(12)
        .align_y(Alignment::Center);

        let hero_copy = column![
            kicker("TOTAL COST THIS CYCLE", VIOLET),
            text(money(Some(used), false)).size(58).color(TEXT),
            row![
                text(format!("{} AI credits", number(Some(used))))
                    .size(13)
                    .color(MUTED),
                dot(),
                text("100 AIC = $1.00").size(13).color(CYAN),
            ]
            .spacing(9)
            .align_y(Alignment::Center),
        ]
        .spacing(7)
        .width(Length::FillPortion(3));

        let cycle_snapshot = container(
            column![
                row![
                    kicker("CYCLE SIGNAL", CYAN),
                    Space::new().width(Length::Fill),
                    status_pill(&pace_label, pace_color),
                ]
                .align_y(Alignment::Center),
                text(format!(
                    "{} projected",
                    money(metrics.projected_at_reset, false)
                ))
                .size(24)
                .color(TEXT),
                text(format!(
                    "{}  ·  {} local samples",
                    reset_text(current.reset_at),
                    self.data.sample_count.unwrap_or(0)
                ))
                .size(11)
                .color(MUTED),
            ]
            .spacing(9),
        )
        .width(Length::FillPortion(2))
        .padding(16)
        .style(|_| glass_style(CYAN));

        let hero = container(row![hero_copy, cycle_snapshot].spacing(28).align_y(Alignment::Center))
            .width(Length::Fill)
            .padding([22, 26])
            .style(|_| hero_style());

        let metrics_row = row![
            metric_card(
                "TODAY",
                money(metrics.delta_today, true),
                format!("{} in the last hour", money(metrics.delta_1h, false)),
                CYAN,
                "↗",
            ),
            metric_card(
                "CURRENT VELOCITY",
                format!("{}/h", money(metrics.rate_per_hour, false)),
                format!("{}/day cycle average", money(metrics.average_per_day, false)),
                PINK,
                "≈",
            ),
            metric_card(
                "30 DAY SIGNAL",
                money(metrics.delta_30d, true),
                format!("{} across 7 days", money(metrics.delta_7d, false)),
                AMBER,
                "◌",
            ),
        ]
        .spacing(14);

        let chart = daily_chart(&self.data.daily);
        let allowance = allowance_panel(current, metrics, used, entitlement, allowance_fraction);
        let lower = row![chart, allowance]
            .spacing(14)
            .height(Length::Fill);

        let footer = row![
            text(format!(
                "{} · local-first history · refreshes every 30 seconds",
                current.plan.as_deref().unwrap_or("GitHub Copilot")
            ))
            .size(10)
            .color(MUTED),
            Space::new().width(Length::Fill),
            text("PRIVATE BY DESIGN").size(10).color(MINT),
        ];

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

fn daily_chart<'a>(days: &[DailyUsage]) -> Element<'a, Message> {
    let start = days.len().saturating_sub(14);
    let visible = &days[start..];
    let max_credits = visible
        .iter()
        .map(|day| day.credits)
        .fold(0.0_f64, f64::max)
        .max(1.0);

    let mut bars: Row<'a, Message> = Row::new()
        .spacing(7)
        .height(Length::Fixed(128.0))
        .align_y(Alignment::End);

    if visible.is_empty() {
        bars = bars.push(
            container(text("Usage bars appear as local history builds.").size(12).color(MUTED))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center),
        );
    } else {
        for (index, day) in visible.iter().enumerate() {
            let intensity = (day.credits / max_credits).clamp(0.0, 1.0) as f32;
            let height = 12.0 + intensity * 82.0;
            let color = if index + 1 == visible.len() { CYAN } else { VIOLET };
            let label = if index % 2 == 0 || index + 1 == visible.len() {
                day.label.clone()
            } else {
                " ".to_owned()
            };
            let bar = container(Space::new().width(Length::Fill).height(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fixed(height))
                .style(move |_| bar_style(color, 0.42 + intensity * 0.58));
            bars = bars.push(
                column![
                    Space::new().height(Length::Fill),
                    bar,
                    text(label).size(9).color(MUTED),
                ]
                .width(Length::FillPortion(1))
                .height(Length::Fill)
                .spacing(6)
                .align_x(Alignment::Center),
            );
        }
    }

    container(
        column![
            row![
                column![
                    kicker("DAILY PULSE", VIOLET),
                    text("14-day usage rhythm").size(11).color(MUTED),
                ]
                .spacing(3),
                Space::new().width(Length::Fill),
                text(money(
                    Some(visible.iter().map(|day| day.credits).sum()),
                    false
                ))
                .size(21)
                .color(TEXT),
            ]
            .align_y(Alignment::Center),
            bars,
        ]
        .spacing(13),
    )
    .width(Length::FillPortion(3))
    .height(Length::Fill)
    .padding(18)
    .style(|_| elevated_style(VIOLET))
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
    container(
        column![
            row![
                kicker("ALLOWANCE", MINT),
                Space::new().width(Length::Fill),
                status_pill(
                    if entitlement > 0.0 { "TRACKED" } else { "UNREPORTED" },
                    if entitlement > 0.0 { MINT } else { MUTED },
                ),
            ]
            .align_y(Alignment::Center),
            text(if entitlement > 0.0 {
                format!("{percent:.0}% used")
            } else {
                "No cap reported".to_owned()
            })
            .size(30)
            .color(TEXT),
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
                text(if entitlement > 0.0 {
                    format!(
                        "{} of {} consumed",
                        money(Some(used), false),
                        money(Some(entitlement), false)
                    )
                } else {
                    "GitHub did not report a monthly allowance.".to_owned()
                })
                .size(10)
                .color(MUTED),
            )
            .width(Length::Fill)
            .padding(10)
            .style(|_| glass_style(MINT)),
        ]
        .spacing(14),
    )
    .width(Length::FillPortion(2))
    .height(Length::Fill)
    .padding(18)
    .style(|_| elevated_style(MINT))
    .into()
}

fn load_dashboard() -> Task<Message> {
    Task::perform(async { run_collector() }, Message::Loaded)
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
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Invalid collector response: {error}"))
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

fn pace_label(value: Option<f64>) -> (String, Color) {
    match value {
        Some(credits) if credits >= 0.0 => {
            (format!("{} BELOW PACE", money(Some(credits), false)), MINT)
        }
        Some(credits) => (
            format!("{} ABOVE PACE", money(Some(credits.abs()), false)),
            AMBER,
        ),
        None => ("LEARNING PACE".to_owned(), MUTED),
    }
}

fn metric_card<'a>(
    title: &'static str,
    value: String,
    detail: String,
    accent: Color,
    glyph: &'static str,
) -> Element<'a, Message> {
    container(
        column![
            row![
                kicker(title, accent),
                Space::new().width(Length::Fill),
                text(glyph).size(20).color(accent),
            ]
            .align_y(Alignment::Center),
            text(value).size(29).color(TEXT),
            text(detail).size(11).color(MUTED),
        ]
        .spacing(8),
    )
    .width(Length::FillPortion(1))
    .padding([15, 17])
    .style(move |_| elevated_style(accent))
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
            background: Some(Background::Color(Color::from_rgb(0.20, 0.07, 0.12))),
            border: Border {
                color: Color { a: 0.55, ..ERROR },
                width: 1.0,
                radius: 10.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn accent_mark_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(VIOLET)),
        border: Border {
            color: Color { a: 0.70, ..CYAN },
            width: 1.0,
            radius: 13.0.into(),
        },
        shadow: Shadow {
            color: Color { a: 0.32, ..VIOLET },
            offset: Vector::new(0.0, 5.0),
            blur_radius: 18.0,
        },
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

fn hero_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(HERO)),
        border: Border {
            color: Color { a: 0.62, ..VIOLET },
            width: 1.0,
            radius: 22.0.into(),
        },
        shadow: Shadow {
            color: Color { a: 0.25, ..VIOLET },
            offset: Vector::new(0.0, 12.0),
            blur_radius: 32.0,
        },
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

fn elevated_style(accent: Color) -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE)),
        border: Border {
            color: Color { a: 0.42, ..accent },
            width: 1.0,
            radius: 17.0.into(),
        },
        shadow: Shadow {
            color: Color { a: 0.12, ..accent },
            offset: Vector::new(0.0, 8.0),
            blur_radius: 22.0,
        },
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

fn glass_style(accent: Color) -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE_HIGH)),
        border: Border {
            color: Color { a: 0.25, ..accent },
            width: 1.0,
            radius: 13.0.into(),
        },
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

fn bar_style(color: Color, alpha: f32) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color { a: alpha, ..color })),
        border: Border {
            color: Color { a: 0.65, ..color },
            width: 1.0,
            radius: 7.0.into(),
        },
        shadow: Shadow {
            color: Color { a: 0.18, ..color },
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..container::Style::default()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct DashboardData {
    error: Option<String>,
    sample_count: Option<u64>,
    current: Current,
    metrics: Metrics,
    daily: Vec<DailyUsage>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Current {
    credits_used: Option<f64>,
    entitlement: Option<f64>,
    remaining: Option<f64>,
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
    pace_delta: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct DailyUsage {
    label: String,
    credits: f64,
}
