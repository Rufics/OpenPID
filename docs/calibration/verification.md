# Round 17: OpenPID as decided, held against the reference tool

Part of #166. Every area had changed after its last round (decisions D-001
to D-011). Round 17 holds OpenPID's own code, with every decision in,
against the reference tool on logs made for no question: rates, lengths,
plants and noise no earlier round used. OpenPID's results were computed
before the run, from one branch that merges every decision pull request
(#187, #188, #193–#199) with the writer (#177) and the probe tool (#180);
the pack records it as `verify-7f192b3`. The reference tool is version
0.94.

## The pack (2026-11-17)

Thirteen logs, written as Betaflight 2025.12 logs (#180, `fdb8812`):

| Log | Rate × length | For |
|---|---|---|
| `e1-spec-1k`, `e2-spec-4k` | 1 kHz × 26 s, 4 kHz × 34 s | the spectrum: flights with stick moves, a wandering throttle, motor noise that follows it, a resonance |
| `e3-step-2k`, `e4-step-8k` | 2 kHz × 28 s, 8 kHz × 22 s | the step response: random stick steps through plants no round flew |
| `e5-maps-2k` | 2 kHz × 47 s | both maps: the throttle wandering from 20 to 80 % |
| `e6-view-2k` | 2 kHz × 24 s | the main window |
| `i1-chan-2k` | 2 kHz × 24 s | every channel of the spectrum window's list with a tone of its own (S8) |
| `j1-path-2k` | 2 kHz × 28 s | the path of the stick: the RC command 40 ms later and half as large as the setpoint, feedforward pulses at every step |
| `x1-samp-2k`, `x2-samp-4k` | 2 and 4 kHz × 7 s | the line smoothing at other rates |
| `x3-len11-2k` … `x5-len13-2k` | 2 kHz × 11, 12, 13 s | where a short log stops being taken whole |

And a set of filters typed into the filter simulator that no round used
(4 kHz; pt2 at 210 Hz, biquad at 420 Hz, RPM notches, a notch at 330 Hz,
Q 2.5). 1 566 steps, 41 pictures, 90 vector copies.

**What came back:** all 1 566 steps in 9.5 minutes; 88 of the 90 copies.
The two of the "Motors" channel failed: that entry draws one large plot
where the plan looked for two.

## How it was read

The copies were held against OpenPID's results at the app's defaults:
the spectrum curve of #187 at "medium", the step response, both maps,
the filter curves of the set, the main window's lines. The comparison
first ran with `openpid-probe compare` (#180), which held the spectra
against the power density instead of the curve the app draws, and had
other defects (below). Each copy was therefore read again with one-off
readers outside the repository.

## What it showed

| Area | Copies | Result |
|---|---|---|
| Spectrum, `e1`, `e2` | 12 | OpenPID's curve: median difference 0.007 … 0.037 dB below 100 Hz, about 0.10 dB over the full range (largest 0.8 … 1.1 dB, in the wiggles of the noise floor; tones and levels the same). The difference is the stretch analysed (below); on the reference tool's stretch the rule of [spectrum.md](spectrum.md) gives every copy to its resolution (median 0.0033 dB) |
| Spectrum, every channel (S8, `i1`) | 18 | OpenPID's curve of the same channel: median 0.006 … 0.009 dB on every copy |
| Step response, `e3`, `e4` | 12 | curves within 0.001, Peaks within 0.0003 |
| Step response, the stick's path (`j1`) | 6 | the reference tool's response follows the logged setpoint on all three axes (median 0.009, 0.010, 0.022 from the closed form); the RC command would be 1.0 off, setpoint with feedforward up to 0.35. OpenPID follows the setpoint too, with the same numbers; Peaks within 0.0001 |
| Throttle map, `e5` | 5 | 99.4 … 99.9 % of the pixels within one colour step; the printed numbers to their three digits on three maps, in the last digit on three |
| Time map, `e5` | 3 | 98.3 … 99.4 % within one colour step (the reference tool's first two columns left out, D-006) |
| Short logs, both maps, `x3`–`x5` | 12 | 90 … 99.99 % within one colour step; a printed peak up to 3.7 % off |
| Main window, `e6`, `x1`, `x2` | 8 | the gyro and setpoint lines; the line smoothing "medium" of #199 within 0.15 … 0.21 deg/s. The motors as decided (D-008: OpenPID's percent stays) |
| Filter Sim | 8 | every printed delay to its last digit (0.97553, 0.51632, 0.31769, 0.15785, 0.10413, 0.18858 ms; the columns 1.4919 and 2.5072 ms); magnitude within 0.00014, phase within 0.14°, step within 0.0004 |

The questions the round asked:

- **S8, which field each channel reads.** Gyro: `gyroADC`; Gyro prefilt:
  `gyroUnfilt`; Dterm: `axisD`; Pterm: `axisP`; Fterm: `axisF`; Set point:
  `setpoint`; PID error: setpoint and filtered gyro (a spectrum cannot show
  its sign); PIDsum: P + I + D + F; debug: `debug[0]`. Each is the best
  match by 0.4 … 2 dB over every other field. Motors: one large plot of the
  eight motors, read from its picture, on the logged number over 20 (0.5 dB
  below OpenPID's percent, D-008). Dterm prefilt: the change of the
  filtered gyro per sample times a factor, not a logged field.
- **The stick's path**: the logged setpoint, without the feedforward and
  not the RC command.
- **The line smoothing is counted in samples**: a span of 21 samples at
  "med" at 2 and at 4 kHz, as at 1 kHz (a span in milliseconds would have
  halved its peak at each doubling of the rate). #199 counts samples.
- **Short logs**: the logs of 11, 12 and 13 s are trimmed by both maps;
  round 16's log of 10 s was taken whole. The switch lies in (10, 11] s.

## The one disagreement: the stretch

OpenPID's analyses start their default stretch at 1.000 s, the reference
tool two samples later; at 1 and 4 kHz OpenPID's stretch is also one
sample longer.

| | First sample | Samples |
|---|---|---|
| The reference tool | `fs + 2` (`fs + 3` on some logs) | `fs·(T − 2) − 1` |
| OpenPID's automatic stretch | `fs` | `fs·(T − 2)`, or one less where the end time is cut |

It moves the spectrum curve by up to 0.38 dB below 100 Hz and about 1 dB
on noise over the full range, and 1 … 4 % of the pixels of a map. On the
reference tool's stretch every copy agrees to its resolution. The step
response is not affected. The automatic stretch is also a different rule
on a flown log (it leaves out the time on the ground) and on a log under
11 s (the reference tool takes it whole). What OpenPID should do is entry
D-012 of [DECISIONS.md](DECISIONS.md): the owner kept the automatic
stretch as the default and added the reference tool's as a choice
(2026-11-18, #200). OpenPID's default is therefore unchanged by round 17.

## The tools

`compare` (#180) gave 15 PASS and 59 FAIL; the readings above replace it.
What it has to learn before its report can stand for a round:

1. hold the spectra against the curve the app draws, not the density;
2. not merge the dotted "Gyro prefilt" line into the solid one (same pen);
3. take the step response and the Peak of the round's own log: the window
   keeps every log run before it, one colour each;
4. take a map's printed numbers from OpenPID's result, not from pixels;
5. not convert the time map to decibels twice;
6. place the cells of a map by pixel and count only the middle of a cell;
7. read a map copied as a dithered 16-bit picture (four copies);
8. hold the line smoothing against #199;
9. mark the motors as a decided difference (D-008);
10. compute the filter curves of the manifest's set;
11. a tolerance on the median of a spectrum, as fine as a copy reads.

And the capture plan: the "Motors" channel's single plot, and a "Reset" of
the step response window before each log.

**After the fixes** (#180, `3e1ec34`, 2026-11-18): `compare` on the same 88
copies gives 69 PASS, 19 decided differences and no FAIL, the readings
above to their digits. The decided differences are D-012 on 18 copies
(the nine spectrum curves of `e1` and `e2` over the full range and below
100 Hz at 1 kHz, four throttle maps and five time maps of the unfiltered
gyro; on the reference tool's stretch they agree, the spectra to 0.002 …
0.007 dB in the median and the maps on 99.47 … 100 % of their pixels) and
D-008 on the main window's lower plot. The "Motors" copy of the fixed plan
was not in this run.
