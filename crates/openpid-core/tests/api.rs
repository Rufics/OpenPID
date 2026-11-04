//! Behavioural tests of the core API surface (units, settings, registries,
//! headers, series, timebase).
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use openpid_core::analysis::{AnalysisError, Grid2D};
use openpid_core::settings::{PerAxis, PidGains};
use openpid_core::{
    Axis, ChannelId, Confidence, FieldMapping, FirmwareInfo, FirmwareProfile, FirmwareVersion,
    FormatError, FormatRegistry, LogFile, LogFormat, OpenOptions, ProfileRegistry, RawHeaders,
    Series, Settings, Timebase, Unit,
};

#[test]
fn unit_symbols_and_display() {
    for (u, sym) in [
        (Unit::Raw, "raw"),
        (Unit::Unitless, ""),
        (Unit::Microseconds, "us"),
        (Unit::Seconds, "s"),
        (Unit::DegreesPerSecond, "deg/s"),
        (Unit::Degrees, "deg"),
        (Unit::Percent, "%"),
        (Unit::Rpm, "rpm"),
        (Unit::Volts, "V"),
        (Unit::Amperes, "A"),
        (Unit::MilliampHours, "mAh"),
        (Unit::Meters, "m"),
        (Unit::Centimeters, "cm"),
        (Unit::MetersPerSecond, "m/s"),
        (Unit::Gs, "g"),
        (Unit::Flags, "flags"),
        (Unit::Custom("ticks".into()), "ticks"),
    ] {
        assert_eq!(u.symbol(), sym);
        assert_eq!(u.to_string(), sym);
    }
}

#[test]
fn per_axis_and_grid() {
    let p = PerAxis {
        roll: 1,
        pitch: 2,
        yaw: 3,
    };
    assert_eq!(*p.get(Axis::Roll), 1);
    assert_eq!(*p.get(Axis::Pitch), 2);
    assert_eq!(*p.get(Axis::Yaw), 3);
    let g = Grid2D {
        x: vec![0.0, 1.0],
        y: vec![0.0, 1.0, 2.0],
        z: (0..6).map(f64::from).collect(),
        x_label: "x".into(),
        y_label: "y".into(),
        z_label: "z".into(),
    };
    assert_eq!(g.get(1, 2), 5.0);
    assert_eq!(g.get(0, 1), 2.0);
    let e = AnalysisError::WrongUnit(ChannelId::Throttle, "raw".into(), "%".into());
    assert!(e.to_string().contains("throttle"));
    assert!(
        AnalysisError::NotEnoughData("x".into())
            .to_string()
            .contains("not enough")
    );
    let s = Settings::default();
    assert!(s.pid.is_none() && s.extra.is_empty());
    let g = PidGains {
        p: 1.0,
        i: 2.0,
        d: 3.0,
        f: None,
        d_min: None,
        d_max: None,
    };
    assert_eq!(
        g,
        PidGains {
            p: 1.0,
            i: 2.0,
            d: 3.0,
            ..Default::default()
        }
    );
}

#[test]
fn raw_headers_and_field_mapping() {
    let h = RawHeaders(vec![
        ("a".into(), "1".into()),
        ("a".into(), "2".into()),
        ("list".into(), "1, 2.5,3".into()),
        ("bad".into(), "x".into()),
    ]);
    assert_eq!(h.get("a"), Some("1"));
    assert_eq!(h.first_of(&["zzz", "a"]), Some("1"));
    assert_eq!(h.get_f64("list"), None);
    assert_eq!(h.get_i64("a"), Some(1));
    assert_eq!(h.get_i64("bad"), None);
    assert_eq!(h.get_list("list"), Some(vec![1.0, 2.5, 3.0]));
    assert_eq!(h.get_list("bad"), None);
    assert_eq!(h.get_list("missing"), None);
    let m = FieldMapping {
        id: ChannelId::Rssi,
        unit: Unit::Percent,
        scale: 0.5,
        offset: -10.0,
    };
    assert_eq!(m.apply(30), 10.0);
    assert_eq!(FieldMapping::new(ChannelId::Rssi, Unit::Raw).apply(7), 7.0);
    assert_eq!(
        FieldMapping::scaled(ChannelId::Rssi, Unit::Raw, 2.0).apply(7),
        14.0
    );
}

