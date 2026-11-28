//! The copies of the maps of frequency against throttle and against time.
//!
//! A map is a picture of the matrix stretched over the box of its plot, so
//! a pixel shows the cell its place in the picture falls in: the cells are
//! placed by pixel index, along each side as many as the picture spans,
//! which its axes tell. Along the frequency the picture runs from the
//! middle of the first row to the middle of the last (the first and the
//! last row half as high as the others); along the throttle it holds whole
//! columns; the time window holds one column more than `OpenPID`'s map,
//! its second drawn twice (decision D-006), and its first two columns are
//! left out. Only a pixel in the middle of its cell is compared: one on
//! the edge of two may show either. A picture of 16 bits is dithered: the
//! colour of a cell is the mean of its pixels, and a level of its colour
//! channels spans a few colour steps of the bar.

use super::{
    BOTH_STRETCHES, Copy, Held, Outcome, Source, Stretch, axis_of, limit, not_compared, number,
    on_stretches, results_name, tokens,
};
use crate::capture::Window;
use crate::emf::{self, Axes, ColourBar, Metafile, Picture};
use openpid_analysis::Grid2D;
use openpid_core::Axis;
use std::collections::HashMap;
use std::fmt::Write as _;

/// A map as compared: columns, rows, values, and the numbers it prints.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Grid {
    x: Vec<f64>,
    y: Vec<f64>,
    z: Vec<f64>,
    /// Whether `z` is in decibels.
    decibels: bool,
    /// The mean and the peak `OpenPID` prints on the map, when given.
    printed: Option<(f64, f64)>,
}

impl Grid {
    /// The map of an analysis; its label says whether it is in decibels.
    pub(super) fn of(g: &Grid2D) -> Grid {
        Grid {
            x: g.x.clone(),
            y: g.y.clone(),
            z: g.z.clone(),
            decibels: g.z_label.contains("dB"),
            printed: None,
        }
    }

    /// The map of a results file.
    pub(super) fn given(m: &super::MapIn) -> Grid {
        Grid {
            x: m.x.clone(),
            y: m.y.clone(),
            z: m.z.clone(),
            decibels: m.decibels,
            printed: m.mean.zip(m.peak),
        }
    }

    /// The value of cell `i` on a colour bar in decibels (`decibels`) or
    /// not: a map of amplitudes is taken to decibels as `20·log10`.
    fn value(&self, i: usize, decibels: bool) -> f64 {
        let z = self.z[i];
        if decibels && !self.decibels {
            20.0 * z.max(1e-12).log10()
        } else {
            z
        }
    }

    /// The mean and the peak the map prints: `OpenPID`'s own when given,
    /// otherwise by the reference tool's rule, which `OpenPID` prints since
    /// decision D-005 (`docs/calibration/throttle.md`): over every column
    /// and the rows from a tenth of the rows up, `rows / 10 − 1`.
    fn printed(&self) -> (f64, f64) {
        if let Some(given) = self.printed {
            return given;
        }
        let nx = self.x.len().max(1);
        let rows = self.z.len() / nx;
        let first = (rows / 10).saturating_sub(1);
        let cells = &self.z[(first * nx).min(self.z.len())..];
        if cells.is_empty() {
            return (0.0, 0.0);
        }
        (
            cells.iter().sum::<f64>() / cells.len() as f64,
            cells.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        )
    }
}

/// The channels of a map copy by their names in the files: the unfiltered
/// and the filtered gyro, and from round 18 on the D term, the PID error
/// and the setpoint of the time window's list; each with the name
/// `OpenPID`'s results give it.
const MAP_CHANNELS: [(&str, &str); 5] = [
    ("prefilt", "gyro_unfiltered"),
    ("gyro", "gyro"),
    ("dterm", "dterm"),
    ("piderror", "piderror"),
    ("setpoint", "setpoint"),
];

/// The axis, channel (as the results name it) and setting of a map copy:
/// `throttle-<setting>-<axis>-<channel>` or `time-<channel>-<setting>-<axis>`.
fn map_parts(rest: &str, window: Window) -> Option<(Axis, &'static str, String)> {
    let (words, stage, axis) = if window == Window::Time {
        let words = tokens(rest.strip_prefix("time-")?);
        let (stage, axis) = (*words.first()?, *words.last()?);
        (
            words[1..words.len().saturating_sub(1).max(1)].to_vec(),
            stage,
            axis,
        )
    } else {
        let words = tokens(rest.strip_prefix("throttle-")?);
        let n = words.len();
        let (axis, stage) = (*words.get(n.checked_sub(2)?)?, *words.last()?);
        (words[..n - 2].to_vec(), stage, axis)
    };
    let (_, channel) = MAP_CHANNELS.iter().find(|(name, _)| *name == stage)?;
    Some((axis_of(axis)?, channel, words.join("-")))
}

