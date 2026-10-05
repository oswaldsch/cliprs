mod style;
mod wayland;

use std::error::Error;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use cliprs_ipc::{Notification, NotificationReceiver};
use iced::futures::Stream;
use iced::futures::channel::oneshot;
use iced::widget::{column, container, row, space};
use iced::{Alignment, Color, Element, Length, Subscription, Task};
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings};
use iced_layershell::{application, to_layer_message};

const NAMESPACE: &str = "cliprs-overlay";
const TOAST_DURATION: Duration = Duration::from_secs(4);
const PENDING_NOTIFICATIONS: usize = 16;

static RECEIVER: OnceLock<NotificationReceiver> = OnceLock::new();

#[derive(Default)]
struct Overlay {
    toast: Option<Notification>,
    toasts_shown: u64,
}

#[to_layer_message]
#[derive(Debug, Clone)]
enum Message {
    Notified(Notification),
    Dismiss(u64),
}

fn main() -> Result<(), Box<dyn Error>> {
    // TODO: add logger
    let receiver = NotificationReceiver::bind()
        .map_err(|error| format!("could not bind overlay socket: {error}"))?;
    if !wayland::layer_shell_available() {
        log::info!("no layer shell, falling back to desktop notifications");
        forward_to_desktop_notifications(&receiver);
    }
    let _ = RECEIVER.set(receiver);

    application(Overlay::default, namespace, update, view)
        .style(style::transparent_surface)
        .subscription(subscription)
        .settings(Settings {
            layer_settings: LayerShellSettings {
                layer: Layer::Overlay,
                anchor: Anchor::Top | Anchor::Right,
                size: Some(style::SURFACE_SIZE),
                margin: (
                    style::SURFACE_MARGIN,
                    style::SURFACE_MARGIN,
                    style::SURFACE_MARGIN,
                    style::SURFACE_MARGIN,
                ),
                keyboard_interactivity: KeyboardInteractivity::None,
                events_transparent: true,
                ..Default::default()
            },
            ..Default::default()
        })
        .run()?;
    Ok(())
}

fn forward_to_desktop_notifications(receiver: &NotificationReceiver) -> ! {
    loop {
        let notification = match receiver.receive() {
            Ok(notification) => notification,
            Err(error) => {
                log::warn!("dropped unreadable notification: {error}");
                continue;
            }
        };
        let Toast { title, details, .. } = Toast::from(&notification);
        let shown = notify_rust::Notification::new()
            .appname(NAMESPACE)
            .summary(title)
            .body(details)
            .show();
        if let Err(error) = shown {
            log::error!("desktop notification failed: {error}, dropped {title}: {details}");
        }
    }
}

fn namespace() -> String {
    NAMESPACE.to_string()
}

fn subscription(_overlay: &Overlay) -> Subscription<Message> {
    Subscription::run(notifications)
}

fn notifications() -> impl Stream<Item = Message> {
    iced::stream::channel(PENDING_NOTIFICATIONS, async |mut output| {
        thread::spawn(move || {
            let Some(receiver) = RECEIVER.get() else {
                return;
            };
            loop {
                match receiver.receive() {
                    Ok(notification) => {
                        let _ = output.try_send(Message::Notified(notification));
                    }
                    Err(error) => log::warn!("dropped unreadable notification: {error}"),
                }
            }
        });
    })
}

fn dismiss_after_timeout(toast_number: u64) -> Task<Message> {
    let (elapsed, wait) = oneshot::channel();
    thread::spawn(move || {
        thread::sleep(TOAST_DURATION);
        let _ = elapsed.send(());
    });
    Task::future(async move {
        let _ = wait.await;
        Message::Dismiss(toast_number)
    })
}

fn update(overlay: &mut Overlay, message: Message) -> Task<Message> {
    match message {
        Message::Notified(notification) => {
            overlay.toast = Some(notification);
            overlay.toasts_shown += 1;
            dismiss_after_timeout(overlay.toasts_shown)
        }
        Message::Dismiss(toast_number) => {
            if toast_number == overlay.toasts_shown {
                overlay.toast = None;
            }
            Task::none()
        }
        _ => Task::none(),
    }
}

struct Toast<'a> {
    title: &'static str,
    details: &'a str,
    accent: Color,
    icon: &'static [u8],
}

impl<'a> From<&'a Notification> for Toast<'a> {
    fn from(notification: &'a Notification) -> Self {
        match notification {
            Notification::ClipSaved { .. } => Toast {
                title: "Clip saved!",
                details: "Find it in your cliprs library",
                accent: style::SUCCESS,
                icon: include_bytes!("../assets/circle-check.svg"),
            },
            Notification::Error { description } => Toast {
                title: "Something went wrong",
                details: description,
                accent: style::DANGER,
                icon: include_bytes!("../assets/triangle-alert.svg"),
            },
        }
    }
}

fn view(overlay: &Overlay) -> Element<'_, Message> {
    let Some(notification) = &overlay.toast else {
        return space().into();
    };
    let toast = Toast::from(notification);
    let card = container(
        row![
            style::icon_badge(toast.icon, toast.accent),
            column![
                style::toast_title(toast.title),
                style::toast_details(toast.details)
            ]
            .spacing(style::TOAST_TEXT_SPACING)
            .width(Length::Fill),
        ]
        .spacing(style::TOAST_SPACING)
        .align_y(Alignment::Center),
    )
    .padding(style::TOAST_PADDING)
    .width(Length::Fill)
    .style(style::toast);
    container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .into()
}
