//! The invitation as a QR code, drawn by the app and read back by the tests.

use qrcode::{QrCode, render::svg};

/// An SVG QR code for `text`, with a quiet zone, at least 240 px wide.
pub fn qr_svg(text: &str) -> String {
    QrCode::new(text.as_bytes())
        .map(|code| {
            code.render::<svg::Color>()
                .min_dimensions(240, 240)
                .quiet_zone(true)
                .dark_color(svg::Color("#1b1c18"))
                .light_color(svg::Color("#ffffff"))
                .build()
        })
        .unwrap_or_default()
}