/// Whether a map setting only changes the view: the view below 100 Hz, the
/// ends of the colour bar.
fn view_only(setting: &str) -> bool {
    setting.is_empty()
        || setting == "sub100"
        || ["scale-", "zmin-", "zmax-"]
            .iter()
            .any(|p| setting.starts_with(p))
}

/// What `compare` reads off the name of a map copy, when it compares it.
#[cfg(test)]
pub(super) fn parsed(rest: &str, window: Window) -> Option<String> {
    let (axis, channel, setting) = map_parts(rest, window)?;
    view_only(&setting).then(|| format!("{} {channel}", axis.name()))
}

/// The picture of the map (the largest) and of its colour bar (the
/// largest other one).
fn map_pictures(m: &Metafile) -> Option<(&Picture, Option<&Picture>)> {
    let area = |p: &&Picture| p.width * p.height;
    let map = m.pictures.iter().max_by_key(area)?;
    let bar = m
        .pictures
        .iter()
        .filter(|p| !std::ptr::eq(*p, map))
        .max_by_key(area);
    Some((map, bar))
}

/// The colour bar of a map copy, or, for a copy without one, that of the
/// copy `sibling` (the throttle window draws one bar, above the roll
/// plot, for the three maps).
fn bar_of(m: &Metafile, sibling: Option<&Metafile>) -> Result<ColourBar, String> {
    let own = map_pictures(m).and_then(|(_, bar)| bar);
    match (own, sibling) {
        (Some(bar), _) => {
            ColourBar::of(m, bar).ok_or_else(|| "the colour bar has no labels".into())
        }
        (None, Some(other)) => {
            bar_of(other, None).map_err(|e| format!("the roll plot's copy: {e}"))
        }
        (None, None) => {
            Err("the copy has no colour bar, and no copy of the roll plot has one".into())
        }
    }
}

/// The place of the picture on the page, on the box of the plot where it
/// fills it: a picture is placed on whole device units, the box to a
/// fraction of one, and a map fills its box exactly. Left, top, width,
/// height.
fn on_frame(picture: &Picture, f: &emf::Frame) -> [f64; 4] {
    let mut place = [picture.at.0, picture.at.1, picture.size.0, picture.size.1];
    let fills = |at: f64, size: f64, from: f64, to: f64| {
        (at - from).abs() <= 1.0 && (at + size - to).abs() <= 1.0
    };
    if fills(picture.at.0, picture.size.0, f.left, f.right) {
        (place[0], place[2]) = (f.left, f.right - f.left);
    }
    if fills(picture.at.1, picture.size.1, f.top, f.bottom) {
        (place[1], place[3]) = (f.top, f.bottom - f.top);
    }
    place
}

/// How the cells of a map lie along one side of its picture.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Along {
    /// The cells the picture spans.
    cells: f64,
    /// Where the low end of the picture lies, in cells: 0 at the low edge
    /// of the first cell, ½ at its middle.
    start: f64,
    /// Pixels along the side.
    pixels: usize,
    /// The first pixel is at the high end of the map's values.
    reversed: bool,
    /// The cells below this one are left out.
    kept_from: usize,
    /// How the cells lie, for the report.
    kind: Layout,
}

/// How the cells of a map lie along a side of its picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// As many as the map has, whole.
    Whole,
    /// From the middle of the first to the middle of the last.
    Halved,
    /// One more, the second drawn twice (the time window, D-006).
    Extra,
    /// Part of the map, placed by its values.
    Part,
}

/// How far the span of a picture its axes give may lie from a whole layout
/// of the map's cells, cells: the axes of a copy are calibrated from its
/// tick labels to about a pixel, and a cell may be less than two.
const LAYOUT_REACH: f64 = 2.0;

