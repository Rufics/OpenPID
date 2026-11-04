//! CSV log reader compatible with the output of Betaflight's `blackbox_decode`.
//!
//! Column headers may carry a unit suffix, e.g. `time (us)` or `vbatLatest (V)`.
//! Columns without a suffix are treated as raw firmware integers and mapped
//! through the Cleanflight-family profiles; suffixed columns are kept as
//! already-scaled `Custom` channels.

use openpid_core::session::Diagnostics;
use openpid_core::{
    Channel, ChannelId, ChannelSet, Confidence, FirmwareInfo, FlightSession, FormatError, LogFile,
    LogFormat, OpenOptions, Origin, ProfileRegistry, RawHeaders, Series, Settings, SourceInfo,
    Timebase, Unit,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvColumn {
    pub name: String,
    pub unit: Option<String>,
}

/// Parsed table; `None` cells were not numeric (e.g. flag names).
#[derive(Debug, Clone, PartialEq)]
pub struct CsvTable {
    pub columns: Vec<CsvColumn>,
    pub rows: Vec<Vec<Option<f64>>>,
    /// Rows skipped because their field count did not match the header.
    pub bad_rows: usize,
}

impl CsvTable {
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }

    pub fn column(&self, name: &str) -> Option<Vec<Option<f64>>> {
        let i = self.column_index(name)?;
        Some(self.rows.iter().map(|r| r[i]).collect())
    }
}

fn parse_header_cell(cell: &str) -> CsvColumn {
    let cell = cell.trim();
    if let Some(open) = cell.rfind(" (")
        && cell.ends_with(')')
    {
        CsvColumn {
            name: cell[..open].trim().to_string(),
            unit: Some(cell[open + 2..cell.len() - 1].to_string()),
        }
    } else {
        CsvColumn {
            name: cell.to_string(),
            unit: None,
        }
    }
}

/// Parse CSV text into a table.
pub fn read_table(data: &[u8]) -> Result<CsvTable, FormatError> {
    let text = String::from_utf8_lossy(data);
    let mut lines = text.lines().map(|l| l.trim_end_matches('\r'));
    let header = lines.next().ok_or(FormatError::Unrecognized)?;
    let columns: Vec<CsvColumn> = header.split(',').map(parse_header_cell).collect();
    if columns.len() < 2 || !columns.iter().any(|c| c.name == "time") {
        return Err(FormatError::Unrecognized);
    }
    let mut rows = Vec::new();
    let mut bad_rows = 0;
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let cells: Vec<&str> = line.split(',').collect();
        if cells.len() != columns.len() {
            bad_rows += 1;
            continue;
        }
        rows.push(cells.iter().map(|c| c.trim().parse::<f64>().ok()).collect());
    }
    Ok(CsvTable {
        columns,
        rows,
        bad_rows,
    })
}

/// [`LogFormat`] for CSV exports.
#[derive(Debug)]
pub struct CsvFormat {
    profiles: ProfileRegistry,
}

impl Default for CsvFormat {
    fn default() -> Self {
        CsvFormat {
            profiles: openpid_fmt_blackbox::profiles::builtin(),
        }
    }
}

impl CsvFormat {
    pub fn new() -> Self {
        Self::default()
    }
}

