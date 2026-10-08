use iced::border::Radius;
use iced::font::Weight;
use iced::widget::svg::{self, Handle, Svg};
use iced::widget::{
    Text, TextInput, button, container, overlay::menu, pick_list, progress_bar, text, text_input,
};
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
const TEXT_FIELD_PADDING: [f32; 2] = [8.0, 12.0];
const SELECTION_ALPHA: f32 = 0.4;
pub const HEADING_SIZE: f32 = 24.0;
const SUBHEADING_SIZE: f32 = 16.0;
pub const LABEL_SIZE: f32 = 14.0;
pub const ICON_SIZE: f32 = 20.0;
pub const CARD_TEXT_SPACING: f32 = 4.0;
const CARD_SELECTED_BORDER_WIDTH: f32 = 2.0;
pub const MONITOR_CARD_WIDTH: f32 = 220.0;
pub const MONITOR_ICON_SIZE: f32 = 56.0;
const CLIP_TITLE_SIZE: f32 = 15.0;
const CLIP_DETAILS_SIZE: f32 = 12.0;
pub const EMPTY_TITLE_SIZE: f32 = 18.0;
pub const EMPTY_STATE_PADDING: f32 = 48.0;
pub const DURATION_SIZE: f32 = 12.0;
pub const DURATION_PADDING: [f32; 2] = [2.0, 6.0];
pub const DURATION_MARGIN: f32 = 8.0;
const DURATION_RADIUS: f32 = 4.0;
const STAT_SIZE: f32 = 13.0;
const STAT_ICON_SIZE: f32 = 16.0;
pub const STAT_ICON_SPACING: f32 = 8.0;
pub const STAT_SPACING: f32 = 10.0;
pub const STAT_BAR_SPACING: f32 = 6.0;
pub const STAT_CARD_PADDING: f32 = 12.0;
pub const USAGE_BAR_GIRTH: f32 = 4.0;

pub const BOLD: Font = Font {
    weight: Weight::Bold,
    ..Font::DEFAULT
};
pub const SEMIBOLD: Font = Font {
    weight: Weight::Semibold,
    ..Font::DEFAULT
};

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

pub fn primary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Active => (ACCENT, Color::WHITE),
        button::Status::Hovered => (ACCENT_HOVER, Color::WHITE),
        button::Status::Pressed => (ACCENT_PRESSED, Color::WHITE),
        button::Status::Disabled => (OUTLINE, MUTED),
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color,
        border: rounded(Color::TRANSPARENT, 0.0),
        shadow: Shadow::default(),
        snap: false,
    }
}

pub fn secondary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, border_color, text_color) = match status {
        button::Status::Active => (RAISED, OUTLINE, TEXT),
        button::Status::Hovered => (OUTLINE, MUTED, TEXT),
        button::Status::Pressed => (SURFACE, MUTED, TEXT),
        button::Status::Disabled => (SURFACE, OUTLINE, MUTED),
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color,
        border: rounded(border_color, 1.0),
        shadow: Shadow::default(),
        snap: false,
    }
}

pub fn card_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, border_color, text_color) = match status {
        button::Status::Active => (RAISED, OUTLINE, TEXT),
        button::Status::Hovered => (RAISED, MUTED, TEXT),
        button::Status::Pressed => (SURFACE, MUTED, TEXT),
        button::Status::Disabled => (SURFACE, OUTLINE, MUTED),
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color,
        border: rounded(border_color, 1.0),
        shadow: Shadow::default(),
        snap: false,
    }
}

pub fn card_button_selected(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Pressed => SURFACE,
        button::Status::Active | button::Status::Hovered | button::Status::Disabled => RAISED,
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: TEXT,
        border: rounded(ACCENT, CARD_SELECTED_BORDER_WIDTH),
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
            color: Color {
                a: 0.4,
                ..Color::BLACK
            },
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
    }
}

fn text_field_style(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let (background, border_color, value) = match status {
        text_input::Status::Active => (RAISED, OUTLINE, TEXT),
        text_input::Status::Hovered => (RAISED, MUTED, TEXT),
        text_input::Status::Focused { .. } => (RAISED, ACCENT, TEXT),
        text_input::Status::Disabled => (SURFACE, OUTLINE, MUTED),
    };
    text_input::Style {
        background: Background::Color(background),
        border: rounded(border_color, 1.0),
        icon: MUTED,
        placeholder: MUTED,
        value,
        selection: Color {
            a: SELECTION_ALPHA,
            ..ACCENT
        },
    }
}

pub fn text_field<'a, Message: Clone + 'a>(
    placeholder: &str,
    value: &str,
) -> TextInput<'a, Message> {
    text_input(placeholder, value)
        .style(text_field_style)
        .width(CONTROL_WIDTH)
        .padding(TEXT_FIELD_PADDING)
}

fn icon(bytes: &'static [u8]) -> Svg<'static> {
    svg::Svg::new(Handle::from_memory(bytes))
        .width(ICON_SIZE)
        .height(ICON_SIZE)
}

pub fn sidebar_icon(bytes: &'static [u8], selected: bool) -> Svg<'static> {
    icon(bytes).style(move |_theme, status| sidebar_icon_style(status, selected))
}

fn sidebar_icon_style(status: svg::Status, selected: bool) -> svg::Style {
    let color = match (selected, status) {
        (true, _) => Color::WHITE,
        (false, svg::Status::Idle) => MUTED,
        (false, svg::Status::Hovered) => TEXT,
    };
    svg::Style { color: Some(color) }
}

pub fn accent_icon(bytes: &'static [u8]) -> Svg<'static> {
    icon(bytes).style(|_theme, _status| svg::Style {
        color: Some(ACCENT),
    })
}

pub fn plain_icon(bytes: &'static [u8]) -> Svg<'static> {
    icon(bytes).style(|_theme, _status| svg::Style { color: Some(TEXT) })
}

pub fn stat_icon(bytes: &'static [u8]) -> Svg<'static> {
    icon(bytes)
        .width(STAT_ICON_SIZE)
        .height(STAT_ICON_SIZE)
        .style(|_theme, _status| svg::Style { color: Some(MUTED) })
}

pub fn stat_label<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content).size(STAT_SIZE).color(MUTED)
}

pub fn stat_value<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content)
        .size(STAT_SIZE)
        .color(TEXT)
        .font(Font::MONOSPACE)
}

pub fn usage_bar(_theme: &Theme) -> progress_bar::Style {
    progress_bar::Style {
        background: Background::Color(OUTLINE),
        bar: Background::Color(ACCENT),
        border: Border::default().rounded(USAGE_BAR_GIRTH / 2.0),
    }
}

pub fn heading(content: &str) -> Text<'_> {
    text(content).size(HEADING_SIZE).font(BOLD)
}

pub fn subheading(content: &str) -> Text<'_> {
    text(content).size(SUBHEADING_SIZE).color(MUTED)
}

pub fn clip_title<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content)
        .size(CLIP_TITLE_SIZE)
        .color(TEXT)
        .font(SEMIBOLD)
}

pub fn clip_details<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content).size(CLIP_DETAILS_SIZE).color(MUTED)
}

pub fn duration_badge(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT),
        background: Some(Background::Color(Color {
            a: 0.7,
            ..Color::BLACK
        })),
        border: Border::default().rounded(DURATION_RADIUS),
        ..container::Style::default()
    }
}
