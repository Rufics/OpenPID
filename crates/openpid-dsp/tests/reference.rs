//! Compare every dsp primitive with NumPy/SciPy fixtures produced by
//! `tools/reference/make_fixtures.py`.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use openpid_dsp::deconv::{DeconvParams, impulse_response, step_from_impulse};
use openpid_dsp::resample::to_uniform;
use openpid_dsp::smooth::{box_filter_2d, moving_average};
use openpid_dsp::spectrum::{WelchParams, amplitude_spectrum, stft, welch};
use openpid_dsp::{Fft, Window};
use serde_json::Value;
use std::path::PathBuf;

fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/reference")
        .join(format!("{name}.json"));
    let bytes = std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e} (run tools/reference/make_fixtures.py)",
            path.display()
        )
    });
    serde_json::from_slice(&bytes).unwrap()
}

fn floats(v: &Value) -> Vec<f64> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect()
}

fn assert_close(name: &str, got: &[f64], want: &[f64], rel: f64, abs: f64) {
    assert_eq!(got.len(), want.len(), "{name}: length");
    let scale = want.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        let tol = abs + rel * scale;
        assert!(
            (g - w).abs() <= tol,
            "{name}[{i}]: got {g}, want {w} (tol {tol})"
        );
    }
}

#[test]
fn windows_match_scipy() {
    let f = fixture("windows");
    for n in [16usize, 64, 1000] {
        assert_close(
            &format!("hann {n}"),
            &Window::Hann.values(n),
            &floats(&f[format!("hann_{n}")]),
            1e-12,
            1e-12,
        );
        assert_close(
            &format!("hamming {n}"),
            &Window::Hamming.values(n),
            &floats(&f[format!("hamming_{n}")]),
            1e-12,
            1e-12,
        );
        assert_close(
            &format!("bh {n}"),
            &Window::BlackmanHarris.values(n),
            &floats(&f[format!("blackmanharris_{n}")]),
            1e-12,
            1e-12,
        );
        assert_close(
            &format!("tukey {n}"),
            &Window::Tukey(0.5).values(n),
            &floats(&f[format!("tukey_0.5_{n}")]),
            1e-12,
            1e-12,
        );
    }
}

#[test]
fn rfft_matches_numpy() {
    let f = fixture("rfft");
    let fft = Fft::new();
    let x = floats(&f["x"]);
    let spec = fft.rfft(&x);
    let re: Vec<f64> = spec.iter().map(|c| c.re).collect();
    let im: Vec<f64> = spec.iter().map(|c| c.im).collect();
    assert_close("rfft re", &re, &floats(&f["rfft_re"]), 1e-12, 1e-9);
    assert_close("rfft im", &im, &floats(&f["rfft_im"]), 1e-12, 1e-9);
    assert_close(
        "freqs",
        &openpid_dsp::fft::rfft_freqs(x.len(), f["fs"].as_f64().unwrap()),
        &floats(&f["freqs"]),
        1e-12,
        0.0,
    );
    assert_close(
        "amplitude",
        &amplitude_spectrum(&fft, &x, Window::Hann),
        &floats(&f["amplitude_hann"]),
        1e-12,
        1e-12,
    );
}

#[test]
fn welch_matches_scipy() {
    let f = fixture("welch");
    let fft = Fft::new();
    let x = floats(&f["x"]);
    let fs = f["fs"].as_f64().unwrap();
    for case in f["cases"].as_array().unwrap() {
        let p = WelchParams {
            nperseg: case["nperseg"].as_u64().unwrap() as usize,
            noverlap: case["noverlap"].as_u64().unwrap() as usize,
            window: Window::Hann,
            detrend: true,
        };
        let psd = welch(&fft, &x, fs, &p).unwrap();
        assert_close(
            &format!("welch freq {}", p.nperseg),
            &psd.freq,
            &floats(&case["freq"]),
            1e-12,
            1e-9,
        );
        assert_close(
            &format!("welch power {}", p.nperseg),
            &psd.power,
            &floats(&case["power"]),
            1e-10,
            1e-12,
        );
    }
}

