use iced::font::Weight;
use iced::widget::svg::{self, Handle, Svg};
use iced::widget::{Text, container, text};
use iced::{Background, Border, Color, Font, Theme, color};

const RAISED: Color = color!(0x363F4C);
const OUTLINE: Color = color!(0x4F5967);
const TEXT: Color = color!(0xF4F5F7);
const MUTED: Color = color!(0xB8BDC5);
pub const ACCENT: Color = color!(0xF4F5F7);
pub const DANGER: Color = color!(0xC94F4F);

pub const SPACE_XS: f32 = 4.0;
pub const SPACE_MD: f32 = 12.0;
pub const SPACE_LG: f32 = 24.0;
const RADIUS_MD: f32 = 10.0;
const STROKE: f32 = 1.0;

pub const SURFACE_SIZE: (u32, u32) = (368, 136);
const ICON_MD: f32 = 20.0;
const BODY: f32 = 16.0;
const LABEL: f32 = 14.0;

const SEMIBOLD: Font = Font {
    weight: Weight::Semibold,
    ..Font::DEFAULT
};

pub fn transparent_surface(_state: &crate::Overlay, _theme: &Theme) -> iced::theme::Style {
    iced::theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: TEXT,
    }
}

pub fn card(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT),
        background: Some(Background::Color(RAISED)),
        border: Border {
            color: OUTLINE,
            width: STROKE,
            radius: RADIUS_MD.into(),
        },
        ..container::Style::default()
    }
}

pub fn icon(bytes: &'static [u8], tint: Color) -> Svg<'static> {
    Svg::new(Handle::from_memory(bytes))
        .width(ICON_MD)
        .height(ICON_MD)
        .style(move |_theme, _status| svg::Style { color: Some(tint) })
}

pub fn title<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content).size(BODY).font(SEMIBOLD).color(TEXT)
}

pub fn label<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content).size(LABEL).color(MUTED)
}
