//! The metafile reader and the calibration of a plot's axes, on metafiles
//! written by the small writer of `support`.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]
#![allow(
    clippy::unreadable_literal,
    reason = "colours are written 0xRRGGBB, as a copy stores them"
)]

mod support;

use openpid_probe::emf::{self, Axes, AxesError, ColourBar, EmfError, Transform};
use support::{Plot, Writer, hot};

/// A metafile of each kind of record the reader keeps.
fn sample() -> Vec<u8> {
    let mut w = Writer::new();
    // a transform set, then one multiplied on the left: (x, y) → (2x + 5, 2y + 10)
    w.set_transform([1.0, 0.0, 0.0, 1.0, 5.0, 10.0]);
    w.modify_transform([2.0, 0.0, 0.0, 2.0, 0.0, 0.0], 2);
    let red = w.pen(0xff0033, 24, 0x10200);
    w.polyline(&[(0, 0), (10, 5), (20, -5)], false);
    // a dashed pen of the older kind, then the red one again
    let blue = w.old_pen(0x0000e6, 3);
    w.polyline(&[(1, 1), (2, 2)], true);
    w.select(red);
    w.delete(blue);
    w.select(blue);
    w.polyline(&[(3, 3), (4, 4), (5, 5)], false);
    // a saved state restored: the transform comes back to the identity
    w.set_transform([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    w.save();
    w.modify_transform([3.0, 0.0, 0.0, 3.0, 0.0, 0.0], 3);
    w.select(0x8000_0007);
    w.polyline(&[(1, 0), (2, 0), (3, 0)], false);
    w.restore(-1);
    w.polyline(&[(1, 0), (2, 0), (3, 0)], false);
    w.text_align(24);
    w.text("\u{2212}250", (40, 300), 15);
    // a picture of three columns and two rows, stored bottom-up, and the same
    // stored top-down
    let rows = vec![vec![0u8, 1, 2], vec![2, 1, 0]];
    w.picture8(
        (100, 50),
        (300, 20),
        &[0x000000, 0x808080, 0xffffff],
        &rows,
        false,
    );
    w.picture8(
        (100, 80),
        (300, 20),
        &[0x000000, 0x808080, 0xffffff],
        &rows,
        true,
    );
    w.picture16(
        (10, 10),
        (2, 2),
        &[vec![0xff0000, 0x00ff00], vec![0x0000ff, 0xffffff]],
    );
    // a record the reader passes over
    w.record(76, &[0; 92]);
    w.finish()
}

#[test]
fn every_kind_of_record_reads_back() {
    let m = emf::read(&sample()).unwrap();
    assert_eq!(m.curves.len(), 5);
    let first = &m.curves[0];
    assert_eq!(first.pen.colour, 0xff0033);
    assert_eq!(first.pen.width, 24);
    assert!(first.pen.solid());
    assert_eq!(first.points, vec![(5.0, 10.0), (25.0, 20.0), (45.0, 0.0)]);
    // the older pen, through the same transform
    assert_eq!(m.curves[1].pen.colour, 0x0000e6);
    assert_eq!(m.curves[1].pen.width, 3);
    assert_eq!(m.curves[1].points, vec![(7.0, 12.0), (9.0, 14.0)]);
    // a deleted pen cannot be selected: the red one stays
    assert_eq!(m.curves[2].pen.colour, 0xff0033);
    // the stock black pen, scaled by three on the right
    assert_eq!(m.curves[3].pen.colour, 0);
    assert_eq!(m.curves[3].points[2], (9.0, 0.0));
    // after the restore: the identity, and the red pen again
    assert_eq!(m.curves[4].points[2], (3.0, 0.0));
    assert_eq!(m.curves[4].pen.colour, 0xff0033);

    let t = &m.texts[0];
    assert_eq!(t.text, "\u{2212}250");
    assert_eq!(t.number(), Some(-250.0));
    assert_eq!((t.at, t.width, t.align), ((40.0, 300.0), 60.0, 24));
    let (cx, cy) = t.centre();
    assert_eq!(cx, 70.0);
    assert!((cy - (300.0 - 0.65 * 15.0)).abs() < 1e-9);

    assert_eq!(m.pictures.len(), 3);
    for p in &m.pictures[..2] {
        assert_eq!((p.width, p.height), (3, 2));
        assert_eq!(p.indices, vec![0, 1, 2, 2, 1, 0]);
        assert_eq!(p.colour(2, 0), Some(0xffffff));
        assert_eq!(p.colour(3, 0), None);
        assert_eq!(p.size, (300.0, 20.0));
    }
    assert_eq!(m.pictures[0].at, (100.0, 50.0));
    assert_eq!(m.pictures[0].centre_of(1, 1), (250.0, 65.0));
    // five bits a colour, as the top bits of a byte
    let bar = &m.pictures[2];
    assert!(bar.indices.is_empty());
    assert_eq!(bar.colours, vec![0xf80000, 0x00f800, 0x0000f8, 0xf8f8f8]);
}

#[test]
fn a_transform_composes_as_a_row_vector_times_the_matrix() {
    let shift = Transform {
        m: [1.0, 0.0, 0.0, 1.0, 3.0, 4.0],
    };
    let turn = Transform {
        m: [0.0, 1.0, -1.0, 0.0, 0.0, 0.0],
    };
    assert_eq!(shift.then(&turn).apply(1.0, 0.0), (-4.0, 4.0));
    assert_eq!(turn.then(&shift).apply(1.0, 0.0), (3.0, 5.0));
    assert_eq!(Transform::IDENTITY.apply(7.0, 8.0), (7.0, 8.0));
}

#[test]
fn a_file_that_is_not_a_whole_metafile_is_an_error() {
    let file = sample();
    assert_eq!(emf::read(&[]), Err(EmfError::NotMetafile));
    assert_eq!(emf::read(b"GIF89a\0\0\0\0\0\0"), Err(EmfError::NotMetafile));
    let mut not_emf = file.clone();
    not_emf[40] = b'X';
    assert_eq!(emf::read(&not_emf), Err(EmfError::NotMetafile));
    // every shorter file is refused
    for len in 0..file.len() {
        assert!(emf::read(&file[..len]).is_err(), "{len} bytes");
    }
    // a record that claims more than the file holds, or an odd size
    let mut long = file.clone();
    long[92..96].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        emf::read(&long),
        Err(EmfError::BadRecord { index: 1, .. })
    ));
    let mut odd = file.clone();
    odd[92..96].copy_from_slice(&7u32.to_le_bytes());
    assert!(matches!(emf::read(&odd), Err(EmfError::BadRecord { .. })));
}

