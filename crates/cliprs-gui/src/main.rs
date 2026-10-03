mod style;

use std::fmt;
use std::io;

use cliprs_ipc::Settings;
use iced::font::Weight;
use iced::widget::{button, column, container, pick_list, row, text};
use iced::{Alignment, Element, Font, Length, Theme};

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
    Library,
    Settings,
}

impl SidebarTab {
    const ALL: &[SidebarTab] = &[SidebarTab::Library, SidebarTab::Settings];
    fn name(&self) -> &'static str {
        match self {
            SidebarTab::Library => "Library",
            SidebarTab::Settings => "Settings",
        }
    }

    fn icon_bytes(&self) -> &'static [u8] {
        match self {
            SidebarTab::Library => include_bytes!("../assets/library.svg"),
            SidebarTab::Settings => include_bytes!("../assets/settings.svg"),
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
            BitrateOptions::Low => write!(formatter, "Low (10 Mbit/sec)"),
            BitrateOptions::Medium => write!(formatter, "Medium (20 Mbit/sec)"),
            BitrateOptions::High => write!(formatter, "High (30 Mbit/sec)"),
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

fn construct_sidebar_button(tab: SidebarTab, state: &State) -> Element<'_, Message> {
    let selected: bool = state.selected_sidebar_tab == tab;

    button(row![style::icon(tab.icon_bytes(), selected), tab.name()].spacing(8))
        .width(Length::Fill)
        .on_press(Message::Navigate(tab))
        .padding(style::BUTTON_PADDING)
        .style(if selected {
            style::sidebar_button_selected
        } else {
            style::sidebar_button
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

fn styled_pick_list<'a, T>(
    options: &'a [T],
    selected: Option<T>,
    on_selected: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message>
where
    T: ToString + PartialEq + Clone + 'a,
{
    pick_list(options, selected, on_selected)
        .style(style::dropdown)
        .menu_style(style::dropdown_menu)
        .width(style::CONTROL_WIDTH)
        .padding(style::PICK_LIST_PADDING)
        .into()
}

fn construct_ram_infobox<'a>(typical: u64, peak: u64) -> Element<'a, Message> {
    let figures = column![
        text(format!(
            "Your configuration will use about {typical} MB RAM"
        ))
        .font(Font {
            weight: Weight::Bold,
            ..Font::DEFAULT
        }),
        text(format!("Up to {peak} MB in very busy scenes"))
            .size(style::LABEL_SIZE)
            .color(style::MUTED),
    ]
    .spacing(4);

    container(
        row![
            style::accent_icon(include_bytes!("../assets/info.svg")),
            figures
        ]
        .spacing(style::SECTION_SPACING)
        .align_y(Alignment::Start),
    )
    .style(style::card)
    .padding(14)
    .width(Length::Fill)
    .into()
}

fn construct_setting_row<'a>(
    label: &'a str,
    control: Element<'a, Message>,
) -> Element<'a, Message> {
    row![text(label).width(Length::Fill), control]
        .spacing(style::SECTION_SPACING)
        .align_y(Alignment::Center)
        .into()
}

fn construct_main_view(state: &State) -> Element<'_, Message> {
    match state.selected_sidebar_tab {
        SidebarTab::Library => container(column![style::heading("Library")]),
        SidebarTab::Settings => container(
            column![
                style::heading("Settings"),
                construct_setting_row(
                    "Recording FPS",
                    styled_pick_list(FPSOptions::ALL, state.fps, Message::FPSChanged),
                ),
                construct_setting_row(
                    "Average bitrate",
                    styled_pick_list(BitrateOptions::ALL, state.bitrate, Message::BitrateChanged),
                ),
                construct_setting_row(
                    "Clip duration",
                    styled_pick_list(
                        DurationOptions::ALL,
                        state.duration,
                        Message::DurationChanged
                    ),
                ),
            ]
            .extend(
                estimate_ram_mb(state).map(|(typical, peak)| construct_ram_infobox(typical, peak)),
            )
            .spacing(style::SECTION_SPACING)
            .max_width(style::CONTENT_MAX_WIDTH),
        )
        .padding(style::PAGE_PADDING)
        .center_x(Length::Fill),
    }
    .into()
}

fn view(state: &State) -> Element<'_, Message> {
    let sidebar = container(
        column(
            SidebarTab::ALL
                .iter()
                .map(|tab| construct_sidebar_button(*tab, state)),
        )
        .spacing(style::SIDEBAR_SPACING),
    )
    .padding(style::SIDEBAR_PADDING)
    .style(style::sidebar)
    .width(style::SIDEBAR_WIDTH)
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
    style::theme()
}

fn main() -> iced::Result {
    iced::application(State::load, update, view)
        .theme(theme)
        .run()
}
