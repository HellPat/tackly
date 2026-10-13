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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_invitation_becomes_an_svg_and_too_much_text_none() {
        let svg = qr_svg("tackly://join?c=abc");
        assert!(svg.contains("<svg") && svg.contains("#1b1c18"), "{svg}");
        assert_eq!(qr_svg(&"x".repeat(8000)), "");
    }
}