impl Along {
    /// The ways the cells of a map `values` (their middles, evenly apart)
    /// may lie along a side of a picture of `pixels` pixels whose ends lie
    /// at the values `ends` (first pixel, last pixel), as its axes
    /// calibrate them: as many cells as the map has, whole; one less, the
    /// first and the last halved (a picture from the middle of the first
    /// cell to the middle of the last); and where `extra` allows it, one
    /// more (the time window's second column drawn twice: its first two
    /// columns are left out, D-006). The axes cannot always tell these
    /// apart where a cell is a pixel or two, so each one within
    /// [`LAYOUT_REACH`] cells of what they say is a candidate; when none
    /// is, the picture is part of the map, placed by its values.
    fn candidates(values: &[f64], ends: (f64, f64), pixels: usize, extra: bool) -> Vec<Along> {
        let n = values.len();
        let (Some(first), Some(last)) = (values.first(), values.last()) else {
            return Vec::new();
        };
        let step = (last - first) / (n.max(2) - 1) as f64;
        if n < 2 || step <= 0.0 || pixels == 0 {
            return Vec::new();
        }
        let (low, high) = (ends.0.min(ends.1), ends.0.max(ends.1));
        let span = (high - low) / step;
        let count = n as f64;
        let along = |(cells, start, kind): (f64, f64, Layout)| Along {
            cells,
            start,
            pixels,
            reversed: ends.0 > ends.1,
            kept_from: usize::from(kind == Layout::Extra),
            kind,
        };
        let mut out: Vec<Along> = [
            (count, 0.0, Layout::Whole),
            (count - 1.0, 0.5, Layout::Halved),
            (count + 1.0, -1.0, Layout::Extra),
        ]
        .into_iter()
        .filter(|(cells, _, kind)| {
            (extra || *kind != Layout::Extra) && (cells - span).abs() <= LAYOUT_REACH
        })
        .map(along)
        .collect();
        if out.is_empty() {
            out.push(along((span, (low - first) / step + 0.5, Layout::Part)));
        }
        out
    }

    /// The layout, for the report.
    fn layout(&self, of: &str) -> String {
        match self.kind {
            Layout::Whole => format!("whole {of}"),
            Layout::Halved => format!("{of} from the middle of the first to the last"),
            Layout::Extra => format!("one {of} more, the first two left out (D-006)"),
            Layout::Part => format!("part of the map's {of}"),
        }
    }

    /// The cell pixel `p` shows, when the pixel lies in its middle: its
    /// edges half a pixel from its centre, and at least 0.15 of a cell
    /// from the cell's edges. Where a pixel is as wide as a cell or wider,
    /// no pixel lies inside one, and every pixel counts with the cell under
    /// its centre.
    fn cell(&self, p: usize) -> Option<usize> {
        let q = if self.reversed {
            self.pixels.checked_sub(p + 1)?
        } else {
            p
        };
        let per_pixel = self.cells / self.pixels as f64;
        let pos = self.start + (q as f64 + 0.5) * per_pixel;
        let k = pos.floor();
        let frac = pos - k;
        let margin = (0.5 * per_pixel).max(0.15);
        let middle = margin >= 0.45 || (frac > margin && frac < 1.0 - margin);
        (k >= self.kept_from as f64 && middle).then_some(k as usize)
    }

    /// The cell of every pixel along the side.
    fn cells(&self, n: usize) -> Vec<Option<usize>> {
        (0..self.pixels)
            .map(|p| self.cell(p).filter(|k| *k < n))
            .collect()
    }
}

/// A map held against a copy: the share that agrees, of how many pixels
/// or cells, how the cells lie, and the printed numbers with `OpenPID`'s.
struct HeldMap {
    share: f64,
    counted: usize,
    what: &'static str,
    level: f64,
    layout: String,
    printed: Vec<(&'static str, String, f64, bool)>,
}

/// How a picture is read against a map.
struct Reading<'a> {
    picture: &'a Picture,
    bar: &'a ColourBar,
    /// The cell of every column and of every row of pixels.
    columns: Vec<Option<usize>>,
    rows: Vec<Option<usize>>,
}

