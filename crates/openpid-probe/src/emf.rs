//! A reader of the Enhanced Metafiles (`.emf`) the reference tool saves
//! when it copies a plot as a vector graphic, after the public format
//! description ([MS-EMF]), and the calibration of a plot's axes from its
//! tick marks and labels.
//!
//! A metafile is a stream of records `(type: u32, size: u32, data)`. The
//! reader keeps what a copied plot is made of:
//!
//! - the **curves**: every polyline with the colour, width and style of the
//!   pen it was drawn with, its points in device units (the world transform
//!   applied);
//! - the **texts**: the tick labels, titles and legends, with where they
//!   stand and how wide they are;
//! - the **pictures**: the colour maps of the throttle and time windows and
//!   their colour bars, with the place they take on the page and their
//!   pixels.
//!
//! Everything else (fills, clipping, brushes, fonts) is passed over. A file
//! that is not a metafile, or whose records do not fit in it, is an error;
//! the reader never panics on any input.
//!
//! [`Axes`] turns the points of a plot into data units: the black two-point
//! lines are its box and tick marks, the numbers below and left of the box
//! are the labels of the ticks, and each axis is the straight line (or the
//! logarithmic one) through the ticks at their labels' values.

use std::collections::HashMap;

/// `EMR_HEADER`.
const HEADER: u32 = 1;
/// `EMR_EOF`.
const EOF: u32 = 14;
/// `EMR_SETTEXTALIGN`.
const SET_TEXT_ALIGN: u32 = 22;
/// `EMR_SAVEDC`.
const SAVE_DC: u32 = 33;
/// `EMR_RESTOREDC`.
const RESTORE_DC: u32 = 34;
/// `EMR_SETWORLDTRANSFORM`.
const SET_WORLD_TRANSFORM: u32 = 35;
/// `EMR_MODIFYWORLDTRANSFORM`.
const MODIFY_WORLD_TRANSFORM: u32 = 36;
/// `EMR_SELECTOBJECT`.
const SELECT_OBJECT: u32 = 37;
/// `EMR_CREATEPEN`.
const CREATE_PEN: u32 = 38;
/// `EMR_DELETEOBJECT`.
const DELETE_OBJECT: u32 = 40;
/// `EMR_STRETCHDIBITS`.
const STRETCH_DIBITS: u32 = 81;
/// `EMR_EXTTEXTOUTW`.
const EXT_TEXT_OUT_W: u32 = 84;
/// `EMR_POLYLINE16`.
const POLYLINE16: u32 = 87;
/// `EMR_POLYLINETO16`.
const POLYLINE_TO16: u32 = 89;
/// `EMR_EXTCREATEPEN`.
const EXT_CREATE_PEN: u32 = 95;
/// Stock objects have this bit set in their index.
const STOCK: u32 = 0x8000_0000;
/// `WHITE_PEN`, `BLACK_PEN` and `NULL_PEN` among the stock objects.
const STOCK_PENS: [(u32, u32); 3] = [(6, 0x00ff_ffff), (7, 0), (8, 0)];
/// `GM_ADVANCED`: text is placed through the world transform as well.
const GM_ADVANCED: u32 = 2;
/// The most points a polyline or pixels a picture may claim: far beyond
/// any copy, small enough that a forged count cannot exhaust memory.
const MOST: usize = 1 << 26;

/// Why a metafile could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EmfError {
    /// The file does not start with a metafile header.
    #[error("not an enhanced metafile")]
    NotMetafile,
    /// A record claims a size that does not fit.
    #[error("record {index} at byte {offset} has a size of {size} bytes, which does not fit")]
    BadRecord {
        /// Number of the record, from 0.
        index: usize,
        /// Where it starts.
        offset: usize,
        /// The size it claims.
        size: u32,
    },
    /// A field of a record lies outside it, or holds an impossible value.
    #[error("record {index} (type {kind}): {what}")]
    BadField {
        /// Number of the record, from 0.
        index: usize,
        /// Its type.
        kind: u32,
        /// What is wrong.
        what: &'static str,
    },
    /// The records end before `EMR_EOF`.
    #[error("the file ends without its last record")]
    Truncated,
}

/// A world transform: a point `(x, y)` goes to
/// `(x·m11 + y·m21 + dx, x·m12 + y·m22 + dy)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// The six numbers: m11, m12, m21, m22, dx, dy.
    pub m: [f64; 6],
}

impl Transform {
    /// The transform that leaves every point where it is.
    pub const IDENTITY: Transform = Transform {
        m: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    };

    /// Where `(x, y)` goes.
    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let [m11, m12, m21, m22, dx, dy] = self.m;
        (x * m11 + y * m21 + dx, x * m12 + y * m22 + dy)
    }

    /// `self` first, then `then`.
    pub fn then(&self, then: &Transform) -> Transform {
        let [a11, a12, a21, a22, adx, ady] = self.m;
        let [b11, b12, b21, b22, bdx, bdy] = then.m;
        Transform {
            m: [
                a11 * b11 + a12 * b21,
                a11 * b12 + a12 * b22,
                a21 * b11 + a22 * b21,
                a21 * b12 + a22 * b22,
                adx * b11 + ady * b21 + bdx,
                adx * b12 + ady * b22 + bdy,
            ],
        }
    }
}

/// A pen: what a line is drawn with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pen {
    /// Its colour, `0xRRGGBB`.
    pub colour: u32,
    /// Its width, logical units.
    pub width: u32,
    /// Its style: the low four bits are solid (0), dashed, dotted …
    pub style: u32,
}

