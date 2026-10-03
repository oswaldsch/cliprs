use std::fmt;
use std::io;

use cliprs_ipc::Settings;
use iced::font::Weight;
use iced::widget::{button, column, container, pick_list, row, text};
use iced::{Element, Font, Length, Theme};

#[derive(Debug, Clone)]
enum Message {
    Navigate(SidebarTab),
    FPSChanged(FPSOptions),
    BitrateChanged(BitrateOptions),
    DurationChanged(DurationOptions),
}

#[derive(Default, Debug, Copy, Clone, PartialEq)]
enum SidebarTab {
    #[default]
    Home,
    Settings,
}

impl SidebarTab {
    const ALL: &[SidebarTab] = &[SidebarTab::Home, SidebarTab::Settings];
    fn name(&self) -> &'static str {
        match self {
            SidebarTab::Home => "Home",
            SidebarTab::Settings => "Settings",
        }
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq)]
enum BitrateOptions {
    Low,
    Medium,
    #[default]
    High,
}

impl BitrateOptions {
    const ALL: &[BitrateOptions] = &[
        BitrateOptions::Low,
        BitrateOptions::Medium,
        BitrateOptions::High,
    ];

    fn average_bps(&self) -> u64 {
        match self {
            BitrateOptions::Low => 10_000_000,
            BitrateOptions::Medium => 20_000_000,
            BitrateOptions::High => 30_000_000,
        }
    }

    fn from_bps(bps: u64) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|option| option.average_bps() == bps)
    }
}

impl std::fmt::Display for BitrateOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> fmt::Result {
        match &self {
            BitrateOptions::Low => write!(formatter, "Low"),
            BitrateOptions::Medium => write!(formatter, "Medium"),
            BitrateOptions::High => write!(formatter, "High"),
        }
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq)]
enum FPSOptions {
    Fps30,
    #[default]
    Fps60,
    Fps120,
    Fps144,
}

impl FPSOptions {
    const ALL: &[FPSOptions] = &[
        FPSOptions::Fps30,
        FPSOptions::Fps60,
        FPSOptions::Fps120,
        FPSOptions::Fps144,
    ];

    fn value(&self) -> u32 {
        match self {
            FPSOptions::Fps30 => 30,
            FPSOptions::Fps60 => 60,
            FPSOptions::Fps120 => 120,
            FPSOptions::Fps144 => 144,
        }
    }

    fn from_value(value: u32) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|option| option.value() == value)
    }
}

impl std::fmt::Display for FPSOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> fmt::Result {
        match &self {
            FPSOptions::Fps30 => write!(formatter, "30 FPS"),
            FPSOptions::Fps60 => write!(formatter, "60 FPS"),
            FPSOptions::Fps120 => write!(formatter, "120 FPS"),
            FPSOptions::Fps144 => write!(formatter, "144 FPS"),
        }
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq)]
enum DurationOptions {
    Sec5,
    Sec15,
    #[default]
    Sec30,
    Sec60,
}

impl DurationOptions {
    const ALL: &[DurationOptions] = &[
        DurationOptions::Sec5,
        DurationOptions::Sec15,
        DurationOptions::Sec30,
        DurationOptions::Sec60,
    ];

    fn seconds(&self) -> u32 {
        match self {
            DurationOptions::Sec5 => 5,
            DurationOptions::Sec15 => 15,
            DurationOptions::Sec30 => 30,
            DurationOptions::Sec60 => 60,
        }
    }

    fn from_seconds(seconds: u32) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|option| option.seconds() == seconds)
    }
}

impl std::fmt::Display for DurationOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> fmt::Result {
        match &self {
            DurationOptions::Sec5 => write!(formatter, "5 Seconds"),
            DurationOptions::Sec15 => write!(formatter, "15 Seconds"),
            DurationOptions::Sec30 => write!(formatter, "30 Seconds"),
            DurationOptions::Sec60 => write!(formatter, "60 Seconds"),
        }
    }
}

#[derive(Default)]
struct State {
    selected_sidebar_tab: SidebarTab,
    fps: Option<FPSOptions>,
    bitrate: Option<BitrateOptions>,
    duration: Option<DurationOptions>,
}

