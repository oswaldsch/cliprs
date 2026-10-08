mod library;
mod resources;
mod settings;
mod setup;
mod style;

use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use cliprs_ipc::{Process, ResourceSample, Settings, install, notify, notify_error};
use iced::futures::channel::oneshot;
use iced::widget::space::vertical;
use iced::widget::{button, column, container, row};
use iced::{Element, Length, Subscription, Task, Theme};

use library::Clip;
use settings::{BitrateOptions, DurationOptions, FPSOptions};

use crate::resources::construct_resource_tab;
use crate::style::icon::{self, Tint, icon};
use crate::style::space;

const DEBUG_SETUP: bool = true;
const SIDEBAR_WIDTH: f32 = 220.0;
const CONTENT_MAX_WIDTH: f32 = 640.0;

#[derive(Debug, Clone)]
enum Message {
    Navigate(SidebarTab),
    FPSChanged(FPSOptions),
    BitrateChanged(BitrateOptions),
    DurationChanged(DurationOptions),
    ApplySettings,
    SettingsApplied(Result<(), String>),
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
    applying_settings: bool,
    clips: Vec<Clip>,
    resource_sample: Option<ResourceSample>,
    cpu_percent: Option<f32>,
}

impl State {
    fn load() -> State {
        State {
            selected_sidebar_tab: SidebarTab::default(),
            settings: settings::Form::load(),
            applying_settings: false,
            clips: library::load_clips(),
            resource_sample: None,
            cpu_percent: None,
        }
    }
}

// A polkit prompt blocks until answered, which would freeze the window on the UI thread.
fn blocking_task(
    work: impl FnOnce() -> io::Result<()> + Send + 'static,
) -> Task<Result<(), String>> {
    let (sender, receiver) = oneshot::channel();
    std::thread::spawn(move || {
        let _ = sender.send(work().map_err(|error| error.to_string()));
    });
    Task::perform(receiver, |result| {
        result.unwrap_or_else(|_| Err("background task stopped unexpectedly".into()))
    })
}

fn construct_sidebar_button(tab: SidebarTab, state: &State) -> Element<'_, Message> {
    let selected: bool = state.selected_sidebar_tab == tab;

    button(
        row![
            icon(tab.icon_bytes(), icon::MD, Tint::Nav { selected }),
            tab.name()
        ]
        .spacing(space::SM),
    )
    .width(Length::Fill)
    .on_press(Message::Navigate(tab))
    .padding(space::CONTROL)
    .style(style::button::nav(selected))
    .into()
}

fn construct_main_view(state: &State) -> Element<'_, Message> {
    let tab = state.selected_sidebar_tab;
    let page = match tab {
        SidebarTab::Library => library::view(&state.clips),
        SidebarTab::Settings => settings::view(&state.settings, state.applying_settings),
    };

    container(
        column![style::text::heading(tab.name()), page]
            .spacing(space::MD)
            .max_width(CONTENT_MAX_WIDTH),
    )
    .padding(space::LG)
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
        .spacing(space::XS),
        vertical(),
    ];
    if let Some(sample) = &state.resource_sample {
        sidebar = sidebar.push(construct_resource_tab(sample, state.cpu_percent));
    }
    let sidebar_container = container(sidebar)
        .padding(space::MD)
        .style(style::surface::panel)
        .width(SIDEBAR_WIDTH)
        .height(Length::Fill);

    let main_view = construct_main_view(state);

    container(row![sidebar_container, main_view]).into()
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::Navigate(tab) => {
            state.selected_sidebar_tab = tab;
            if tab == SidebarTab::Library {
                state.clips = library::load_clips();
            }
        }
        Message::FPSChanged(selected) => state.settings.fps = Some(selected),
        Message::BitrateChanged(selected) => state.settings.bitrate = Some(selected),
        Message::DurationChanged(selected) => state.settings.duration = Some(selected),
        Message::ApplySettings => match state.settings.save() {
            Ok(()) => {
                state.applying_settings = true;
                return blocking_task(install::restart_daemon).map(Message::SettingsApplied);
            }
            Err(error) => {
                log::error!("settings save failed: {error}");
                notify_error(format!("Could not save settings: {error}"));
            }
        },
        Message::SettingsApplied(result) => {
            state.applying_settings = false;
            if let Err(error) = result {
                log::error!("daemon restart failed: {error}");
                notify_error(format!("Settings saved, but the restart failed: {error}"));
            }
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
    Task::none()
}

fn core_count() -> f32 {
    std::thread::available_parallelism().map_or(1.0, |cores| cores.get() as f32)
}

fn subscription(_state: &State) -> Subscription<Message> {
    iced::time::every(Duration::from_secs(1)).map(|_| Message::ResourceTick)
}

enum Screen {
    Setup(setup::State),
    Main(State),
}

#[derive(Debug, Clone)]
enum ScreenMessage {
    Setup(setup::Message),
    Main(Message),
}

fn boot() -> Screen {
    if DEBUG_SETUP {
        return Screen::Setup(setup::boot());
    }

    match Settings::check_setup() {
        Ok(true) => Screen::Main(State::load()),
        Ok(false) => Screen::Setup(setup::boot()),
        Err(error) => {
            log::error!("could not check for existence of config file: {error}");
            notify_error(format!("Configuration file could not be opened: {error}"));
            Screen::Setup(setup::boot())
        }
    }
}

fn update_screen(screen: &mut Screen, message: ScreenMessage) -> Task<ScreenMessage> {
    match (&mut *screen, message) {
        (Screen::Setup(state), ScreenMessage::Setup(message)) => {
            let task = setup::update(state, message).map(ScreenMessage::Setup);
            if state.finished() {
                *screen = Screen::Main(State::load());
            }
            task
        }
        (Screen::Main(state), ScreenMessage::Main(message)) => {
            update(state, message).map(ScreenMessage::Main)
        }
        _ => Task::none(),
    }
}

fn view_screen(screen: &Screen) -> Element<'_, ScreenMessage> {
    match screen {
        Screen::Setup(state) => setup::view(state).map(ScreenMessage::Setup),
        Screen::Main(state) => view(state).map(ScreenMessage::Main),
    }
}

fn screen_subscription(screen: &Screen) -> Subscription<ScreenMessage> {
    match screen {
        Screen::Setup(_) => Subscription::none(),
        Screen::Main(state) => subscription(state).map(ScreenMessage::Main),
    }
}

fn theme(_screen: &Screen) -> Theme {
    style::theme()
}

fn main() -> iced::Result {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,cliprs_gui=info,cliprs_ipc=info"),
    )
    .init();

    iced::application(boot, update_screen, view_screen)
        .theme(theme)
        .subscription(screen_subscription)
        .run()
}
