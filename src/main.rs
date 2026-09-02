use chrono::{Datelike, NaiveDate, Weekday};
use clap::Parser;
use gh_ai_credit_pulse::collector::{
    Collector, Current, DailyUsage, DashboardData, Metrics, UsageSample, Window, default_db_path,
};
use iced::alignment::{Horizontal, Vertical};
use iced::border;
use iced::font::Weight;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, LineCap, Path, Stroke};
use iced::widget::{Space, button, canvas as canvas_widget, column, container, row, text};
use iced::{
    Alignment, Background, Border, Color, Degrees, Element, Font, Gradient, Length, Pixels, Point,
    Radians, Rectangle, Renderer, Size, Subscription, Task, Theme, Vector, gradient, mouse, window,
};
use std::time::Duration;

// Palette. The GNOME extension stylesheet mirrors these values so both
// surfaces read as one product: violet is the brand accent, mint marks
// "live", "remaining", and "today", amber warns, and the error tone is
// reserved for failures.
const BACKGROUND: Color = Color::from_rgb(0.031, 0.039, 0.071);
const BACKGROUND_TOP: Color = Color::from_rgb(0.059, 0.055, 0.118);
const SURFACE: Color = Color::from_rgb(0.071, 0.086, 0.137);
const SURFACE_HIGH: Color = Color::from_rgb(0.098, 0.114, 0.173);
const HERO_TINT: Color = Color::from_rgb(0.157, 0.098, 0.298);
const BORDER: Color = Color::from_rgb(0.180, 0.212, 0.310);
const TEXT: Color = Color::from_rgb(0.973, 0.980, 1.0);
const MUTED: Color = Color::from_rgb(0.596, 0.643, 0.741);
const VIOLET: Color = Color::from_rgb(0.545, 0.361, 0.965);
const VIOLET_LIGHT: Color = Color::from_rgb(0.749, 0.639, 1.0);
const MINT: Color = Color::from_rgb(0.204, 0.827, 0.600);
const AMBER: Color = Color::from_rgb(0.984, 0.749, 0.141);
const ERROR: Color = Color::from_rgb(0.984, 0.443, 0.522);

// Inter is bundled so the dashboard looks the same on every machine.
const INTER: Font = Font::with_name("Inter");
const INTER_SEMIBOLD: Font = Font {
    weight: Weight::Semibold,
    ..INTER
};
const INTER_BOLD: Font = Font {
    weight: Weight::Bold,
    ..INTER
};
const INTER_REGULAR_BYTES: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
const INTER_SEMIBOLD_BYTES: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");
const INTER_BOLD_BYTES: &[u8] = include_bytes!("../assets/fonts/Inter-Bold.ttf");

// Type scale. Nothing renders below 11px so labels stay legible on a
// 1080p desktop without display scaling.
const TYPE_DISPLAY: f32 = 64.0;
const TYPE_STAT: f32 = 26.0;
const TYPE_CARD: f32 = 30.0;
const TYPE_TITLE: f32 = 20.0;
const TYPE_BODY: f32 = 14.0;
const TYPE_LABEL: f32 = 12.5;
const TYPE_KICKER: f32 = 11.0;