impl Pen {
    /// Whether the pen draws a solid line.
    pub fn solid(&self) -> bool {
        self.style.trailing_zeros() >= 4
    }
}

/// A polyline as drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    /// The pen it was drawn with.
    pub pen: Pen,
    /// Its points, device units: x to the right, y down.
    pub points: Vec<(f64, f64)>,
    /// Drawn on from the current position (`EMR_POLYLINETO16`) rather than
    /// as a polyline of its own: the dashes of a line the program breaks
    /// up itself. The reference tool draws a dotted line so, with the pen
    /// of the solid one beside it.
    pub joined: bool,
}

/// A text as drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    /// What it says.
    pub text: String,
    /// Its reference point, device units.
    pub at: (f64, f64),
    /// Its width: the sum of the advances of its characters, device units
    /// (0 when the record gives none).
    pub width: f64,
    /// The text alignment in force (`TA_*` flags).
    pub align: u32,
}

impl Text {
    /// The middle of the text, device units: from the reference point, the
    /// alignment and the width; the height is taken as 1.3 times the mean
    /// advance of a character.
    pub fn centre(&self) -> (f64, f64) {
        let chars = self.text.chars().count().max(1) as f64;
        let height = 1.3 * self.width / chars;
        let x = match self.align & 0x6 {
            0x6 => self.at.0,
            2 => self.at.0 - self.width,
            _ => self.at.0 + self.width / 2.0,
        };
        let y = match self.align & 0x18 {
            0x18 => self.at.1 - height / 2.0,
            8 => self.at.1 - 0.6 * height,
            _ => self.at.1 + height / 2.0,
        };
        (x, y)
    }

    /// The number the text writes, if it is one; the minus sign may be
    /// U+2212.
    pub fn number(&self) -> Option<f64> {
        let text = self.text.trim().replace('\u{2212}', "-");
        let value: f64 = text.parse().ok()?;
        value.is_finite().then_some(value)
    }
}

/// A picture as drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    /// Its top left corner on the page, device units.
    pub at: (f64, f64),
    /// The width and height it takes on the page, device units.
    pub size: (f64, f64),
    /// Its pixels across.
    pub width: usize,
    /// Its pixels down.
    pub height: usize,
    /// The palette, `0xRRGGBB` (empty for a picture without one).
    pub palette: Vec<u32>,
    /// The palette index of every pixel, rows from the top (empty for a
    /// picture without a palette).
    pub indices: Vec<u8>,
    /// The colour of every pixel, `0xRRGGBB`, rows from the top.
    pub colours: Vec<u32>,
    /// The bits of the coarsest of its three colour channels: 8 for a
    /// picture with a palette or of 24 or 32 bits, 5 for one of 16.
    pub depth: u32,
}

impl Picture {
    /// The colour of the pixel in column `x` of row `y` (from the top).
    pub fn colour(&self, x: usize, y: usize) -> Option<u32> {
        (x < self.width)
            .then(|| self.colours.get(y * self.width + x).copied())
            .flatten()
    }

    /// The middle of pixel `(x, y)` on the page, device units.
    pub fn centre_of(&self, x: usize, y: usize) -> (f64, f64) {
        let w = self.size.0 / self.width.max(1) as f64;
        let h = self.size.1 / self.height.max(1) as f64;
        (
            self.at.0 + (x as f64 + 0.5) * w,
            self.at.1 + (y as f64 + 0.5) * h,
        )
    }
}

/// What a metafile holds.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Metafile {
    /// The polylines, in the order they were drawn.
    pub curves: Vec<Curve>,
    /// The texts.
    pub texts: Vec<Text>,
    /// The pictures.
    pub pictures: Vec<Picture>,
}

/// The state of the drawing that `EMR_SAVEDC` saves.
#[derive(Debug, Clone, Copy)]
struct State {
    transform: Transform,
    pen: Pen,
    align: u32,
}

/// One record: its number, type and bytes.
struct Record<'a> {
    index: usize,
    kind: u32,
    data: &'a [u8],
}

impl Record<'_> {
    fn error(&self, what: &'static str) -> EmfError {
        EmfError::BadField {
            index: self.index,
            kind: self.kind,
            what,
        }
    }

    fn bytes<const N: usize>(&self, at: usize) -> Result<[u8; N], EmfError> {
        self.data
            .get(at..at + N)
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| self.error("a field lies outside the record"))
    }

    fn u32(&self, at: usize) -> Result<u32, EmfError> {
        self.bytes::<4>(at).map(u32::from_le_bytes)
    }

    fn i32(&self, at: usize) -> Result<i32, EmfError> {
        self.bytes::<4>(at).map(i32::from_le_bytes)
    }

    fn i16(&self, at: usize) -> Result<i16, EmfError> {
        self.bytes::<2>(at).map(i16::from_le_bytes)
    }

    fn f32(&self, at: usize) -> Result<f64, EmfError> {
        let v = f64::from(self.bytes::<4>(at).map(f32::from_le_bytes)?);
        if v.is_finite() {
            Ok(v)
        } else {
            Err(self.error("a transform holds a number that is not finite"))
        }
    }

    /// `len` bytes from `at`, or an error.
    fn slice(&self, at: usize, len: usize) -> Result<&[u8], EmfError> {
        at.checked_add(len)
            .and_then(|end| self.data.get(at..end))
            .ok_or_else(|| self.error("a block lies outside the record"))
    }

    fn transform(&self) -> Result<Transform, EmfError> {
        let mut m = [0.0; 6];
        for (i, v) in m.iter_mut().enumerate() {
            *v = self.f32(8 + 4 * i)?;
        }
        Ok(Transform { m })
    }
}

