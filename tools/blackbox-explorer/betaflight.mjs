// node betaflight.mjs <betaflight/blackbox-log-viewer checkout> <label> <out.json> <log> [--csv <dir>]
// Decodes a log with Betaflight's Blackbox Explorer parser (see hooks.mjs, README.md).
import { register } from "node:module";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { main } from "./reference.mjs";

const explorerSrc = path.resolve(process.argv[2], "src");
register("./hooks.mjs", import.meta.url, { data: { explorerSrc } });
const { FlightLogParser } = await import(pathToFileURL(path.join(explorerSrc, "flightlog_parser.js")).href);
main(FlightLogParser, process.argv.slice(2));
