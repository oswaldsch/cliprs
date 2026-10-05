use iced::border::Radius;
use iced::font::Weight;
use iced::widget::svg::{self, Handle, Svg};
use iced::widget::{Container, Text, container, text};
use iced::{Background, Border, Color, Font, Theme, color};

const SURFACE: Color = color!(0x161B24);
const OUTLINE: Color = color!(0x2A3140);
const TEXT: Color = color!(0xE6E9EF);
const MUTED: Color = color!(0x8B93A5);
pub const SUCCESS: Color = color!(0x22C55E);
pub const DANGER: Color = color!(0xEF4444);

pub const SURFACE_SIZE: (u32, u32) = (368, 136);
pub const SURFACE_MARGIN: i32 = 24;
pub const TOAST_PADDING: [f32; 2] = [14.0, 16.0];
pub const TOAST_SPACING: f32 = 14.0;
pub const TOAST_TEXT_SPACING: f32 = 3.0;
const TOAST_RADIUS: f32 = 16.0;
const TOAST_OPACITY: f32 = 0.96;
const TOAST_TITLE_SIZE: f32 = 16.0;
const TOAST_DETAILS_SIZE: f32 = 13.0;
const BADGE_SIZE: f32 = 42.0;
const BADGE_RADIUS: f32 = 12.0;
const BADGE_TINT: f32 = 0.16;
const ICON_SIZE: f32 = 22.0;

const BOLD: Font = Font {
    weight: Weight::Bold,
    ..Font::DEFAULT
};

pub fn transparent_surface(_state: &crate::Overlay, _theme: &Theme) -> iced::theme::Style {
    iced::theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: TEXT,
    }
}

pub fn toast(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT),
        background: Some(Background::Color(Color {
            a: TOAST_OPACITY,
            ..SURFACE
        })),
        border: Border {
            color: OUTLINE,
            width: 1.0,
            radius: Radius::from(TOAST_RADIUS),
        },
        ..container::Style::default()
    }
}

pub fn icon_badge<'a, Message: 'a>(bytes: &'static [u8], accent: Color) -> Container<'a, Message> {
    let icon = Svg::new(Handle::from_memory(bytes))
        .width(ICON_SIZE)
        .height(ICON_SIZE)
        .style(move |_theme, _status| svg::Style {
            color: Some(accent),
        });
    container(icon)
        .center(BADGE_SIZE)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(Color {
                a: BADGE_TINT,
                ..accent
            })),
            border: Border::default().rounded(BADGE_RADIUS),
            ..container::Style::default()
        })
}

pub fn toast_title<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content).size(TOAST_TITLE_SIZE).color(TEXT).font(BOLD)
}

pub fn toast_details<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content).size(TOAST_DETAILS_SIZE).color(MUTED)
}
