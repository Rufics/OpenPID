# Calibration decisions

Part of #167 (epic #166).

Every disagreement between the reference tool, OpenPID and the exact value
is recorded here, one entry each. **OpenPID's behaviour changes only after
the owner has chosen**: an entry without a decision changes no code.

An entry compares three things for one synthetic probe:

- what **the reference tool** shows, as read from the owner's screenshots
  (the numbers are kept in `tests/calibration/`, the screenshots are not
  committed);
- what **OpenPID** shows for the same probe;
- **the exact value**, which is known because the probe is synthetic.

The quantities are the rows of [INVENTORY.md](INVENTORY.md); the rounds that
produce the evidence are in [ROUNDS.md](ROUNDS.md).

## Process decisions

Taken by the owner on 2026-11-11:

1. The probe tool is a workspace crate, `crates/openpid-probe`, not a Python
   tool.
2. A pack reaches the owner as a zip and is reproducible from the
   repository.
3. Screenshots of the reference tool are not committed, only the numbers
   read from them (`tests/calibration/`).
4. The order of the areas: Spectrum, Step response, Freq × throttle,
   Freq × time, the rest.

## How an entry is written

- The id is `D-001`, `D-002`, … in the order the entries are opened; an id
  is not reused.
- "The exact value" is what the probe was built to give, with its
  derivation in one line. Where no exact value exists (a display choice,
  such as a default colour range), the field says so.
- An entry is finished when it names the owner's decision with its date and
  the pull request that carried it out.

## Entry template

```markdown
### D-000 Short title

| Field | |
|---|---|
| Id | D-000 |
| Area | the area and the row of INVENTORY.md, for example "Spectrum, S1" |
| Probe | the probe (name, round, pack commit) and what it contains |
| The reference tool shows | the number or curve read from the screenshot, with the file in `tests/calibration/` |
| OpenPID shows | the number or curve for the same probe, with the test or command that gives it |
| The exact value | the value the probe was built to give and why; or "none: a display choice" |
| Options | A. … B. … C. … (what a user would see with each) |
| Recommendation | the option proposed and the reason |
| The owner's decision | the option chosen, the date (YYYY-MM-DD) |
| Carried out by | the pull request |
```

## Register

No entries yet.

The open points at the end of [INVENTORY.md](INVENTORY.md) concern OpenPID's
own mathematics. Whether one of them becomes an entry here is the owner's
call.