/// The records of a file, checked against its length.
fn records(file: &[u8]) -> Result<Vec<Record<'_>>, EmfError> {
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < file.len() {
        let index = out.len();
        let head = file.get(offset..offset + 8).ok_or(EmfError::Truncated)?;
        let kind = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
        let size = u32::from_le_bytes([head[4], head[5], head[6], head[7]]);
        let bad = EmfError::BadRecord {
            index,
            offset,
            size,
        };
        if index == 0 && kind != HEADER {
            return Err(EmfError::NotMetafile);
        }
        if size < 8 || size % 4 != 0 {
            return Err(bad);
        }
        let data = file.get(offset..offset + size as usize).ok_or(bad)?;
        if index == 0 && (size < 88 || data.get(40..44) != Some(b" EMF")) {
            return Err(EmfError::NotMetafile);
        }
        out.push(Record { index, kind, data });
        offset += size as usize;
        if kind == EOF {
            return Ok(out);
        }
    }
    if out.is_empty() {
        Err(EmfError::NotMetafile)
    } else {
        Err(EmfError::Truncated)
    }
}

/// Reads a metafile.
///
/// # Errors
/// [`EmfError`] when the file is not a metafile, or a record does not fit
/// in it or holds a field that does not fit in the record.
pub fn read(file: &[u8]) -> Result<Metafile, EmfError> {
    let mut reader = Reader::default();
    for record in records(file)? {
        reader.record(&record)?;
    }
    Ok(reader.out)
}

/// The state of the reading.
#[derive(Debug)]
struct Reader {
    state: State,
    saved: Vec<State>,
    pens: HashMap<u32, Pen>,
    out: Metafile,
}

impl Default for Reader {
    fn default() -> Self {
        Reader {
            state: State {
                transform: Transform::IDENTITY,
                pen: Pen {
                    colour: 0,
                    width: 1,
                    style: 0,
                },
                align: 0,
            },
            saved: Vec::new(),
            pens: HashMap::new(),
            out: Metafile::default(),
        }
    }
}

impl Reader {
    fn record(&mut self, r: &Record) -> Result<(), EmfError> {
        match r.kind {
            SET_WORLD_TRANSFORM => self.state.transform = r.transform()?,
            MODIFY_WORLD_TRANSFORM => self.modify(r)?,
            EXT_CREATE_PEN => {
                let pen = Pen {
                    style: r.u32(28)?,
                    width: r.u32(32)?,
                    colour: bgr(r.u32(40)?),
                };
                self.pens.insert(r.u32(8)?, pen);
            }
            CREATE_PEN => {
                let pen = Pen {
                    style: r.u32(12)?,
                    width: r.i32(16)?.unsigned_abs(),
                    colour: bgr(r.u32(24)?),
                };
                self.pens.insert(r.u32(8)?, pen);
            }
            SELECT_OBJECT => self.select(r.u32(8)?),
            DELETE_OBJECT => {
                self.pens.remove(&r.u32(8)?);
            }
            SET_TEXT_ALIGN => self.state.align = r.u32(8)?,
            SAVE_DC => self.saved.push(self.state),
            RESTORE_DC => self.restore(r.i32(8)?),
            POLYLINE16 | POLYLINE_TO16 => self.polyline(r)?,
            EXT_TEXT_OUT_W => self.text(r)?,
            STRETCH_DIBITS => self.picture(r)?,
            _ => {}
        }
        Ok(())
    }

    fn modify(&mut self, r: &Record) -> Result<(), EmfError> {
        let m = r.transform()?;
        let now = self.state.transform;
        self.state.transform = match r.u32(32)? {
            1 => Transform::IDENTITY,
            2 => m.then(&now),
            3 => now.then(&m),
            4 => m,
            _ => return Err(r.error("an unknown way to modify the transform")),
        };
        Ok(())
    }

    fn select(&mut self, index: u32) {
        if let Some(pen) = self.pens.get(&index) {
            self.state.pen = *pen;
        } else if index & STOCK != 0 {
            let stock = index & !STOCK;
            if let Some((_, colour)) = STOCK_PENS.iter().find(|(i, _)| *i == stock) {
                self.state.pen = Pen {
                    colour: *colour,
                    width: 1,
                    style: if stock == 8 { 5 } else { 0 },
                };
            }
        }
    }

    /// `EMR_RESTOREDC`: a negative number goes back that many saves, a
    /// positive one to that save, counting from the first.
    fn restore(&mut self, which: i32) {
        let keep = if which < 0 {
            self.saved.len().checked_sub(which.unsigned_abs() as usize)
        } else {
            (which as usize).checked_sub(1)
        };
        if let Some(keep) = keep.filter(|k| *k < self.saved.len()) {
            self.state = self.saved[keep];
            self.saved.truncate(keep);
        }
    }

    fn polyline(&mut self, r: &Record) -> Result<(), EmfError> {
        let count = r.u32(24)? as usize;
        if count > MOST {
            return Err(r.error("a polyline claims too many points"));
        }
        r.slice(28, 4 * count)?;
        let t = self.state.transform;
        let mut points = Vec::with_capacity(count);
        for i in 0..count {
            let x = f64::from(r.i16(28 + 4 * i)?);
            let y = f64::from(r.i16(30 + 4 * i)?);
            points.push(t.apply(x, y));
        }
        self.out.curves.push(Curve {
            pen: self.state.pen,
            points,
            joined: r.kind == POLYLINE_TO16,
        });
        Ok(())
    }

