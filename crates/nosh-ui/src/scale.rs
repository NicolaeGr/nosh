//! Single scale factor for the bar.
//!
//! `SCALE = 1.0` matches the 1080p design in
//! `docs/reference/design_proto.html`. A 1440p display uses `1080 / 1440`.

pub const SCALE: f64 = 0.75;

const BASE_FONT_PX: f64 = 13.0;
const ICON_PX: f64 = 16.0;
const SCALE_TOKEN: &str = "{{base_font}}";

/// Fills the font-size placeholder in `style.css`.
pub fn stylesheet(source: &str) -> String {
    source.replace(SCALE_TOKEN, &format!("{:.2}px", BASE_FONT_PX * SCALE))
}

/// Icon size for `gtk::Image::set_pixel_size`.
pub fn icon_px() -> i32 {
    (ICON_PX * SCALE).round().max(1.0) as i32
}

#[cfg(test)]
mod tests {
    use super::{stylesheet, SCALE_TOKEN};

    #[test]
    fn stylesheet_fills_the_scale_token() {
        let css = stylesheet(&format!("window {{ font-size: {SCALE_TOKEN}; }}"));
        assert!(!css.contains(SCALE_TOKEN));
    }
}
