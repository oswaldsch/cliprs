mod export;
mod library;
mod resources;
mod settings;
mod setup;
mod style;

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use cliprs_ipc::{AUDIO_BITRATE_BPS, Process, ResourceSample, Settings, install, notify_error};
use iced::futures::channel::oneshot;
use iced::widget::space::{horizontal, vertical};
use iced::widget::{button, center, column, container, mouse_area, opaque, row, rule, stack};
use iced::{Alignment, Element, Length, Subscription, Task, Theme};

use export::ExportPlan;
use library::{Clip, ExportSource};
use settings::{BitrateOptions, DurationOptions, FPSOptions};

use crate::resources::construct_resource_tab;
use crate::settings::{HotkeyOptions, ResolutionOptions};
use crate::style::icon::{self, Tint, icon};
use crate::style::{space, text};

const DEBUG_SETUP: bool = false;
const SIDEBAR_WIDTH: f32 = 220.0;
const CONTENT_MAX_WIDTH: f32 = 640.0;
const DISCORD_LIMIT_MB: u64 = 20;
const ADVANCED_LABEL_WIDTH: f32 = 100.0;
const ORIGINAL: &str = "Original";
const AUTO: &str = "Auto";

#[derive(Debug, Clone)]
enum Message {
    Navigate(SidebarTab),
    FPSChanged(FPSOptions),
    BitrateChanged(BitrateOptions),
    DurationChanged(DurationOptions),
    HotkeyChanged(HotkeyOptions),
    ApplySettings,
    SettingsApplied(Result<(), String>),
    ClipClicked(PathBuf),
    DeleteRequested(PathBuf, PathBuf, PathBuf),
    DeleteCancelled,
    ExportRequested(ExportSource),
    ExportCancelled,
    ExportSizeChanged(SizeOptions),
    ExportAdvancedToggled,
    ExportFPSChanged(Option<FPSOptions>),
    ExportBitrateChanged(Option<BitrateOptions>),
    ExportResolutionChanged(Option<ResolutionOptions>),
    ExportClip,
    ExportFinished(Result<(), String>),
    ClipDeleted,
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
    pending_delete: Option<(PathBuf, PathBuf, PathBuf)>,
    pending_export: Option<PendingExport>,
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
            pending_delete: None,
            pending_export: None,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum SizeOptions {
    Discord,
    Small,
    Medium,
    Best,
}

impl SizeOptions {
    const ALL: &[SizeOptions] = &[
        SizeOptions::Discord,
        SizeOptions::Small,
        SizeOptions::Medium,
        SizeOptions::Best,
    ];

    fn video_bps(&self, duration_secs: f64) -> u64 {
        match self {
            SizeOptions::Discord => {
                let total_bps = (DISCORD_LIMIT_MB * 8_000_000) as f64 / duration_secs;
                (total_bps as u64)
                    .saturating_sub(AUDIO_BITRATE_BPS)
                    .min(BitrateOptions::High.average_bps())
            }
            SizeOptions::Small => BitrateOptions::Low.average_bps(),
            SizeOptions::Medium => BitrateOptions::Medium.average_bps(),
            SizeOptions::Best => BitrateOptions::High.average_bps(),
        }
    }
}

impl fmt::Display for SizeOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SizeOptions::Discord => write!(formatter, "Discord"),
            SizeOptions::Small => write!(formatter, "Small"),
            SizeOptions::Medium => write!(formatter, "Medium"),
            SizeOptions::Best => write!(formatter, "Best quality"),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Quality(BitrateOptions);

impl fmt::Display for Quality {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            BitrateOptions::Low => write!(formatter, "Low"),
            BitrateOptions::Medium => write!(formatter, "Medium"),
            BitrateOptions::High => write!(formatter, "High"),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum Pick<T> {
    Keep(&'static str),
    Value(T),
}

impl<T: Copy> Pick<T> {
    fn choices(keep: &'static str, values: impl IntoIterator<Item = T>) -> Vec<Pick<T>> {
        std::iter::once(Pick::Keep(keep))
            .chain(values.into_iter().map(Pick::Value))
            .collect()
    }

    fn selected(keep: &'static str, value: Option<T>) -> Pick<T> {
        value.map_or(Pick::Keep(keep), Pick::Value)
    }

    fn value(self) -> Option<T> {
        match self {
            Pick::Keep(_) => None,
            Pick::Value(value) => Some(value),
        }
    }
}

impl<T: fmt::Display> fmt::Display for Pick<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Pick::Keep(label) => write!(formatter, "{label}"),
            Pick::Value(value) => value.fmt(formatter),
        }
    }
}

