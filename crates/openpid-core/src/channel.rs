//! Firmware-independent channel identifiers.
//!
//! A format crate maps the field names of a concrete firmware (for example
//! `gyroADC[0]`) onto these identifiers; analyses only ever talk in terms of
//! [`ChannelId`].

use serde::{Deserialize, Serialize};
use std::fmt;

/// Rotation axis of the craft.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    Roll,
    Pitch,
    Yaw,
}

impl Axis {
    pub const ALL: [Axis; 3] = [Axis::Roll, Axis::Pitch, Axis::Yaw];

    /// Index used by firmwares for array fields (`[0]`, `[1]`, `[2]`).
    pub fn index(self) -> usize {
        match self {
            Axis::Roll => 0,
            Axis::Pitch => 1,
            Axis::Yaw => 2,
        }
    }

    pub fn from_index(i: usize) -> Option<Axis> {
        match i {
            0 => Some(Axis::Roll),
            1 => Some(Axis::Pitch),
            2 => Some(Axis::Yaw),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Axis::Roll => "roll",
            Axis::Pitch => "pitch",
            Axis::Yaw => "yaw",
        }
    }

    fn parse(s: &str) -> Option<Axis> {
        match s {
            "roll" => Some(Axis::Roll),
            "pitch" => Some(Axis::Pitch),
            "yaw" => Some(Axis::Yaw),
            _ => None,
        }
    }
}

/// Where in the gyro signal chain a sample was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GyroStage {
    /// Before the firmware's software filters (after the sensor's own LPF).
    Raw,
    /// After the complete gyro filter chain, as fed into the PID controller.
    Filtered,
}

impl GyroStage {
    pub fn name(self) -> &'static str {
        match self {
            GyroStage::Raw => "raw",
            GyroStage::Filtered => "filtered",
        }
    }
}

/// Term of the PID controller output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PidTerm {
    P,
    I,
    D,
    /// Feed-forward.
    F,
    /// Setpoint/"S" term used by some firmwares (e.g. Betaflight wing mode).
    S,
    /// Sum of all available terms.
    Sum,
}

impl PidTerm {
    pub fn name(self) -> &'static str {
        match self {
            PidTerm::P => "p",
            PidTerm::I => "i",
            PidTerm::D => "d",
            PidTerm::F => "f",
            PidTerm::S => "s",
            PidTerm::Sum => "sum",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatteryField {
    Voltage,
    Current,
    Consumed,
}

impl BatteryField {
    pub fn name(self) -> &'static str {
        match self {
            BatteryField::Voltage => "voltage",
            BatteryField::Current => "current",
            BatteryField::Consumed => "consumed",
        }
    }
}

/// Semantic identity of a data channel, independent of the logging firmware.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelId {
    /// Timestamp of each sample (microseconds).
    Time,
    /// Flight-controller loop counter.
    LoopIteration,
    Gyro {
        axis: Axis,
        stage: GyroStage,
    },
    Setpoint {
        axis: Axis,
    },
    PidTerm {
        axis: Axis,
        term: PidTerm,
    },
    /// Setpoint minus filtered gyro.
    PidError {
        axis: Axis,
    },
    /// Raw RC command for channel `ch` (0..3 = roll, pitch, yaw, throttle).
    RcCommand {
        ch: u8,
    },
    /// Throttle in percent (0..100), derived from the best available source.
    Throttle,
    Motor {
        idx: u8,
    },
    /// Electrical RPM as reported by ESC telemetry (raw firmware units).
    Erpm {
        idx: u8,
    },
    /// Mechanical motor RPM derived from `Erpm` and the motor pole count.
    MotorRpm {
        idx: u8,
    },
    Debug {
        idx: u8,
    },
    Acc {
        axis: Axis,
    },
    Battery(BatteryField),
    Rssi,
    /// A field the firmware profile could not classify; kept under its logged name.
    Custom(String),
}

