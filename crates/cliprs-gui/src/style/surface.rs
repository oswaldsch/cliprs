use iced::widget::container::Style;
use iced::{Background, Border, Theme, border};

use super::{STROKE, color, outlined, radius};

fn style(background: iced::Color, border: Border) -> Style {
    Style {
        text_color: Some(color::TEXT),
        background: Some(Background::Color(background)),
        border,
        ..Style::default()
    }
}

pub fn panel(_theme: &Theme) -> Style {
    style(
        color::SURFACE,
        Border {
            color: color::OUTLINE,
            width: STROKE,
            radius: 0.0.into(),
        },
    )
}

pub fn card(_theme: &Theme) -> Style {
    style(color::RAISED, outlined(color::OUTLINE))
}

pub fn badge(_theme: &Theme) -> Style {
    style(color::SCRIM, border::rounded(radius::SM))
}