impl State {
    fn load() -> State {
        let settings = Settings::load().unwrap_or_default();
        State {
            fps: FPSOptions::from_value(settings.fps),
            bitrate: BitrateOptions::from_bps(settings.average_bitrate_bps),
            duration: DurationOptions::from_seconds(settings.clip_seconds),
            ..State::default()
        }
    }

    fn save(&self) -> io::Result<()> {
        let (Some(fps), Some(bitrate), Some(duration)) = (self.fps, self.bitrate, self.duration)
        else {
            return Ok(());
        };
        Settings {
            fps: fps.value(),
            average_bitrate_bps: bitrate.average_bps(),
            clip_seconds: duration.seconds(),
        }
        .save()
    }
}

fn construct_heading(content: &str) -> Element<'_, Message> {
    text(content)
        .size(24)
        .font(Font {
            weight: Weight::Bold,
            ..Font::DEFAULT
        })
        .width(Length::Fill)
        .center()
        .into()
}

fn construct_sidebar_button(tab: SidebarTab, state: &State) -> Element<'_, Message> {
    button(tab.name())
        .width(Length::Fill)
        .on_press(Message::Navigate(tab))
        .style(if state.selected_sidebar_tab == tab {
            button::primary // TODO: remove rounded corners
        } else {
            button::text
        })
        .into()
}

const GOP_FRAMES: f64 = 120.0;
const PEAK_TO_AVERAGE: f64 = 2.0;

fn estimate_ram_mb(state: &State) -> Option<(u64, u64)> {
    let (fps, bitrate, duration) = (state.fps?, state.bitrate?, state.duration?);
    let buffered_seconds = f64::from(duration.seconds()) + GOP_FRAMES / f64::from(fps.value());
    let typical_mb = bitrate.average_bps() as f64 / 1_000_000.0 / 8.0 * buffered_seconds;
    Some((
        typical_mb.round() as u64,
        (typical_mb * PEAK_TO_AVERAGE).round() as u64,
    ))
}

fn construct_main_view(state: &State) -> Element<'_, Message> {
    match state.selected_sidebar_tab {
        SidebarTab::Home => container(column![construct_heading("Welcome to cliprs")]),
        SidebarTab::Settings => container(
            column![
                construct_heading("Settings"),
                text("Recording FPS"),
                pick_list(FPSOptions::ALL, state.fps, Message::FPSChanged),
                text("Average bitrate"),
                pick_list(BitrateOptions::ALL, state.bitrate, Message::BitrateChanged),
                text("Clip duration"),
                pick_list(
                    DurationOptions::ALL,
                    state.duration,
                    Message::DurationChanged
                ),
            ]
            .extend(estimate_ram_mb(state).map(|(typical, peak)| {
                Element::from(text(format!(
                    "With your current settings, the recorder will use ~{typical} MB RAM (up to {peak} MB in very busy scenes)."
                )))
            }))
            .spacing(10),
        )
        .padding(10),
    }
    .into()
}

fn view(state: &State) -> Element<'_, Message> {
    let sidebar = container(column(
        SidebarTab::ALL
            .iter()
            .map(|tab| construct_sidebar_button(*tab, state)),
    ))
    .style(container::dark)
    .width(200)
    .height(Length::Fill);

    let main_view = construct_main_view(state);

    container(row![sidebar, main_view]).into()
}

fn update(state: &mut State, message: Message) {
    match message {
        Message::Navigate(page) => {
            state.selected_sidebar_tab = page;
            return;
        }
        Message::FPSChanged(selected) => state.fps = Some(selected),
        Message::BitrateChanged(selected) => state.bitrate = Some(selected),
        Message::DurationChanged(selected) => state.duration = Some(selected),
    }
    // TODO: surface save errors in the UI, there is no logger in this crate yet
    let _ = state.save();
}

fn theme(_state: &State) -> Theme {
    Theme::Dark
}

fn main() -> iced::Result {
    iced::application(State::load, update, view)
        .theme(theme)
        .run()
}
