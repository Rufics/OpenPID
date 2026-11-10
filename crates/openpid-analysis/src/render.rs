//! Rasterising results for files: the heatmap colour maps (viridis and the
//! Blackbox Explorer ramp), a heatmap grid to RGBA pixels, and a minimal PNG
//! writer (stored deflate, no dependency) that the CLI's `--png` and the
//! GUI's "Export PNG" share.

use openpid_core::analysis::Grid2D;

/// Viridis anchor colours, dark to bright.
pub const VIRIDIS: [[u8; 3]; 8] = [
    [68, 1, 84],
    [70, 50, 126],
    [54, 92, 141],
    [39, 127, 142],
    [31, 161, 135],
    [74, 194, 109],
    [159, 218, 58],
    [253, 231, 37],
];

/// RGBA colour for `t` in `0..=1` along the viridis anchors; `t <= 0` (or
/// NaN) is fully transparent so empty cells do not hide what is behind them.
pub fn viridis(t: f64) -> [u8; 4] {
    if t.is_nan() || t <= 0.0 {
        return [0, 0, 0, 0];
    }
    let t = t.min(1.0) * (VIRIDIS.len() - 1) as f64;
    let i = (t.floor() as usize).min(VIRIDIS.len() - 2);
    let f = t - i as f64;
    let (a, b) = (VIRIDIS[i], VIRIDIS[i + 1]);
    let mix = |x: u8, y: u8| (f64::from(x) * (1.0 - f) + f64::from(y) * f).round() as u8;
    [mix(a[0], b[0]), mix(a[1], b[1]), mix(a[2], b[2]), 255]
}

/// The colour maps a heatmap can be drawn with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Colormap {
    /// Blackbox Explorer's ramp, black → red → white (`hsl(0, 100 %, L)`
    /// with the lightness L following the value, `graph_spectrum_plot.js`).
    #[default]
    Explorer,
    /// Viridis: perceptually ordered and readable with colour-vision
    /// deficiencies.
    Viridis,
}

impl Colormap {
    /// RGBA colour for `t` in `0..=1`; `t <= 0` (or NaN) is transparent.
    pub fn rgba(self, t: f64) -> [u8; 4] {
        match self {
            Colormap::Explorer => explorer(t),
            Colormap::Viridis => viridis(t),
        }
    }

    /// Anchor colours from 0 to 1, for a colour bar.
    pub fn stops(self) -> Vec<[u8; 3]> {
        match self {
            Colormap::Explorer => vec![[0, 0, 0], [255, 0, 0], [255, 255, 255]],
            Colormap::Viridis => VIRIDIS.to_vec(),
        }
    }
}

/// Blackbox Explorer's heatmap colour, `hsl(0, 100 %, t·100 %)`: black at 0,
/// pure red at 0.5, white at 1; `t <= 0` (or NaN) is transparent.
pub fn explorer(t: f64) -> [u8; 4] {
    if t.is_nan() || t <= 0.0 {
        return [0, 0, 0, 0];
    }
    let l = t.min(1.0);
    let channel = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    if l <= 0.5 {
        [channel(2.0 * l), 0, 0, 255]
    } else {
        let w = channel(2.0 * l - 1.0);
        [255, w, w, 255]
    }
}

/// Robust colour-scale maximum: the 99th percentile of the positive values
/// (1 when there are none).
pub fn robust_max(values: &[f64]) -> f64 {
    let mut v: Vec<f64> = values
        .iter()
        .copied()
        .filter(|x| *x > 0.0 && x.is_finite())
        .collect();
    if v.is_empty() {
        return 1.0;
    }
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * 0.99) as usize].max(1e-12)
}

/// A grid as an RGBA image, one pixel per cell, rows from the highest `y`
/// at the top to the lowest at the bottom; `max` = 0 picks [`robust_max`]
/// over the rows with `y >= min_y_for_scale`. Returns `(width, height, rgba)`.
pub fn grid_rgba(g: &Grid2D, max: f64, min_y_for_scale: f64) -> (usize, usize, Vec<u8>) {
    grid_rgba_with(g, max, min_y_for_scale, Colormap::Viridis)
}