    fn text(&mut self, r: &Record) -> Result<(), EmfError> {
        let mode = r.u32(24)?;
        let x = f64::from(r.i32(36)?);
        let y = f64::from(r.i32(40)?);
        let chars = r.u32(44)? as usize;
        if chars > MOST {
            return Err(r.error("a text claims too many characters"));
        }
        let start = r.u32(48)? as usize;
        let units: Vec<u16> = r
            .slice(start, 2 * chars)?
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let dx = r.u32(72)? as usize;
        let width = if dx == 0 {
            0.0
        } else {
            r.slice(dx, 4 * chars)?
                .chunks_exact(4)
                .map(|c| f64::from(u32::from_le_bytes([c[0], c[1], c[2], c[3]])))
                .sum()
        };
        let at = if mode == GM_ADVANCED {
            self.state.transform.apply(x, y)
        } else {
            (x, y)
        };
        self.out.texts.push(Text {
            text: String::from_utf16_lossy(&units),
            at,
            width,
            align: self.state.align,
        });
        Ok(())
    }

    fn picture(&mut self, r: &Record) -> Result<(), EmfError> {
        let place = (f64::from(r.i32(24)?), f64::from(r.i32(28)?));
        let page = (f64::from(r.i32(72)?), f64::from(r.i32(76)?));
        let info = r.slice(r.u32(48)? as usize, r.u32(52)? as usize)?;
        let bits = r.slice(r.u32(56)? as usize, r.u32(60)? as usize)?;
        let mut picture = dib(info, bits).map_err(|what| r.error(what))?;
        let t = self.state.transform;
        let corner = t.apply(place.0, place.1);
        let far = t.apply(place.0 + page.0, place.1 + page.1);
        picture.at = (corner.0.min(far.0), corner.1.min(far.1));
        picture.size = ((far.0 - corner.0).abs(), (far.1 - corner.1).abs());
        self.out.pictures.push(picture);
        Ok(())
    }
}

/// `0x00BBGGRR` as `0xRRGGBB`.
fn bgr(colour: u32) -> u32 {
    let (r, g, b) = (colour & 0xff, (colour >> 8) & 0xff, (colour >> 16) & 0xff);
    (r << 16) | (g << 8) | b
}

fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The header of a device-independent bitmap, as far as it is read.
struct DibHeader {
    width: usize,
    height: usize,
    top_down: bool,
    bits: u16,
    compression: u32,
    colours_used: usize,
    size: usize,
}

fn dib_header(info: &[u8]) -> Result<DibHeader, &'static str> {
    let short = "the bitmap header is too short";
    let size = le32(info, 0).ok_or(short)? as usize;
    let width = le32(info, 4).ok_or(short)? as i32;
    let height = le32(info, 8).ok_or(short)? as i32;
    let bits = u16::from_le_bytes(
        info.get(14..16)
            .ok_or(short)?
            .try_into()
            .map_err(|_| short)?,
    );
    let compression = le32(info, 16).ok_or(short)?;
    let colours_used = le32(info, 32).ok_or(short)? as usize;
    if size < 40 || width <= 0 || height == 0 {
        return Err("the bitmap has no pixels");
    }
    let (w, h) = (width as usize, height.unsigned_abs() as usize);
    if w.checked_mul(h).is_none_or(|n| n > MOST) {
        return Err("the bitmap claims too many pixels");
    }
    Ok(DibHeader {
        width: w,
        height: h,
        top_down: height < 0,
        bits,
        compression,
        colours_used,
        size,
    })
}

/// A device-independent bitmap: `info` is the header with its palette,
/// `bits` the rows of pixels.
fn dib(info: &[u8], bits: &[u8]) -> Result<Picture, &'static str> {
    let h = dib_header(info)?;
    let paletted = matches!(h.bits, 1 | 4 | 8);
    if !(paletted || matches!(h.bits, 16 | 24 | 32)) {
        return Err("a bitmap of an unknown depth");
    }
    if !matches!(h.compression, 0 | 3) {
        return Err("a compressed bitmap");
    }
    let entries = if paletted && h.colours_used == 0 {
        1 << h.bits
    } else if paletted {
        h.colours_used.min(1 << h.bits)
    } else {
        0
    };
    let palette: Vec<u32> = (0..entries)
        .map(|i| le32(info, h.size + 4 * i).map(|q| q & 0x00ff_ffff))
        .collect::<Option<_>>()
        .ok_or("the palette lies outside the bitmap header")?;
    let masks = if h.compression == 3 {
        let m = |i: usize| le32(info, h.size + 4 * i).ok_or("the bit masks lie outside the header");
        Some([m(0)?, m(1)?, m(2)?])
    } else {
        None
    };
    let stride = (h.width * usize::from(h.bits)).div_ceil(32) * 4;
    if bits.len() < stride * h.height {
        return Err("the pixels lie outside the record");
    }
    let depth = match (h.bits, masks) {
        (16, Some(m)) => m.iter().map(|c| c.count_ones()).min().unwrap_or(0).min(8),
        (16, None) => 5,
        _ => 8,
    };
    let mut picture = Picture {
        at: (0.0, 0.0),
        size: (0.0, 0.0),
        width: h.width,
        height: h.height,
        palette,
        indices: Vec::new(),
        colours: Vec::with_capacity(h.width * h.height),
        depth,
    };
    for y in 0..h.height {
        let stored = if h.top_down { y } else { h.height - 1 - y };
        let row = &bits[stored * stride..(stored + 1) * stride];
        for x in 0..h.width {
            pixel(&mut picture, row, x, h.bits, masks)?;
        }
    }
    Ok(picture)
}

