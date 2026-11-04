#!/usr/bin/env python3
"""Generate NumPy/SciPy reference fixtures for openpid-dsp.

Run from the repository root with a Python that has numpy and scipy:

    python3 -I tools/reference/make_fixtures.py

Writes tests/reference/*.json. Each fixture holds the exact inputs and the
expected outputs; the Rust tests compare with the tolerances noted here.
The functions below re-implement the OpenPID *specification* of each
algorithm (not the Rust code), so they are an independent oracle.
"""
import json
import math
import os
import sys

import numpy as np
from scipy import signal

OUT = os.path.join(os.path.dirname(__file__), "..", "..", "tests", "reference")
os.makedirs(OUT, exist_ok=True)


def dump(name, obj):
    path = os.path.join(OUT, name + ".json")
    with open(path, "w") as f:
        json.dump(obj, f, separators=(",", ":"))
    print("wrote", os.path.relpath(path))


rng = np.random.default_rng(20261104)


def signal_mix(n, fs):
    t = np.arange(n) / fs
    x = (3.0 * np.sin(2 * np.pi * 50.0 * t)
         + 1.2 * np.sin(2 * np.pi * 173.0 * t + 0.4)
         + 0.5 * rng.standard_normal(n)
         + 0.7)
    return t, x


# --- windows ---------------------------------------------------------------
windows = {}
for n in (16, 64, 1000):
    windows[f"hann_{n}"] = signal.get_window("hann", n, fftbins=True).tolist()
    windows[f"hamming_{n}"] = signal.get_window("hamming", n, fftbins=True).tolist()
    windows[f"blackmanharris_{n}"] = signal.get_window("blackmanharris", n, fftbins=True).tolist()
    windows[f"tukey_0.5_{n}"] = signal.get_window(("tukey", 0.5), n, fftbins=True).tolist()
dump("windows", windows)

# --- rfft / amplitude spectrum --------------------------------------------
fs = 2000.0
t, x = signal_mix(1000, fs)
spec = np.fft.rfft(x)
w = signal.get_window("hann", 1000, fftbins=True)
amp = np.abs(np.fft.rfft(x * w)) / w.sum()
amp[1:500] *= 2.0
dump("rfft", {
    "fs": fs,
    "x": x.tolist(),
    "rfft_re": spec.real.tolist(),
    "rfft_im": spec.imag.tolist(),
    "freqs": np.fft.rfftfreq(1000, 1 / fs).tolist(),
    "amplitude_hann": amp.tolist(),
})

# --- welch -----------------------------------------------------------------
t, x = signal_mix(8192, fs)
cases = []
for nperseg, noverlap in ((256, 128), (1024, 512), (300, 100)):
    f, p = signal.welch(x, fs=fs, window="hann", nperseg=nperseg, noverlap=noverlap,
                        detrend="constant", scaling="density", average="mean")
    cases.append({"nperseg": nperseg, "noverlap": noverlap, "freq": f.tolist(), "power": p.tolist()})
dump("welch", {"fs": fs, "x": x.tolist(), "cases": cases})

# --- stft (amplitude, hann, hop) ------------------------------------------
t, x = signal_mix(3000, fs)
nperseg, hop = 400, 100
w = signal.get_window("hann", nperseg, fftbins=True)
frames = []
times = []
start = 0
while start + nperseg <= len(x):
    seg = x[start:start + nperseg] * w
    a = np.abs(np.fft.rfft(seg)) / w.sum()
    a[1:nperseg // 2] *= 2.0
    frames.append(a.tolist())
    times.append((start + nperseg / 2) / fs)
    start += hop
dump("stft", {"fs": fs, "x": x.tolist(), "nperseg": nperseg, "hop": hop, "time": times,
              "freq": np.fft.rfftfreq(nperseg, 1 / fs).tolist(), "amp": frames})

# --- wiener deconvolution (spec re-implemented) ----------------------------
def next_smooth(n):
    while True:
        m = n
        for p in (2, 3, 5):
            while m % p == 0:
                m //= p
        if m == 1:
            return n
        n += 1


def regularisation(f, cutoff_hz, reg_low, reg_high, transition):
    lo = cutoff_hz * (1 - transition)
    hi = cutoff_hz * (1 + transition)
    if f <= lo:
        return reg_low
    if f >= hi:
        return reg_high
    s = 0.5 - 0.5 * math.cos(math.pi * (f - lo) / (hi - lo))
    return math.exp(math.log(reg_low) * (1 - s) + math.log(reg_high) * s)


fs = 1000.0
n = 2000
tau = 0.02
level = 0.0
xin = np.zeros(n)
for i in range(n):
    if i % 150 == 0:
        level = rng.uniform(-800, 800)
    xin[i] = level
yout = np.zeros(n)
alpha = (1 / fs) / tau
for i in range(1, n):
    yout[i] = yout[i - 1] + alpha * (xin[i] - yout[i - 1])
yout += 2.0 * rng.standard_normal(n)
params = {"cutoff_hz": 60.0, "reg_low": 0.1, "reg_high": 1e8, "transition": 0.2}  # window: hann
nfft = next_smooth(n + n // 2)
wdec = signal.get_window("hann", n, fftbins=True)  # periodic Hann on both signals
X = np.fft.rfft(np.concatenate([xin * wdec, np.zeros(nfft - n)]))
Y = np.fft.rfft(np.concatenate([yout * wdec, np.zeros(nfft - n)]))
freqs = np.fft.rfftfreq(nfft, 1 / fs)
lam = np.array([regularisation(f, **params) for f in freqs])
H = Y * np.conj(X) / (np.abs(X) ** 2 + lam)
h = np.fft.irfft(H, nfft)[:300]
step = np.cumsum(h)
dump("deconv", {"fs": fs, "input": xin.tolist(), "output": yout.tolist(), "resp_len": 300,
                "params": params, "nfft": nfft, "impulse": h.tolist(), "step": step.tolist(),
                "tau": tau})

# --- smoothing -------------------------------------------------------------
x = rng.standard_normal(50)
ma = []
for i in range(50):
    r = min(2, i, 49 - i)
    ma.append(float(np.mean(x[i - r:i + r + 1])))
z = rng.standard_normal((7, 11))  # ny=7 rows, nx=11 cols
kernel = np.ones((3, 5)) / 15.0
from scipy.ndimage import correlate
box = correlate(z, kernel, mode="constant", cval=0.0)
dump("smooth", {"x": x.tolist(), "moving_average_5": ma, "z": z.flatten().tolist(), "nx": 11, "ny": 7,
                "box_5x3": box.flatten().tolist()})

# --- resample --------------------------------------------------------------
t_us = np.cumsum(rng.integers(900, 1100, 200)).astype(np.int64)
t_us -= t_us[0]
t_us[120:] += 4000  # a gap
xv = np.sin(t_us / 1e6 * 2 * np.pi * 30.0) * 100
dt = 1000.0
grid = np.arange(0, t_us[-1] + 1e-9, dt)
interp = np.interp(grid, t_us, xv)
valid = []
for g in grid:
    j = int(np.searchsorted(t_us, g, side="right") - 1)
    if j + 1 >= len(t_us) or abs(t_us[j] - g) < 1e-9:
        valid.append(True)
    else:
        valid.append(bool(t_us[j + 1] - t_us[j] <= 1500.0))
dump("resample", {"time_us": t_us.tolist(), "x": xv.tolist(), "dt_us": dt, "max_gap_us": 1500.0,
                  "values": [float(v) if ok else 0.0 for v, ok in zip(interp, valid)], "valid": valid})
print("done")