impl LogFormat for CsvFormat {
    fn id(&self) -> &'static str {
        "blackbox-csv"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["csv"]
    }

    fn sniff(&self, head: &[u8], _file_name: Option<&str>) -> Confidence {
        let first = head.split(|b| *b == b'\n').next().unwrap_or(&[]);
        let first = String::from_utf8_lossy(first);
        if first.contains("loopIteration") && first.contains("time") {
            Confidence::HIGH
        } else if first.contains("time") && first.contains(',') {
            Confidence::LOW
        } else {
            Confidence::NONE
        }
    }

    fn open(&self, data: &[u8], options: &OpenOptions) -> Result<LogFile, FormatError> {
        let table = read_table(data)?;
        let time_idx = table
            .column_index("time")
            .ok_or(FormatError::Unrecognized)?;
        let time_us: Vec<i64> = table
            .rows
            .iter()
            .map(|r| r[time_idx].unwrap_or(0.0).round() as i64)
            .collect();
        let firmware = FirmwareInfo::unknown();
        let headers = RawHeaders::default();
        let profile = if options.raw {
            None
        } else {
            self.profiles.select(&firmware, &headers)
        };
        let mut channels = ChannelSet::new();
        let mut unmapped = Vec::new();
        for (i, col) in table.columns.iter().enumerate() {
            let cells = table.rows.iter().map(|r| r[i]);
            let (id, unit, data) = match (&col.unit, profile) {
                (None, Some(p)) => {
                    if let Some(m) = p.map_field(&col.name, &firmware, &headers) {
                        let data =
                            Series::from_options(cells.map(|v| v.map(|x| m.apply(x as i64))));
                        (m.id, m.unit, data)
                    } else {
                        unmapped.push(col.name.clone());
                        (
                            ChannelId::Custom(col.name.clone()),
                            Unit::Raw,
                            Series::from_options(cells),
                        )
                    }
                }
                (Some(u), _) => (
                    ChannelId::Custom(col.name.clone()),
                    Unit::Custom(u.clone()),
                    Series::from_options(cells),
                ),
                (None, None) => (
                    ChannelId::Custom(col.name.clone()),
                    Unit::Raw,
                    Series::from_options(cells),
                ),
            };
            channels.insert(Channel {
                id,
                name: col.name.clone(),
                unit,
                data,
                origin: Origin::Logged {
                    field: col.name.clone(),
                },
            });
        }
        let mut diagnostics = Diagnostics {
            total_bytes: data.len() as u64,
            unmapped_fields: unmapped,
            ..Default::default()
        };
        if table.bad_rows > 0 {
            diagnostics
                .notes
                .push(format!("{} malformed rows skipped", table.bad_rows));
        }
        let mut session = FlightSession {
            index: 0,
            offset_bytes: 0,
            firmware,
            profile_id: profile.map_or("raw", |p| p.id()).to_string(),
            settings: Settings::default(),
            raw_headers: Vec::new(),
            timebase: Timebase::from_times(time_us, None),
            channels,
            events: Vec::new(),
            aux: Vec::new(),
            diagnostics,
        };
        if let Some(p) = profile {
            p.derive(&mut session);
        }
        Ok(LogFile {
            source: SourceInfo {
                path: None,
                size_bytes: data.len() as u64,
                format_id: self.id().to_string(),
            },
            sessions: vec![session],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "loopIteration, time (us), gyroADC[0], vbatLatest (V), flightModeFlags (flags)\n\
0, 1000, 5, 16.80, ANGLE_MODE\n\
16, 3000, -7, 16.79, ANGLE_MODE\n\
bad,row\n";

    #[test]
    fn parses_table() {
        let t = read_table(SAMPLE.as_bytes()).unwrap();
        assert_eq!(
            t.columns[1],
            CsvColumn {
                name: "time".into(),
                unit: Some("us".into())
            }
        );
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.bad_rows, 1);
        assert_eq!(t.column("gyroADC[0]").unwrap(), vec![Some(5.0), Some(-7.0)]);
        assert_eq!(t.rows[0][4], None);
    }

    #[test]
    fn opens_as_session() {
        let f = CsvFormat::new();
        assert_eq!(f.sniff(SAMPLE.as_bytes(), None), Confidence::HIGH);
        let log = f.open(SAMPLE.as_bytes(), &OpenOptions::default()).unwrap();
        let s = &log.sessions[0];
        assert_eq!(s.len(), 2);
        assert_eq!(s.timebase.time_us, vec![1000, 3000]);
        let g = s
            .channel(&ChannelId::Gyro {
                axis: openpid_core::Axis::Roll,
                stage: openpid_core::GyroStage::Filtered,
            })
            .unwrap();
        assert_eq!(g.data.values(), &[5.0, -7.0]);
        let v = s.channel(&ChannelId::Custom("vbatLatest".into())).unwrap();
        assert_eq!(v.unit, Unit::Custom("V".into()));
        assert_eq!(s.diagnostics.notes.len(), 1);
    }
}