/// The offset of the first record of `kind` after the header.
fn record_at(file: &[u8], kind: u32) -> usize {
    let mut at = 0;
    loop {
        let k = u32::from_le_bytes(file[at..at + 4].try_into().unwrap());
        if k == kind {
            return at;
        }
        at += u32::from_le_bytes(file[at + 4..at + 8].try_into().unwrap()) as usize;
    }
}

#[test]
fn a_field_outside_its_record_is_an_error() {
    let file = sample();
    let field = |kind: u32, offset: usize, value: u32| {
        let mut f = file.clone();
        let at = record_at(&f, kind) + offset;
        f[at..at + 4].copy_from_slice(&value.to_le_bytes());
        emf::read(&f)
    };
    // a polyline of more points than its record
    assert!(matches!(
        field(87, 24, 1000),
        Err(EmfError::BadField { kind: 87, .. })
    ));
    assert!(matches!(
        field(87, 24, u32::MAX),
        Err(EmfError::BadField { .. })
    ));
    // a text whose string or advances lie outside
    assert!(matches!(
        field(84, 48, 5000),
        Err(EmfError::BadField { kind: 84, .. })
    ));
    assert!(matches!(
        field(84, 72, 5000),
        Err(EmfError::BadField { kind: 84, .. })
    ));
    // a picture whose header, palette or pixels lie outside
    assert!(matches!(
        field(81, 48, 9999),
        Err(EmfError::BadField { kind: 81, .. })
    ));
    assert!(matches!(
        field(81, 80 + 32, 200),
        Err(EmfError::BadField { .. })
    ));
    assert!(matches!(field(81, 60, 2), Err(EmfError::BadField { .. })));
    assert!(matches!(
        field(81, 80 + 4, 0),
        Err(EmfError::BadField { .. })
    ));
    assert!(matches!(
        field(81, 80 + 4, 1 << 20),
        Err(EmfError::BadField { .. })
    ));
    // an unknown depth, a compressed picture
    assert!(matches!(
        field(81, 80 + 12, 7 << 16 | 1),
        Err(EmfError::BadField { .. })
    ));
    assert!(matches!(
        field(81, 80 + 16, 1),
        Err(EmfError::BadField { .. })
    ));
    // an unknown way to modify the transform, a transform that is not a number
    assert!(matches!(
        field(36, 32, 9),
        Err(EmfError::BadField { kind: 36, .. })
    ));
    assert!(matches!(
        field(35, 8, 0x7fc0_0000),
        Err(EmfError::BadField { kind: 35, .. })
    ));
}

