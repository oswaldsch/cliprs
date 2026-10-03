use iced::border::Radius;
use iced::widget::svg::{self, Handle, Svg};
use iced::font::Weight;
use iced::widget::{Text, button, container, overlay::menu, pick_list, text};
use iced::{Background, Border, Color, Font, Shadow, Theme, color};

pub const BG: Color = color!(0x0F1218);
pub const SURFACE: Color = color!(0x161B24);
pub const RAISED: Color = color!(0x1E2430);
pub const OUTLINE: Color = color!(0x2A3140);
pub const TEXT: Color = color!(0xE6E9EF);
pub const MUTED: Color = color!(0x8B93A5);
pub const ACCENT: Color = color!(0x3B82F6);
pub const ACCENT_HOVER: Color = color!(0x5B9BF8);
pub const ACCENT_PRESSED: Color = color!(0x2563EB);

pub const RADIUS: f32 = 10.0;
pub const SIDEBAR_WIDTH: f32 = 220.0;
pub const SIDEBAR_PADDING: f32 = 12.0;
pub const SIDEBAR_SPACING: f32 = 6.0;
pub const PAGE_PADDING: f32 = 24.0;
pub const SECTION_SPACING: f32 = 12.0;
pub const CONTROL_WIDTH: f32 = 260.0;
pub const CONTENT_MAX_WIDTH: f32 = 640.0;
pub const BUTTON_PADDING: [f32; 2] = [10.0, 14.0];
pub const PICK_LIST_PADDING: [f32; 2] = [8.0, 12.0];
pub const HEADING_SIZE: f32 = 24.0;
pub const LABEL_SIZE: f32 = 14.0;
pub const ICON_SIZE: f32 = 20.0;

pub fn theme() -> Theme {
    Theme::custom(
        "cliprs",
        iced::theme::Palette {
            background: BG,
            text: TEXT,
            primary: ACCENT,
            success: color!(0x22C55E),
            warning: color!(0xF59E0B),
            danger: color!(0xEF4444),
        },
    )
}

fn rounded(color: Color, width: f32) -> Border {
    Border {
        color,
        width,
        radius: Radius::from(RADIUS),
    }
}

pub fn sidebar(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT),
        background: Some(Background::Color(SURFACE)),
        border: Border {
            color: OUTLINE,
            width: 1.0,
            radius: Radius::from(0.0),
        },
        ..container::Style::default()
    }
}

pub fn card(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT),
        background: Some(Background::Color(RAISED)),
        border: rounded(OUTLINE, 1.0),
        ..container::Style::default()
    }
}

pub fn sidebar_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Active => (None, MUTED),
        button::Status::Hovered => (Some(RAISED), TEXT),
        button::Status::Pressed => (Some(OUTLINE), TEXT),
        button::Status::Disabled => (None, OUTLINE),
    };
    button::Style {
        background: background.map(Background::Color),
        text_color,
        border: rounded(Color::TRANSPARENT, 0.0),
        shadow: Shadow::default(),
        snap: false,
    }
}

pub fn sidebar_button_selected(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => ACCENT_HOVER,
        button::Status::Pressed => ACCENT_PRESSED,
        button::Status::Active | button::Status::Disabled => ACCENT,
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: Color::WHITE,
        border: rounded(Color::TRANSPARENT, 0.0),
        shadow: Shadow::default(),
        snap: false,
    }
}

pub fn dropdown(_theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let border_color = match status {
        pick_list::Status::Active => OUTLINE,
        pick_list::Status::Hovered => MUTED,
        pick_list::Status::Opened { .. } => ACCENT,
    };
    pick_list::Style {
        text_color: TEXT,
        placeholder_color: MUTED,
        handle_color: MUTED,
        background: Background::Color(RAISED),
        border: rounded(border_color, 1.0),
    }
}

pub fn dropdown_menu(_theme: &Theme) -> menu::Style {
    menu::Style {
        background: Background::Color(RAISED),
        border: rounded(OUTLINE, 1.0),
        text_color: TEXT,
        selected_text_color: Color::WHITE,
        selected_background: Background::Color(ACCENT),
        shadow: Shadow {
            color: Color { a: 0.4, ..Color::BLACK },
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
    }
}

pub fn icon(bytes: &'static [u8], selected: bool) -> Svg<'static> {
    svg::Svg::new(Handle::from_memory(bytes))
        .width(ICON_SIZE)
        .height(ICON_SIZE)
        .style(move |_theme, status| icon_style(status, selected))
}

fn icon_style(status: svg::Status, selected: bool) -> svg::Style {
    let color = match (selected, status) {
        (true, _) => Color::WHITE,
        (false, svg::Status::Idle) => MUTED,
        (false, svg::Status::Hovered) => TEXT,
    };
    svg::Style { color: Some(color) }
}

pub fn accent_icon(bytes: &'static [u8]) -> Svg<'static> {
    svg::Svg::new(Handle::from_memory(bytes))
        .width(ICON_SIZE)
        .height(ICON_SIZE)
        .style(|_theme, _status| svg::Style {
            color: Some(ACCENT),
        })
}

pub fn heading(content: &str) -> Text<'_> {
    text(content).size(HEADING_SIZE).font(Font {
        weight: Weight::Bold,
        ..Font::DEFAULT
    })
}
