mod library;
mod resources;
mod settings;
mod style;

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use cliprs_ipc::{Process, ResourceSample, notify_error};
use iced::widget::space::vertical;
use iced::widget::{button, column, container, row};
use iced::{Element, Length, Subscription, Theme};

use library::Clip;
use settings::{BitrateOptions, DurationOptions, FPSOptions};

use crate::resources::construct_resource_tab;

#[derive(Debug, Clone)]
enum Message {
    Navigate(SidebarTab),
    FPSChanged(FPSOptions),
    BitrateChanged(BitrateOptions),
    DurationChanged(DurationOptions),
    ClipClicked(PathBuf),
    ResourceTick,
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

struct State {
    selected_sidebar_tab: SidebarTab,
    settings: settings::Form,
    clips: Vec<Clip>,
    resource_sample: Option<ResourceSample>,
    cpu_percent: Option<f32>,
}

impl State {
    fn load() -> State {
        State {
            selected_sidebar_tab: SidebarTab::default(),
            settings: settings::Form::load(),
            clips: library::load_clips(),
            resource_sample: None,
            cpu_percent: None,
        }
    }

    fn save_settings(&self) {
        if let Err(error) = self.settings.save() {
            log::error!("settings save failed: {error}");
            notify_error(format!("Could not save settings: {error}"));
        }
    }
}

fn construct_sidebar_button(tab: SidebarTab, state: &State) -> Element<'_, Message> {
    let selected: bool = state.selected_sidebar_tab == tab;

    button(row![style::sidebar_icon(tab.icon_bytes(), selected), tab.name()].spacing(8))
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

fn construct_main_view(state: &State) -> Element<'_, Message> {
    let tab = state.selected_sidebar_tab;
    let page = match tab {
        SidebarTab::Library => library::view(&state.clips),
        SidebarTab::Settings => settings::view(&state.settings),
    };

    container(
        column![style::heading(tab.name()), page]
            .spacing(style::SECTION_SPACING)
            .max_width(style::CONTENT_MAX_WIDTH),
    )
    .padding(style::PAGE_PADDING)
    .center_x(Length::Fill)
    .into()
}

fn view(state: &State) -> Element<'_, Message> {
    let mut sidebar = column![
        column(
            SidebarTab::ALL
                .iter()
                .map(|tab| construct_sidebar_button(*tab, state)),
        )
        .spacing(style::SIDEBAR_SPACING),
        vertical(),
    ];
    if let Some(sample) = &state.resource_sample {
        sidebar = sidebar.push(construct_resource_tab(sample, state.cpu_percent));
    }
    let sidebar_container = container(sidebar)
        .padding(style::SIDEBAR_PADDING)
        .style(style::sidebar)
        .width(style::SIDEBAR_WIDTH)
        .height(Length::Fill);

    let main_view = construct_main_view(state);

    container(row![sidebar_container, main_view]).into()
}

fn update(state: &mut State, message: Message) {
    match message {
        Message::Navigate(tab) => {
            state.selected_sidebar_tab = tab;
            if tab == SidebarTab::Library {
                state.clips = library::load_clips();
            }
        }
        Message::FPSChanged(selected) => {
            state.settings.fps = Some(selected);
            state.save_settings();
        }
        Message::BitrateChanged(selected) => {
            state.settings.bitrate = Some(selected);
            state.save_settings();
        }
        Message::DurationChanged(selected) => {
            state.settings.duration = Some(selected);
            state.save_settings();
        }
        Message::ClipClicked(video_path) => {
            if let Err(error) = Command::new("xdg-open").arg(&video_path).spawn() {
                log::error!(
                    "failed to open clip at {} via xdg-open: {}",
                    video_path.display(),
                    error
                );
                notify_error(format!("Failed to open file: {error}"));
            }
        }
        Message::ResourceTick => match Process::Daemon.sample_resources() {
            Ok(sample) => {
                state.cpu_percent = sample
                    .zip(state.resource_sample)
                    .and_then(|(now, previous)| now.cpu_percent_since(&previous))
                    .map(|percent_of_one_core| percent_of_one_core / core_count());
                state.resource_sample = sample;
            }
            Err(error) => log::warn!("failed to sample daemon resources: {error}"),
        },
    }
}

fn core_count() -> f32 {
    std::thread::available_parallelism().map_or(1.0, |cores| cores.get() as f32)
}

fn subscription(_state: &State) -> Subscription<Message> {
    iced::time::every(Duration::from_secs(1)).map(|_| Message::ResourceTick)
}

fn theme(_state: &State) -> Theme {
    style::theme()
}

fn main() -> iced::Result {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,cliprs_gui=info,cliprs_ipc=info"),
    )
    .init();
    iced::application(State::load, update, view)
        .theme(theme)
        .subscription(subscription)
        .run()
}