#[test]
fn no_damage_makes_the_reader_panic() {
    let file = sample();
    // a fixed sequence of damages: every byte flipped in turn somewhere,
    // and runs of bytes overwritten
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    for _ in 0..3000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let mut f = file.clone();
        let at = (seed % f.len() as u64) as usize;
        let len = ((seed >> 32) % 8 + 1) as usize;
        for (i, b) in f.iter_mut().skip(at).take(len).enumerate() {
            *b = (seed >> (8 * (i % 8))) as u8;
        }
        let _ = emf::read(&f);
    }
}

/// A plot of time 0 … 20 against −500 … 500 with ticks every 2 and 250.
fn plot() -> Plot {
    Plot {
        frame: (100.0, 20.0, 2100.0, 520.0),
        x: (0.0, 20.0),
        y: (-500.0, 500.0),
    }
}

#[test]
fn the_axes_come_from_the_ticks_and_their_labels() {
    let mut w = Writer::new();
    let p = plot();
    let xs: Vec<f64> = (0..10).map(|i| f64::from(2 * i)).collect();
    p.axes(&mut w, &xs, &[-250.0, 0.0, 250.0]);
    p.curve(
        &mut w,
        0xff0033,
        0,
        &[(0.0, 0.0), (10.0, 100.0), (20.0, -400.0)],
    );
    let m = emf::read(&w.finish()).unwrap();
    let a = Axes::of(&m).unwrap();
    assert_eq!(
        (a.frame.left, a.frame.top, a.frame.right, a.frame.bottom),
        p.frame
    );
    assert!(!a.x.log && !a.y.log);
    assert!((a.x.value(100.0)).abs() < 1e-9);
    assert!((a.x.value(2100.0) - 20.0).abs() < 1e-9);
    assert!((a.y.value(520.0) + 500.0).abs() < 1e-9);
    assert!((a.y.position(500.0) - 20.0).abs() < 1e-9);
    let curve = m.curves.iter().find(|c| c.points.len() == 3).unwrap();
    let data = a.curve(curve);
    assert!((data[1].0 - 10.0).abs() < 1e-3 && (data[1].1 - 100.0).abs() < 0.3);
    let (x, y) = a.data((1100.0, 270.0));
    assert!((x - 10.0).abs() < 1e-9 && y.abs() < 1e-9);
}