impl Reading<'_> {
    /// `OpenPID`'s colour step of cell `(column, row)`.
    fn ours(&self, grid: &Grid, (c, r): (usize, usize)) -> f64 {
        let i = r * grid.x.len() + c;
        let decibels = self.bar.low < 0.0;
        self.bar.step_of(grid.value(i, decibels))
    }

    /// Pixel by pixel, for a picture with a palette: those within `level`
    /// steps and those counted.
    fn by_pixel(&self, grid: &Grid, level: f64) -> (usize, usize) {
        let steps = centred(
            self.picture
                .palette
                .iter()
                .map(|c| self.bar.step(*c))
                .collect(),
        );
        let whole = |s: f64| s.min(255.999).floor();
        let (mut within, mut counted) = (0, 0);
        for (y, row) in self.rows.iter().enumerate() {
            let Some(r) = row else { continue };
            for (x, column) in self.columns.iter().enumerate() {
                let Some(c) = column else { continue };
                let index = self.picture.indices[y * self.picture.width + x];
                let drawn = steps.get(usize::from(index)).copied().unwrap_or(0.0);
                if (whole(drawn) - whole(self.ours(grid, (*c, *r)))).abs() <= level {
                    within += 1;
                }
                counted += 1;
            }
        }
        (within, counted)
    }

    /// Cell by cell, for a picture without a palette: the mean colour of
    /// the pixels in the middle of each cell.
    fn by_cell(&self, grid: &Grid, level: f64) -> (usize, usize) {
        let mut sums: HashMap<(usize, usize), ([f64; 3], usize)> = HashMap::new();
        for (y, row) in self.rows.iter().enumerate() {
            let Some(r) = row else { continue };
            for (x, column) in self.columns.iter().enumerate() {
                let (Some(c), Some(colour)) = (column, self.picture.colour(x, y)) else {
                    continue;
                };
                let rgb = [(colour >> 16) & 0xff, (colour >> 8) & 0xff, colour & 0xff];
                let (sum, n) = sums.entry((*c, *r)).or_insert(([0.0; 3], 0));
                for (s, v) in sum.iter_mut().zip(rgb) {
                    *s += f64::from(v);
                }
                *n += 1;
            }
        }
        let within = sums
            .iter()
            .filter(|(cell, (sum, n))| {
                let mean = sum.map(|s| s / *n as f64);
                let drawn = self.bar.step_rgb(mean);
                (drawn - self.ours(grid, **cell)).abs() <= level
            })
            .count();
        (within, sums.len())
    }
}

/// The colour steps of the colours of a palette, each colour put in the
/// middle of its step. A palette holds colours of the colour map exactly,
/// and each is the middle of a step of the bar; the bar is read from a
/// picture of fewer bits, whose colours lie off the exact ones by a part
/// of a step the same way all along. That part is the mean of where the
/// palette's colours fall within their steps, taken round the circle of a
/// step, less one half.
fn centred(steps: Vec<f64>) -> Vec<f64> {
    let (sin, cos) = steps.iter().fold((0.0, 0.0), |(s, c), v| {
        let a = std::f64::consts::TAU * (v - v.floor());
        (s + a.sin(), c + a.cos())
    });
    if steps.is_empty() || (sin * sin + cos * cos).sqrt() < 0.5 * steps.len() as f64 {
        // the colours do not fall at one place within their steps
        return steps;
    }
    let mean = sin.atan2(cos) / std::f64::consts::TAU;
    let shift = 0.5 - mean.rem_euclid(1.0);
    steps.into_iter().map(|v| v + shift).collect()
}

/// The numbers a map copy prints (`mean=…`, `peak=…`) with `OpenPID`'s:
/// the name, the text, `OpenPID`'s number and whether they agree to the
/// last printed digit.
fn printed(m: &Metafile, grid: &Grid) -> Vec<(&'static str, String, f64, bool)> {
    let (mean, peak) = grid.printed();
    let mut out = Vec::new();
    for (name, ours) in [("mean", mean), ("peak", peak)] {
        let given = m.texts.iter().find_map(|t| {
            let text = t.text.trim().strip_prefix(&format!("{name}="))?.trim();
            let value: f64 = text.parse().ok()?;
            Some((text.to_string(), value))
        });
        if let Some((text, value)) = given {
            let digits = text.split_once('.').map_or(0, |(_, d)| d.len());
            let unit = 10f64.powi(-i32::try_from(digits).unwrap_or(0));
            let pass = (value - ours).abs() <= limit("map numbers") * unit * (1.0 + 1e-9);
            out.push((name, text, ours, pass));
        }
    }
    out
}