/// Adds pixel `x` of `row` to `picture`.
fn pixel(
    picture: &mut Picture,
    row: &[u8],
    x: usize,
    bits: u16,
    masks: Option<[u32; 3]>,
) -> Result<(), &'static str> {
    let colour = match bits {
        1 | 4 | 8 => {
            let per = 8 / usize::from(bits);
            let byte = row[x / per];
            let shift = 8 - usize::from(bits) * (x % per + 1);
            let index = (byte >> shift) & ((1u16 << bits) - 1) as u8;
            picture.indices.push(index);
            *picture
                .palette
                .get(usize::from(index))
                .ok_or("a pixel points past the palette")?
        }
        16 => {
            let v = u32::from(u16::from_le_bytes([row[2 * x], row[2 * x + 1]]));
            let [r, g, b] = masks.unwrap_or([0x7c00, 0x03e0, 0x001f]);
            (channel(v, r) << 16) | (channel(v, g) << 8) | channel(v, b)
        }
        _ => {
            let n = usize::from(bits) / 8;
            let p = &row[n * x..n * x + 3];
            (u32::from(p[2]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[0])
        }
    };
    picture.colours.push(colour);
    Ok(())
}

/// The part of `value` under `mask` as the top bits of a byte: a colour
/// stored with five bits is the colour's byte with its three low bits
/// cleared, so a dithered stretch of it averages to the colour itself.
fn channel(value: u32, mask: u32) -> u32 {
    if mask == 0 {
        return 0;
    }
    let bits = (value & mask) >> mask.trailing_zeros();
    let width = mask.count_ones();
    if width >= 8 {
        bits >> (width - 8)
    } else {
        bits << (8 - width)
    }
}

/// One axis of a plot: data value against position on the page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    /// The value (or its decimal logarithm) at position 0.
    pub offset: f64,
    /// The change of the value (or of its logarithm) per device unit.
    pub slope: f64,
    /// Whether the axis is logarithmic.
    pub log: bool,
}

impl Scale {
    /// The value at position `p`.
    pub fn value(&self, p: f64) -> f64 {
        let v = self.offset + self.slope * p;
        if self.log { 10f64.powf(v) } else { v }
    }

    /// The position of `value`.
    pub fn position(&self, value: f64) -> f64 {
        let v = if self.log { value.log10() } else { value };
        (v - self.offset) / self.slope
    }
}

/// Why the axes of a plot could not be found.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AxesError {
    /// No black lines make a box.
    #[error("the plot has no box")]
    NoBox,
    /// An axis has fewer than two ticks with a label.
    #[error("the {0} axis has fewer than two labelled ticks")]
    TooFewLabels(&'static str),
}

/// The box of a plot, device units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// Left edge.
    pub left: f64,
    /// Top edge.
    pub top: f64,
    /// Right edge.
    pub right: f64,
    /// Bottom edge.
    pub bottom: f64,
}

/// The axes of a plot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Axes {
    /// Its box.
    pub frame: Frame,
    /// Across.
    pub x: Scale,
    /// Up.
    pub y: Scale,
}

/// An axis-aligned two-point black line.
#[derive(Debug, Clone, Copy)]
struct Segment {
    /// Where it lies across its direction.
    at: f64,
    /// Where it starts and ends along it.
    from: f64,
    to: f64,
}

impl Segment {
    fn len(&self) -> f64 {
        self.to - self.from
    }
}

/// The black two-point lines: horizontal ones and vertical ones.
fn segments(m: &Metafile) -> (Vec<Segment>, Vec<Segment>) {
    let (mut across, mut up) = (Vec::new(), Vec::new());
    for c in m.curves.iter().filter(|c| c.pen.colour == 0) {
        let [(x0, y0), (x1, y1)] = c.points[..] else {
            continue;
        };
        if (y0 - y1).abs() < 1e-6 && (x0 - x1).abs() > 1e-6 {
            across.push(Segment {
                at: y0,
                from: x0.min(x1),
                to: x0.max(x1),
            });
        } else if (x0 - x1).abs() < 1e-6 && (y0 - y1).abs() > 1e-6 {
            up.push(Segment {
                at: x0,
                from: y0.min(y1),
                to: y0.max(y1),
            });
        }
    }
    (across, up)
}

/// The box: of the horizontal lines nearly as long as the longest, the
/// one that makes the tallest box with the lines spanning the same
/// stretch gives the left and right edges and the bottom and top. (A
/// colour bar beside a map can be as wide as the map, but its box is
/// lower.)
fn frame(across: &[Segment], up: &[Segment]) -> Option<Frame> {
    let longest = across.iter().map(Segment::len).fold(0.0, f64::max);
    across
        .iter()
        .filter(|s| s.len() > 0.9 * longest)
        .filter_map(|s| frame_along(s, across, up))
        .max_by(|a, b| (a.bottom - a.top).total_cmp(&(b.bottom - b.top)))
}