#[derive(Debug)]
struct Dummy(&'static str, u8);

impl LogFormat for Dummy {
    fn id(&self) -> &'static str {
        self.0
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["dmy"]
    }
    fn sniff(&self, head: &[u8], _name: Option<&str>) -> Confidence {
        if head.starts_with(b"DUMMY") {
            Confidence(self.1)
        } else {
            Confidence::NONE
        }
    }
    fn open(&self, _data: &[u8], _options: &OpenOptions) -> Result<LogFile, FormatError> {
        Err(FormatError::Unsupported(self.0.to_string()))
    }
}

#[test]
fn format_registry_detects_best_match() {
    let mut r = FormatRegistry::new();
    r.register(Box::new(Dummy("weak", 30)))
        .register(Box::new(Dummy("strong", 100)));
    assert_eq!(r.iter().count(), 2);
    assert_eq!(r.get("weak").map(LogFormat::id), Some("weak"));
    assert!(r.get("none").is_none());
    assert_eq!(
        r.detect(b"DUMMY data", None).map(LogFormat::id),
        Some("strong")
    );
    assert!(r.detect(b"other", None).is_none());
    assert!(matches!(
        r.open(b"other", None, &OpenOptions::default()),
        Err(FormatError::Unrecognized)
    ));
    let err = r
        .open(b"DUMMY", Some("x.dmy"), &OpenOptions::default())
        .unwrap_err();
    assert!(err.to_string().contains("strong"));
    assert!(
        FormatError::Malformed {
            offset: 3,
            message: "bad".into()
        }
        .to_string()
        .contains("byte 3")
    );
    let io: FormatError = std::io::Error::other("disk").into();
    assert!(io.to_string().contains("disk"));
    assert!(Confidence::LOW < Confidence::MEDIUM && Confidence::MEDIUM < Confidence::HIGH);
}

#[derive(Debug)]
struct NoProfile;

impl FirmwareProfile for NoProfile {
    fn id(&self) -> &'static str {
        "none"
    }
    fn score(&self, _fw: &FirmwareInfo, h: &RawHeaders) -> Option<u8> {
        h.get("magic").map(|_| 7)
    }
    fn map_field(&self, _f: &str, _fw: &FirmwareInfo, _h: &RawHeaders) -> Option<FieldMapping> {
        None
    }
    fn settings(&self, _fw: &FirmwareInfo, _h: &RawHeaders) -> Settings {
        Settings::default()
    }
}

#[test]
fn profile_registry_selects_by_score() {
    let mut r = ProfileRegistry::new();
    r.register(Box::new(NoProfile));
    assert_eq!(r.iter().count(), 1);
    assert!(r.get("none").is_some() && r.get("other").is_none());
    let fw = FirmwareInfo::unknown();
    assert!(r.select(&fw, &RawHeaders::default()).is_none());
    let h = RawHeaders(vec![("magic".into(), "1".into())]);
    let p = r.select(&fw, &h).unwrap();
    assert_eq!(p.id(), "none");
    assert!(p.slider_model(&fw, &h).is_none());
    assert!(p.map_field("x", &fw, &h).is_none());
}

#[test]
fn versions_series_and_timebase_edges() {
    assert_eq!(
        FirmwareVersion::parse("v2025.12"),
        Some(FirmwareVersion::new(2025, 12, 0))
    );
    assert_eq!(FirmwareVersion::parse("junk"), None);
    assert_eq!(FirmwareVersion::new(4, 5, 1).to_string(), "4.5.1");
    assert!(FirmwareVersion::new(2025, 12, 0) > FirmwareVersion::new(4, 5, 9));
    let fw = FirmwareInfo::parse_revision("Betaflight 4.4.3 (738127e7e) STM32F405");
    assert!(fw.is_at_least(4, 4) && !fw.is_at_least(4, 5));
    let empty = Timebase::from_times(vec![], Some(500.0));
    assert!(empty.is_empty() && empty.duration_s() == 0.0 && empty.start_us().is_none());
    assert_eq!(empty.sample_rate_hz(), 2000.0);
    let s = Series::from_options([None, None]);
    assert_eq!(s.valid_count(), 0);
    assert!(s.get(5).is_none());
    assert!(!s.is_valid(5));
    assert!(ChannelId::parse("rc.x").is_none() && ChannelId::parse("pid.q.roll").is_none());
    assert!(ChannelId::parse("battery.power").is_none() && ChannelId::parse("nonsense").is_none());
}