#[derive(Default, Debug)]
struct ExportOptions {
    size: Option<SizeOptions>,
    resolution: Option<ResolutionOptions>,
    fps: Option<FPSOptions>,
    bitrate: Option<BitrateOptions>,
}

struct PendingExport {
    source: ExportSource,
    options: ExportOptions,
    show_advanced: bool,
}

impl PendingExport {
    fn plan(&self) -> Option<ExportPlan> {
        let video_bps = match (self.options.bitrate, self.options.size) {
            (Some(bitrate), _) => bitrate.average_bps(),
            (None, Some(size)) => size.video_bps(self.source.duration_secs?),
            (None, None) => return None,
        };
        let height = self
            .options
            .resolution
            .map(|resolution| resolution.height())
            .filter(|height| Some(*height) != self.source.height);
        let fps = self
            .options
            .fps
            .map(|fps| fps.value())
            .filter(|fps| Some(*fps) != self.source.fps);
        Some(ExportPlan {
            video_bps,
            height,
            fps,
        })
    }

    fn estimate_mb(&self, video_bps: impl FnOnce(f64) -> u64) -> Option<u64> {
        let duration_secs = self.source.duration_secs?;
        let fps = self
            .options
            .fps
            .map(|fps| fps.value())
            .or(self.source.fps)
            .unwrap_or(FPSOptions::default().value());
        Some(estimate_mp4_size_mb(
            video_bps(duration_secs),
            AUDIO_BITRATE_BPS,
            f64::from(fps),
            duration_secs,
        ))
    }
}

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

fn modal<'a, Message: Clone + 'a>(
    base: impl Into<Element<'a, Message>>,
    content: impl Into<Element<'a, Message>>,
    on_blur: Message,
) -> Element<'a, Message> {
    stack![
        base.into(),
        opaque(mouse_area(center(opaque(content)).style(style::surface::scrim)).on_press(on_blur))
    ]
    .into()
}

fn segmented_control<'a, T: Copy + PartialEq + ToString>(
    options: &[T],
    selected: Option<T>,
    on_change: impl Fn(T) -> Message,
) -> Element<'a, Message> {
    let last = options.len().saturating_sub(1);
    row(options.iter().enumerate().map(|(i, option)| {
        button(iced::widget::text(option.to_string()).center())
            .style(style::button::segment(
                i == 0,
                i == last,
                selected == Some(*option),
            ))
            .on_press(on_change(*option))
            .width(Length::Fill)
            .into()
    }))
    .into()
}

fn delete_dialog() -> Element<'static, Message> {
    container(
        column![
            column![
                text::title("Delete clip?"),
                text::label("This cannot be undone."),
            ]
            .spacing(space::XS),
            row![
                button(text::label("Cancel").center())
                    .style(style::button::secondary)
                    .on_press(Message::DeleteCancelled)
                    .width(Length::Fill),
                button(text::label("Delete").center())
                    .style(style::button::danger)
                    .on_press(Message::ClipDeleted)
                    .width(Length::Fill),
            ]
            .spacing(space::SM)
        ]
        .spacing(space::LG),
    )
    .padding(space::LG)
    .style(style::surface::card)
    .width(400)
    .into()
}

fn size_card(export: &PendingExport, size: SizeOptions) -> Element<'_, Message> {
    let estimate = export
        .estimate_mb(|duration_secs| size.video_bps(duration_secs))
        .map_or_else(|| String::from("size unknown"), |mb| format!("~{mb} MB"));
    let usable = size != SizeOptions::Discord || export.source.duration_secs.is_some();

    button(
        row![
            text::title(size.to_string()),
            horizontal(),
            text::label(estimate)
        ]
        .align_y(Alignment::Center),
    )
    .style(style::button::card(export.options.size == Some(size)))
    .padding(space::MD)
    .width(Length::Fill)
    .on_press_maybe(usable.then_some(Message::ExportSizeChanged(size)))
    .into()
}