#[test]
fn stft_matches_reference() {
    let f = fixture("stft");
    let fft = Fft::new();
    let x = floats(&f["x"]);
    let s = stft(
        &fft,
        &x,
        f["fs"].as_f64().unwrap(),
        f["nperseg"].as_u64().unwrap() as usize,
        f["hop"].as_u64().unwrap() as usize,
        Window::Hann,
    )
    .unwrap();
    assert_close("stft time", &s.time, &floats(&f["time"]), 1e-12, 1e-12);
    assert_close("stft freq", &s.freq, &floats(&f["freq"]), 1e-12, 1e-9);
    let frames = f["amp"].as_array().unwrap();
    assert_eq!(s.time.len(), frames.len());
    for (ti, frame) in frames.iter().enumerate() {
        let want = floats(frame);
        let got: Vec<f64> = (0..s.freq.len()).map(|fi| s.get(ti, fi)).collect();
        assert_close(&format!("stft frame {ti}"), &got, &want, 1e-11, 1e-12);
    }
}

#[test]
fn deconvolution_matches_reference() {
    let f = fixture("deconv");
    let fft = Fft::new();
    let input = floats(&f["input"]);
    let output = floats(&f["output"]);
    let p = f["params"].clone();
    let params = DeconvParams {
        cutoff_hz: p["cutoff_hz"].as_f64().unwrap(),
        reg_low: p["reg_low"].as_f64().unwrap(),
        reg_high: p["reg_high"].as_f64().unwrap(),
        transition: p["transition"].as_f64().unwrap(),
        window: Window::Hann,
    };
    let fs = f["fs"].as_f64().unwrap();
    let resp_len = f["resp_len"].as_u64().unwrap() as usize;
    assert_eq!(
        openpid_dsp::next_smooth(input.len() + input.len() / 2),
        f["nfft"].as_u64().unwrap() as usize
    );
    let h = impulse_response(&fft, &input, &output, fs, resp_len, &params).unwrap();
    assert_close("impulse", &h, &floats(&f["impulse"]), 1e-9, 1e-12);
    let s = step_from_impulse(&h);
    assert_close("step", &s, &floats(&f["step"]), 1e-9, 1e-12);
    // and the physics: a first-order plant with the fixture's tau
    let tau = f["tau"].as_f64().unwrap();
    let alpha = (1.0 / fs) / tau;
    for (i, v) in s.iter().enumerate().step_by(30) {
        let expected = 1.0 - (1.0 - alpha).powi(i as i32 + 1);
        assert!(
            (v - expected).abs() < 0.05,
            "step[{i}] = {v}, plant says {expected}"
        );
    }
}

#[test]
fn smoothing_matches_reference() {
    let f = fixture("smooth");
    let x = floats(&f["x"]);
    assert_close(
        "moving average",
        &moving_average(&x, 5),
        &floats(&f["moving_average_5"]),
        1e-12,
        1e-12,
    );
    let z = floats(&f["z"]);
    let (nx, ny) = (
        f["nx"].as_u64().unwrap() as usize,
        f["ny"].as_u64().unwrap() as usize,
    );
    assert_close(
        "box 5x3",
        &box_filter_2d(&z, nx, ny, 5, 3),
        &floats(&f["box_5x3"]),
        1e-12,
        1e-12,
    );
}

#[test]
fn resample_matches_reference() {
    let f = fixture("resample");
    let t: Vec<i64> = f["time_us"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    let x = floats(&f["x"]);
    let u = to_uniform(
        &t,
        &x,
        None,
        f["dt_us"].as_f64().unwrap(),
        f["max_gap_us"].as_f64().unwrap(),
    );
    let valid: Vec<bool> = f["valid"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_bool().unwrap())
        .collect();
    assert_eq!(u.valid, valid);
    assert_close("resampled", &u.values, &floats(&f["values"]), 1e-12, 1e-9);
}
