# Development process: Scrum on GitHub

One Product Owner and development with an AI assistant. The process is
deliberately light: everything that does not help decide faster is left out.

## Artifacts

| Scrum | Here | Where |
|-------|------|-------|
| Product Backlog | repository issues | [Issues](https://github.com/Rufics/OpenPID/issues), the *Backlog* view of the Project |
| Epics / release goals | **Milestones** = the phases of the plan (`docs/DESIGN.md` §12) plus an issue labelled `epic` with sub-issues | [Milestones](https://github.com/Rufics/OpenPID/milestones) |
| Sprint Backlog | the *Sprint* field (iteration, 2 weeks) of the Project | the *Sprint board* view |
| Increment | squash-merge into `master` → release-plz → tag and release with binaries | `docs/RELEASING.md` |
| Definition of Done | below | every issue carries this block |
| PR ↔ issue link | mandatory: `Closes #N` / `Part of #N` in the PR description | the "PR references an issue" step in CI; a required status check in the `master` ruleset |
| Closing a PR without merging | only with a comment: the reason plus a link to the replacement PR or issue | by hand; example: #107 → #110 |

### Milestones

| Milestone | Content | Criterion |
|-----------|---------|-----------|
| `v0.2 — Phase 2a: formats` | INAV, ArduPilot follow-ups, Rotorflight, missing fixtures | every supported firmware decodes its fixtures cleanly |
| `v0.3 — Phase 2b: filters & overlays` | `openpid-sim`, Filter Sim, RPM / dynamic-notch overlay 2D/3D | a log with `gyroUnfilt` reproduces its own filtered gyro |
| `v0.4 — Phase 2c: Pro parity` | RPM estimator, motor noise, generator, player, projects, export, i18n, touch, performance | every row of the `docs/DESIGN.md` §2 table is ✓ |
| `v1.0 — Phase 3: recommendations` | recommender, diff/exporter, (optional) MSP | a user applies a recommendation |

An issue without a milestone is unassigned backlog (for example research
waiting for sample logs).

### Labels

- **Type** (one): `type: feature`, `bug`, `type: tech-debt`, `type: test`, `type: ci`, `type: research`, `documentation`; `epic` is an umbrella issue with sub-issues.
- **Area** (one or more): `area: core | dsp | analysis | sim | gui | cli | fmt-blackbox | fmt-ardupilot | ci | docs`.
- **Priority** (one): `priority: P0` (blocks the sprint or the release) … `P3` (when there is time or samples).
- `blocked`: waiting for something external (sample logs, access, upstream); `good first issue`, `help wanted`: for the community.

Status, size and sprint are **not** duplicated as labels: they live in the
Project fields.

## GitHub Project (the Scrum dashboard)

The Project is created once by `tools/github-project.sh` (it needs
`gh auth refresh -s project`; GraphQL is not reachable from CI or agent
sessions, so the owner runs it locally). The script creates the project and its
fields, links it to the repository and adds every open issue. The *Sprint*
field (iteration) and the views cannot be created through the API; they are set
up in the UI from this list:

| View | Layout | Settings |
|------|--------|----------|
| **Backlog** | Table | group by *Milestone*, sort by *Priority* ↑ then *Size*; fields: Title, Status, Priority, Size, Sprint, Area, Labels |
| **Sprint board** | Board by *Status* | filter `sprint:@current`; columns Backlog → Ready → In progress → In review → Done; swimlanes off |
| **Roadmap** | Roadmap | date field = *Sprint*, group by *Milestone*, zoom = month |
| **Bugs** | Table | filter `label:bug is:open`, sort by Priority |
| **Blocked** | Table | filter `label:blocked is:open` |

Fields: *Status* (built in: Backlog, Ready, In progress, In review, Done),
*Priority* (P0–P3, mirrors the label), *Size* (XS ≈ ½ day, S ≈ 1, M ≈ 2–3,
L ≈ 5, XL = split it), *Sprint* (iteration, 2 weeks, starting on Monday),
*Area*.

Built-in Project workflows (Settings → Workflows) worth enabling: *Item added
to project → Status Backlog*, *Pull request merged → Done*, *Item closed →
Done*, *Auto-add to project* for `is:issue is:open`. Insights: a *Burn up*
chart over the Sprint field (Settings → Insights → New chart, layout Burn up,
X = time, group by Status).

## Rituals (the minimum)

| Ritual | When | What happens |
|--------|------|--------------|
| Sprint planning | every 2 weeks, at the sprint start | the PO picks the sprint goal (one sentence in the iteration description) and moves issues from *Ready* into the sprint by priority and size |
| Refinement | asynchronous, before planning | every *Backlog* issue that aims at the sprint has Context / Scope / Acceptance and a Size → status *Ready* |
| Daily | asynchronous | a comment on the issue or PR: what was done, what blocks; nothing separate |
| Review | merging a PR | the PR summary comment (coverage, Sonar), a demonstration through screenshots or CLI output in the PR |
| Retro | end of the sprint | three points in the sprint issue: what to keep, what to change, what to do with the blockers; process changes are edits to this file |

## Definition of Ready

- Context (why, with links to `docs/DESIGN.md` or an ADR), Scope, Acceptance criteria.
- Labels type / area / priority, a milestone, a Size.
- Dependencies named (sub-issue, `blocked`).

## Definition of Done

- A PR with green CI: fmt, clippy `-D warnings`, tests on three platforms, coverage ≥ 90 %, cargo-deny, the Sonar quality gate; the PR summary comment without open findings.
- Documentation updated (`docs/`, README, ADR) when the architecture, the dependencies or a format changed.
- Fixtures only under an MIT-compatible licence (ADR-0009).
- The issue closed by the PR reference (`Closes #N`), the epic updated.

## Branches and commits

Feature branch → PR → squash-merge into `master` (the ruleset requires `CI OK`).
The PR title and the commits follow Conventional Commits (`feat(area): …`,
`fix: …`, `ci: …`); release-plz builds the CHANGELOG and the version from them
(`docs/RELEASING.md`).