/// [`grid_rgba`] in a chosen colour map.
pub fn grid_rgba_with(
    g: &Grid2D,
    max: f64,
    min_y_for_scale: f64,
    cmap: Colormap,
) -> (usize, usize, Vec<u8>) {
    let (nx, ny) = (g.x.len(), g.y.len());
    let max = if max > 0.0 {
        max
    } else {
        let rows: Vec<f64> =
            g.y.iter()
                .enumerate()
                .filter(|(_, y)| **y >= min_y_for_scale)
                .flat_map(|(iy, _)| g.z[iy * nx..(iy + 1) * nx].iter().copied())
                .collect();
        robust_max(if rows.is_empty() { &g.z } else { &rows })
    };
    let mut rgba = Vec::with_capacity(nx * ny * 4);
    for iy in (0..ny).rev() {
        for ix in 0..nx {
            rgba.extend_from_slice(&cmap.rgba(g.z[iy * nx + ix] / max));
        }
    }
    (nx, ny, rgba)
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn chunk(out: &mut Vec<u8>, kind: [u8; 4], payload: &[u8]) {
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(&kind);
    out.extend_from_slice(payload);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// An 8-bit RGBA PNG of `width × height` pixels (`rgba.len() == 4 · w · h`),
/// zlib-wrapped with stored (uncompressed) deflate blocks: larger than a
/// compressed file, but exact and dependency-free.
///
/// # Panics
/// Panics if `rgba` is not `4 × width × height` bytes long.
pub fn png_rgba(width: usize, height: usize, rgba: &[u8]) -> Vec<u8> {
    assert_eq!(rgba.len(), 4 * width * height, "rgba size");
    let mut out = Vec::with_capacity(rgba.len() + height + 128);
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(height as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8 bit, RGBA, deflate, no filter, no interlace
    chunk(&mut out, *b"IHDR", &ihdr);
    // scanlines, each with the "none" filter byte
    let mut raw = Vec::with_capacity(height * (1 + 4 * width));
    for row in rgba.chunks(4 * width.max(1)).take(height) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    // zlib: header, stored blocks of at most 65535 bytes, adler32
    let mut z = Vec::with_capacity(raw.len() + raw.len() / 65_535 * 5 + 11);
    z.extend_from_slice(&[0x78, 0x01]);
    let mut blocks = raw.chunks(65_535).peekable();
    if blocks.peek().is_none() {
        z.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(block) = blocks.next() {
        z.push(u8::from(blocks.peek().is_none()));
        let len = block.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    chunk(&mut out, *b"IDAT", &z);
    chunk(&mut out, *b"IEND", &[]);
    out
}

/// The colour maximum a grid is drawn with: `max` when it is set (> 0), else
/// [`robust_max`] over the rows at or above `min_y` (all rows when none is).
pub fn colour_max(g: &Grid2D, max: f64, min_y: f64) -> f64 {
    if max > 0.0 {
        return max;
    }
    let nx = g.x.len();
    let rows: Vec<f64> =
        g.y.iter()
            .enumerate()
            .filter(|(_, y)| **y >= min_y)
            .flat_map(|(iy, _)| g.z[iy * nx..(iy + 1) * nx].iter().copied())
            .collect();
    robust_max(if rows.is_empty() { &g.z } else { &rows })
}

/// The value of the grid cell nearest to the data coordinates `(x, y)`
/// (`None` outside the grid): what a pointer over a heatmap reads.
pub fn value_at(g: &Grid2D, x: f64, y: f64) -> Option<f64> {
    let nearest = |v: &[f64], t: f64| -> Option<usize> {
        if v.is_empty() {
            return None;
        }
        let (lo, hi) = (v[0].min(v[v.len() - 1]), v[0].max(v[v.len() - 1]));
        if t < lo || t > hi {
            return None;
        }
        v.iter()
            .enumerate()
            .min_by(|a, b| (a.1 - t).abs().total_cmp(&(b.1 - t).abs()))
            .map(|(i, _)| i)
    };
    let ix = nearest(&g.x, x)?;
    let iy = nearest(&g.y, y)?;
    Some(g.z[iy * g.x.len() + ix])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse the stored-deflate PNG back: `(width, height, rgba)`.
    fn decode(png: &[u8]) -> (usize, usize, Vec<u8>) {
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        let mut i = 8;
        let (mut w, mut h) = (0usize, 0usize);
        let mut idat = Vec::new();
        while i < png.len() {
            let len = u32::from_be_bytes(png[i..i + 4].try_into().unwrap()) as usize;
            let kind = &png[i + 4..i + 8];
            let payload = &png[i + 8..i + 8 + len];
            let crc = u32::from_be_bytes(png[i + 8 + len..i + 12 + len].try_into().unwrap());
            assert_eq!(crc, crc32(&png[i + 4..i + 8 + len]), "chunk crc");
            match kind {
                b"IHDR" => {
                    w = u32::from_be_bytes(payload[..4].try_into().unwrap()) as usize;
                    h = u32::from_be_bytes(payload[4..8].try_into().unwrap()) as usize;
                    assert_eq!(&payload[8..], &[8, 6, 0, 0, 0]);
                }
                b"IDAT" => idat.extend_from_slice(payload),
                b"IEND" => assert!(payload.is_empty()),
                other => panic!("unexpected chunk {other:?}"),
            }
            i += 12 + len;
        }
        assert_eq!(&idat[..2], &[0x78, 0x01]);
        let mut raw = Vec::new();
        let mut j = 2;
        loop {
            let last = idat[j];
            let len = u16::from_le_bytes([idat[j + 1], idat[j + 2]]) as usize;
            let nlen = u16::from_le_bytes([idat[j + 3], idat[j + 4]]);
            assert_eq!(nlen, !(len as u16));
            raw.extend_from_slice(&idat[j + 5..j + 5 + len]);
            j += 5 + len;
            if last == 1 {
                break;
            }
        }
        let (mut a, mut b) = (1u32, 0u32);
        for &byte in &raw {
            a = (a + u32::from(byte)) % 65_521;
            b = (b + a) % 65_521;
        }
        assert_eq!(
            u32::from_be_bytes(idat[j..j + 4].try_into().unwrap()),
            (b << 16) | a,
            "adler32"
        );
        let mut rgba = Vec::new();
        for row in raw.chunks(1 + 4 * w) {
            assert_eq!(row[0], 0, "filter none");
            rgba.extend_from_slice(&row[1..]);
        }
        (w, h, rgba)
    }

    #[test]
    fn png_round_trips_through_a_stored_deflate_decoder() {
        let (w, h) = (300, 70); // > 65535 bytes: two stored blocks
        let rgba: Vec<u8> = (0..w * h * 4).map(|i| (i * 7 % 251) as u8).collect();
        let png = png_rgba(w, h, &rgba);
        assert_eq!(decode(&png), (w, h, rgba));
        let tiny = png_rgba(1, 1, &[1, 2, 3, 4]);
        assert_eq!(decode(&tiny), (1, 1, vec![1, 2, 3, 4]));
        let empty = png_rgba(0, 0, &[]);
        assert_eq!(decode(&empty).2, Vec::<u8>::new());
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926, "CRC-32 check value");
    }

    #[test]
    fn palette_and_grid_rendering() {
        assert_eq!(explorer(0.0), [0, 0, 0, 0]);
        assert_eq!(explorer(0.5), [255, 0, 0, 255], "pure red half way");
        assert_eq!(explorer(0.25), [128, 0, 0, 255]);
        assert_eq!(explorer(1.0), [255, 255, 255, 255]);
        assert_eq!(explorer(3.0), [255, 255, 255, 255], "clamped");
        assert_eq!(Colormap::default().rgba(0.75), [255, 128, 128, 255]);
        assert_eq!(Colormap::Viridis.rgba(1.0), viridis(1.0));
        assert_eq!(Colormap::Explorer.stops().len(), 3);
        assert_eq!(viridis(0.0), [0, 0, 0, 0]);
        assert_eq!(viridis(f64::NAN), [0, 0, 0, 0]);
        assert_eq!(viridis(1.0), [253, 231, 37, 255]);
        assert_eq!(viridis(2.0), [253, 231, 37, 255], "clamped");
        let mid = viridis(0.5);
        assert_eq!(mid[3], 255);
        assert!(mid[1] > viridis(0.1)[1], "greener towards the top");
        assert_eq!(robust_max(&[]), 1.0);
        assert_eq!(robust_max(&[-1.0, 0.0, f64::NAN]), 1.0);
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(robust_max(&v), 99.0);
        let g = Grid2D {
            x: vec![0.0, 1.0],
            y: vec![10.0, 20.0, 30.0],
            z: vec![0.0, 1.0, 2.0, 3.0, 4.0, 8.0],
            x_label: String::new(),
            y_label: String::new(),
            z_label: String::new(),
        };
        let (w, h, px) = grid_rgba(&g, 8.0, 0.0);
        assert_eq!((w, h), (2, 3));
        assert_eq!(&px[..4], &viridis(0.5), "top-left is the highest row");
        // rows run from the highest y (z[4], z[5]) down to the lowest (z[0], z[1])
        assert_eq!(&px[4..8], &viridis(1.0));
        assert_eq!(&px[16..20], &[0, 0, 0, 0], "z = 0 is transparent");
        let (_, _, auto) = grid_rgba(&g, 0.0, 25.0);
        assert_eq!(
            &auto[4..8],
            &viridis(1.0),
            "auto scale over the top row only"
        );
    }

    #[test]
    fn colour_maximum_and_value_lookup() {
        // 11 throttle columns 0…100 %, 6 frequency rows 0…500 Hz, z = row·3 + column/10·...
        let (nx, ny) = (11usize, 6usize);
        let g = Grid2D {
            x: (0..nx).map(|i| i as f64 * 10.0).collect(),
            y: (0..ny).map(|i| i as f64 * 100.0).collect(),
            z: (0..ny)
                .flat_map(|iy| (0..nx).map(move |ix| iy as f64 + ix as f64))
                .collect(),
            x_label: String::new(),
            y_label: String::new(),
            z_label: String::new(),
        };
        // an explicit maximum wins; the automatic one is the robust maximum
        assert_eq!(colour_max(&g, 3.0, 0.0), 3.0);
        let auto = colour_max(&g, 0.0, 0.0);
        assert!(auto > 10.0 && auto <= 15.0, "{auto}");
        // rows below the floor do not count; a floor above every row falls back to all
        assert!(colour_max(&g, 0.0, 400.0) >= auto);
        assert_eq!(colour_max(&g, 0.0, 1e9), auto);
        // the nearest cell, nothing outside the grid
        assert_eq!(value_at(&g, 0.0, 0.0), Some(0.0));
        assert_eq!(value_at(&g, 100.0, 500.0), Some(15.0));
        assert_eq!(value_at(&g, 52.0, 101.0), Some(5.0 + 1.0));
        assert_eq!(value_at(&g, -1.0, 0.0), None);
        assert_eq!(value_at(&g, 0.0, 501.0), None);
    }
}
