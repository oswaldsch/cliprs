use iced::widget::button::{Status, Style};
use iced::{Background, Border, Color, Shadow, Theme, border};

use super::{STROKE_SELECTED, color, outlined, radius};

fn style(background: Option<Color>, text_color: Color, border: Border) -> Style {
    Style {
        background: background.map(Background::Color),
        text_color,
        border,
        shadow: Shadow::default(),
        snap: false,
    }
}

fn accent(status: Status) -> Color {
    match status {
        Status::Hovered => color::ACCENT_HOVER,
        Status::Pressed => color::ACCENT_PRESSED,
        Status::Active | Status::Disabled => color::ACCENT,
    }
}

pub fn primary(_theme: &Theme, status: Status) -> Style {
    let (background, text_color) = match status {
        Status::Disabled => (color::OUTLINE, color::MUTED),
        _ => (accent(status), color::ON_ACCENT),
    };
    style(Some(background), text_color, border::rounded(radius::MD))
}

pub fn secondary(_theme: &Theme, status: Status) -> Style {
    let (background, border_color, text_color) = match status {
        Status::Active => (color::RAISED, color::OUTLINE, color::TEXT),
        Status::Hovered => (color::OUTLINE, color::MUTED, color::TEXT),
        Status::Pressed => (color::SURFACE, color::MUTED, color::TEXT),
        Status::Disabled => (color::SURFACE, color::OUTLINE, color::MUTED),
    };
    style(Some(background), text_color, outlined(border_color))
}

pub fn nav(selected: bool) -> impl Fn(&Theme, Status) -> Style {
    move |_theme, status| {
        let (background, text_color) = match status {
            _ if selected => (Some(accent(status)), color::ON_ACCENT),
            Status::Active => (None, color::MUTED),
            Status::Hovered => (Some(color::RAISED), color::TEXT),
            Status::Pressed => (Some(color::OUTLINE), color::TEXT),
            Status::Disabled => (None, color::OUTLINE),
        };
        style(background, text_color, border::rounded(radius::MD))
    }
}

pub fn card(selected: bool) -> impl Fn(&Theme, Status) -> Style {
    move |_theme, status| {
        let (background, text_color) = match status {
            Status::Pressed => (color::SURFACE, color::TEXT),
            Status::Disabled if !selected => (color::SURFACE, color::MUTED),
            _ => (color::RAISED, color::TEXT),
        };
        let border = match status {
            _ if selected => Border {
                width: STROKE_SELECTED,
                ..outlined(color::ACCENT)
            },
            Status::Hovered | Status::Pressed => outlined(color::MUTED),
            Status::Active | Status::Disabled => outlined(color::OUTLINE),
        };
        style(Some(background), text_color, border)
    }
}
