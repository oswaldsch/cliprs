use iced::widget::container::Style;
use iced::widget::text::IntoFragment;
use iced::widget::{column, container, row, rule};
use iced::{Alignment, Background, Border, Element, Length, Theme, border};

use super::icon::{self, Tint, icon};
use super::{color, outlined, radius, space, text};

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

pub fn scrim(_theme: &Theme) -> Style {
    style(color::SCRIM, Border::default())
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

pub fn infobox<'a, Message: 'a>(
    title: impl IntoFragment<'a>,
    detail: impl IntoFragment<'a>,
) -> Element<'a, Message> {
    container(
        row![
            icon(include_bytes!("../../assets/info.svg"), icon::MD, Tint::Accent),
            column![text::title(title), text::label(detail)].spacing(space::XS),
        ]
        .spacing(space::MD)
        .align_y(Alignment::Start),
    )
    .style(card)
    .padding(space::MD)
    .width(Length::Fill)
    .into()
}

pub fn warning<'a, Message: 'a>(title: impl IntoFragment<'a>) -> Element<'a, Message> {
    container(
        row![
            icon(
                include_bytes!("../../assets/triangle-alert.svg"),
                icon::MD,
                Tint::Danger
            ),
            text::title(title),
        ]
        .spacing(space::MD)
        .align_y(Alignment::Center),
    )
    .style(card)
    .padding(space::MD)
    .width(Length::Fill)
    .into()
}