/// A copy held against `grid`.
fn hold_map(
    m: &Metafile,
    (scales, bar, picture): (&Axes, &ColourBar, &Picture),
    grid: &Grid,
    extra: bool,
) -> Result<HeldMap, String> {
    let [left, top, width, height] = on_frame(picture, &scales.frame);
    let across = (scales.x.value(left), scales.x.value(left + width));
    let up = (scales.y.value(top), scales.y.value(top + height));
    let columns = Along::candidates(&grid.x, across, picture.width, extra);
    let rows = Along::candidates(&grid.y, up, picture.height, false);
    if columns.is_empty() || rows.is_empty() {
        return Err("OpenPID's map has fewer than two columns or rows".into());
    }
    if grid.z.len() < grid.x.len() * grid.y.len() {
        return Err("OpenPID's map has fewer values than cells".into());
    }
    let paletted = !picture.indices.is_empty();
    let level = if paletted {
        1.0
    } else {
        bar.steps_per_level(picture.depth)
    };
    // the layout of the cells under which the copy agrees best, the first
    // of those that agree as well
    let (share, counted, ax, ay) = columns
        .iter()
        .flat_map(|ax| rows.iter().map(move |ay| (ax, ay)))
        .map(|(ax, ay)| {
            let (within, counted) = agreement((picture, bar), grid, (ax, ay), level);
            (within as f64 / counted.max(1) as f64, counted, ax, ay)
        })
        .filter(|(_, counted, _, _)| *counted > 0)
        .min_by(|a, b| b.0.total_cmp(&a.0))
        .ok_or("the copy and OpenPID's map share no cells")?;
    Ok(HeldMap {
        share,
        counted,
        what: if paletted { "pixels" } else { "cells" },
        level,
        layout: format!("{}, {}", ax.layout("columns"), ay.layout("rows")),
        printed: printed(m, grid),
    })
}

/// How many pixels of `picture` (cells, for a picture without a palette)
/// agree with `grid` to `level` colour steps, its cells laid out along the
/// columns and the rows as `ax` and `ay`; and how many are counted.
fn agreement(
    (picture, bar): (&Picture, &ColourBar),
    grid: &Grid,
    (ax, ay): (&Along, &Along),
    level: f64,
) -> (usize, usize) {
    let reading = Reading {
        picture,
        bar,
        columns: ax.cells(grid.x.len()),
        rows: ay.cells(grid.y.len()),
    };
    if picture.indices.is_empty() {
        reading.by_cell(grid, level)
    } else {
        reading.by_pixel(grid, level)
    }
}

impl HeldMap {
    fn held(&self) -> Held {
        let mut numbers = format!(
            "{:.2} % of {} {} within {} colour step{} ({})",
            100.0 * self.share,
            self.counted,
            self.what,
            self.level,
            if self.level == 1.0 { "" } else { "s" },
            self.layout
        );
        let mut pass = self.share >= limit("map");
        for (name, text, ours, agree) in &self.printed {
            let _ = write!(numbers, "; {name} copy {text}, OpenPID {}", number(*ours));
            pass &= agree;
        }
        Held { numbers, pass }
    }
}

