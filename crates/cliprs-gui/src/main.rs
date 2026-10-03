use std::fmt;

use iced::font::{Family, Weight};
use iced::widget::{button, column, container, pick_list, row, text, text_input};
use iced::{Element, Font, Length, Theme, padding};

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

    fn average_mbps(&self) -> f64 {
        match self {
            BitrateOptions::Low => 10.0,
            BitrateOptions::Medium => 20.0,
            BitrateOptions::High => 30.0,
        }
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

    fn value(&self) -> f64 {
        match self {
            FPSOptions::Fps30 => 30.0,
            FPSOptions::Fps60 => 60.0,
            FPSOptions::Fps120 => 120.0,
            FPSOptions::Fps144 => 144.0,
        }
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

    fn seconds(&self) -> f64 {
        match self {
            DurationOptions::Sec5 => 5.0,
            DurationOptions::Sec15 => 15.0,
            DurationOptions::Sec30 => 30.0,
            DurationOptions::Sec60 => 60.0,
        }
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
    let buffered_seconds = duration.seconds() + GOP_FRAMES / fps.value();
    let typical_mb = bitrate.average_mbps() / 8.0 * buffered_seconds;
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
        Message::Navigate(page) => state.selected_sidebar_tab = page,
        Message::FPSChanged(selected) => match selected {
            FPSOptions::Fps30 => state.fps = Some(FPSOptions::Fps30),
            FPSOptions::Fps60 => state.fps = Some(FPSOptions::Fps60),
            FPSOptions::Fps120 => state.fps = Some(FPSOptions::Fps120),
            FPSOptions::Fps144 => state.fps = Some(FPSOptions::Fps144),
        },
        Message::BitrateChanged(selected) => match selected {
            BitrateOptions::Low => state.bitrate = Some(BitrateOptions::Low),
            BitrateOptions::Medium => state.bitrate = Some(BitrateOptions::Medium),
            BitrateOptions::High => state.bitrate = Some(BitrateOptions::High),
        },
        Message::DurationChanged(selected) => match selected {
            DurationOptions::Sec5 => state.duration = Some(DurationOptions::Sec5),
            DurationOptions::Sec15 => state.duration = Some(DurationOptions::Sec15),
            DurationOptions::Sec30 => state.duration = Some(DurationOptions::Sec30),
            DurationOptions::Sec60 => state.duration = Some(DurationOptions::Sec60),
        },
    }
}

fn theme(_state: &State) -> Theme {
    Theme::Dark
}

fn main() -> iced::Result {
    iced::application(State::default, update, view)
        .theme(theme)
        .run()
}
