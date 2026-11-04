#!/usr/bin/env python3
"""Build the CI summary comment for a pull request.

Reads the lcov report produced by the coverage job and the SonarCloud
analysis of the pull request (public web API, the token is only needed to
wait for the analysis to be processed), and prints Markdown on stdout.

Environment:
  LCOV               path to lcov.info (missing file: coverage section says so)
  COVERAGE_MIN       gate in percent (default 90)
  SONAR_PROJECT      Sonar project key
  SONAR_HOST         default https://sonarcloud.io
  SONAR_TOKEN        optional; lets the script wait for the background task
  SONAR_CE_TASK_ID   optional; the scanner's compute-engine task to wait for
  PR_NUMBER          pull request number
  BASE_BRANCH        base branch (coverage delta is measured against it)
  COVERAGE_RESULT    result of the coverage job (success/failure/skipped)
  SONAR_RESULT       result of the sonar job
  RUN_URL            link to the workflow run

Standard library only: it runs on a bare runner.
"""

from __future__ import annotations

import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import defaultdict

MARKER = "<!-- openpid-ci-summary -->"
RATINGS = {"1": "A", "2": "B", "3": "C", "4": "D", "5": "E"}
CONDITION_NAMES = {
    "new_coverage": "Coverage on new code",
    "new_duplicated_lines_density": "Duplication on new code",
    "new_reliability_rating": "Reliability (new code)",
    "new_security_rating": "Security (new code)",
    "new_maintainability_rating": "Maintainability (new code)",
    "new_security_hotspots_reviewed": "Security hotspots reviewed",
    "new_bugs": "Bugs (new code)",
    "new_vulnerabilities": "Vulnerabilities (new code)",
    "new_code_smells": "Code smells (new code)",
}


def env(name: str, default: str = "") -> str:
    return os.environ.get(name, default).strip()


def icon(ok: bool | None) -> str:
    if ok is None:
        return "⚪"
    return "✅" if ok else "❌"


# ----------------------------------------------------------------- coverage


def parse_lcov(path: str) -> dict[str, tuple[int, int]]:
    """`{file: (lines_found, lines_hit)}` from an lcov tracefile."""
    files: dict[str, tuple[int, int]] = {}
    if not os.path.exists(path):
        return files
    current = None
    found = hit = 0
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.rstrip("\n")
            if line.startswith("SF:"):
                current = line[3:]
                found = hit = 0
            elif line.startswith("LF:"):
                found = int(line[3:])
            elif line.startswith("LH:"):
                hit = int(line[3:])
            elif line == "end_of_record" and current:
                files[current] = (found, hit)
                current = None
    return files


def pct(hit: int, found: int) -> float:
    return 100.0 * hit / found if found else 100.0


def crate_of(path: str) -> str:
    parts = path.replace("\\", "/").split("/")
    if "crates" in parts:
        i = parts.index("crates")
        if i + 1 < len(parts):
            return parts[i + 1]
    return parts[0] if parts else path


def coverage_section(lcov: str, gate: float, result: str) -> str:
    files = parse_lcov(lcov)
    if not files:
        return f"### {icon(None)} Coverage\n\nNo lcov report found (coverage job: `{result}`).\n"
    total_found = sum(f for f, _ in files.values())
    total_hit = sum(h for _, h in files.values())
    total = pct(total_hit, total_found)
    ok = total >= gate
    by_crate: dict[str, list[int]] = defaultdict(lambda: [0, 0])
    for path, (found, hit) in files.items():
        c = by_crate[crate_of(path)]
        c[0] += found
        c[1] += hit
    out = [f"### {icon(ok)} Coverage: {total:.1f} % lines (gate ≥ {gate:.0f} %, cargo-llvm-cov)", ""]
    out.append("| Crate | Lines | Covered | % |")
    out.append("|---|---:|---:|---:|")
    for crate in sorted(by_crate):
        found, hit = by_crate[crate]
        p = pct(hit, found)
        flag = "" if p >= gate else " ⚠️"
        out.append(f"| `{crate}` | {found} | {hit} | {p:.1f}{flag} |")
    out.append(f"| **total** | {total_found} | {total_hit} | **{total:.1f}** |")
    weakest = sorted(
        ((pct(h, f), path, f - h) for path, (f, h) in files.items() if f),
        key=lambda t: t[0],
    )[:5]
    if weakest:
        out.append("")
        out.append("<details><summary>Least covered files</summary>")
        out.append("")
        out.append("| File | % | Missed lines |")
        out.append("|---|---:|---:|")
        for p, path, missed in weakest:
            short = path.replace("\\", "/")
            if "/crates/" in short:
                short = short[short.index("/crates/") + 1 :]
            out.append(f"| `{short}` | {p:.1f} | {missed} |")
        out.append("")
        out.append("</details>")
    return "\n".join(out) + "\n"


