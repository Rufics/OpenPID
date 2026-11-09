// node rotorflight.mjs <rotorflight/rotorflight-blackbox checkout> <label> <out.json> <log> [--csv <dir>]
// Decodes a log with Rotorflight's Blackbox Explorer parser. That fork still uses
// classic browser scripts sharing globals, so they are run in one VM context in the
// order of its index.html; jQuery, used only to tag the page with the firmware, is
// a stub that ignores every call (`$.extend` is the one function the parser needs).
import { createRequire } from "node:module";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { main } from "./reference.mjs";

const SCRIPTS = [
  "tools.js",
  "datastream.js",
  "decoders.js",
  "flightlog_fielddefs.js",
  "flightlog_fields_presenter.js",
  "flightlog_parser.js",
];

const extend = (target, ...sources) => Object.assign(target, ...sources);
const ignore = new Proxy(function () {}, {
  get: (_t, key) => (key === "extend" ? extend : ignore),
  apply: () => ignore,
});

const require = createRequire(import.meta.url);
const context = vm.createContext({ console, $: ignore, semver: require("semver") });
context.window = context;
const js = path.resolve(process.argv[2], "js");
for (const file of SCRIPTS) {
  vm.runInContext(fs.readFileSync(path.join(js, file), "utf8"), context, { filename: file });
}
main(vm.runInContext("FlightLogParser", context), process.argv.slice(2));