impl ChannelId {
    /// Stable textual key, e.g. `gyro.filtered.roll` or `pid.d.pitch`.
    pub fn key(&self) -> String {
        match self {
            ChannelId::Time => "time".into(),
            ChannelId::LoopIteration => "loop_iteration".into(),
            ChannelId::Gyro { axis, stage } => format!("gyro.{}.{}", stage.name(), axis.name()),
            ChannelId::Setpoint { axis } => format!("setpoint.{}", axis.name()),
            ChannelId::PidTerm { axis, term } => format!("pid.{}.{}", term.name(), axis.name()),
            ChannelId::PidError { axis } => format!("pid_error.{}", axis.name()),
            ChannelId::RcCommand { ch } => format!("rc.{ch}"),
            ChannelId::Throttle => "throttle".into(),
            ChannelId::Motor { idx } => format!("motor.{idx}"),
            ChannelId::Erpm { idx } => format!("erpm.{idx}"),
            ChannelId::MotorRpm { idx } => format!("motor_rpm.{idx}"),
            ChannelId::Debug { idx } => format!("debug.{idx}"),
            ChannelId::Acc { axis } => format!("acc.{}", axis.name()),
            ChannelId::Battery(f) => format!("battery.{}", f.name()),
            ChannelId::Rssi => "rssi".into(),
            ChannelId::Custom(name) => format!("custom:{name}"),
        }
    }

    /// Inverse of [`ChannelId::key`].
    pub fn parse(key: &str) -> Option<ChannelId> {
        if let Some(name) = key.strip_prefix("custom:") {
            return Some(ChannelId::Custom(name.to_string()));
        }
        let parts: Vec<&str> = key.split('.').collect();
        let idx = |s: &str| s.parse::<u8>().ok();
        match parts.as_slice() {
            ["time"] => Some(ChannelId::Time),
            ["loop_iteration"] => Some(ChannelId::LoopIteration),
            ["throttle"] => Some(ChannelId::Throttle),
            ["rssi"] => Some(ChannelId::Rssi),
            ["gyro", stage, axis] => {
                let stage = match *stage {
                    "raw" => GyroStage::Raw,
                    "filtered" => GyroStage::Filtered,
                    _ => return None,
                };
                Some(ChannelId::Gyro {
                    axis: Axis::parse(axis)?,
                    stage,
                })
            }
            ["setpoint", axis] => Some(ChannelId::Setpoint {
                axis: Axis::parse(axis)?,
            }),
            ["pid", term, axis] => {
                let term = match *term {
                    "p" => PidTerm::P,
                    "i" => PidTerm::I,
                    "d" => PidTerm::D,
                    "f" => PidTerm::F,
                    "s" => PidTerm::S,
                    "sum" => PidTerm::Sum,
                    _ => return None,
                };
                Some(ChannelId::PidTerm {
                    axis: Axis::parse(axis)?,
                    term,
                })
            }
            ["pid_error", axis] => Some(ChannelId::PidError {
                axis: Axis::parse(axis)?,
            }),
            ["rc", ch] => Some(ChannelId::RcCommand { ch: idx(ch)? }),
            ["motor", i] => Some(ChannelId::Motor { idx: idx(i)? }),
            ["erpm", i] => Some(ChannelId::Erpm { idx: idx(i)? }),
            ["motor_rpm", i] => Some(ChannelId::MotorRpm { idx: idx(i)? }),
            ["debug", i] => Some(ChannelId::Debug { idx: idx(i)? }),
            ["acc", axis] => Some(ChannelId::Acc {
                axis: Axis::parse(axis)?,
            }),
            ["battery", f] => {
                let f = match *f {
                    "voltage" => BatteryField::Voltage,
                    "current" => BatteryField::Current,
                    "consumed" => BatteryField::Consumed,
                    _ => return None,
                };
                Some(ChannelId::Battery(f))
            }
            _ => None,
        }
    }
}

impl fmt::Display for ChannelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_roundtrip() {
        let ids = [
            ChannelId::Time,
            ChannelId::LoopIteration,
            ChannelId::Gyro {
                axis: Axis::Pitch,
                stage: GyroStage::Raw,
            },
            ChannelId::Setpoint { axis: Axis::Yaw },
            ChannelId::PidTerm {
                axis: Axis::Roll,
                term: PidTerm::Sum,
            },
            ChannelId::PidError { axis: Axis::Roll },
            ChannelId::RcCommand { ch: 3 },
            ChannelId::Throttle,
            ChannelId::Motor { idx: 7 },
            ChannelId::Erpm { idx: 1 },
            ChannelId::MotorRpm { idx: 2 },
            ChannelId::Debug { idx: 5 },
            ChannelId::Acc { axis: Axis::Yaw },
            ChannelId::Battery(BatteryField::Consumed),
            ChannelId::Rssi,
            ChannelId::Custom("BaroAlt".into()),
        ];
        for id in ids {
            assert_eq!(
                ChannelId::parse(&id.key()).as_ref(),
                Some(&id),
                "{}",
                id.key()
            );
        }
        assert_eq!(ChannelId::parse("gyro.bogus.roll"), None);
    }
}
