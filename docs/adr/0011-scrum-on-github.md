# ADR-0011: Planning with Scrum on GitHub Issues, Milestones and Projects

- Status: accepted
- Date: 2026-11-04

## Context

The plan (`docs/DESIGN.md` §12) lived only in the document; what was done and what was not was invisible, and priorities were not recorded. The team is a product owner plus an AI assistant, so the process must be light and entirely on GitHub.

## Decision

- **Issues** are the single backlog; each has Context / Scope / Acceptance (the Definition of Ready) and the same Definition of Done. Large topics are an `epic` with sub-issues (GitHub's native sub-issues).
- **Milestones = the phases of the plan** (`v0.2 … v1.0`) with the completion criterion in the description; no dates until velocity is measured.
- **Labels**: type / area / priority / blocked. Status, size and sprint are fields of the GitHub Project, to avoid duplication.
- **A Project (Projects v2)** as the Scrum dashboard: Backlog, Sprint board, Roadmap, Bugs, Blocked; a sprint is a two-week iteration. It is created by `tools/github-project.sh` (the Projects API is GraphQL only, unreachable from agent sessions) and finished in the UI per `docs/PROCESS.md`.
- Rituals are minimal and asynchronous; process changes are edits to `docs/PROCESS.md`.

## Consequences

- New work starts with an issue; a PR references `Closes #N`.
- The backlog of phases 2–3 (#10–#44) was created on the day of the decision.