const CARD_RADIUS: f32 = 18.0;
const INSET_RADIUS: f32 = 12.0;
const CONTROL_RADIUS: f32 = 10.0;
const CHART_DAYS: usize = 14;
const GAUGE_SIZE: f32 = 124.0;
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
        .font(INTER_REGULAR_BYTES)
        .font(INTER_SEMIBOLD_BYTES)
        .font(INTER_BOLD_BYTES)
        .default_font(INTER)
        .window(window::Settings {
            size: Size::new(1120.0, 760.0),
            min_size: Some(Size::new(900.0, 660.0)),
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

        let plan = current
            .plan
            .as_deref()
            .map(|plan| format!("GitHub Copilot · {plan} plan"))
            .unwrap_or_else(|| "GitHub Copilot".to_owned());
        let brand = row![
            app_mark(),
            column![
                text("Copilot Usage")
                    .size(TYPE_TITLE)
                    .font(INTER_BOLD)
                    .color(TEXT),
                text(plan).size(TYPE_LABEL).color(MUTED),
            ]
            .spacing(2),
        ]
        .spacing(12)
        .align_y(Alignment::Center);

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
                .font(INTER_SEMIBOLD)
            )
            .on_press_maybe((!self.refreshing).then_some(Message::Refresh))
            .padding([9, 16])
            .style(refresh_button_style),
        ]
        .spacing(12)
        .align_y(Alignment::Center);

        let pace_chip: Element<'_, Message> = if entitlement.is_some() {
            let (label, color) = pace_label(metrics.pace_delta);
            chip(label, color)
        } else {
            chip("Allowance not reported by GitHub".to_owned(), MUTED)
        };
        let hero_copy = column![
            kicker("CURRENT BILLING CYCLE", VIOLET_LIGHT),
            text(money(current.credits_used, false))
                .size(TYPE_DISPLAY)
                .font(INTER_BOLD)
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
            Space::new().height(Length::Fixed(4.0)),
            pace_chip,
        ]
        .spacing(6)
        .width(Length::FillPortion(3));

        let stats = column![
            row![
                stat_tile(
                    "PROJECTED TOTAL",
                    money(metrics.projected_at_reset, false),
                    "at cycle reset"
                ),
                stat_tile(
                    "DAILY AVERAGE",
                    money(metrics.average_per_day, false),
                    "across the cycle"
                ),
            ]
            .spacing(10),
            row![
                stat_tile(
                    "RESET",
                    reset_text(current.reset_at),
                    &format!(
                        "{} samples",
                        number(self.data.sample_count.map(|n| n as f64))
                    )
                ),
                stat_tile(
                    "FORECAST BASIS",
                    "Weekdays".to_owned(),
                    "06:00 – 19:00 local"
                ),
            ]
            .spacing(10),
        ]
        .spacing(10)
        .width(Length::FillPortion(3));

        let hero = container(
            row![hero_copy, stats]
                .spacing(28)
                .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([24, 28])
        .style(|_| hero_style());

        let metrics_row = row![
            metric_card(
                "TODAY",
                VIOLET,
                money(metrics.delta_today, true),
                "since local midnight",
            ),
            metric_card(
                "LAST HOUR",
                VIOLET_LIGHT,
                money(metrics.delta_1h, true),
                "recorded usage",
            ),
            metric_card(
                "6-HOUR RATE",
                MINT,
                format!("{}/h", money(metrics.rate_per_hour, false)),
                "average over the last six hours",
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

        let footer = text("History is stored locally · refreshes every 30 seconds")
            .size(TYPE_KICKER)
            .color(MUTED);

        // Problems surface directly under the header, where the eye lands
        // first, instead of trailing the footer.
        let mut content = column![header].spacing(18);
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
            .padding([24, 30])
            .style(|_| container::Style {
                background: Some(Background::Gradient(Gradient::Linear(
                    gradient::Linear::new(Degrees(170.0))
                        .add_stop(0.0, BACKGROUND_TOP)
                        .add_stop(0.55, BACKGROUND)
                        .add_stop(1.0, BACKGROUND),
                ))),
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

#[derive(Debug, Clone)]
struct Bar {
    share: f32,
    credits: f64,
    kind: DayKind,
    label: String,
}

/// Bar chart drawn on a canvas: faint gridlines with dollar ticks, rounded
/// gradient bars, and a value callout above the highlighted bar.
#[derive(Debug, Clone)]
struct BarChart {
    bars: Vec<Bar>,
    max_credits: f64,
}

impl<Message> canvas::Program<Message> for BarChart {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas_frame(renderer, bounds);
        if self.bars.is_empty() {
            return vec![frame.into_geometry()];
        }

        let tick_width = 44.0;
        let label_height = 22.0;
        let callout_height = 22.0;
        let plot = Rectangle {
            x: tick_width,
            y: callout_height,
            width: (bounds.width - tick_width).max(1.0),
            height: (bounds.height - callout_height - label_height).max(1.0),
        };

        // Gridlines at 0, 50 and 100 percent of the tallest bar.
        for step in 0..=2 {
            let fraction = step as f32 / 2.0;
            let y = plot.y + plot.height * (1.0 - fraction);
            let line = Path::line(Point::new(plot.x, y), Point::new(plot.x + plot.width, y));
            frame.stroke(
                &line,
                Stroke::default()
                    .with_color(Color {
                        a: if step == 0 { 0.55 } else { 0.28 },
                        ..BORDER
                    })
                    .with_width(1.0),
            );
            frame.fill_text(canvas::Text {
                content: money(Some(self.max_credits * f64::from(fraction)), false),
                position: Point::new(plot.x - 10.0, y),
                color: MUTED,
                size: Pixels(TYPE_KICKER),
                font: INTER,
                align_x: Horizontal::Right.into(),
                align_y: Vertical::Center,
                ..canvas::Text::default()
            });
        }

        let slot = plot.width / self.bars.len() as f32;
        let bar_width = (slot * 0.56).clamp(6.0, 40.0);
        for (index, bar) in self.bars.iter().enumerate() {
            let center_x = plot.x + slot * (index as f32 + 0.5);
            let x = center_x - bar_width / 2.0;
            let color = bar.kind.color();
            let height = if bar.share > 0.0 {
                (plot.height * bar.share).max(4.0)
            } else {
                3.0
            };
            let top = plot.y + plot.height - height;
            let radius = (bar_width / 2.0).min(6.0);
            let shape = Path::rounded_rectangle(
                Point::new(x, top),
                Size::new(bar_width, height),
                border::Radius::new(radius).bottom(2.0),
            );
            if bar.share > 0.0 {
                let fill =
                    canvas::gradient::Linear::new(Point::new(x, top), Point::new(x, top + height))
                        .add_stop(0.0, lighten(color, 0.18))
                        .add_stop(1.0, Color { a: 0.55, ..color });
                frame.fill(&shape, fill);
            } else {
                frame.fill(&shape, Color { a: 0.45, ..color });
            }

            let is_today = bar.kind == DayKind::Today;
            frame.fill_text(canvas::Text {
                content: bar.label.clone(),
                position: Point::new(center_x, plot.y + plot.height + 8.0),
                color: if is_today { MINT } else { MUTED },
                size: Pixels(TYPE_KICKER),
                font: if is_today { INTER_SEMIBOLD } else { INTER },
                align_x: Horizontal::Center.into(),
                align_y: Vertical::Top,
                ..canvas::Text::default()
            });
            if is_today && bar.credits > 0.0 {
                frame.fill_text(canvas::Text {
                    content: money(Some(bar.credits), false),
                    position: Point::new(center_x, top - 6.0),
                    color: MINT,
                    size: Pixels(TYPE_LABEL),
                    font: INTER_SEMIBOLD,
                    align_x: Horizontal::Center.into(),
                    align_y: Vertical::Bottom,
                    ..canvas::Text::default()
                });
            }
        }

        vec![frame.into_geometry()]
    }
}

/// Ring gauge for the monthly allowance.
#[derive(Debug, Clone)]
struct Gauge {
    fraction: f32,
    tone: Color,
}

impl<Message> canvas::Program<Message> for Gauge {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas_frame(renderer, bounds);
        let thickness = 12.0;
        let radius = (bounds.width.min(bounds.height) / 2.0 - thickness / 2.0 - 2.0).max(10.0);
        let center = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        let start = Radians(-std::f32::consts::FRAC_PI_2);

        let track = Path::circle(center, radius);
        frame.stroke(
            &track,
            Stroke::default()
                .with_color(Color { a: 0.9, ..BORDER })
                .with_width(thickness),
        );

        let sweep = self.fraction.clamp(0.0, 1.0) * std::f32::consts::TAU;
        if sweep > 0.0 {
            let progress = Path::new(|builder| {
                builder.arc(canvas::path::Arc {
                    center,
                    radius,
                    start_angle: start,
                    end_angle: Radians(start.0 + sweep),
                });
            });
            frame.stroke(
                &progress,
                Stroke::default()
                    .with_color(self.tone)
                    .with_width(thickness)
                    .with_line_cap(LineCap::Round),
            );
        }

        frame.fill_text(canvas::Text {
            content: format!("{:.0}%", self.fraction * 100.0),
            position: center - Vector::new(0.0, 7.0),
            color: TEXT,
            size: Pixels(TYPE_STAT),
            font: INTER_BOLD,
            align_x: Horizontal::Center.into(),
            align_y: Vertical::Center,
            ..canvas::Text::default()
        });
        frame.fill_text(canvas::Text {
            content: "used".to_owned(),
            position: center + Vector::new(0.0, 13.0),
            color: MUTED,
            size: Pixels(TYPE_KICKER),
            font: INTER,
            align_x: Horizontal::Center.into(),
            align_y: Vertical::Center,
            ..canvas::Text::default()
        });

        vec![frame.into_geometry()]
    }
}

/// Builds a frame whose clip region survives `iced_tiny_skia` 0.14.0.
///
/// That renderer stores a geometry group's clip bounds already translated to
/// the canvas position and then translates them again when rasterising, so a
/// frame created with `Frame::new` is culled for every canvas that is not at
/// the window origin (fixed upstream after 0.14.0). Anchoring the clip at the
/// negated canvas position and widening it by that offset keeps the region
/// covered under both the buggy double translation and a corrected single one.
/// Drawing coordinates are unaffected: they stay relative to the canvas.
fn canvas_frame(renderer: &Renderer, bounds: Rectangle) -> Frame {
    Frame::with_bounds(
        renderer,
        Rectangle {
            x: -bounds.x,
            y: -bounds.y,
            width: bounds.width + bounds.x,
            height: bounds.height + bounds.y,
        },
    )
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

    let (title, subtitle, summary, body): (String, String, String, Element<'a, Message>) =
        if daily_ready {
            let max_credits = visible
                .iter()
                .map(|day| day.credits)
                .fold(0.0_f64, f64::max)
                .max(1.0);
            let bars = visible
                .iter()
                .enumerate()
                .map(|(index, day)| {
                    let today = index + 1 == visible.len();
                    Bar {
                        share: (day.credits / max_credits).clamp(0.0, 1.0) as f32,
                        credits: day.credits,
                        kind: day_kind(&day.date, today),
                        label: if today {
                            "Today".to_owned()
                        } else {
                            weekday_initial(day)
                        },
                    }
                })
                .collect();
            (
                "USAGE HISTORY".to_owned(),
                date_range(visible).unwrap_or_else(|| format!("Last {CHART_DAYS} days")),
                money(Some(visible.iter().map(|day| day.credits).sum()), false),
                chart_canvas(BarChart { bars, max_credits }),
            )
        } else if series.len() >= 2 {
            let buckets = recent_buckets(series, 24);
            let maximum = buckets.iter().copied().fold(0.0_f64, f64::max).max(1.0);
            let bars = buckets
                .iter()
                .enumerate()
                .map(|(index, credits)| {
                    let last = index + 1 == buckets.len();
                    Bar {
                        share: (*credits / maximum).clamp(0.0, 1.0) as f32,
                        credits: *credits,
                        kind: if last {
                            DayKind::Today
                        } else {
                            DayKind::Weekday
                        },
                        label: if index == 0 {
                            "Earlier".to_owned()
                        } else if last {
                            "Now".to_owned()
                        } else {
                            String::new()
                        },
                    }
                })
                .collect();
            (
                "RECENT ACTIVITY".to_owned(),
                "Recorded samples; the daily view starts after a second day".to_owned(),
                format!("{} samples", sample_count.unwrap_or(series.len() as u64)),
                chart_canvas(BarChart {
                    bars,
                    max_credits: maximum,
                }),
            )
        } else {
            (
                "USAGE HISTORY".to_owned(),
                "No recorded changes yet".to_owned(),
                "—".to_owned(),
                container(
                    column![
                        text("Waiting for usage samples")
                            .size(TYPE_TITLE)
                            .font(INTER_SEMIBOLD)
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
                .align_y(Alignment::Center)
                .into(),
            )
        };

    let mut heading = row![
        column![
            kicker_owned(title, MUTED),
            text(subtitle).size(TYPE_LABEL).color(MUTED),
        ]
        .spacing(3),
        Space::new().width(Length::Fill),
    ]
    .spacing(18)
    .align_y(Alignment::Center);
    if daily_ready {
        heading = heading.push(chart_legend());
    }
    heading = heading.push(
        text(summary)
            .size(TYPE_BODY)
            .font(INTER_SEMIBOLD)
            .color(TEXT),
    );

    container(column![heading, body].spacing(12).height(Length::Fill))
        .width(Length::FillPortion(3))
        .height(Length::Fill)
        .padding([18, 20])
        .style(|_| elevated_style())
        .into()
}

fn chart_canvas<'a>(chart: BarChart) -> Element<'a, Message> {
    let canvas: Canvas<BarChart, Message> = canvas_widget(chart)
        .width(Length::Fill)
        .height(Length::Fill);
    canvas.into()
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

fn chart_legend<'a>() -> Element<'a, Message> {
    row![
        legend_item(DayKind::Weekday.color(), "Weekday"),
        legend_item(DayKind::Weekend.color(), "Weekend"),
        legend_item(DayKind::Today.color(), "Today"),
    ]
    .spacing(14)
    .align_y(Alignment::Center)
    .into()
}

fn legend_item<'a>(color: Color, label: &'static str) -> Element<'a, Message> {
    row![
        container(Space::new())
            .width(Length::Fixed(9.0))
            .height(Length::Fixed(9.0))
            .style(move |_| container::Style {
                background: Some(Background::Color(color)),
                border: Border {
                    radius: 3.0.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            }),
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
    let tone = allowance_tone(fraction);
    let gauge: Canvas<Gauge, Message> = canvas_widget(Gauge { fraction, tone })
        .width(Length::Fixed(GAUGE_SIZE))
        .height(Length::Fixed(GAUGE_SIZE));
    let figures = column![
        column![
            kicker("REMAINING", MUTED),
            text(money(current.remaining, false))
                .size(TYPE_STAT)
                .font(INTER_BOLD)
                .color(tone),
        ]
        .spacing(3),
        column![
            kicker("DAILY AVERAGE", MUTED),
            text(money(metrics.average_per_day, false))
                .size(TYPE_STAT)
                .font(INTER_BOLD)
                .color(TEXT),
        ]
        .spacing(3),
    ]
    .spacing(12)
    .width(Length::Fill);

    container(
        column![
            row![
                kicker("ALLOWANCE", tone),
                Space::new().width(Length::Fill),
                text("Monthly cap").size(TYPE_KICKER).color(MUTED),
            ]
            .align_y(Alignment::Center),
            container(row![gauge, figures].spacing(22).align_y(Alignment::Center))
                .height(Length::Fill)
                .align_y(Alignment::Center),
            container(
                text(format!(
                    "{} of {} consumed this cycle",
                    money(Some(used), false),
                    money(Some(entitlement), false)
                ))
                .size(TYPE_KICKER)
                .color(MUTED),
            )
            .width(Length::Fill)
            .padding([9, 12])
            .style(|_| glass_style()),
        ]
        .spacing(12),
    )
    .width(Length::FillPortion(2))
    .height(Length::Fill)
    .padding([18, 20])
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
    let whole = dollars.abs().trunc() as u64;
    let cents = ((dollars.abs() - whole as f64) * 100.0).round() as u64;
    let (whole, cents) = if cents == 100 {
        (whole + 1, 0)
    } else {
        (whole, cents)
    };
    let sign = if dollars < 0.0 { "-" } else { "" };
    format!("{prefix}{sign}${}.{cents:02}", thousands(whole))
}

fn number(value: Option<f64>) -> String {
    let Some(value) = value.filter(|value| value.is_finite()) else {
        return "—".to_owned();
    };
    if value.fract().abs() < 0.001 {
        thousands(value.round().abs() as u64)
    } else {
        format!("{value:.1}")
    }
}

fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn reset_text(epoch: Option<i64>) -> String {
    let Some(reset) = epoch else {
        return "Not reported".to_owned();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64);
    let days = ((reset - now).max(0) + 86_399) / 86_400;
    match days {
        0 => "Today".to_owned(),
        1 => "Tomorrow".to_owned(),
        _ => format!("In {days} days"),
    }
}

fn pace_label(value: Option<f64>) -> (String, Color) {
    match value {
        Some(credits) if credits >= 0.0 => (
            format!("{} under expected pace", money(Some(credits), false)),
            MINT,
        ),
        Some(credits) => (
            format!("{} over expected pace", money(Some(credits.abs()), false)),
            AMBER,
        ),
        None => ("Not enough data to compare pace".to_owned(), MUTED),
    }
}

fn app_mark<'a>() -> Element<'a, Message> {
    container(
        text("$")
            .size(18)
            .font(INTER_BOLD)
            .color(Color::WHITE)
            .align_x(Horizontal::Center)
            .align_y(Vertical::Center),
    )
    .width(Length::Fixed(38.0))
    .height(Length::Fixed(38.0))
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .style(|_| container::Style {
        background: Some(Background::Gradient(Gradient::Linear(
            gradient::Linear::new(Degrees(135.0))
                .add_stop(0.0, VIOLET_LIGHT)
                .add_stop(1.0, VIOLET),
        ))),
        border: Border {
            radius: 11.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    })
    .into()
}

fn stat_tile<'a>(title: &'static str, value: String, detail: &str) -> Element<'a, Message> {
    container(
        column![
            kicker(title, MUTED),
            text(value).size(TYPE_STAT).font(INTER_BOLD).color(TEXT),
            text(detail.to_owned()).size(TYPE_KICKER).color(MUTED),
        ]
        .spacing(4),
    )
    .width(Length::Fill)
    .padding([12, 14])
    .style(|_| glass_style())
    .into()
}

fn metric_card<'a>(
    title: &'static str,
    accent: Color,
    value: String,
    detail: &'static str,
) -> Element<'a, Message> {
    container(
        column![
            row![accent_dot(accent), kicker(title, MUTED)]
                .spacing(7)
                .align_y(Alignment::Center),
            text(value).size(TYPE_CARD).font(INTER_BOLD).color(TEXT),
            text(detail).size(TYPE_LABEL).color(MUTED),
        ]
        .spacing(6),
    )
    .width(Length::FillPortion(1))
    .padding([16, 20])
    .style(|_| elevated_style())
    .into()
}

fn accent_dot<'a>(color: Color) -> Element<'a, Message> {
    container(Space::new())
        .width(Length::Fixed(8.0))
        .height(Length::Fixed(8.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                radius: 4.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

fn kicker<'a>(value: &'static str, color: Color) -> iced::widget::Text<'a> {
    text(value)
        .size(TYPE_KICKER)
        .font(INTER_SEMIBOLD)
        .color(color)
}

fn kicker_owned<'a>(value: String, color: Color) -> iced::widget::Text<'a> {
    text(value)
        .size(TYPE_KICKER)
        .font(INTER_SEMIBOLD)
        .color(color)
}

fn dot<'a>() -> Element<'a, Message> {
    text("•").size(TYPE_LABEL).color(BORDER).into()
}

fn chip<'a>(value: String, color: Color) -> Element<'a, Message> {
    container(
        text(value)
            .size(TYPE_LABEL)
            .font(INTER_SEMIBOLD)
            .color(color),
    )
    .padding([6, 11])
    .style(move |_| container::Style {
        background: Some(Background::Color(Color { a: 0.14, ..color })),
        border: Border {
            color: Color { a: 0.35, ..color },
            width: 1.0,
            radius: CONTROL_RADIUS.into(),
        },
        ..container::Style::default()
    })
    .into()
}

fn status_pill<'a>(value: &str, color: Color) -> Element<'a, Message> {
    container(
        text(value.to_owned())
            .size(TYPE_KICKER)
            .font(INTER_SEMIBOLD)
            .color(color),
    )
    .padding([6, 11])
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

fn refresh_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Hovered => (Color { a: 0.32, ..VIOLET }, TEXT),
        button::Status::Pressed => (Color { a: 0.45, ..VIOLET }, TEXT),
        button::Status::Disabled => (Color { a: 0.10, ..VIOLET }, MUTED),
        button::Status::Active => (Color { a: 0.20, ..VIOLET }, VIOLET_LIGHT),
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color,
        border: Border {
            color: Color { a: 0.45, ..VIOLET },
            width: 1.0,
            radius: CONTROL_RADIUS.into(),
        },
        ..button::Style::default()
    }
}

fn error_banner<'a>(message: &'a str) -> Element<'a, Message> {
    container(text(message).size(TYPE_LABEL).color(ERROR))
        .width(Length::Fill)
        .padding([10, 14])
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

/// The hero carries a violet wash so the headline figure sits apart
/// from the plain metric cards below it.
fn hero_style() -> container::Style {
    let wash = gradient::Linear::new(Degrees(160.0))
        .add_stop(0.0, HERO_TINT)
        .add_stop(0.6, Color::from_rgb(0.094, 0.082, 0.196))
        .add_stop(1.0, SURFACE);
    container::Style {
        background: Some(Background::Gradient(Gradient::Linear(wash))),
        border: Border {
            color: Color { a: 0.45, ..VIOLET },
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
            color: Color { a: 0.55, ..BORDER },
            width: 1.0,
            radius: CARD_RADIUS.into(),
        },
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

fn glass_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(Color {
            a: 0.75,
            ..SURFACE_HIGH
        })),
        border: Border {
            color: Color { a: 0.45, ..BORDER },
            width: 1.0,
            radius: INSET_RADIUS.into(),
        },
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

fn lighten(color: Color, amount: f32) -> Color {
    Color {
        r: color.r + (1.0 - color.r) * amount,
        g: color.g + (1.0 - color.g) * amount,
        b: color.b + (1.0 - color.b) * amount,
        a: color.a,
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
        assert_eq!(money(Some(123_456.0), false), "$1,234.56");
        assert_eq!(money(Some(12.0), true), "+$0.12");
        assert_eq!(money(Some(99.999), false), "$1.00");
        assert_eq!(money(None, false), "—");
        assert_eq!(number(Some(3027.0)), "3,027");
        assert_eq!(reset_text(None), "Not reported");
    }
}
