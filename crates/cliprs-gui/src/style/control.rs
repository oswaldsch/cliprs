use iced::widget::{overlay::menu, pick_list, progress_bar};
use iced::{Background, Shadow, Theme, Vector, border};

use super::{METER_GIRTH, color, outlined};

pub fn dropdown(_theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let border_color = match status {
        pick_list::Status::Active => color::OUTLINE,
        pick_list::Status::Hovered => color::MUTED,
        pick_list::Status::Opened { .. } => color::ACCENT,
    };
    pick_list::Style {
        text_color: color::TEXT,
        placeholder_color: color::MUTED,
        handle_color: color::MUTED,
        background: Background::Color(color::RAISED),
        border: outlined(border_color),
    }
}

pub fn dropdown_menu(_theme: &Theme) -> menu::Style {
    menu::Style {
        background: Background::Color(color::RAISED),
        border: outlined(color::OUTLINE),
        text_color: color::TEXT,
        selected_text_color: color::ON_ACCENT,
        selected_background: Background::Color(color::ACCENT),
        shadow: Shadow {
            color: color::SHADOW,
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
    }
}

pub fn meter(_theme: &Theme) -> progress_bar::Style {
    progress_bar::Style {
        background: Background::Color(color::OUTLINE),
        bar: Background::Color(color::ACCENT),
        border: border::rounded(METER_GIRTH / 2.0),
    }
}
