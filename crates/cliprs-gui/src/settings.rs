use std::fmt;
use std::io;

use cliprs_ipc::{Capabilities, Settings};
use iced::widget::{button, column, container, pick_list, row, text};
use iced::{Alignment, Element, Length};

use crate::{Message, style};

#[derive(Default, Debug, Copy, Clone, PartialEq)]
pub enum BitrateOptions {
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

impl fmt::Display for BitrateOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match &self {
            BitrateOptions::Low => write!(formatter, "Low (10 Mbit/sec)"),
            BitrateOptions::Medium => write!(formatter, "Medium (20 Mbit/sec)"),
            BitrateOptions::High => write!(formatter, "High (30 Mbit/sec)"),
        }
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq)]
pub enum FPSOptions {
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

impl fmt::Display for FPSOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match &self {
            FPSOptions::Fps30 => write!(formatter, "30 FPS"),
            FPSOptions::Fps60 => write!(formatter, "60 FPS"),
            FPSOptions::Fps120 => write!(formatter, "120 FPS"),
            FPSOptions::Fps144 => write!(formatter, "144 FPS"),
        }
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq)]
pub enum DurationOptions {
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

impl fmt::Display for DurationOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match &self {
            DurationOptions::Sec5 => write!(formatter, "5 Seconds"),
            DurationOptions::Sec15 => write!(formatter, "15 Seconds"),
            DurationOptions::Sec30 => write!(formatter, "30 Seconds"),
            DurationOptions::Sec60 => write!(formatter, "60 Seconds"),
        }
    }
}

pub struct Form {
    pub fps: Option<FPSOptions>,
    pub bitrate: Option<BitrateOptions>,
    pub duration: Option<DurationOptions>,
    monitor: Option<String>,
    gop_frames: Option<u32>,
}

impl Form {
    pub fn load() -> Form {
        let settings = Settings::load().unwrap_or_default();
        Form {
            fps: FPSOptions::from_value(settings.fps),
            bitrate: BitrateOptions::from_bps(settings.average_bitrate_bps),
            duration: DurationOptions::from_seconds(settings.clip_seconds),
            monitor: settings.monitor,
            gop_frames: Capabilities::load()
                .ok()
                .flatten()
                .map(|capabilities| capabilities.gop_frames),
        }
    }

    pub fn save(&self) -> io::Result<()> {
        match self.settings() {
            Some(settings) => settings.save(),
            None => Ok(()),
        }
    }

    fn settings(&self) -> Option<Settings> {
        Some(Settings {
            fps: self.fps?.value(),
            average_bitrate_bps: self.bitrate?.average_bps(),
            clip_seconds: self.duration?.seconds(),
            monitor: self.monitor.clone(),
        })
    }

    fn estimate_ram_mb(&self) -> Option<(u64, u64)> {
        let settings = self.settings()?;
        let buffered_seconds = f64::from(settings.clip_seconds)
            + f64::from(self.gop_frames?) / f64::from(settings.fps);
        let megabytes = |bps: u64| (bps as f64 / 1_000_000.0 / 8.0 * buffered_seconds).round();
        Some((
            megabytes(settings.average_bitrate_bps) as u64,
            megabytes(settings.peak_bitrate_bps()) as u64,
        ))
    }
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

fn ram_infobox<'a>(typical: u64, peak: u64) -> Element<'a, Message> {
    let figures = column![
        text(format!(
            "Your configuration will use about {typical} MB RAM"
        ))
        .font(style::BOLD),
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

fn setting_row<'a>(label: &'a str, control: Element<'a, Message>) -> Element<'a, Message> {
    row![text(label).width(Length::Fill), control]
        .spacing(style::SECTION_SPACING)
        .align_y(Alignment::Center)
        .into()
}

pub fn view(form: &Form, applying: bool) -> Element<'_, Message> {
    column![
        setting_row(
            "Recording FPS",
            styled_pick_list(FPSOptions::ALL, form.fps, Message::FPSChanged),
        ),
        setting_row(
            "Average bitrate",
            styled_pick_list(BitrateOptions::ALL, form.bitrate, Message::BitrateChanged),
        ),
        setting_row(
            "Clip duration",
            styled_pick_list(
                DurationOptions::ALL,
                form.duration,
                Message::DurationChanged
            ),
        ),
    ]
    .extend(
        form.estimate_ram_mb()
            .map(|(typical, peak)| ram_infobox(typical, peak)),
    )
    .push(
        container(
            button("Apply")
                .padding(style::BUTTON_PADDING)
                .style(style::primary_button)
                .on_press_maybe((!applying).then_some(Message::ApplySettings)),
        )
        .align_right(Length::Fill),
    )
    .spacing(style::SECTION_SPACING)
    .into()
}
