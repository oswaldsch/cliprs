use std::cmp::Reverse;
use std::error::Error;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Local};
use cliprs_ipc::{ClipMeta, clips_dir, thumbnail_path};
use iced::widget::{column, container, grid, image, mouse_area, row, scrollable, stack, text};
use iced::{Alignment, ContentFit, Element, Length};
use webm_iterable::WebmIterator;
use webm_iterable::matroska_spec::{Master, MatroskaSpec};

use crate::{Message, style};

pub struct Clip {
    video_path: PathBuf,
    thumbnail_path: PathBuf,
    title: String,
    details: Option<String>,
    saved_at_unix_secs: Option<u64>,
    duration_secs: Option<f64>,
}

pub fn load_clips() -> Vec<Clip> {
    let Ok(entries) = clips_dir().and_then(fs::read_dir) else {
        return Vec::new();
    };

    let mut clips: Vec<Clip> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "mkv"))
        .map(load_clip)
        .collect();
    clips.sort_by_key(|clip| Reverse(clip.saved_at_unix_secs));
    clips
}

fn load_clip(video_path: PathBuf) -> Clip {
    let id = read_tag(&video_path, "CLIPRS_UUID").ok().flatten();
    let meta = id
        .as_deref()
        .and_then(|id| ClipMeta::load(id).ok().flatten());
    let thumbnail_path = id
        .as_deref()
        .and_then(|id| thumbnail_path(id).ok())
        .unwrap_or_default();
    let size_mb = fs::metadata(&video_path).map(|file| file.len() as f64 / 1_000_000.0);
    let details = meta.as_ref().map(|meta| {
        let mut details = format!("{}x{} · {} fps", meta.width, meta.height, meta.fps);
        if let Ok(size_mb) = size_mb {
            details.push_str(&format!(" · {size_mb:.1} MB"));
        }
        details
    });

    Clip {
        video_path,
        thumbnail_path,
        details,
        saved_at_unix_secs: meta.as_ref().map(|meta| meta.saved_at_unix_secs),
        duration_secs: meta.as_ref().map(|meta| meta.duration_secs),
        title: meta
            .and_then(|meta| meta.title)
            .unwrap_or_else(|| String::from("Unnamed Clip")),
    }
}

fn read_tag(video_path: &Path, wanted: &str) -> Result<Option<String>, Box<dyn Error>> {
    let mut file = BufReader::new(File::open(video_path)?);
    let tags = WebmIterator::new(&mut file, &[MatroskaSpec::SimpleTag(Master::Start)]);
    for tag in tags {
        match tag? {
            MatroskaSpec::SimpleTag(Master::Full(children)) => {
                let name = children.iter().find_map(|child| match child {
                    MatroskaSpec::TagName(name) => Some(name.as_str()),
                    _ => None,
                });
                if name == Some(wanted) {
                    return Ok(children.into_iter().find_map(|child| match child {
                        MatroskaSpec::TagString(value) => Some(value),
                        _ => None,
                    }));
                }
            }
            MatroskaSpec::Cluster(_) => break,
            _ => {}
        }
    }
    Ok(None)
}

fn format_saved_at(unix_secs: u64) -> Option<String> {
    let saved = DateTime::from_timestamp(i64::try_from(unix_secs).ok()?, 0)?.with_timezone(&Local);
    let today = Local::now().date_naive();
    let saved_day = saved.date_naive();

    Some(if saved_day == today {
        saved.format("%H:%M").to_string()
    } else if today.pred_opt() == Some(saved_day) {
        String::from("Yesterday")
    } else if saved_day.year() == today.year() {
        saved.format("%b %-d").to_string()
    } else {
        saved.format("%b %-d, %Y").to_string()
    })
}

fn format_duration(secs: f64) -> String {
    let total = secs.round() as u64;
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn clip_thumbnail(clip: &Clip) -> Element<'_, Message> {
    let thumbnail = image::Image::new(image::Handle::from_path(&clip.thumbnail_path))
        .width(Length::Fill)
        .content_fit(ContentFit::Contain)
        .border_radius(style::RADIUS);
    let Some(duration_secs) = clip.duration_secs else {
        return thumbnail.into();
    };

    let badge = container(text(format_duration(duration_secs)).size(style::DURATION_SIZE))
        .padding(style::DURATION_PADDING)
        .style(style::duration_badge);

    stack![
        thumbnail,
        container(badge)
            .align_right(Length::Fill)
            .align_bottom(Length::Fill)
            .padding(style::DURATION_MARGIN)
    ]
    .into()
}

fn clip_card(clip: &Clip) -> Element<'_, Message> {
    let saved_at = clip.saved_at_unix_secs.and_then(format_saved_at);

    mouse_area(
        column![
            clip_thumbnail(clip),
            row![style::clip_title(clip.title.as_str()).width(Length::Fill)]
                .extend(saved_at.map(|saved_at| Element::from(style::clip_details(saved_at))))
                .spacing(style::CARD_TEXT_SPACING * 2.0)
                .align_y(Alignment::Center),
        ]
        .extend(
            clip.details
                .as_deref()
                .map(|details| Element::from(style::clip_details(details))),
        )
        .spacing(style::CARD_TEXT_SPACING),
    )
    .on_press(Message::ClipClicked(clip.video_path.clone()))
    .into()
}

fn empty_state<'a>() -> Element<'a, Message> {
    container(
        column![
            text("No clips yet")
                .size(style::EMPTY_TITLE_SIZE)
                .color(style::TEXT)
                .font(style::SEMIBOLD),
            text("Come back here once you recorded a clip")
                .size(style::LABEL_SIZE)
                .color(style::MUTED),
        ]
        .spacing(style::CARD_TEXT_SPACING)
        .align_x(Alignment::Center),
    )
    .center_x(Length::Fill)
    .padding(style::EMPTY_STATE_PADDING)
    .into()
}

pub fn view(clips: &[Clip]) -> Element<'_, Message> {
    if clips.is_empty() {
        empty_state()
    } else {
        scrollable(grid(clips.iter().map(clip_card)).spacing(10))
            .spacing(10)
            .into()
    }
}