pub(super) fn map(copy: &Copy, m: &Metafile, src: &mut Source) -> Result<Outcome, String> {
    let (Some((axis, channel, setting)), Some(probe)) =
        (map_parts(&copy.rest, copy.window), copy.probe)
    else {
        return Ok(not_compared(
            "the name does not say the log, the axis and the channel",
        ));
    };
    if !view_only(&setting) {
        return Ok(not_compared(format!(
            "the setting {setting} has no counterpart in OpenPID's results"
        )));
    }
    let scales = Axes::of(m).map_err(|e| e.to_string())?;
    let (picture, _) = map_pictures(m).ok_or("the copy has no map")?;
    let sibling = (axis != Axis::Roll)
        .then(|| src.sibling(&copy.name, axis))
        .flatten();
    let bar = bar_of(m, sibling.as_ref())?;
    let extra = copy.window == Window::Time;
    let which = (axis, channel, copy.window);
    let own = hold_map(
        m,
        (&scales, &bar, picture),
        &src.map(probe, which, Stretch::Own)?,
        extra,
    )?;
    let both = src.round >= BOTH_STRETCHES;
    let judged = on_stretches(own.held(), both, |stretch| {
        let grid = src.map(probe, which, stretch)?;
        hold_map(m, (&scales, &bar, picture), &grid, extra).map(|h| h.held())
    });
    let against = format!(
        "OpenPID's map of {} of {} on the copy's colour scale ({} … {}{}){}",
        results_name(channel),
        axis.name(),
        number(bar.low),
        number(bar.high),
        if bar.low < 0.0 { " dB" } else { "" },
        if picture.indices.is_empty() {
            format!(", a picture of {} bits a colour channel", picture.depth)
        } else {
            String::new()
        }
    );
    Ok(Outcome::on(against, judged))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_colours_of_a_palette_are_put_in_the_middle_of_their_steps() {
        let read = centred(vec![7.96, 4.92, 64.01, 33.97, 12.03]);
        let want = [7.5, 4.5, 63.5, 33.5, 11.5];
        for (r, w) in read.iter().zip(want) {
            assert!((r - w).abs() < 0.12, "{read:?}");
        }
        // colours spread over their steps are left as read
        let spread = vec![1.1, 2.3, 3.5, 4.7, 5.9];
        assert_eq!(centred(spread.clone()), spread);
        assert!(centred(Vec::new()).is_empty());
    }

    #[test]
    fn cells_are_placed_by_pixel_and_counted_in_their_middle() {
        // ten columns over 100 pixels: whole cells, the middle 7 of each 10
        let values: Vec<f64> = (0..10).map(|i| 5.0 + 10.0 * f64::from(i)).collect();
        let only = |v: &[f64], ends: (f64, f64), pixels: usize, extra: bool, k: usize| {
            Along::candidates(v, ends, pixels, extra)[k]
        };
        let a = only(&values, (0.0, 100.0), 100, false, 0);
        assert_eq!((a.cells, a.start), (10.0, 0.0));
        assert_eq!(a.layout("columns"), "whole columns");
        let cells = a.cells(10);
        assert_eq!(cells[0], None);
        assert_eq!(cells[5], Some(0));
        assert_eq!(cells[95], Some(9));
        assert_eq!(cells.iter().filter(|c| c.is_some()).count(), 70);
        // rows from the middle of the first to the middle of the last,
        // the picture's top the highest
        let rows: Vec<f64> = (0..11).map(|i| 100.0 * f64::from(i)).collect();
        // the axes say 10.2 cells: both whole layouts are candidates
        assert_eq!(
            Along::candidates(&rows, (1015.0, -5.0), 200, false).len(),
            2
        );
        let a = only(&rows, (1000.0, 0.0), 200, false, 1);
        assert_eq!((a.cells, a.start, a.reversed), (10.0, 0.5, true));
        assert!(a.layout("rows").starts_with("rows from the middle"));
        // the top pixel in the half of the highest row the picture shows,
        // the bottom one in the lowest's; none on the edge of two rows
        assert_eq!(a.cell(0), Some(10));
        assert_eq!(a.cell(199), Some(0));
        assert_eq!(a.cell(190), None);
        assert_eq!(a.cell(180), Some(1));
        assert_eq!(a.cell(10), None);
        assert_eq!(a.cell(20), Some(9));
        // one column more, the first two left out (the time window)
        let a = Along::candidates(&values, (-10.0, 100.0), 110, true)
            .into_iter()
            .find(|a| a.kind == Layout::Extra)
            .unwrap();
        assert_eq!((a.cells, a.start, a.kept_from), (11.0, -1.0, 1));
        assert!(a.layout("columns").contains("D-006"));
        assert_eq!(a.cell(5), None);
        assert_eq!(a.cell(15), None);
        assert_eq!(a.cell(25), Some(1));
        assert_eq!(
            Along::candidates(&values, (-10.0, 100.0), 110, true).len(),
            3
        );
        assert_eq!(
            Along::candidates(&values, (-10.0, 100.0), 110, false).len(),
            2
        );
        // cells narrower than a pixel: every pixel counts
        let a = only(&values, (0.0, 100.0), 8, false, 0);
        assert!(a.cells(10).iter().all(Option::is_some));
        // a view of part of the map: placed by its values
        let part = Along::candidates(&values, (0.0, 50.0), 50, false);
        assert_eq!(part.len(), 1);
        assert_eq!((part[0].cells, part[0].start), (5.0, 0.0));
        assert!(part[0].layout("rows").starts_with("part"));
        assert!(Along::candidates(&values[..1], (0.0, 1.0), 10, false).is_empty());
    }
}
