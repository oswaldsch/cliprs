use iced::widget::container::Style;
use iced::widget::rule;
use iced::{Background, Border, Theme, border};

use super::{color, outlined, radius};

fn style(background: iced::Color, border: Border) -> Style {
    Style {
        text_color: Some(color::TEXT),
        background: Some(Background::Color(background)),
        border,
        ..Style::default()
    }
}

pub fn panel(_theme: &Theme) -> Style {
    style(color::SURFACE, Border::default())
}

pub fn divider(_theme: &Theme) -> rule::Style {
    rule::Style {
        color: color::OUTLINE,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

pub fn card(_theme: &Theme) -> Style {
    style(color::RAISED, outlined(color::OUTLINE))
}

pub fn badge(_theme: &Theme) -> Style {
    style(color::SCRIM, border::rounded(radius::SM))
}
