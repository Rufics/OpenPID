//! A small writer of enhanced metafiles for the tests: the records a copy
//! of the reference tool's plots is made of, and a plot drawn with them
//! (a box, ticks and their labels, curves, a map with its colour bar).

#![allow(dead_code, reason = "each test uses a part of the writer")]
#![allow(clippy::unreadable_literal, reason = "colours are written 0xRRGGBB")]
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

/// The records of a metafile being written.
#[derive(Debug, Default)]
pub struct Writer {
    records: Vec<Vec<u8>>,
    next_pen: u32,
}

fn u32s(out: &mut Vec<u8>, values: &[u32]) {
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

fn i32s(out: &mut Vec<u8>, values: &[i32]) {
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

fn f32s(out: &mut Vec<u8>, values: &[f32]) {
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

/// `0xRRGGBB` as the `0x00BBGGRR` of a metafile.
pub fn colorref(rgb: u32) -> u32 {
    ((rgb & 0xff) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 0xff)
}

impl Writer {
    pub fn new() -> Writer {
        Writer {
            records: Vec::new(),
            next_pen: 1,
        }
    }

    /// A record of `kind` with `body` after its type and size, padded to
    /// four bytes.
    pub fn record(&mut self, kind: u32, body: &[u8]) {
        let mut r = Vec::new();
        let size = (8 + body.len()).div_ceil(4) * 4;
        u32s(&mut r, &[kind, size as u32]);
        r.extend_from_slice(body);
        r.resize(size, 0);
        self.records.push(r);
    }

    /// The file: a header, the records, the end.
    pub fn finish(mut self) -> Vec<u8> {
        self.record(14, &[0; 12]);
        let mut header = Vec::new();
        i32s(&mut header, &[0, 0, 3000, 1000, 0, 0, 30000, 10000]);
        header.extend_from_slice(b" EMF");
        u32s(
            &mut header,
            &[0x10000, 0, self.records.len() as u32 + 1, 0, 0, 0, 0],
        );
        i32s(&mut header, &[3840, 2560, 596, 397]);
        let mut out = Vec::new();
        u32s(&mut out, &[1, 8 + header.len() as u32]);
        out.extend_from_slice(&header);
        for r in &self.records {
            out.extend_from_slice(r);
        }
        let total = out.len() as u32;
        out[48..52].copy_from_slice(&total.to_le_bytes());
        out
    }

    pub fn set_transform(&mut self, m: [f32; 6]) {
        let mut b = Vec::new();
        f32s(&mut b, &m);
        self.record(35, &b);
    }

    pub fn modify_transform(&mut self, m: [f32; 6], mode: u32) {
        let mut b = Vec::new();
        f32s(&mut b, &m);
        u32s(&mut b, &[mode]);
        self.record(36, &b);
    }

    pub fn save(&mut self) {
        self.record(33, &[]);
    }

    pub fn restore(&mut self, which: i32) {
        let mut b = Vec::new();
        i32s(&mut b, &[which]);
        self.record(34, &b);
    }

    pub fn text_align(&mut self, align: u32) {
        let mut b = Vec::new();
        u32s(&mut b, &[align]);
        self.record(22, &b);
    }

    /// `EMR_EXTCREATEPEN` and `EMR_SELECTOBJECT`; the pen's index.
    pub fn pen(&mut self, rgb: u32, width: u32, style: u32) -> u32 {
        let index = self.next_pen;
        self.next_pen += 1;
        let mut b = Vec::new();
        u32s(&mut b, &[index, 0, 0, 0, 0]);
        u32s(&mut b, &[style, width, 0, colorref(rgb), 0, 0]);
        self.record(95, &b);
        self.select(index);
        index
    }

    /// `EMR_CREATEPEN` and `EMR_SELECTOBJECT`; the pen's index.
    pub fn old_pen(&mut self, rgb: u32, width: i32) -> u32 {
        let index = self.next_pen;
        self.next_pen += 1;
        let mut b = Vec::new();
        u32s(&mut b, &[index, 0]);
        i32s(&mut b, &[width, width]);
        u32s(&mut b, &[colorref(rgb)]);
        self.record(38, &b);
        self.select(index);
        index
    }

    pub fn select(&mut self, index: u32) {
        let mut b = Vec::new();
        u32s(&mut b, &[index]);
        self.record(37, &b);
    }

    pub fn delete(&mut self, index: u32) {
        let mut b = Vec::new();
        u32s(&mut b, &[index]);
        self.record(40, &b);
    }

    /// `EMR_POLYLINE16` (or `EMR_POLYLINETO16` with `to`) of logical points.
    pub fn polyline(&mut self, points: &[(i16, i16)], to: bool) {
        let mut b = Vec::new();
        i32s(&mut b, &[0, 0, 0, 0]);
        u32s(&mut b, &[points.len() as u32]);
        for (x, y) in points {
            b.extend_from_slice(&x.to_le_bytes());
            b.extend_from_slice(&y.to_le_bytes());
        }
        self.record(if to { 89 } else { 87 }, &b);
    }

    /// `EMR_EXTTEXTOUTW`: `text` at `(x, y)`, each character `advance` wide.
    pub fn text(&mut self, text: &str, (x, y): (i32, i32), advance: u32) {
        let units: Vec<u16> = text.encode_utf16().collect();
        let n = units.len() as u32;
        // offsets from the start of the record
        let string_at = 76u32;
        let dx_at = string_at + (2 * n).div_ceil(4) * 4;
        let mut b = Vec::new();
        i32s(&mut b, &[0, 0, 0, 0]);
        u32s(&mut b, &[1]);
        f32s(&mut b, &[1.0, 1.0]);
        i32s(&mut b, &[x, y]);
        u32s(&mut b, &[n, string_at, 0]);
        i32s(&mut b, &[0, 0, -1, -1]);
        u32s(&mut b, &[dx_at]);
        for u in &units {
            b.extend_from_slice(&u.to_le_bytes());
        }
        b.resize((dx_at - 8) as usize, 0);
        for _ in 0..n {
            u32s(&mut b, &[advance]);
        }
        self.record(84, &b);
    }

    /// `EMR_STRETCHDIBITS` of an 8-bit picture: `rows` of palette indices
    /// from the top, stored bottom-up (or top-down with `top_down`).
    pub fn picture8(
        &mut self,
        at: (i32, i32),
        size: (i32, i32),
        palette: &[u32],
        rows: &[Vec<u8>],
        top_down: bool,
    ) {
        let (w, h) = (rows[0].len(), rows.len());
        let stride = w.div_ceil(4) * 4;
        let mut bits = Vec::new();
        let order: Vec<&Vec<u8>> = if top_down {
            rows.iter().collect()
        } else {
            rows.iter().rev().collect()
        };
        for row in order {
            let mut r = row.clone();
            r.resize(stride, 0);
            bits.extend_from_slice(&r);
        }
        let mut info = Vec::new();
        let height = if top_down { -(h as i32) } else { h as i32 };
        u32s(&mut info, &[40]);
        i32s(&mut info, &[w as i32, height]);
        info.extend_from_slice(&1u16.to_le_bytes());
        info.extend_from_slice(&8u16.to_le_bytes());
        u32s(&mut info, &[0, 0, 0, 0, palette.len() as u32, 0]);
        for c in palette {
            u32s(&mut info, &[*c & 0x00ff_ffff]);
        }
        self.dib(at, size, &info, &bits);
    }

    /// `EMR_STRETCHDIBITS` of a 16-bit (5-5-5) picture of colours `0xRRGGBB`.
    pub fn picture16(&mut self, at: (i32, i32), size: (i32, i32), rows: &[Vec<u32>]) {
        let (w, h) = (rows[0].len(), rows.len());
        let stride = (2 * w).div_ceil(4) * 4;
        let mut bits = Vec::new();
        for row in rows.iter().rev() {
            let mut r = Vec::new();
            for c in row {
                let (red, green, blue) = ((c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff);
                let v = ((red >> 3) << 10) | ((green >> 3) << 5) | (blue >> 3);
                r.extend_from_slice(&(v as u16).to_le_bytes());
            }
            r.resize(stride, 0);
            bits.extend_from_slice(&r);
        }
        let mut info = Vec::new();
        u32s(&mut info, &[40]);
        i32s(&mut info, &[w as i32, h as i32]);
        info.extend_from_slice(&1u16.to_le_bytes());
        info.extend_from_slice(&16u16.to_le_bytes());
        u32s(&mut info, &[0, 0, 0, 0, 0, 0]);
        self.dib(at, size, &info, &bits);
    }

    fn dib(&mut self, at: (i32, i32), size: (i32, i32), info: &[u8], bits: &[u8]) {
        let info_at = 80u32;
        let bits_at = info_at + info.len() as u32;
        let mut b = Vec::new();
        i32s(&mut b, &[0, 0, 0, 0, at.0, at.1, 0, 0, 0, 0]);
        u32s(
            &mut b,
            &[
                info_at,
                info.len() as u32,
                bits_at,
                bits.len() as u32,
                0,
                0x00cc_0020,
            ],
        );
        i32s(&mut b, &[size.0, size.1]);
        b.extend_from_slice(info);
        b.extend_from_slice(bits);
        self.record(81, &b);
    }
}

/// A plot: where its box is on the page and the data its edges stand for.
#[derive(Debug, Clone, Copy)]
pub struct Plot {
    /// Left, top, right, bottom of the box, device units.
    pub frame: (f64, f64, f64, f64),
    /// The data at the left and the right edge.
    pub x: (f64, f64),
    /// The data at the bottom and the top edge.
    pub y: (f64, f64),
}

impl Plot {
    pub fn page(&self, (x, y): (f64, f64)) -> (f64, f64) {
        let (l, t, r, b) = self.frame;
        (
            l + (x - self.x.0) / (self.x.1 - self.x.0) * (r - l),
            b - (y - self.y.0) / (self.y.1 - self.y.0) * (b - t),
        )
    }

    /// The box, the ticks at `xs` and `ys` (and minor ticks between them),
    /// and their labels below and left of the box. Lines are drawn through
    /// a transform of 1/8, as the reference tool draws them, so a logical
    /// unit is an eighth of a device unit.
    pub fn axes(&self, w: &mut Writer, xs: &[f64], ys: &[f64]) {
        let (l, t, r, b) = self.frame;
        w.modify_transform([0.125, 0.0, 0.0, 0.125, 0.0, 0.0], 2);
        let black = w.pen(0, 8, 0x10200);
        let line = |w: &mut Writer, a: (f64, f64), c: (f64, f64)| {
            let p = |v: f64| (v * 8.0).round() as i16;
            w.polyline(&[(p(a.0), p(a.1)), (p(c.0), p(c.1))], false);
        };
        line(w, (l, b), (r, b));
        line(w, (l, t), (r, t));
        line(w, (l, t), (l, b));
        line(w, (r, t), (r, b));
        for x in xs {
            let (px, _) = self.page((*x, self.y.0));
            line(w, (px, b), (px, b + 25.0));
            line(w, (px + 7.0, b), (px + 7.0, b + 12.0));
        }
        for y in ys {
            let (_, py) = self.page((self.x.0, *y));
            line(w, (l - 25.0, py), (l, py));
            line(w, (l - 12.0, py - 7.0), (l, py - 7.0));
        }
        w.delete(black);
        w.modify_transform([8.0, 0.0, 0.0, 8.0, 0.0, 0.0], 2);
        w.text_align(24);
        for x in xs {
            let label = format!("{x}").replace('-', "\u{2212}");
            let (px, _) = self.page((*x, self.y.0));
            let width = 15 * label.chars().count() as i32;
            w.text(
                &label,
                (px.round() as i32 - width / 2, (b + 50.0) as i32),
                15,
            );
        }
        for y in ys {
            let label = format!("{y}");
            let (_, py) = self.page((self.x.0, *y));
            let width = 15 * label.chars().count() as i32;
            w.text(
                &label,
                ((l - 30.0) as i32 - width, (py + 10.0).round() as i32),
                15,
            );
        }
    }

    /// A curve of data points drawn with a pen of `rgb`, in pieces of at
    /// most `piece` points.
    pub fn curve(&self, w: &mut Writer, rgb: u32, style: u32, points: &[(f64, f64)]) {
        w.modify_transform([0.125, 0.0, 0.0, 0.125, 0.0, 0.0], 2);
        let pen = w.pen(rgb, 24, 0x10200 | style);
        let logical: Vec<(i16, i16)> = points
            .iter()
            .map(|p| {
                let (x, y) = self.page(*p);
                ((x * 8.0).round() as i16, (y * 8.0).round() as i16)
            })
            .collect();
        for chunk in logical.chunks(1000) {
            w.polyline(chunk, false);
        }
        w.delete(pen);
        w.modify_transform([8.0, 0.0, 0.0, 8.0, 0.0, 0.0], 2);
    }
}

/// The colour of step `s` (0 … 255) of a map: black, red, yellow, white.
pub fn hot(s: usize) -> u32 {
    let ramp = |from: usize| ((s.saturating_sub(from)) * 3).min(255) as u32;
    (ramp(0) << 16) | (ramp(85) << 8) | ramp(170)
}