#[test]
fn a_stray_zero_of_the_other_axis_goes_to_its_own_axis() {
    let mut w = Writer::new();
    let p = plot();
    let xs: Vec<f64> = (1..10).map(|i| f64::from(2 * i)).collect();
    p.axes(&mut w, &xs, &[-250.0, 0.0, 250.0]);
    // the time axis's 0 at the left end, below the box and as far left as
    // the labels of the other axis, beside a tick of its own
    w.modify_transform([0.125, 0.0, 0.0, 0.125, 0.0, 0.0], 2);
    w.polyline(&[(800, 4160), (800, 4360)], false);
    w.modify_transform([8.0, 0.0, 0.0, 8.0, 0.0, 0.0], 2);
    w.text("0", (60, 570), 15);
    // and a wrong number far from any tick, and words
    w.text("7", (1000, 900), 15);
    w.text("Time (s)", (1000, 600), 15);
    let m = emf::read(&w.finish()).unwrap();
    let a = Axes::of(&m).unwrap();
    assert!((a.x.value(100.0)).abs() < 1e-9, "{:?}", a.x);
    assert!((a.y.value(270.0)).abs() < 1e-9, "{:?}", a.y);
}

#[test]
fn labels_rounded_off_their_ticks_leave_the_exact_ones() {
    // ticks at 1, 4, 7, 10 and at 12.1, 15.1, … labelled 12, 15, …: as on
    // the time axis of the time map, whose ticks stand at its columns
    let mut w = Writer::new();
    // 80 device units a second: every tick on a whole unit
    let p = Plot {
        frame: (100.0, 20.0, 2320.0, 520.0),
        x: (0.925, 28.675),
        y: (0.0, 1000.0),
    };
    p.axes(&mut w, &[], &[0.0, 500.0, 1000.0]);
    w.modify_transform([0.125, 0.0, 0.0, 0.125, 0.0, 0.0], 2);
    for t in [1.0, 4.0, 7.0, 10.0, 12.1, 15.1, 18.1, 21.1, 24.1, 27.1] {
        let (px, _) = p.page((t, 0.0));
        let x = (px * 8.0).round() as i16;
        w.polyline(&[(x, 4160), (x, 4360)], false);
    }
    w.modify_transform([8.0, 0.0, 0.0, 8.0, 0.0, 0.0], 2);
    for t in [1.0f64, 4.0, 7.0, 10.0, 12.1, 15.1, 18.1, 21.1, 24.1, 27.1] {
        let (px, _) = p.page((t, 0.0));
        let label = format!("{:.0}", t.floor());
        let width = 15 * label.len() as i32;
        w.text(&label, (px.round() as i32 - width / 2, 570), 15);
    }
    let m = emf::read(&w.finish()).unwrap();
    let a = Axes::of(&m).unwrap();
    assert!((a.x.value(100.0) - 0.925).abs() < 0.002, "{:?}", a.x);
    assert!((a.x.value(2320.0) - 28.675).abs() < 0.002, "{:?}", a.x);
}

#[test]
fn a_logarithmic_axis_is_told_from_its_labels() {
    let mut w = Writer::new();
    let p = Plot {
        frame: (100.0, 20.0, 2500.0, 520.0),
        x: (0.0, 3.0),
        y: (0.0, 1.0),
    };
    // decades drawn evenly: the labels 1, 10, 100, 1000
    p.axes(&mut w, &[], &[0.0, 0.5, 1.0]);
    w.modify_transform([0.125, 0.0, 0.0, 0.125, 0.0, 0.0], 2);
    for d in 0..4 {
        let (px, _) = p.page((f64::from(d), 0.0));
        let x = (px * 8.0).round() as i16;
        w.polyline(&[(x, 4160), (x, 4360)], false);
    }
    w.modify_transform([8.0, 0.0, 0.0, 8.0, 0.0, 0.0], 2);
    for d in 0..4 {
        let (px, _) = p.page((f64::from(d), 0.0));
        let label = format!("{}", 10i32.pow(d));
        let width = 15 * label.len() as i32;
        w.text(&label, (px.round() as i32 - width / 2, 570), 15);
    }
    let m = emf::read(&w.finish()).unwrap();
    let a = Axes::of(&m).unwrap();
    assert!(a.x.log);
    assert!((a.x.value(1300.0) - 10f64.powf(1.5)).abs() < 1e-6);
    assert!((a.x.position(100.0) - p.page((2.0, 0.0)).0).abs() < 1e-6);
}

