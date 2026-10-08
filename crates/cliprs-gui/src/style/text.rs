use iced::Font;
use iced::font::Weight;
use iced::widget::text;
use iced::widget::text::{IntoFragment, Text};

use super::color;

const HEADING: f32 = 24.0;
const BODY: f32 = 16.0;
const LABEL: f32 = 14.0;
const CAPTION: f32 = 12.0;

const BOLD: Font = Font {
    weight: Weight::Bold,
    ..Font::DEFAULT
};
const SEMIBOLD: Font = Font {
    weight: Weight::Semibold,
    ..Font::DEFAULT
};

pub fn heading<'a>(content: impl IntoFragment<'a>) -> Text<'a> {
    text(content).size(HEADING).font(BOLD)
}

pub fn title<'a>(content: impl IntoFragment<'a>) -> Text<'a> {
    text(content).size(BODY).font(SEMIBOLD).color(color::TEXT)
}

pub fn lead<'a>(content: impl IntoFragment<'a>) -> Text<'a> {
    text(content).size(BODY).color(color::MUTED)
}

pub fn label<'a>(content: impl IntoFragment<'a>) -> Text<'a> {
    text(content).size(LABEL).color(color::MUTED)
}

pub fn value<'a>(content: impl IntoFragment<'a>) -> Text<'a> {
    text(content)
        .size(LABEL)
        .font(Font::MONOSPACE)
        .color(color::TEXT)
}

pub fn caption<'a>(content: impl IntoFragment<'a>) -> Text<'a> {
    text(content).size(CAPTION).color(color::MUTED)
}

pub fn badge<'a>(content: impl IntoFragment<'a>) -> Text<'a> {
    text(content).size(CAPTION).color(color::TEXT)
}
