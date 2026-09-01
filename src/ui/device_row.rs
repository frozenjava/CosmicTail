// SPDX-License-Identifier: MIT

//! The device row, in the two shapes the app needs.
//!
//! These return inert content, not buttons. The window wraps them in a
//! selectable button, the applet in `applet::menu_button`, and neither has to
//! know how the other styles its rows.

use cosmic::Element;
use cosmic::iced::Alignment;
use cosmic::widget;

use crate::tailscale::{Device, DeviceStatus};
use crate::ui::format;

/// The coloured dot in front of a device name.
///
/// `theme::Text::Custom` takes a plain `fn` pointer, so each of these captures
/// nothing and costs one theme lookup at draw time rather than one per row.
pub fn status_dot<'a, M: 'a>(status: &DeviceStatus) -> Element<'a, M> {
    let class = match status {
        DeviceStatus::Online => {
            cosmic::theme::Text::Custom(|theme| cosmic::iced::widget::text::Style {
                color: Some(theme.cosmic().success_color().into()),
                ..Default::default()
            })
        }
        DeviceStatus::Expired { .. } => {
            cosmic::theme::Text::Custom(|theme| cosmic::iced::widget::text::Style {
                color: Some(theme.cosmic().warning_color().into()),
                ..Default::default()
            })
        }
        DeviceStatus::Offline { .. } => {
            cosmic::theme::Text::Custom(|theme| cosmic::iced::widget::text::Style {
                color: Some(theme.cosmic().palette.neutral_6.into()),
                ..Default::default()
            })
        }
    };

    widget::text("\u{25cf}").class(class).into()
}

/// Dot, name, and the "(offline)" qualifier — a single line.
pub fn compact<'a, M: 'a>(device: &'a Device) -> Element<'a, M> {
    let space_xs = cosmic::theme::spacing().space_xs;

    widget::row::with_capacity(2)
        .push(status_dot(&device.status))
        .push(widget::text::body(format!(
            "{}{}",
            device.short_name(),
            format::name_suffix(device)
        )))
        .spacing(space_xs)
        .align_y(Alignment::Center)
        .into()
}

/// [`compact`] with the device's primary address on a second line, as in the
/// window's device list.
pub fn stacked<'a, M: 'a>(device: &'a Device) -> Element<'a, M> {
    let space_xxxs = cosmic::theme::spacing().space_xxxs;

    widget::column::with_capacity(2)
        .push(compact(device))
        .push(
            widget::text::caption(format::primary_ip(device)).class(cosmic::theme::Text::Custom(
                |theme| cosmic::iced::widget::text::Style {
                    color: Some(theme.cosmic().palette.neutral_7.into()),
                    ..Default::default()
                },
            )),
        )
        .spacing(space_xxxs)
        .into()
}
