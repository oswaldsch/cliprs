use std::sync::LazyLock;

use cliprs_ipc::{ResourceSample, total_memory_bytes};
use iced::widget::space::horizontal;
use iced::widget::{column, container, progress_bar, row};
use iced::{Alignment, Element};

use crate::Message;
use crate::style;

const BYTES_PER_MIB: f64 = 1024.0 * 1024.0;
const MIB_PER_GIB: f64 = 1024.0;

static TOTAL_MEMORY_BYTES: LazyLock<Option<u64>> = LazyLock::new(|| {
    total_memory_bytes()
        .inspect_err(|error| log::warn!("failed to read total memory: {error}"))
        .ok()
});

pub fn construct_resource_tab(
    resource_sample: &ResourceSample,
    cpu_percent: Option<f32>,
) -> Element<'_, Message> {
    let ram_percent = TOTAL_MEMORY_BYTES
        .map(|total| 100.0 * resource_sample.resident_bytes as f32 / total as f32);

    let cpu = construct_stat(
        include_bytes!("../assets/cpu.svg"),
        "CPU",
        cpu_percent.map_or_else(|| "...".to_owned(), |percent| format!("{percent:.1}%")),
        cpu_percent,
    );
    let ram = construct_stat(
        include_bytes!("../assets/memory-stick.svg"),
        "RAM",
        format_bytes(resource_sample.resident_bytes),
        ram_percent,
    );

    container(column![cpu, ram].spacing(style::STAT_SPACING))
        .padding(style::STAT_CARD_PADDING)
        .style(style::card)
        .into()
}

fn construct_stat(
    icon_bytes: &'static [u8],
    label: &'static str,
    value: String,
    percent: Option<f32>,
) -> Element<'static, Message> {
    column![
        row![
            style::stat_icon(icon_bytes),
            style::stat_label(label),
            horizontal(),
            style::stat_value(value),
        ]
        .spacing(style::STAT_ICON_SPACING)
        .align_y(Alignment::Center),
        progress_bar(0.0..=100.0, percent.unwrap_or(0.0))
            .girth(style::USAGE_BAR_GIRTH)
            .style(style::usage_bar),
    ]
    .spacing(style::STAT_BAR_SPACING)
    .into()
}

fn format_bytes(bytes: u64) -> String {
    let mib = bytes as f64 / BYTES_PER_MIB;
    if mib >= MIB_PER_GIB {
        format!("{:.2} GB", mib / MIB_PER_GIB)
    } else {
        format!("{mib:.0} MB")
    }
}