#[test]
fn a_plot_without_box_or_labels_has_no_axes() {
    let mut w = Writer::new();
    w.pen(0, 8, 0);
    w.polyline(&[(0, 0), (0, 0)], false);
    let m = emf::read(&w.finish()).unwrap();
    assert_eq!(Axes::of(&m), Err(AxesError::NoBox));
    let mut w = Writer::new();
    plot().axes(&mut w, &[4.0], &[0.0, 250.0]);
    let m = emf::read(&w.finish()).unwrap();
    assert_eq!(Axes::of(&m), Err(AxesError::TooFewLabels("x")));
    assert!(Axes::upright(&m).is_ok());
    let mut w = Writer::new();
    plot().axes(&mut w, &[4.0, 8.0], &[0.0]);
    let m = emf::read(&w.finish()).unwrap();
    assert_eq!(Axes::of(&m), Err(AxesError::TooFewLabels("y")));
    assert_eq!(
        Axes::upright(&m).map(|_| ()),
        Err(AxesError::TooFewLabels("y"))
    );
}

/// A bar of `steps` colours beside a box, labelled at its ends.
fn bar(w: &mut Writer, upright: bool, labels: &[(f64, &str)]) -> (i32, i32, i32, i32) {
    let (x, y, width, height) = if upright {
        (2200, 20, 30, 512)
    } else {
        (100, 600, 512, 30)
    };
    let rows: Vec<Vec<u32>> = if upright {
        (0..height)
            .map(|r| vec![hot((height - 1 - r) as usize / 2); width as usize])
            .collect()
    } else {
        (0..height)
            .map(|_| (0..width).map(|c| hot(c as usize / 2)).collect())
            .collect()
    };
    w.picture16((x, y), (width, height), &rows);
    for (share, label) in labels {
        let n = label.chars().count() as i32;
        if upright {
            let py = f64::from(y + height) - share * f64::from(height);
            w.text(label, (x + width + 10, (py + 9.75).round() as i32), 15);
        } else {
            let px = f64::from(x) + share * f64::from(width);
            w.text(label, (px.round() as i32 - 15 * n / 2, y - 10), 15);
        }
    }
    (x, y, width, height)
}

#[test]
fn a_colour_bar_gives_the_step_of_every_colour_and_value() {
    for upright in [true, false] {
        let mut w = Writer::new();
        w.text_align(24);
        bar(&mut w, upright, &[(0.0, "-30"), (0.5, "-10"), (1.0, "10")]);
        let m = emf::read(&w.finish()).unwrap();
        let b = ColourBar::of(&m, &m.pictures[0]).unwrap();
        assert_eq!((b.low, b.high), (-30.0, 10.0), "upright {upright}");
        // the colours of the map are exact; the bar's have five bits
        for s in [3usize, 40, 100, 128, 200, 250] {
            let step = b.step(hot(s));
            assert!((step - (s as f64 + 0.5)).abs() < 1.6, "step {s}: {step}");
        }
        assert_eq!(b.step_of(-30.0), 0.0);
        assert_eq!(b.step_of(-10.0), 128.0);
        assert_eq!(b.step_of(50.0), 256.0);
    }
}

#[test]
fn a_bar_without_labels_is_none() {
    let mut w = Writer::new();
    bar(&mut w, true, &[(0.5, "0")]);
    let m = emf::read(&w.finish()).unwrap();
    assert!(ColourBar::of(&m, &m.pictures[0]).is_none());
}
