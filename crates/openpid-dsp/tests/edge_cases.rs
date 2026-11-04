//! Edge cases of the dsp primitives that the reference fixtures do not reach.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use openpid_dsp::deconv::{DeconvParams, impulse_response};
use openpid_dsp::resample::to_uniform;
use openpid_dsp::spectrum::{WelchParams, amplitude_spectrum, stft, welch};
use openpid_dsp::{DspError, Fft, Window};

#[test]
fn resample_edges() {
    let u = to_uniform(&[], &[], None, 1000.0, 1500.0);
    assert!(u.is_empty());
    let u = to_uniform(&[0, 1000], &[1.0, 2.0], None, 0.0, 1500.0);
    assert!(u.is_empty(), "non-positive step yields nothing");
    let u = to_uniform(&[0, 1000, 2000], &[1.0, 2.0, 3.0], None, 1000.0, 1500.0);
    assert_eq!(u.sample_rate_hz(), 1000.0);
    assert_eq!(u.values, vec![1.0, 2.0, 3.0]);
    assert!(u.valid.iter().all(|v| *v));
    // last grid point coincides with the last source sample but it is invalid
    let u = to_uniform(
        &[0, 1000],
        &[1.0, 2.0],
        Some(&[true, false]),
        1000.0,
        1500.0,
    );
    assert_eq!(u.valid, vec![true, false]);
}

#[test]
fn spectrum_and_deconv_errors() {
    let fft = Fft::new();
    assert!(format!("{fft:?}").contains("Fft"));
    let x: Vec<f64> = (0..64).map(|i| f64::from(i % 5)).collect();
    assert!(matches!(
        welch(
            &fft,
            &x,
            100.0,
            &WelchParams {
                nperseg: 1,
                ..WelchParams::new(8)
            }
        ),
        Err(DspError::InvalidParam(_))
    ));
    assert!(matches!(
        welch(
            &fft,
            &x,
            100.0,
            &WelchParams {
                noverlap: 8,
                ..WelchParams::new(8)
            }
        ),
        Err(DspError::InvalidParam(_))
    ));
    let p = welch(
        &fft,
        &x,
        100.0,
        &WelchParams {
            detrend: false,
            window: Window::Rectangular,
            ..WelchParams::new(16)
        },
    )
    .unwrap();
    assert_eq!(p.segments, 7);
    // odd segment length: no Nyquist bin to leave undoubled
    let p = welch(&fft, &x, 100.0, &WelchParams::new(15)).unwrap();
    assert_eq!(p.freq.len(), 8);
    assert!(amplitude_spectrum(&fft, &[], Window::Hann).is_empty());
    assert_eq!(
        amplitude_spectrum(&fft, &x[..7], Window::Rectangular).len(),
        4
    );
    assert!(matches!(
        stft(&fft, &x, 100.0, 1, 1, Window::Hann),
        Err(DspError::InvalidParam(_))
    ));
    assert!(matches!(
        stft(&fft, &x, 100.0, 8, 0, Window::Hann),
        Err(DspError::InvalidParam(_))
    ));
    assert!(matches!(
        stft(&fft, &x[..4], 100.0, 8, 4, Window::Hann),
        Err(DspError::TooShort { .. })
    ));
    let s = stft(&fft, &x, 100.0, 16, 16, Window::Hamming).unwrap();
    assert_eq!(s.time.len(), 4);
    let p = DeconvParams::default();
    assert!(matches!(
        impulse_response(&fft, &x, &x[..10], 100.0, 5, &p),
        Err(DspError::InvalidParam(_))
    ));
    assert!(matches!(
        impulse_response(&fft, &x[..1], &x[..1], 100.0, 1, &p),
        Err(DspError::TooShort { .. })
    ));
    assert!(matches!(
        impulse_response(&fft, &x, &x, 100.0, 100, &p),
        Err(DspError::InvalidParam(_))
    ));
    assert!(
        DspError::TooShort { need: 2, got: 1 }
            .to_string()
            .contains("at least 2")
    );
    for w in [
        Window::Rectangular,
        Window::Hamming,
        Window::BlackmanHarris,
        Window::Tukey(0.3),
        Window::Tukey(2.0),
    ] {
        assert_eq!(w.values(0).len(), 0);
        assert!(w.coherent_gain(64) > 0.0 && w.power_gain(64) > 0.0 && w.enbw_bins(64) >= 1.0);
    }
    assert_eq!(Window::default(), Window::Hann);
}