fn advanced_row<'a>(label: &'a str, control: Element<'a, Message>) -> Element<'a, Message> {
    row![text::label(label).width(ADVANCED_LABEL_WIDTH), control]
        .spacing(space::MD)
        .align_y(Alignment::Center)
        .into()
}

fn advanced_settings(export: &PendingExport) -> Element<'_, Message> {
    let options = &export.options;
    let fps_choices = Pick::choices(
        ORIGINAL,
        FPSOptions::ALL.iter().copied().filter(|fps| {
            export
                .source
                .fps
                .is_none_or(|clip_fps| fps.value() < clip_fps)
        }),
    );
    let resolution_choices = Pick::choices(
        ORIGINAL,
        ResolutionOptions::ALL.iter().copied().filter(|resolution| {
            export
                .source
                .height
                .is_none_or(|clip_height| resolution.height() < clip_height)
        }),
    );
    let quality_choices = Pick::choices(AUTO, BitrateOptions::ALL.iter().copied().map(Quality));

    let manual_estimate = options.bitrate.and_then(|bitrate| {
        let mb = export.estimate_mb(|_| bitrate.average_bps())?;
        Some(if mb > DISCORD_LIMIT_MB {
            style::surface::warning(format!("About {mb} MB, too big for Discord"))
        } else {
            text::label(format!("About {mb} MB")).into()
        })
    });

    column![
        advanced_row(
            "Smoothness",
            segmented_control(
                &fps_choices,
                Some(Pick::selected(ORIGINAL, options.fps)),
                |pick| Message::ExportFPSChanged(pick.value()),
            ),
        ),
        advanced_row(
            "Resolution",
            segmented_control(
                &resolution_choices,
                Some(Pick::selected(ORIGINAL, options.resolution)),
                |pick| Message::ExportResolutionChanged(pick.value()),
            ),
        ),
        advanced_row(
            "Quality",
            segmented_control(
                &quality_choices,
                Some(Pick::selected(AUTO, options.bitrate.map(Quality))),
                |pick| Message::ExportBitrateChanged(pick.value().map(|quality| quality.0)),
            ),
        ),
    ]
    .extend(manual_estimate)
    .spacing(space::SM)
    .into()
}

