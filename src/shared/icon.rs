//! Procedurally drawn app icon (a sun on a rounded tile).
//!
//! Dependency-free on purpose: `build.rs` includes this file to generate the
//! Windows `.ico`, and the app uses it at runtime for the tray icon.

/// Renders a `size`×`size` icon as straight (non-premultiplied) RGBA bytes.
/// `active = false` draws the grey "disabled" variant.
pub fn render_rgba(size: u32, active: bool) -> Vec<u8> {
    const SS: u32 = 4; // supersampling per axis
    let (top, bottom) = if active {
        ([0xFF, 0xB8, 0x3D], [0xF0, 0x7A, 0x14])
    } else {
        ([0x9A, 0xA0, 0xA8], [0x6E, 0x74, 0x7D])
    };

    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let (mut tile, mut sun) = (0.0, 0.0);
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = (px as f64 + (sx as f64 + 0.5) / SS as f64) / size as f64;
                    let y = (py as f64 + (sy as f64 + 0.5) / SS as f64) / size as f64;
                    if round_box(x, y, 0.5, 0.5, 0.47, 0.47, 0.22) <= 0.0 {
                        tile += 1.0;
                    }
                    if in_sun(x, y) {
                        sun += 1.0;
                    }
                }
            }
            let n = (SS * SS) as f64;
            let (tile, sun) = (tile / n, sun / n);

            let t = py as f64 / size.max(1) as f64;
            let bg = [0, 1, 2].map(|i| lerp(top[i] as f64, bottom[i] as f64, t) / 255.0);

            // Composite in premultiplied space: tile, then the white sun.
            let mut rgb = bg.map(|c| c * tile);
            for c in &mut rgb {
                *c = sun + *c * (1.0 - sun);
            }
            let a = sun + tile * (1.0 - sun);

            for c in rgb {
                let straight = if a > 0.0 { c / a } else { 0.0 };
                out.push((straight * 255.0).round().clamp(0.0, 255.0) as u8);
            }
            out.push((a * 255.0).round().clamp(0.0, 255.0) as u8);
        }
    }
    out
}

/// A disc with eight rays, centred on the tile.
fn in_sun(x: f64, y: f64) -> bool {
    let (dx, dy) = (x - 0.5, y - 0.5);
    let r = dx.hypot(dy);
    if r <= 0.16 {
        return true;
    }
    if !(0.23..=0.36).contains(&r) {
        return false;
    }
    // Distance from the nearest ray's centre line, in units of the tile.
    let ray = std::f64::consts::FRAC_PI_4;
    let angle = dy.atan2(dx);
    let offset = angle - (angle / ray).round() * ray;
    (r * offset.sin()).abs() <= 0.042
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Signed distance to a rounded box centred at (cx, cy) with half-size (hx, hy).
fn round_box(x: f64, y: f64, cx: f64, cy: f64, hx: f64, hy: f64, r: f64) -> f64 {
    let qx = (x - cx).abs() - (hx - r);
    let qy = (y - cy).abs() - (hy - r);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
}
