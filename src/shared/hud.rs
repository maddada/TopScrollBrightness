//! Artwork for the on-screen brightness indicator, laid out like Windows 11's
//! own volume indicator: a rounded panel with an icon, a thin level bar and
//! the number. The panel and the bar are drawn here as signed distance fields;
//! the platform draws the icon and the number with the system fonts, in
//! [`Hud::icon_box`] and [`Hud::number_box`].

// Layout in 96-DPI pixels, measured from Windows 11's volume indicator.
const WIDTH: f64 = 192.0;
const HEIGHT: f64 = 48.0;
const CORNER_RADIUS: f64 = 8.0;
const PADDING: f64 = 12.0;
const ICON_SIZE: f64 = 16.0;
const TRACK_LEFT: f64 = 42.0;
const TRACK_RIGHT: f64 = 152.0;
const TRACK_THICKNESS: f64 = 4.0;

/// The indicator's colours (straight RGB).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub background: [u8; 3],
    /// The empty part of the level bar.
    pub track: [u8; 3],
    /// The filled part of the level bar.
    pub fill: [u8; 3],
    /// Icon and number.
    pub foreground: [u8; 3],
}

impl Palette {
    /// Dark theme. `accent` is the theme's light accent shade.
    pub fn dark(accent: [u8; 3]) -> Self {
        Self { background: [44, 44, 44], track: [159, 159, 159], fill: accent, foreground: [255, 255, 255] }
    }

    /// Light theme. `accent` is the theme's dark accent shade.
    pub fn light(accent: [u8; 3]) -> Self {
        Self { background: [249, 249, 249], track: [138, 138, 138], fill: accent, foreground: [26, 26, 26] }
    }
}

pub struct Hud {
    pub width: u32,
    pub height: u32,
    scale: f64,
}

impl Hud {
    /// The indicator at a display scale (1.0 = 96 DPI).
    pub fn new(scale: f64) -> Self {
        Self { width: (WIDTH * scale).round() as u32, height: (HEIGHT * scale).round() as u32, scale }
    }

    /// Size of the icon font, in pixels.
    pub fn icon_px(&self) -> i32 {
        (ICON_SIZE * self.scale).round() as i32
    }

    /// Size of the number's font, in pixels.
    pub fn number_px(&self) -> i32 {
        (14.0 * self.scale).round() as i32
    }

    /// Where the icon goes, as (left, top, right, bottom) in pixels.
    pub fn icon_box(&self) -> (i32, i32, i32, i32) {
        let s = self.scale;
        ((PADDING * s) as i32, 0, ((PADDING + ICON_SIZE) * s).round() as i32, self.height as i32)
    }

    /// Where the number goes (right-aligned), as (left, top, right, bottom) in pixels.
    pub fn number_box(&self) -> (i32, i32, i32, i32) {
        let s = self.scale;
        ((TRACK_RIGHT * s) as i32, 0, self.width as i32 - (PADDING * s).round() as i32, self.height as i32)
    }

    /// Straight-alpha RGBA. Inside the panel the colour is opaque and the alpha
    /// byte is the panel's coverage, so text can be drawn over the colour before
    /// the coverage is applied.
    pub fn render(&self, fraction: f64, palette: &Palette) -> Vec<u8> {
        let s = self.scale;
        let (w, h) = (self.width as f64, self.height as f64);
        let (left, right) = (TRACK_LEFT * s, TRACK_RIGHT * s);
        let half = TRACK_THICKNESS * s / 2.0;
        let fill_end = left + (right - left) * fraction.clamp(0.0, 1.0);
        let [background, track_colour, fill_colour] = [palette.background, palette.track, palette.fill].map(|c| c.map(f64::from));

        let mut out = Vec::with_capacity((self.width * self.height * 4) as usize);
        for py in 0..self.height {
            for px in 0..self.width {
                let (x, y) = (px as f64 + 0.5, py as f64 + 0.5);
                let panel = coverage(round_box(x, y, w / 2.0, h / 2.0, w / 2.0, h / 2.0, CORNER_RADIUS * s));
                let track = coverage(round_box(x, y, (left + right) / 2.0, h / 2.0, (right - left) / 2.0, half, half));
                // The filled part ends square at the level.
                let fill = track * (fill_end - x + 0.5).clamp(0.0, 1.0);

                let mut colour = background;
                for (layer, alpha) in [(track_colour, track), (fill_colour, fill)] {
                    for i in 0..3 {
                        colour[i] = layer[i] * alpha + colour[i] * (1.0 - alpha);
                    }
                }
                out.extend(colour.map(|c| c.round() as u8));
                out.push((panel * 255.0).round() as u8);
            }
        }
        out
    }
}

/// Anti-aliased coverage of a shape from its signed distance in pixels.
fn coverage(distance: f64) -> f64 {
    (0.5 - distance).clamp(0.0, 1.0)
}

/// Signed distance to a rounded box centred at (cx, cy) with half-size (hx, hy).
fn round_box(x: f64, y: f64, cx: f64, cy: f64, hx: f64, hy: f64, r: f64) -> f64 {
    let qx = (x - cx).abs() - (hx - r);
    let qy = (y - cy).abs() - (hy - r);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCENT: [u8; 3] = [64, 189, 255];

    fn pixel(hud: &Hud, rgba: &[u8], x: u32, y: u32) -> [u8; 4] {
        let i = ((y * hud.width + x) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    }

    #[test]
    fn the_bar_fills_up_to_the_level_in_the_accent_colour() {
        let hud = Hud::new(1.0);
        let palette = Palette::dark(ACCENT);
        let mid = hud.height / 2;
        let rgba = hud.render(0.4, &palette);
        assert_eq!(pixel(&hud, &rgba, 60, mid), [64, 189, 255, 255]); // 42 + 110 × 0.4 = 86
        assert_eq!(pixel(&hud, &rgba, 120, mid), [159, 159, 159, 255]);
        assert_eq!(pixel(&hud, &rgba, 100, 4), [44, 44, 44, 255]);

        let empty = hud.render(0.0, &palette);
        assert_eq!(pixel(&hud, &empty, 60, mid), [159, 159, 159, 255]);
    }

    #[test]
    fn matches_the_windows_volume_indicator_size() {
        let hud = Hud::new(1.5);
        assert_eq!((hud.width, hud.height), (288, 72));
        assert_eq!(hud.icon_box(), (18, 0, 42, 72));
        assert_eq!(hud.number_box(), (228, 0, 270, 72));
    }

    #[test]
    fn corners_are_transparent_and_the_middle_opaque() {
        let hud = Hud::new(1.25);
        let rgba = hud.render(0.3, &Palette::light([0, 85, 161]));
        assert_eq!(rgba.len(), (hud.width * hud.height * 4) as usize);
        assert_eq!(pixel(&hud, &rgba, 0, 0)[3], 0);
        assert_eq!(pixel(&hud, &rgba, hud.width / 2, hud.height / 2)[3], 255);
    }
}
