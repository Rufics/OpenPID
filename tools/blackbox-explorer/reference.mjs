// Reference data for crates/openpid-cli/tests/explorer_conformance.rs, from the
// Blackbox Explorer parser (see tools/blackbox-explorer/README.md).
//
// The parser is handed in by betaflight.mjs or rotorflight.mjs; this module
// only drives it the way Explorer's own FlightLogIndex does (parseHeader, then
// parseLogData for every log in the file) and records what it delivers:
// every valid main frame, slow, GPS and GPS-home frame and every event, with
// the number of main frames seen before it.
//
// Main, slow, GPS and GPS-home frames are stored as FNV-1a 64 digests per
// field and per chunk of CHUNK rows (each value as a little-endian i64), so the
// whole log is compared without committing megabytes; events are stored in
// full. `--csv <dir>` also writes every main frame for a look at a difference.

import fs from "node:fs";
import path from "node:path";

export const CHUNK = 1024;

const FNV_OFFSET = 0xcbf29ce484222325n;
const FNV_PRIME = 0x100000001b3n;
const MASK = (1n << 64n) - 1n;

function fnvAdd(h, v) {
  let x = BigInt.asUintN(64, BigInt(v));
  for (let i = 0; i < 8; i++) {
    h ^= x & 0xffn;
    h = (h * FNV_PRIME) & MASK;
    x >>= 8n;
  }
  return h;
}

const hex = (h) => h.toString(16).padStart(16, "0");

function integer(v, what) {
  if (typeof v === "bigint") return v;
  if (!Number.isInteger(v)) throw new Error(`${what}: not an integer: ${v}`);
  return v;
}

function startsOf(data, marker) {
  const starts = [];
  outer: for (let i = 0; i <= data.length - marker.length; i++) {
    for (let j = 0; j < marker.length; j++) if (data[i + j] !== marker[j]) continue outer;
    starts.push(i);
    i += marker.length - 1;
  }
  return starts;
}

// Decode every flight log of `data` with `Parser` (Explorer's FlightLogParser).
export function decodeLogs(Parser, data) {
  const starts = startsOf(data, Parser.prototype.FLIGHT_LOG_START_MARKER);
  starts.push(data.length);
  const logs = [];
  for (let i = 0; i + 1 < starts.length; i++) {
    const parser = new Parser(data);
    const log = { error: null, main: [], slow: [], gps: [], home: [], events: [], defs: {} };
    try {
      parser.parseHeader(starts[i], starts[i + 1]);
      parser.onFrameReady = (valid, frame, type) => {
        // G frames before the first home frame count too (Explorer's FlightLog keeps them)
        if (!frame || (!valid && type !== "G")) return;
        const at = log.main.length;
        switch (type) {
          case "I":
          case "P":
            log.main.push([type === "I" ? 1 : 0, ...frame.map((v) => integer(v, "main frame"))]);
            break;
          case "S":
            log.slow.push([at, ...frame.map((v) => integer(v, "slow frame"))]);
            break;
          case "G":
            log.gps.push([at, ...frame.map((v) => integer(v, "GPS frame"))]);
            break;
          case "H":
            log.home.push([at, ...frame.map((v) => integer(v, "GPS home frame"))]);
            break;
          case "E":
            log.events.push({ at, event: frame.event, data: frame.data ?? {} });
            break;
        }
      };
      parser.parseLogData(false);
    } catch (e) {
      log.error = String(e);
    }
    for (const k of ["I", "S", "G", "H"]) log.defs[k] = parser.frameDefs?.[k]?.name ?? [];
    logs.push(log);
  }
  return logs;
}

// FNV-1a 64 digests of `rows` per column, one per chunk of CHUNK rows.
function digestColumns(columns, rows) {
  const digests = Object.fromEntries(columns.map((c) => [c, []]));
  for (let start = 0; start < rows.length; start += CHUNK) {
    const chunk = rows.slice(start, start + CHUNK);
    columns.forEach((name, c) => {
      let h = FNV_OFFSET;
      for (const r of chunk) h = fnvAdd(h, r[c]);
      digests[name].push(hex(h));
    });
  }
  return digests;
}

const table = (fields, rows, extra) => ({
  fields,
  rows: rows.length,
  digests: digestColumns([...extra, ...fields], rows),
});

// The committed form of one log. Every table is stored as digests; events in full.
export function summarise(log) {
  const fields = log.defs.I;
  return {
    error: log.error,
    frames: log.main.length,
    first_time: log.main.length ? log.main[0][1 + fields.indexOf("time")] : null,
    fields,
    digests: digestColumns(["(intra)", ...fields], log.main),
    slow: table(log.defs.S, log.slow, ["(at)"]),
    gps: table(log.defs.G, log.gps, ["(at)"]),
    gps_home: table(log.defs.H, log.home, ["(at)"]),
    events: log.events,
  };
}

// JSON with one line per array of scalars, so diffs stay readable.
export function stringify(value, indent = "") {
  const next = indent + "  ";
  const scalar = (v) => v === null || typeof v !== "object";
  const json = (v) => JSON.stringify(v, (_k, x) => (typeof x === "bigint" ? Number(x) : x));
  if (scalar(value)) return json(value);
  if (Array.isArray(value)) {
    if (value.every(scalar)) return json(value);
    if (value.length === 0) return "[]";
    return "[\n" + value.map((v) => next + stringify(v, next)).join(",\n") + "\n" + indent + "]";
  }
  const keys = Object.keys(value);
  if (keys.length === 0) return "{}";
  if (keys.every((k) => scalar(value[k]))) return json(value);
  return "{\n" + keys.map((k) => `${next}${JSON.stringify(k)}: ${stringify(value[k], next)}`).join(",\n") + "\n" + indent + "}";
}

// Command line shared by betaflight.mjs and rotorflight.mjs:
//   node <loader> <explorer checkout> <explorer label> <out.json> <log file> [--csv <dir>]
export function main(Parser, argv) {
  const [, label, out, file, ...rest] = argv;
  const csvDir = rest[0] === "--csv" ? rest[1] : null;
  const data = new Uint8Array(fs.readFileSync(file));
  // Explorer reports every header it does not use on the console
  const { log: consoleLog, warn: consoleWarn } = console;
  console.log = console.warn = () => {};
  let logs;
  try {
    logs = decodeLogs(Parser, data);
  } finally {
    console.log = consoleLog;
    console.warn = consoleWarn;
  }
  const doc = {
    fixture: path.basename(file),
    explorer: label,
    chunk: CHUNK,
    logs: logs.map(summarise),
  };
  fs.writeFileSync(out, stringify(doc) + "\n");
  if (csvDir) {
    logs.forEach((log, i) => {
      const lines = ["intra," + log.defs.I.join(","), ...log.main.map((r) => r.join(","))];
      fs.writeFileSync(path.join(csvDir, `${doc.fixture}.${i}.csv`), lines.join("\n") + "\n");
    });
  }
  const frames = logs.map((l) => l.main.length);
  const errors = logs.filter((l) => l.error).length;
  console.log(`${doc.fixture}: ${logs.length} log(s), ${frames.reduce((a, b) => a + b, 0)} main frames${errors ? `, ${errors} with errors` : ""}`);
}
