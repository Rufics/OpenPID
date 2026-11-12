#!/usr/bin/env python3
"""Read the Blackbox format of a Betaflight release out of its source tree.

The log writer of `crates/openpid-fmt-blackbox/src/write/` describes each
Betaflight release it can write for: the fields of every frame (`version.rs`)
and the system-information header lines (`sysinfo/`). Those descriptions are
facts of the firmware source at the release's tag. This script reads them out
of a Betaflight checkout, so that a new release can be added (or an existing
one checked) without reading 2000 lines of C by eye:

    python3 tools/betaflight-blackbox/extract.py <betaflight checkout> <tag> [<tag> ...]

For every tag it prints

  * the field definitions of the I/P, G and H frames with every field the
    release can log, in the format of `src/write/field-tables.txt` (the unit
    tests of `version.rs` compare the Rust tables with that file);
  * every header line `blackboxWriteSysinfo` can write, in order, with the
    build options (`#ifdef`) it sits under. `CUSTOM` marks a line written by
    hand under a run-time condition; read those in the source.

It only reads names, numbers and preprocessor conditions; no firmware code is
copied into this repository. What it cannot see must still be read in the
source: the conditions of the fields (`testBlackboxConditionUncached`), the
bits of `fields_disabled_mask`, the units (`loadMainState`), the schedule
(`blackboxInit`, `blackboxLogIteration`). `docs/BLACKBOX-VERSIONS.md` lists
what differed so far and where each fact lives.
"""

import re
import subprocess
import sys

PREDICTORS = {
    "0": 0, "PREVIOUS": 1, "STRAIGHT_LINE": 2, "AVERAGE_2": 3, "MINTHROTTLE": 4,
    "MOTOR_0": 5, "INC": 6, "HOME_COORD": 7, "1500": 8, "VBATREF": 9,
    "LAST_MAIN_FRAME_TIME": 10, "MINMOTOR": 11,
}
ENCODINGS = {
    "SIGNED_VB": 0, "UNSIGNED_VB": 1, "NEG_14BIT": 3, "TAG8_8SVB": 6,
    "TAG2_3S32": 7, "TAG8_4S16": 8, "NULL": 9, "TAG2_3SVARIABLE": 10,
}


def show(repo, tag, path):
    """The file `path` at `tag`, or an empty string when it does not exist."""
    result = subprocess.run(
        ["git", "-C", repo, "show", f"{tag}:{path}"], capture_output=True, text=True
    )
    return result.stdout if result.returncode == 0 else ""


def walk(lines, start, end):
    """Lines between the regex `start` and the regex `end`, each with the
    preprocessor conditions it sits under (those opened after `start`)."""
    stack, base, active = [], [], False
    for line in lines:
        directive = re.match(r"#\s*(ifdef|ifndef|if|elif|else|endif)\b(.*)", line.strip())
        if directive:
            kind, rest = directive.group(1), directive.group(2).split("//")[0].strip()
            if kind == "ifdef":
                stack.append(rest)
            elif kind == "ifndef":
                stack.append("!" + rest)
            elif kind == "if":
                stack.append(rest)
            elif kind == "else":
                stack[-1] = "!(" + stack[-1] + ")"
            elif kind == "elif":
                stack[-1] = rest
            else:
                stack.pop()
            continue
        if not active:
            if re.search(start, line):
                active, base = True, list(stack)
            continue
        if re.search(end, line):
            active = False
            continue
        yield line, [f for f in stack if f not in base]


def fields(source, table):
    """The rows of one field table: name, sign, predictors, encodings."""
    rows = []
    for line, _ in walk(source, table + r"\[\] = \{", r"^\};"):
        row = re.match(r'\s*\{"(\w+)",\s*(-?\d+),\s*(\w+),(.*)\},?', line.split("//")[0])
        if not row:
            continue
        name, index, sign, rest = row.group(1), int(row.group(2)), row.group(3), row.group(4)
        predictors = re.findall(r"PREDICT\((\w+)\)", rest)
        encodings = [
            a or b
            for a, b in re.findall(r"ENCODING\((\w+)\)|FLIGHT_LOG_FIELD_ENCODING_(\w+)", rest)
        ]
        rows.append(
            {
                "name": name + (f"[{index}]" if index >= 0 else ""),
                "signed": "1" if sign == "SIGNED" else "0",
                "predictors": [str(PREDICTORS[p]) for p in predictors],
                "encodings": [str(ENCODINGS[e]) for e in encodings],
            }
        )
    return rows


def headers(source, names):
    """The header lines of `blackboxWriteSysinfo` with their conditions."""
    for line, conditions in walk(source, r"static bool blackboxWriteSysinfo", r"^\}"):
        macro = re.search(r'BLACKBOX_PRINT_HEADER_LINE\(\s*("([^"]*)"|(\w+))\s*,', line)
        custom = re.search(r'blackboxPrintfHeaderLine\(\s*("([^"]*)"|(\w+))\s*,', line)
        found = macro or custom
        if not found:
            continue
        name = found.group(2) if found.group(2) is not None else names.get(found.group(3))
        conditions = [c for c in conditions if c != "!UNIT_TEST"]
        yield name or found.group(3), conditions + (["CUSTOM"] if custom else [])


def main(repo, tags):
    for tag in tags:
        source = show(repo, tag, "src/main/blackbox/blackbox.c").split("\n")
        names = dict(
            re.findall(
                r'#define\s+(PARAM_NAME_\w+)\s+"([^"]*)"',
                show(repo, tag, "src/main/fc/parameter_names.h"),
            )
        )
        print(f"# {tag}: field definitions")
        for table, letter in [
            ("blackboxMainFields", "I"),
            ("blackboxGpsGFields", "G"),
            ("blackboxGpsHFields", "H"),
        ]:
            rows = fields(source, table)
            columns = [
                ("name", [r["name"] for r in rows]),
                ("signed", [r["signed"] for r in rows]),
                ("predictor", [r["predictors"][0] for r in rows]),
                ("encoding", [r["encodings"][0] for r in rows]),
            ]
            if letter == "I":
                columns.append(("P predictor", [r["predictors"][1] for r in rows]))
                columns.append(("P encoding", [r["encodings"][1] for r in rows]))
            for row, values in columns:
                print(f"{tag} {letter} {row}: {','.join(values)}")
        print(f"# {tag}: header lines")
        for name, conditions in headers(source, names):
            print(f"{name}\t{' & '.join(conditions)}")


if __name__ == "__main__":
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2:])