fn export_dialog(export: &PendingExport) -> Element<'_, Message> {
    let chevron: &'static [u8] = if export.show_advanced {
        include_bytes!("../assets/chevron-down.svg")
    } else {
        include_bytes!("../assets/chevron-right.svg")
    };
    let advanced_toggle = button(
        row![
            text::label("Advanced"),
            icon(chevron, icon::SM, Tint::Muted)
        ]
        .spacing(space::XS)
        .align_y(Alignment::Center),
    )
    .style(style::button::nav(false))
    .padding([space::SM, 0.0])
    .on_press(Message::ExportAdvancedToggled);

    container(
        column![
            column![text::title("Export clip"),].spacing(space::XS),
            column(SizeOptions::ALL.iter().map(|size| size_card(export, *size))).spacing(space::SM),
            column![advanced_toggle]
                .extend(export.show_advanced.then(|| advanced_settings(export)))
                .spacing(space::SM),
            row![
                button(text::label("Cancel").center())
                    .style(style::button::secondary)
                    .on_press(Message::ExportCancelled)
                    .width(Length::Fill),
                button(text::action("Export").center())
                    .style(style::button::primary)
                    .on_press_maybe(
                        (export.options.size.is_some() || export.options.bitrate.is_some())
                            .then_some(Message::ExportClip)
                    )
                    .width(Length::Fill),
            ]
            .spacing(space::SM)
        ]
        .spacing(space::LG),
    )
    .padding(space::LG)
    .style(style::surface::card)
    .width(600)
    .into()
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
    ]
    .spacing(space::MD);
    if let Some(sample) = &state.resource_sample {
        sidebar = sidebar.push(construct_resource_tab(sample, state.cpu_percent));
    }
    let sidebar_container = container(sidebar)
        .padding(space::MD)
        .style(style::surface::panel)
        .width(SIDEBAR_WIDTH)
        .height(Length::Fill);

    let main_view = construct_main_view(state);

    let divider = rule::vertical(style::STROKE).style(style::surface::divider);

    let full_view: Element<Message, _> =
        container(row![sidebar_container, divider, main_view]).into();

    if state.pending_delete.is_some() {
        modal(full_view, delete_dialog(), Message::DeleteCancelled)
    } else if let Some(export) = &state.pending_export {
        modal(full_view, export_dialog(export), Message::ExportCancelled)
    } else {
        full_view.into()
    }
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
        Message::HotkeyChanged(selected) => state.settings.hotkey = Some(selected),
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
        Message::DeleteRequested(video_path, meta_path, thumb_path) => {
            state.pending_delete = Some((video_path, meta_path, thumb_path))
        }
        Message::DeleteCancelled => state.pending_delete = None,
        Message::ExportRequested(source) => {
            let options = ExportOptions {
                size: source.duration_secs.map(|_| SizeOptions::Discord),
                ..ExportOptions::default()
            };
            state.pending_export = Some(PendingExport {
                source,
                options,
                show_advanced: false,
            })
        }
        Message::ExportCancelled => state.pending_export = None,
        Message::ExportSizeChanged(selected) => {
            if let Some(export) = &mut state.pending_export {
                export.options.size = Some(selected);
                export.options.bitrate = None;
            }
        }
        Message::ExportAdvancedToggled => {
            if let Some(export) = &mut state.pending_export {
                export.show_advanced = !export.show_advanced
            }
        }
        Message::ExportFPSChanged(selected) => {
            if let Some(export) = &mut state.pending_export {
                export.options.fps = selected
            }
        }
        Message::ExportBitrateChanged(selected) => {
            if let Some(export) = &mut state.pending_export {
                export.options.bitrate = selected;
                if selected.is_some() {
                    export.options.size = None;
                }
            }
        }
        Message::ExportResolutionChanged(selected) => {
            if let Some(export) = &mut state.pending_export {
                export.options.resolution = selected
            }
        }
        Message::ExportClip => {
            let Some(export) = state.pending_export.take() else {
                return Task::none();
            };
            let Some(plan) = export.plan() else {
                return Task::none();
            };
            let source = export.source.video_path;
            let out = match export::output_path(&source) {
                Ok(out) => out,
                Err(error) => {
                    log::error!("could not resolve export path: {error}");
                    notify_error(format!("Could not export clip: {error}"));
                    return Task::none();
                }
            };
            return blocking_task(move || export::run(&source, &plan, &out))
                .map(Message::ExportFinished); // TODO: add progress bar or indicator
        }
        Message::ExportFinished(Err(error)) => {
            log::error!("export failed: {error}");
            notify_error(format!("Export failed: {error}"));
        }
        Message::ExportFinished(Ok(())) => {
            let opened =
                export::exports_dir().and_then(|dir| Command::new("xdg-open").arg(dir).spawn());
            if let Err(error) = opened {
                log::error!("failed to open exports folder via xdg-open: {error}");
            }
        }
        Message::ClipDeleted => {
            let paths = state.pending_delete.take();
            for path in paths
                .iter()
                .flat_map(|(video, meta, thumb)| [video, meta, thumb])
            {
                match std::fs::remove_file(path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        log::error!("failed to delete {}: {}", path.display(), error);
                        notify_error(format!("Failed to delete file: {error}"));
                        break;
                    }
                }
            }
            state.clips = library::load_clips();
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

fn estimate_mp4_size_mb(video_bps: u64, audio_bps: u64, fps: f64, duration_s: f64) -> u64 {
    let payload = (video_bps + audio_bps) as f64 / 8.0 * duration_s;
    let video_frames = fps * duration_s;
    let audio_frames = 50.0 * duration_s;
    let overhead = 2048.0 + (video_frames + audio_frames) * 12.0;
    ((payload + overhead) / 1_000_000.0).round() as u64
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
