// Node module hooks that load Blackbox Explorer's parser (betaflight/blackbox-log-viewer,
// Vite-style ES modules) straight from its checkout, without its build:
// - `./module` imports get the `.js` the Vite resolver would add;
// - `semver` comes from this directory's node_modules (Explorer's own dependency);
// - the Pinia settings store, used only to format values for display, is a stub;
// - flightlog_parser.js gets the 32-bit wrap of the MINTHROTTLE and MINMOTOR
//   predictors back, lost in Explorer PR #862 (see README.md). If the two lines are
//   not found (Explorer changed), loading fails instead of silently skipping it.

import { fileURLToPath, pathToFileURL } from "node:url";
import fs from "node:fs";

let explorerSrc;

export async function initialize(data) {
  explorerSrc = pathToFileURL(data.explorerSrc + "/").href;
}

const SETTINGS_STUB = "data:text/javascript,export const useSettingsStore = () => ({ userSettings: {} });";

export const REVERTS = [
  ["value = Math.trunc(value) + this.sysConfig.minthrottle;", "value = (value | 0) + this.sysConfig.minthrottle;"],
  [
    "value = Math.trunc(value) + Math.trunc(this.sysConfig.motorOutput[0]);",
    "value = (value | 0) + (this.sysConfig.motorOutput[0] | 0);",
  ],
];

export async function resolve(specifier, context, next) {
  const fromExplorer = context.parentURL?.startsWith(explorerSrc);
  if (fromExplorer && /stores\/settings(\.js)?$/.test(specifier)) {
    return { url: SETTINGS_STUB, shortCircuit: true };
  }
  if (fromExplorer && specifier === "semver") {
    return next(specifier, { ...context, parentURL: import.meta.url });
  }
  if (fromExplorer && specifier.startsWith(".") && !/\.[cm]?js$/.test(specifier)) {
    return next(specifier + ".js", context);
  }
  return next(specifier, context);
}

export async function load(url, context, next) {
  if (!url.startsWith(explorerSrc)) return next(url, context);
  let source = fs.readFileSync(fileURLToPath(url), "utf8");
  if (url.endsWith("/flightlog_parser.js")) {
    for (const [from, to] of REVERTS) {
      const n = source.split(from).length - 1;
      if (n !== 1) throw new Error(`flightlog_parser.js: expected the PR #862 line once, found ${n}: ${from}`);
      source = source.replace(from, to);
    }
  }
  return { format: "module", source, shortCircuit: true };
}
