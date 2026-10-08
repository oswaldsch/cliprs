pub mod button;
pub mod control;
pub mod icon;
pub mod surface;
pub mod text;

use iced::{Border, Color, Theme};

pub mod color {
    use iced::{Color, color};

    pub const BG: Color = color!(0x0F1218);
    pub const SURFACE: Color = color!(0x161B24);
    pub const RAISED: Color = color!(0x1E2430);
    pub const OUTLINE: Color = color!(0x2A3140);
    pub const TEXT: Color = color!(0xE6E9EF);
    pub const MUTED: Color = color!(0x8B93A5);
    pub const ACCENT: Color = color!(0x3B82F6);
    pub const ACCENT_HOVER: Color = color!(0x5B9BF8);
    pub const ACCENT_PRESSED: Color = color!(0x2563EB);
    pub const ON_ACCENT: Color = Color::WHITE;
    pub const SUCCESS: Color = color!(0x22C55E);
    pub const WARNING: Color = color!(0xF59E0B);
    pub const DANGER: Color = color!(0xEF4444);
    pub const SCRIM: Color = Color {
        a: 0.7,
        ..Color::BLACK
    };
    pub const SHADOW: Color = Color {
        a: 0.4,
        ..Color::BLACK
    };
}

pub mod space {
    pub const XXS: f32 = 2.0;
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 24.0;
    pub const XL: f32 = 48.0;
    pub const CONTROL: [f32; 2] = [SM, MD];
    pub const BADGE: [f32; 2] = [XXS, SM];
}

pub mod radius {
    pub const SM: f32 = 4.0;
    pub const MD: f32 = 10.0;
}

pub const METER_GIRTH: f32 = 4.0;

const STROKE: f32 = 1.0;
const STROKE_SELECTED: f32 = 2.0;

pub fn theme() -> Theme {
    Theme::custom(
        "cliprs",
        iced::theme::Palette {
            background: color::BG,
            text: color::TEXT,
            primary: color::ACCENT,
            success: color::SUCCESS,
            warning: color::WARNING,
            danger: color::DANGER,
        },
    )
}

fn outlined(color: Color) -> Border {
    Border {
        color,
        width: STROKE,
        radius: radius::MD.into(),
    }
}
