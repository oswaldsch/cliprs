use std::io;

use cliprs_ipc::{Monitor, Settings, install, monitors};
use iced::{
    Center, Element,
    Length::Fill,
    Task,
    widget::{button, column, container, row},
};

use crate::style::{
    BUTTON_PADDING, CARD_TEXT_SPACING, MONITOR_CARD_WIDTH, MONITOR_ICON_SIZE, PAGE_PADDING,
    SECTION_SPACING, accent_icon, card_button, card_button_selected, clip_details, clip_title,
    heading, plain_icon, primary_button, secondary_button, subheading,
};

const PAGE_COUNT: u32 = 2;

pub struct State {
    current_page: u32,
    monitors: Vec<Monitor>,
    selected_monitor: Option<Monitor>,
    installing: bool,
    installed: bool,
    install_error: Option<String>,
}

impl State {
    pub fn finished(&self) -> bool {
        self.installed
    }

    fn on_last_page(&self) -> bool {
        self.current_page + 1 == PAGE_COUNT
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    PreviousPage,
    NextPage,
    MonitorSelected(Monitor),
    Installed(Result<(), String>),
}

fn save_settings(state: &State) -> io::Result<()> {
    let mut settings = Settings::load().unwrap_or_default();
    settings.monitor = state
        .selected_monitor
        .as_ref()
        .map(|monitor| monitor.id.clone());
    settings.save()
}

fn construct_button_row(state: &State) -> Element<'_, Message> {
    row![
        button("Back")
            .padding(BUTTON_PADDING)
            .style(secondary_button)
            .width(Fill)
            .on_press_maybe(
                (state.current_page > 0 && !state.installing).then_some(Message::PreviousPage)
            ),
        button(if state.on_last_page() {
            "Finish"
        } else {
            "Next"
        })
        .padding(BUTTON_PADDING)
        .style(primary_button)
        .width(Fill)
        .on_press_maybe((!state.installing).then_some(Message::NextPage))
    ]
    .spacing(10)
    .into()
}

fn construct_monitor_card<'a>(monitor: &'a Monitor, selected: bool) -> Element<'a, Message> {
    let icon = if selected { accent_icon } else { plain_icon };
    let (width, height) = monitor.resolution;
    let mode = format!("{width}x{height} at {} Hz", monitor.refresh_hz);
    let connector = monitor.name.as_ref().map(|_| monitor.id.as_str());
    button(
        column![
            icon(include_bytes!("../assets/monitor.svg"))
                .width(MONITOR_ICON_SIZE)
                .height(MONITOR_ICON_SIZE),
            clip_title(monitor.name.as_deref().unwrap_or(&monitor.id)),
            clip_details(mode),
        ]
        .extend(connector.map(|connector| Element::from(clip_details(connector))))
        .spacing(CARD_TEXT_SPACING)
        .align_x(Center)
        .width(Fill),
    )
    .width(MONITOR_CARD_WIDTH)
    .padding(PAGE_PADDING)
    .style(if selected {
        card_button_selected
    } else {
        card_button
    })
    .on_press(Message::MonitorSelected(monitor.clone()))
    .into()
}

fn construct_monitor_cards(state: &State) -> Element<'_, Message> {
    let cards = state.monitors.iter().map(|monitor| {
        let selected = state.selected_monitor.as_ref() == Some(monitor);
        construct_monitor_card(monitor, selected)
    });
    container(row(cards).spacing(SECTION_SPACING))
        .center(Fill)
        .into()
}

pub fn boot() -> State {
    let monitors = monitors().unwrap_or_else(|error| {
        log::warn!("could not list monitors: {error}");
        Vec::new()
    });
    let largest = monitors
        .iter()
        .max_by_key(|monitor| u64::from(monitor.resolution.0) * u64::from(monitor.resolution.1))
        .cloned();
    State {
        current_page: 0,
        monitors,
        selected_monitor: largest,
        installing: false,
        installed: false,
        install_error: None,
    }
}

pub fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::NextPage if state.on_last_page() => {
            state.install_error = None;
            if let Err(error) = save_settings(state) {
                log::error!("settings save failed: {error}");
                state.install_error = Some(format!("Could not save settings: {error}"));
                return Task::none();
            }
            state.installing = true;
            return crate::blocking_task(install::install).map(Message::Installed);
        }
        Message::NextPage => state.current_page += 1,
        Message::PreviousPage => state.current_page -= 1,
        Message::MonitorSelected(monitor) => state.selected_monitor = Some(monitor),
        Message::Installed(result) => {
            state.installing = false;
            match result {
                Ok(()) => state.installed = true,
                Err(error) => {
                    log::error!("service install failed: {error}");
                    state.install_error = Some(error);
                }
            }
        }
    }
    Task::none()
}

pub fn view(state: &State) -> Element<'_, Message> {
    match state.current_page {
        0 => column![
            heading("Welcome to cliprs!")
                .width(Fill)
                .height(Fill)
                .center(),
            construct_button_row(&state)
        ]
        .padding(PAGE_PADDING)
        .into(),
        1 => column![
            heading("Monitors").width(Fill).center(),
            subheading("First, lets pick your primary monitor")
                .width(Fill)
                .center(),
            construct_monitor_cards(state),
        ]
        .extend(
            state
                .install_error
                .as_deref()
                .map(|error| subheading(error).width(Fill).center().into()),
        )
        .push(construct_button_row(&state))
        .padding(PAGE_PADDING)
        .into(),
        _ => column![heading("Page count invalid")].into(),
    }
}