/// The box whose bottom or top is `edge`: the horizontal lines that span
/// the same stretch give its bottom and top; a box drawn with one of them
/// only takes its top from the vertical lines standing on its bottom at
/// its left or right edge.
fn frame_along(edge: &Segment, across: &[Segment], up: &[Segment]) -> Option<Frame> {
    let tol = 0.01 * edge.len();
    let same = |s: &&Segment| (s.from - edge.from).abs() < tol && (s.to - edge.to).abs() < tol;
    let ys: Vec<f64> = across.iter().filter(same).map(|s| s.at).collect();
    let bottom = ys.iter().copied().fold(f64::MIN, f64::max);
    let mut top = ys.iter().copied().fold(f64::MAX, f64::min);
    if bottom - top < tol {
        top = up
            .iter()
            .filter(|s| {
                ((s.at - edge.from).abs() < tol || (s.at - edge.to).abs() < tol)
                    && (s.to - bottom).abs() < tol
            })
            .map(|s| s.from)
            .fold(top, f64::min);
    }
    (bottom - top > tol).then_some(Frame {
        left: edge.from,
        top,
        right: edge.to,
        bottom,
    })
}

/// The positions of the major ticks on one edge: lines across the edge at
/// `edge` (one end on it) within `span`, shorter than `short`, the longest
/// of them and those nearly as long.
fn major_ticks(lines: &[Segment], edge: f64, span: (f64, f64), short: f64) -> Vec<f64> {
    let tol = 0.002 * (span.1 - span.0).abs().max(1.0);
    let ticks: Vec<&Segment> = lines
        .iter()
        .filter(|s| (s.from - edge).abs() < tol || (s.to - edge).abs() < tol)
        .filter(|s| s.len() < short && s.at > span.0 - tol && s.at < span.1 + tol)
        .collect();
    let longest = ticks.iter().map(|s| s.len()).fold(0.0, f64::max);
    let mut at: Vec<f64> = ticks
        .iter()
        .filter(|s| s.len() > 0.75 * longest)
        .map(|s| s.at)
        .collect();
    at.sort_by(f64::total_cmp);
    at.dedup_by(|a, b| (*a - *b).abs() < tol);
    at
}

/// A label that may belong to an axis: its value and its distance from
/// the nearest major tick, as a share of the distance between two ticks.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    tick: f64,
    value: f64,
    off: f64,
}

