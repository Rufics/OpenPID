# ADR-0011: Планування — Scrum на GitHub Issues, Milestones і Projects

- Статус: прийнято
- Дата: 2026-11-04

## Контекст

План (`docs/DESIGN.md` §12) жив лише в документі; виконане і невиконане не було видно, пріоритети не фіксувалися. Команда — власник продукту плюс AI‑асистент, тому процес має бути легким і повністю в GitHub.

## Рішення

- **Issues** — єдиний backlog; кожне має Context / Scope / Acceptance (Definition of Ready) і однаковий Definition of Done. Великі теми — `epic` із sub‑issues (нативні sub‑issues GitHub).
- **Milestones = фази плану** (`v0.2 … v1.0`) з критерієм готовності в описі; дати не ставимо, поки немає виміряної швидкості.
- **Мітки**: type / area / priority / blocked. Статус, розмір і спринт — поля GitHub Project, щоб не дублювати.
- **Project (Projects v2)** як Scrum‑dashboard: Backlog, Sprint board, Roadmap, Bugs, Blocked; спринт = iteration 2 тижні. Створюється скриптом `tools/github-project.sh` (API Projects — лише GraphQL, недоступний з агентних сесій), доналаштовується в UI за `docs/PROCESS.md`.
- Ритуали мінімальні й асинхронні; зміни процесу — правки `docs/PROCESS.md`.

## Наслідки

- Нова робота починається з issue; PR посилається `Closes #N`.
- Backlog фаз 2–3 заведено (#10–#44) у день ухвалення.