# ----------------------------------------------------------------- sonar


class Sonar:
    def __init__(self, host: str, project: str, token: str) -> None:
        self.host = host.rstrip("/")
        self.project = project
        self.token = token

    def get(self, path: str, **params: str) -> dict | None:
        url = f"{self.host}/api/{path}?{urllib.parse.urlencode(params)}"
        req = urllib.request.Request(url)
        if self.token:
            req.add_header("Authorization", f"Bearer {self.token}")
        try:
            with urllib.request.urlopen(req, timeout=30) as r:
                return json.load(r)
        except (urllib.error.URLError, json.JSONDecodeError, TimeoutError) as e:
            print(f"sonar {path}: {e}", file=sys.stderr)
            return None

    def wait_for_task(self, task_id: str, timeout_s: int = 600) -> str | None:
        """Analysis id once the compute-engine task is done (needs the token)."""
        if not task_id or not self.token:
            return None
        t0 = time.time()
        while time.time() - t0 < timeout_s:
            data = self.get("ce/task", id=task_id)
            task = (data or {}).get("task", {})
            status = task.get("status")
            if status == "SUCCESS":
                return task.get("analysisId")
            if status in ("FAILED", "CANCELED"):
                print(f"sonar task {task_id}: {status}", file=sys.stderr)
                return None
            time.sleep(10)
        print("sonar task: timed out", file=sys.stderr)
        return None

    def measures(self, **scope: str) -> dict[str, str]:
        data = self.get(
            "measures/component",
            component=self.project,
            metricKeys="coverage,new_coverage,bugs,vulnerabilities,code_smells,"
            "security_hotspots,duplicated_lines_density,ncloc",
            **scope,
        )
        out: dict[str, str] = {}
        for m in ((data or {}).get("component") or {}).get("measures", []):
            v = m.get("value")
            if v is None:
                v = (m.get("period") or {}).get("value")
            if v is None and m.get("periods"):
                v = m["periods"][0].get("value")
            if v is not None:
                out[m["metric"]] = v
        return out


def fmt_condition(c: dict) -> tuple[str, str, str]:
    key = c.get("metricKey", "")
    name = CONDITION_NAMES.get(key, key)
    actual = c.get("actualValue", "–")
    threshold = c.get("errorThreshold", "")
    comp = c.get("comparator", "")
    if key.endswith("_rating"):
        actual = RATINGS.get(actual, actual)
        threshold = RATINGS.get(threshold, threshold)
        want = f"≤ {threshold}" if threshold else ""
    else:
        unit = " %" if "coverage" in key or "density" in key or "reviewed" in key else ""
        actual = f"{actual}{unit}" if actual != "–" else actual
        sign = {"LT": "≥", "GT": "≤"}.get(comp, comp)
        want = f"{sign} {threshold}{unit}" if threshold else ""
    return name, actual, want


