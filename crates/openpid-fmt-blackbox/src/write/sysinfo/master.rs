//! The system-information headers of Betaflight `master` (2026.12-dev): every line the
//! firmware can write after the field definitions, in its order, with the
//! build option or run-time condition each one depends on.
//!
//! Names, order and conditions are read from `blackboxWriteSysinfo` in
//! `src/main/blackbox/blackbox.c` (parameter names from
//! `src/main/fc/parameter_names.h`) at the tag `master` (4b1520e).
//!
//! The values are an example configuration, not facts of the version: those
//! of a real craft (`betaflight-2025.12.2-btfl_027.bbl` of `tests/logs/`) where it
//! logs the line, with linear rates of 1000 deg/s, no debug mode and no GPS
//! feature; a line that log does not have carries the firmware's default,
//! and the comment on it says where it was read. A line without a value is
//! written only for a session that brings it.

use super::Build::{
    Acc, AdvancedTpa, AltHold, Baro, BoardInfo, Chirp, CloudKey, CurrentAdc, DMinMax,
    DshotTelemetry, DynIdle, DynLpf, DynNotch, Feedforward, Gps, ItermRelax, Mag, NotWing,
    OpticalFlow, PosHold, RcSmoothing, ReleaseName, RescueFlown, RpmFilter, SagCompensation,
    SimplifiedTuning, Vbat, Wing,
};
use super::{Line, line, open};

