use iced::widget::svg::{Handle, Status, Style, Svg};

use super::color;

pub const SM: f32 = 16.0;
pub const MD: f32 = 20.0;
pub const LG: f32 = 56.0;
pub const XL: f32 = 96.0;

#[derive(Clone, Copy)]
pub enum Tint {
    Text,
    Muted,
    Accent,
    Nav { selected: bool },
}

pub fn logo(size: f32) -> Svg<'static> {
    Svg::new(Handle::from_memory(include_bytes!("../../assets/logo.svg")))
        .width(size)
        .height(size)
}

pub fn icon(bytes: &'static [u8], size: f32, tint: Tint) -> Svg<'static> {
    Svg::new(Handle::from_memory(bytes))
        .width(size)
        .height(size)
        .style(move |_theme, status| {
            let color = match (tint, status) {
                (Tint::Text, _) => color::TEXT,
                (Tint::Muted, _) => color::MUTED,
                (Tint::Accent, _) => color::ACCENT,
                (Tint::Nav { selected: true }, _) => color::ON_ACCENT,
                (Tint::Nav { selected: false }, Status::Idle) => color::MUTED,
                (Tint::Nav { selected: false }, Status::Hovered) => color::TEXT,
            };
            Style { color: Some(color) }
        })
}