def sonar_section(sonar: Sonar, pr: str, base: str, task_id: str, result: str) -> str:
    if result == "skipped":
        return "### ⚪ SonarCloud\n\nSkipped (no `SONAR_TOKEN`).\n"
    analysis_id = sonar.wait_for_task(task_id)
    if analysis_id:
        gate = sonar.get("qualitygates/project_status", analysisId=analysis_id)
    else:
        gate = sonar.get("qualitygates/project_status", projectKey=sonar.project, pullRequest=pr)
    status = ((gate or {}).get("projectStatus") or {}).get("status")
    conditions = ((gate or {}).get("projectStatus") or {}).get("conditions", [])
    pr_m = sonar.measures(pullRequest=pr)
    base_m = sonar.measures(branch=base)
    dash = f"{sonar.host}/summary/new_code?id={sonar.project}&pullRequest={pr}"
    if status is None:
        return (
            f"### {icon(None)} SonarCloud\n\nAnalysis not available yet "
            f"(sonar job: `{result}`). [Dashboard]({dash})\n"
        )
    ok = status == "OK"
    out = [f"### {icon(ok)} SonarCloud quality gate: **{status}** · [dashboard]({dash})", ""]
    out.append("| Condition | Actual | Required | |")
    out.append("|---|---:|---:|:-:|")
    for c in conditions:
        name, actual, want = fmt_condition(c)
        out.append(f"| {name} | {actual} | {want} | {icon(c.get('status') == 'OK')} |")
    out.append("")
    cov = pr_m.get("coverage")
    cov_txt = f"{cov} %" if cov else "–"
    if cov and base_m.get("coverage"):
        try:
            delta = float(cov) - float(base_m["coverage"])
            cov_txt += f" ({delta:+.1f} vs `{base}`)"
        except ValueError:
            pass
    out.append("| Bugs | Vulnerabilities | Code smells | Hotspots | Duplication | Coverage (Sonar) |")
    out.append("|---:|---:|---:|---:|---:|---:|")
    out.append(
        "| {} | {} | {} | {} | {} % | {} |".format(
            pr_m.get("bugs", "0"),
            pr_m.get("vulnerabilities", "0"),
            pr_m.get("code_smells", "0"),
            pr_m.get("security_hotspots", "0"),
            pr_m.get("duplicated_lines_density", "0"),
            cov_txt,
        )
    )
    issues = sonar.get(
        "issues/search",
        componentKeys=sonar.project,
        pullRequest=pr,
        resolved="false",
        ps="100",
        s="SEVERITY",
        asc="false",
    )
    items = (issues or {}).get("issues", [])
    total = (issues or {}).get("total", len(items))
    out.append("")
    if not items:
        out.append("No open issues on this pull request. 🎉")
    else:
        out.append(f"<details open><summary><b>{total} open issue(s)</b></summary>")
        out.append("")
        out.append("| Severity | Type | File | Rule | Message |")
        out.append("|---|---|---|---|---|")
        for i in items[:50]:
            comp = i.get("component", "").split(":", 1)[-1]
            line = i.get("line")
            where = f"`{comp}:{line}`" if line else f"`{comp}`"
            rule = i.get("rule", "")
            rule_url = f"{sonar.host}/project/issues?id={sonar.project}&pullRequest={pr}&open={i.get('key', '')}"
            msg = i.get("message", "").replace("|", "\\|")
            out.append(
                f"| {i.get('severity', '').title()} | {i.get('type', '').replace('_', ' ').title()} "
                f"| {where} | [{rule}]({rule_url}) | {msg} |"
            )
        if total > 50:
            out.append(f"| … | | | | {total - 50} more on the dashboard |")
        out.append("")
        out.append("</details>")
    hotspots = sonar.get(
        "hotspots/search", projectKey=sonar.project, pullRequest=pr, status="TO_REVIEW", ps="50"
    )
    hs = (hotspots or {}).get("hotspots", [])
    if hs:
        out.append("")
        out.append(f"<details open><summary><b>{len(hs)} security hotspot(s) to review</b></summary>")
        out.append("")
        out.append("| Probability | File | Message |")
        out.append("|---|---|---|")
        for h in hs:
            comp = h.get("component", "").split(":", 1)[-1]
            out.append(
                f"| {h.get('vulnerabilityProbability', '').title()} | `{comp}:{h.get('line', '')}` "
                f"| {h.get('message', '').replace('|', '\\|')} |"
            )
        out.append("")
        out.append("</details>")
    return "\n".join(out) + "\n"


# ----------------------------------------------------------------- main


def main() -> None:
    gate = float(env("COVERAGE_MIN", "90") or 90)
    sonar = Sonar(env("SONAR_HOST", "https://sonarcloud.io"), env("SONAR_PROJECT"), env("SONAR_TOKEN"))
    sonar_md = sonar_section(
        sonar,
        env("PR_NUMBER"),
        env("BASE_BRANCH", "master"),
        env("SONAR_CE_TASK_ID"),
        env("SONAR_RESULT", "unknown"),
    )
    cov_md = coverage_section(env("LCOV", "lcov.info"), gate, env("COVERAGE_RESULT", "unknown"))
    run = env("RUN_URL")
    footer = f"_Updated by CI{f' · [run]({run})' if run else ''}._"
    print("\n".join([MARKER, "## CI summary", "", cov_md, sonar_md, footer]))


if __name__ == "__main__":
    main()