/// The nearest of `ticks` to `p`, and the distance as a share of the
/// distance between two ticks.
fn nearest(ticks: &[f64], p: f64) -> Option<(f64, f64)> {
    let spacing = if ticks.len() > 1 {
        (ticks[ticks.len() - 1] - ticks[0]) / (ticks.len() - 1) as f64
    } else {
        f64::INFINITY
    };
    ticks
        .iter()
        .map(|t| (*t, (t - p).abs() / spacing))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// The least-squares line through `(position, value)` pairs: offset,
/// slope, and the largest miss as a share of the spread of the values.
fn line(pairs: &[(f64, f64)]) -> Option<(f64, f64, f64)> {
    let n = pairs.len() as f64;
    let (sp, sv) = pairs
        .iter()
        .fold((0.0, 0.0), |(a, b), (p, v)| (a + p, b + v));
    let (mp, mv) = (sp / n, sv / n);
    let spp: f64 = pairs.iter().map(|(p, _)| (p - mp).powi(2)).sum();
    let spv: f64 = pairs.iter().map(|(p, v)| (p - mp) * (v - mv)).sum();
    let slope = spv / spp;
    let offset = mv - slope * mp;
    let spread = pairs
        .iter()
        .map(|(_, v)| (v - mv).abs())
        .fold(0.0, f64::max);
    let worst = pairs
        .iter()
        .map(|(p, v)| (offset + slope * p - v).abs())
        .fold(0.0, f64::max);
    (slope.is_finite() && slope != 0.0 && spread > 0.0).then_some((offset, slope, worst / spread))
}

/// A share of the spread of the values below which labels lie exactly on
/// one line.
const EXACT: f64 = 1e-4;

/// When the labels do not lie on one line, those that do with the label
/// of the lowest value: the largest such set of three or more. Some axes
/// put their ticks at the times of the columns of a map and print them
/// rounded ("12" for 12.1 s); the lowest tick is the first column, whose
/// time is whole.
fn exact_with_lowest(pairs: &[(f64, f64)]) -> Option<Vec<(f64, f64)>> {
    let lowest = *pairs.iter().min_by(|a, b| a.1.total_cmp(&b.1))?;
    let spread = pairs.iter().map(|p| p.1).fold(f64::MIN, f64::max) - lowest.1;
    pairs
        .iter()
        .filter(|q| q.0 != lowest.0)
        .map(|q| {
            let slope = (q.1 - lowest.1) / (q.0 - lowest.0);
            pairs
                .iter()
                .copied()
                .filter(|p| (lowest.1 + slope * (p.0 - lowest.0) - p.1).abs() <= EXACT * spread)
                .collect::<Vec<_>>()
        })
        .max_by_key(Vec::len)
        .filter(|set| set.len() >= 3)
}

/// The straight line through the ticks at their labels' values, or the
/// logarithmic one when the values are positive and fit it much better.
fn fit(pairs: &[(f64, f64)]) -> Option<Scale> {
    let (mut offset, mut slope, miss) = line(pairs)?;
    if miss > EXACT
        && let Some((o, s, _)) = exact_with_lowest(pairs).and_then(|set| line(&set))
    {
        (offset, slope) = (o, s);
    }
    let linear = Scale {
        offset,
        slope,
        log: false,
    };
    if pairs.len() < 3 || pairs.iter().any(|(_, v)| *v <= 0.0) {
        return Some(linear);
    }
    let logs: Vec<(f64, f64)> = pairs.iter().map(|(p, v)| (*p, v.log10())).collect();
    match line(&logs) {
        Some((offset, slope, log_miss)) if miss > 1e-3 && log_miss < miss / 10.0 => Some(Scale {
            offset,
            slope,
            log: true,
        }),
        _ => Some(linear),
    }
}

/// The ticks with the label nearest to each: one value per tick.
fn labelled(candidates: &[Candidate]) -> Vec<(f64, f64)> {
    let mut best: Vec<Candidate> = Vec::new();
    for c in candidates.iter().filter(|c| c.off < 0.4) {
        match best.iter_mut().find(|b| b.tick == c.tick) {
            Some(b) if b.off > c.off => *b = *c,
            Some(_) => {}
            None => best.push(*c),
        }
    }
    best.iter().map(|c| (c.tick, c.value)).collect()
}

/// The box of a plot and the labelled ticks of its two axes: positions
/// across and values, then positions up and values.
type Labelled = (Frame, Vec<(f64, f64)>, Vec<(f64, f64)>);

/// The box of the plot a metafile holds and the labels of its ticks.
fn labelled_ticks(m: &Metafile) -> Result<Labelled, AxesError> {
    let (across, up) = segments(m);
    let frame = frame(&across, &up).ok_or(AxesError::NoBox)?;
    let short = 0.1 * (frame.right - frame.left).min(frame.bottom - frame.top);
    let x_ticks = major_ticks(&up, frame.bottom, (frame.left, frame.right), short);
    let y_ticks = major_ticks(&across, frame.left, (frame.top, frame.bottom), short);
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    for t in &m.texts {
        let Some(value) = t.number() else { continue };
        let (cx, cy) = t.centre();
        let below = cy > frame.bottom;
        let left = t.at.0 + t.width.max(1.0) <= frame.left + 0.002 * (frame.right - frame.left)
            || cx < frame.left;
        let as_x = nearest(&x_ticks, cx).filter(|_| below);
        let as_y = nearest(&y_ticks, cy).filter(|_| left && cy <= frame.bottom + short);
        match (as_x, as_y) {
            (Some(x), Some(y)) if x.1 <= y.1 => xs.push(candidate(x, value)),
            (_, Some(y)) => ys.push(candidate(y, value)),
            (Some(x), None) => xs.push(candidate(x, value)),
            (None, None) => {}
        }
    }
    Ok((frame, labelled(&xs), labelled(&ys)))
}

impl Axes {
    /// The axes of the plot a metafile holds.
    ///
    /// # Errors
    /// [`AxesError`] when there is no box or an axis has fewer than two
    /// labelled ticks.
    pub fn of(m: &Metafile) -> Result<Axes, AxesError> {
        let (frame, xs, ys) = labelled_ticks(m)?;
        Ok(Axes {
            frame,
            x: fit(&xs).ok_or(AxesError::TooFewLabels("x"))?,
            y: fit(&ys).ok_or(AxesError::TooFewLabels("y"))?,
        })
    }

    /// The box and the upright axis of a plot whose horizontal axis has no
    /// numbers (the Peak plot of the step response window).
    ///
    /// # Errors
    /// [`AxesError`] when there is no box or the upright axis has fewer
    /// than two labelled ticks.
    pub fn upright(m: &Metafile) -> Result<(Frame, Scale), AxesError> {
        let (frame, _, ys) = labelled_ticks(m)?;
        Ok((frame, fit(&ys).ok_or(AxesError::TooFewLabels("y"))?))
    }

    /// A point of the page in data units.
    pub fn data(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (self.x.value(x), self.y.value(y))
    }

    /// The points of a curve in data units.
    pub fn curve(&self, curve: &Curve) -> Vec<(f64, f64)> {
        curve.points.iter().map(|p| self.data(*p)).collect()
    }
}

fn candidate((tick, off): (f64, f64), value: f64) -> Candidate {
    Candidate { tick, value, off }
}

/// An RGB colour as three numbers 0 … 255.
type Rgb = [f64; 3];

fn rgb(c: u32) -> Rgb {
    [(c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff].map(f64::from)
}

/// The colour bar of a map: the two values at its ends and the path its
/// colours take from the lower end, each point with its colour step (0 …
/// 256 from the lower end).
///
/// A bar is a picture of a few hundred pixels whose colours may be stored
/// with fewer bits than the map's, and dithered: the path is the colour of
/// each line across the bar averaged over the bar's width, then over a few
/// neighbouring lines.
#[derive(Debug, Clone, PartialEq)]
pub struct ColourBar {
    /// The value at the lower end.
    pub low: f64,
    /// The value at the upper end.
    pub high: f64,
    /// The colours of the bar from its lower end, with their steps.
    pub path: Vec<(Rgb, f64)>,
}

impl ColourBar {
    /// The colour bar beside a map: `bar` is the picture of the bar; the
    /// numbers written beside its long side are its labels. Its colours
    /// are taken in order along the middle of the bar from its lower end;
    /// the `n` distinct ones are the steps `0, 255/(n−1), …, 255`.
    ///
    /// `None` when fewer than two labels stand beside it.
    pub fn of(m: &Metafile, bar: &Picture) -> Option<ColourBar> {
        let upright = bar.size.1 > bar.size.0;
        // along the bar, and across it
        let (from, length) = if upright {
            (bar.at.1, bar.size.1)
        } else {
            (bar.at.0, bar.size.0)
        };
        let (side0, side1) = if upright {
            (bar.at.0, bar.at.0 + bar.size.0)
        } else {
            (bar.at.1, bar.at.1 + bar.size.1)
        };
        // the labels stand close beside the bar and within its length
        let reach = (3.0 * (side1 - side0)).max(0.05 * length);
        let mut pairs: Vec<(f64, f64)> = m
            .texts
            .iter()
            .filter_map(|t| {
                let value = t.number()?;
                let (cx, cy) = t.centre();
                let (along, across) = if upright { (cy, cx) } else { (cx, cy) };
                let beside = (across > side1 && across < side1 + reach)
                    || (across < side0 && across > side0 - reach);
                let within = along > from - 0.015 * length && along < from + 1.015 * length;
                (beside && within).then_some((along, value))
            })
            .collect();
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let scale = fit(&pairs)?;
        let snap = |p: f64| {
            pairs
                .iter()
                .find(|(q, _)| (q - p).abs() < 0.03 * length)
                .map_or_else(|| scale.value(p), |(_, v)| *v)
        };
        let (a, b) = (snap(from), snap(from + length));
        let (low, high, low_first) = if a < b { (a, b, true) } else { (b, a, false) };
        // on the page the bar starts at its top (upright) or left; the
        // lower end is where the labels are lowest
        let mut lines = along_bar(bar, upright);
        if !low_first {
            lines.reverse();
        }
        let n = lines.len();
        let reach = n / 128;
        let path = (0..n)
            .map(|i| {
                let near = &lines[i.saturating_sub(reach)..(i + reach + 1).min(n)];
                let mean =
                    [0, 1, 2].map(|k| near.iter().map(|c| c[k]).sum::<f64>() / near.len() as f64);
                (mean, 256.0 * (i as f64 + 0.5) / n as f64)
            })
            .collect();
        Some(ColourBar { low, high, path })
    }

    /// The colour step of `colour`, 0 … 256 from the lower end: where on
    /// the path of the bar's colours it lies nearest.
    pub fn step(&self, colour: u32) -> f64 {
        self.step_rgb(rgb(colour))
    }

    /// The colour step of the colour `p` (three numbers 0 … 255, which may
    /// lie between whole ones: the mean colour of a few pixels), as
    /// [`ColourBar::step`].
    pub fn step_rgb(&self, p: [f64; 3]) -> f64 {
        let mut best = (f64::INFINITY, 0.0);
        for pair in self.path.windows(2) {
            let ((a, sa), (b, sb)) = (pair[0], pair[1]);
            let ab: f64 = (0..3).map(|k| (b[k] - a[k]).powi(2)).sum();
            let t = if ab > 0.0 {
                ((0..3).map(|k| (p[k] - a[k]) * (b[k] - a[k])).sum::<f64>() / ab).clamp(0.0, 1.0)
            } else {
                0.5
            };
            let d: f64 = (0..3)
                .map(|k| (a[k] + t * (b[k] - a[k]) - p[k]).powi(2))
                .sum();
            if d < best.0 {
                best = (d, sa + t * (sb - sa));
            }
        }
        match self.path[..] {
            [(_, only)] => only,
            _ => best.1,
        }
    }

    /// How many colour steps one level of a colour channel of `depth` bits
    /// spans along the bar, at least one: the levels of such a channel are
    /// `255 / (2^depth − 1)` apart, and the bar's colours move by the
    /// typical change of their fastest channel each step. A picture of
    /// 8-bit channels resolves every step; one of 16 bits (5 bits a
    /// channel) on the reference tool's colour map resolves three.
    pub fn steps_per_level(&self, depth: u32) -> f64 {
        let n = self.path.len();
        if n < 2 || depth == 0 || depth >= 8 {
            return 1.0;
        }
        let per_entry = 256.0 / n as f64;
        let mut moves: Vec<f64> = self
            .path
            .windows(2)
            .map(|p| {
                (0..3)
                    .map(|k| (p[1].0[k] - p[0].0[k]).abs())
                    .fold(0.0, f64::max)
            })
            .map(|m| m / per_entry)
            .filter(|m| *m > 1e-9)
            .collect();
        if moves.is_empty() {
            return 1.0;
        }
        moves.sort_by(f64::total_cmp);
        let typical = moves[moves.len() / 2];
        let level = 255.0 / f64::from((1u32 << depth) - 1);
        (level / typical).round().max(1.0)
    }

    /// The colour step of `value`, 0 … 256: its share of the way from the
    /// lower end to the upper one, those beyond the ends at the ends.
    pub fn step_of(&self, value: f64) -> f64 {
        let share = (value - self.low) / (self.high - self.low);
        (share * 256.0).clamp(0.0, 256.0)
    }
}
/// The mean colour of every line across a bar, from its top (upright)
/// or left end.
fn along_bar(bar: &Picture, upright: bool) -> Vec<Rgb> {
    let (lines, across) = if upright {
        (bar.height, bar.width)
    } else {
        (bar.width, bar.height)
    };
    (0..lines)
        .map(|i| {
            let colours: Vec<Rgb> = (0..across)
                .filter_map(|j| {
                    if upright {
                        bar.colour(j, i)
                    } else {
                        bar.colour(i, j)
                    }
                })
                .map(rgb)
                .collect();
            let n = colours.len().max(1) as f64;
            [0, 1, 2].map(|k| colours.iter().map(|c| c[k]).sum::<f64>() / n)
        })
        .collect()
}