/// The lines of this version, in the firmware's order.
pub(in crate::write) const LINES: &[Line] = &[
    line("Firmware type", &[], "Betaflight"),
    line(
        "Firmware revision",
        &[],
        "Betaflight 2026.12.0 (4b1520ef6) STM32F7X2",
    ),
    line("Firmware date", &[], "Nov 12 2026 11:38:56"),
    open("Build key", &[CloudKey]),
    open("Release name", &[CloudKey, ReleaseName]),
    line("DeviceUID", &[], "000000000000000000000000"),
    line("Board information", &[BoardInfo], "OPID SYNTHETIC"),
    line("Log start datetime", &[], "2026-01-01T00:00:00.000+00:00"),
    line("Craft name", &[], "OpenPID synthetic"),
    line("I interval", &[], "128"),
    line("P interval", &[], "4"),
    line("P ratio", &[], "32"),
    line("maxthrottle", &[], "2000"),
    line("gyro_scale", &[], "0x3f800000"),
    line("motorOutput", &[], "158,2047"),
    line("motor_kv", &[], "1960"),
    line("acc_1G", &[Acc], "2048"),
    line("vbat_scale", &[Vbat], "110"),
    line("vbatcellvoltage", &[Vbat], "330,350,430"),
    line("vbatref", &[Vbat], "1680"),
    line("currentSensor", &[CurrentAdc], "0,250"),
    line("looptime", &[], "125"),
    line("gyro_sync_denom", &[], "1"),
    line("pid_process_denom", &[], "2"),
    line("thr_mid", &[], "50"),
    line("thr_expo", &[], "0"),
    line("thr_hover", &[], "50"),
    line("tpa_mode", &[], "1"),
    line("tpa_rate", &[], "65"),
    line("tpa_breakpoint", &[], "1350"),
    line("tpa_low_rate", &[], "20"),
    line("tpa_low_breakpoint", &[], "1050"),
    line("tpa_low_always", &[], "0"),
    line("mixer_type", &[], "LEGACY"),
    line("ez_landing_threshold", &[], "25"),
    line("ez_landing_limit", &[], "15"),
    line("ez_landing_speed", &[], "50"),
    line("landing_disarm_threshold", &[], "0"),
    line("spa_roll_center", &[Wing], "0"),
    line("spa_roll_width", &[Wing], "0"),
    line("spa_roll_mode", &[Wing], "0"),
    line("spa_pitch_center", &[Wing], "0"),
    line("spa_pitch_width", &[Wing], "0"),
    line("spa_pitch_mode", &[Wing], "0"),
    line("spa_yaw_center", &[Wing], "0"),
    line("spa_yaw_width", &[Wing], "0"),
    line("spa_yaw_mode", &[Wing], "0"),
    line("tpa_curve_type", &[AdvancedTpa], "0"),
    line("tpa_curve_stall_throttle", &[AdvancedTpa], "30"),
    line("tpa_curve_pid_thr0", &[AdvancedTpa], "200"),
    line("tpa_curve_pid_thr100", &[AdvancedTpa], "70"),
    line("tpa_curve_expo", &[AdvancedTpa], "20"),
    line("rc_rates", &[], "100,100,100"),
    line("rc_expo", &[], "0,0,0"),
    line("rates", &[], "100,100,100"),
    line("rate_limits", &[], "1998,1998,1998"),
    line("rollPID", &[], "45,80,30"),
    line("pitchPID", &[], "47,84,34"),
    line("yawPID", &[], "45,80,0"),
    line("levelPID", &[], "50,75,75"),
    line("d_max", &[DMinMax], "40,46,0"),
    line("d_max_gain", &[DMinMax], "37"),
    line("d_max_advance", &[DMinMax], "20"),
    line("dterm_lpf1_type", &[], "0"),
    line("dterm_lpf1_static_hz", &[], "75"),
    line("dterm_lpf1_dyn_hz", &[DynLpf], "75,150"),
    line("dterm_lpf1_dyn_expo", &[DynLpf], "5"),
    line("dterm_lpf2_type", &[], "0"),
    line("dterm_lpf2_static_hz", &[], "150"),
    line("yaw_lowpass_hz", &[], "100"),
    line("dterm_notch_hz", &[], "0"),
    line("dterm_notch_cutoff", &[], "0"),
    line("iterm_windup", &[], "80"),
    line("iterm_relax", &[ItermRelax], "1"),
    line("iterm_relax_type", &[ItermRelax], "1"),
    line("iterm_relax_cutoff", &[ItermRelax], "15"),
    line("pid_at_min_throttle", &[], "1"),
    line("anti_gravity_gain", &[], "80"),
    line("anti_gravity_cutoff_hz", &[], "5"),
    line("anti_gravity_p_gain", &[], "100"),
    line("ff_weight", &[], "120,125,120"),
    line("feedforward_transition", &[Feedforward], "0"),
    line("feedforward_averaging", &[Feedforward], "1"),
    line("feedforward_smooth_factor", &[Feedforward], "65"),
    line("feedforward_jitter_factor", &[Feedforward], "7"),
    line("feedforward_boost", &[Feedforward], "15"),
    line("feedforward_max_rate_limit", &[Feedforward], "90"),
    line("angle_feedforward", &[Feedforward], "50"),
    line("angle_feedforward_smoothing_ms", &[Feedforward], "80"),
    line("feedforward_yaw_hold_gain", &[Feedforward], "15"),
    line("feedforward_yaw_hold_time", &[Feedforward], "100"),
    line("angle_limit", &[], "60"),
    line("angle_earth_ref", &[], "100"),
    line("horizon_limit_degrees", &[], "135"),
    line("horizon_delay_ms", &[], "500"),
    line("acc_limit_yaw", &[], "0"),
    line("acc_limit", &[], "0"),
    line("pidsum_limit", &[], "500"),
    line("pidsum_limit_yaw", &[], "400"),
    line("s_roll", &[Wing], "0"),
    line("s_pitch", &[Wing], "0"),
    line("s_yaw", &[Wing], "0"),
    line("chirp_lag_freq_hz", &[Chirp], "3"),
    line("chirp_lead_freq_hz", &[Chirp], "30"),
    line("chirp_amplitude_roll", &[Chirp], "230"),
    line("chirp_amplitude_pitch", &[Chirp], "230"),
    line("chirp_amplitude_yaw", &[Chirp], "180"),
    line("chirp_frequency_start_deci_hz", &[Chirp], "2"),
    line("chirp_frequency_end_deci_hz", &[Chirp], "6000"),
    line("chirp_time_seconds", &[Chirp], "20"),
    line("deadband", &[], "0"),
    line("yaw_deadband", &[], "0"),
    line("gyro_hardware_lpf", &[], "0"),
    line("gyro_lpf1_type", &[], "0"),
    line("gyro_lpf1_static_hz", &[], "250"),
    line("gyro_lpf1_dyn_hz", &[DynLpf], "250,500"),
    line("gyro_lpf1_dyn_expo", &[DynLpf], "5"),
    line("gyro_lpf2_type", &[], "0"),
    line("gyro_lpf2_static_hz", &[], "500"),
    line("gyro_notch_hz", &[], "0,0"),
    line("gyro_notch_cutoff", &[], "0,0"),
    line("gyro_enabled_bitmask", &[], "1"),
    line("gyro_debug_axis", &[], "0"),
    line("dyn_notch_max_hz", &[DynNotch], "600"),
    line("dyn_notch_count", &[DynNotch], "1"),
    line("dyn_notch_q", &[DynNotch], "500"),
    line("dyn_notch_min_hz", &[DynNotch], "100"),
    line("dshot_bidir", &[DshotTelemetry], "1"),
    line("motor_poles", &[DshotTelemetry], "14"),
    line("rpm_filter_harmonics", &[RpmFilter], "3"),
    line("rpm_filter_weights", &[RpmFilter], "100,100,100"),
    line("rpm_filter_q", &[RpmFilter], "500"),
    line("rpm_filter_min_hz", &[RpmFilter], "100"),
    line("rpm_filter_fade_range_hz", &[RpmFilter], "50"),
    line("rpm_filter_lpf_hz", &[RpmFilter], "150"),
    line("acc_lpf_hz", &[Acc], "2500"),
    line("acc_hardware", &[Acc], "0"),
    line("baro_hardware", &[Baro], "0"),
    // the default of `src/main/sensors/barometer.c`
    line("baro_drift", &[Baro], "0"),
    line("altitude_source", &[], "0"),
    line("altitude_prefer_baro", &[], "100"),
    line("altitude_lpf", &[], "300"),
    line("altitude_d_lpf", &[], "100"),
    // the default of `src/main/pg/autopilot.c`
    line("ap_landing_altitude_m", &[NotWing], "4"),
    // the default of `src/main/pg/autopilot.c`
    line("ap_hover_throttle", &[NotWing], "1275"),
    // the default of `src/main/pg/autopilot.c`
    line("ap_throttle_min", &[NotWing], "1100"),
    // the default of `src/main/pg/autopilot.c`
    line("ap_throttle_max", &[NotWing], "1900"),
    // the default of `src/main/pg/autopilot.c`
    line("ap_altitude_p", &[NotWing], "30"),
    // the default of `src/main/pg/autopilot.c`
    line("ap_altitude_i", &[NotWing], "30"),
    // the default of `src/main/pg/autopilot.c`
    line("ap_altitude_d", &[NotWing], "30"),
    // the default of `src/main/pg/autopilot.c`
    line("ap_altitude_a", &[NotWing], "30"),
    // the default of `src/main/pg/autopilot.c`
    line("ap_altitude_f", &[NotWing], "30"),
    // the default of `src/main/pg/autopilot.c`
    line("ap_position_p", &[NotWing], "30"),
    // the default of `src/main/pg/autopilot.c`
    line("ap_position_i", &[NotWing], "30"),
    // the default of `src/main/pg/autopilot.c`
    line("ap_position_d", &[NotWing], "30"),
    // the default of `src/main/pg/autopilot.c`
    line("ap_position_a", &[NotWing], "30"),
    // the default of `src/main/pg/autopilot.c`
    line("ap_position_f", &[NotWing], "30"),
    // the default of `src/main/pg/autopilot.c`
    line("ap_position_cutoff", &[NotWing], "30"),
    // the default of `src/main/pg/autopilot.c`
    line("ap_stop_threshold", &[NotWing], "10"),
    // the default of `src/main/pg/autopilot.c`
    line("ap_max_angle", &[NotWing], "50"),
    // the default of `src/main/pg/autopilot.c`
    line("ap_max_yaw_rate", &[NotWing], "150"),
    // the default of `src/main/pg/autopilot.c`
    line("ap_yaw_p", &[NotWing], "30"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_cruise_throttle", &[Wing], "50"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_min_throttle", &[Wing], "25"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_max_throttle", &[Wing], "90"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_bank_throttle", &[Wing], "10"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_max_climb_angle", &[Wing], "15"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_max_dive_angle", &[Wing], "12"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_max_climb_rate", &[Wing], "30"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_max_sink_rate", &[Wing], "30"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_alt_time_constant", &[Wing], "40"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_climb_p", &[Wing], "10"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_climb_i", &[Wing], "10"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_vert_accel", &[Wing], "50"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_cruise_speed", &[Wing], "150"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_turn_pitch_ff", &[Wing], "100"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_turn_yaw_ff", &[Wing], "100"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_max_bank", &[Wing], "35"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_l1_period", &[Wing], "130"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_l1_damping", &[Wing], "75"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_loiter_radius", &[Wing], "60"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_loiter_direction", &[Wing], "0"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_land_heading", &[Wing], "-1"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_land_side", &[Wing], "1"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_land_final_length", &[Wing], "150"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_land_glide_angle", &[Wing], "8"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_land_approach_alt", &[Wing], "30"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_land_flare_height", &[Wing], "300"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_land_flare_pitch", &[Wing], "5"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_land_flare_sink", &[Wing], "5"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_land_final_bank", &[Wing], "15"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_land_slope_tolerance", &[Wing], "10"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_land_attempts", &[Wing], "3"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_land_max_tailwind", &[Wing], "30"),
    // DEFAULT_LANDING_DETECTION_TIME of a wing, `src/main/pg/autopilot.c`
    line("ap_landing_detection_time", &[Wing], "20"),
    // the default of `src/main/pg/autopilot_wing.c`
    line("ap_wing_land_launch_height", &[Wing], "150"),
    line("mag_hardware", &[Mag], "0"),
    open("opticalflow_hardware", &[OpticalFlow]),
    open("opticalflow_rotation", &[OpticalFlow]),
    open("opticalflow_flip_x", &[OpticalFlow]),
    line("gyro_cal_on_first_arm", &[], "1"),
    line("airmode_activate_throttle", &[], "25"),
    line("serialrx_provider", &[], "9"),
    line("use_unsynced_pwm", &[], "0"),
    line("motor_pwm_protocol", &[], "6"),
    line("motor_pwm_rate", &[], "480"),
    line("motor_idle", &[], "550"),
    line("debug_mode", &[], "0"),
    // `debugModeNames[DEBUG_NONE]`, `src/main/build/debug.c`
    line("debug_mode_name", &[], "NONE"),
    line("features", &[], "272892940"),
    line("rc_smoothing", &[RcSmoothing], "1"),
    line("rc_smoothing_auto_factor", &[RcSmoothing], "30"),
    line("rc_smoothing_auto_factor_throttle", &[RcSmoothing], "30"),
    line("rc_smoothing_setpoint_cutoff", &[RcSmoothing], "0"),
    line("rc_smoothing_throttle_cutoff", &[RcSmoothing], "0"),
    line("rc_smoothing_debug_axis", &[RcSmoothing], "0"),
    line(
        "rc_smoothing_active_cutoffs_ff_sp_thr",
        &[RcSmoothing],
        "18,18",
    ),
    line("rc_smoothing_rx_smoothed", &[RcSmoothing], "50"),
    line("rates_type", &[], "3"),
    line("fields_disabled_mask", &[], "0"),
    line("blackbox_high_resolution", &[], "0"),
    // BLACKBOX_RC_COMMAND_SCALE
    line("rc_command_scale", &[], "1000"),
    line("vbat_sag_compensation", &[SagCompensation], "0"),
    line("dyn_idle_min_rpm", &[DynIdle], "0"),
    line("dyn_idle_p_gain", &[DynIdle], "50"),
    line("dyn_idle_i_gain", &[DynIdle], "50"),
    line("dyn_idle_d_gain", &[DynIdle], "50"),
    line("dyn_idle_max_increase", &[DynIdle], "150"),
    line("simplified_pids_mode", &[SimplifiedTuning], "2"),
    line("simplified_master_multiplier", &[SimplifiedTuning], "100"),
    line("simplified_i_gain", &[SimplifiedTuning], "100"),
    line("simplified_d_gain", &[SimplifiedTuning], "100"),
    line("simplified_pi_gain", &[SimplifiedTuning], "100"),
    line("simplified_d_max_gain", &[SimplifiedTuning], "100"),
    line("simplified_feedforward_gain", &[SimplifiedTuning], "100"),
    line("simplified_pitch_d_gain", &[SimplifiedTuning], "100"),
    line("simplified_pitch_pi_gain", &[SimplifiedTuning], "100"),
    line("simplified_dterm_filter", &[SimplifiedTuning], "1"),
    line(
        "simplified_dterm_filter_multiplier",
        &[SimplifiedTuning],
        "100",
    ),
    line("simplified_gyro_filter", &[SimplifiedTuning], "1"),
    line(
        "simplified_gyro_filter_multiplier",
        &[SimplifiedTuning],
        "100",
    ),
    line("motor_output_limit", &[], "100"),
    line("throttle_limit_type", &[], "0"),
    line("throttle_limit_percent", &[], "100"),
    line("throttle_boost", &[], "5"),
    line("throttle_boost_cutoff", &[], "15"),
    line("thrust_linear", &[], "0"),
    line("gps_provider", &[Gps], "1"),
    line("gps_set_home_point_once", &[Gps], "0"),
    line("gps_use_3d_speed", &[Gps], "0"),
    open("gps_rescue_min_start_dist", &[Gps, RescueFlown]),
    open("gps_rescue_alt_mode", &[Gps, RescueFlown]),
    open("gps_rescue_initial_climb", &[Gps, RescueFlown]),
    open("gps_rescue_ascend_rate", &[Gps, RescueFlown]),
    open("gps_rescue_return_alt", &[Gps, RescueFlown]),
    open("gps_rescue_ground_speed", &[Gps, RescueFlown, NotWing]),
    open("gps_rescue_descent_dist", &[Gps, RescueFlown, NotWing]),
    open("gps_rescue_descend_rate", &[Gps, RescueFlown]),
    open("gps_rescue_sanity_checks", &[Gps, RescueFlown, NotWing]),
    open("gps_rescue_min_sats", &[Gps, RescueFlown]),
    open("gps_rescue_allow_arming_without_fix", &[Gps, RescueFlown]),
    open("alt_hold_climb_rate", &[AltHold]),
    open("alt_hold_deadband", &[AltHold]),
    open("pos_hold_deadband", &[PosHold]),
    line("tpa_speed_type", &[Wing], "0"),
    line("tpa_speed_basic_delay", &[Wing], "1000"),
    line("tpa_speed_basic_gravity", &[Wing], "50"),
    line("tpa_speed_adv_prop_pitch", &[Wing], "370"),
    line("tpa_speed_adv_mass", &[Wing], "1000"),
    line("tpa_speed_adv_drag_k", &[Wing], "1000"),
    line("tpa_speed_adv_thrust", &[Wing], "2000"),
    line("tpa_speed_max_voltage", &[Wing], "2520"),
    line("tpa_speed_pitch_offset", &[Wing], "0"),
    line("yaw_type", &[Wing], "0"),
    line("angle_pitch_offset", &[Wing], "0"),
];
