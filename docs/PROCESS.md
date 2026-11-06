# Процес розробки: Scrum на GitHub

Один власник продукту (Product Owner) і розробка з AI‑асистентом. Процес навмисно
легкий: усе, що не допомагає ухвалювати рішення швидше, вилучено.

## Артефакти

| Scrum | У нас | Де |
|-------|-------|----|
| Product Backlog | issues репозиторію | [Issues](https://github.com/Rufics/OpenPID/issues), вид *Backlog* у Project |
| Епіки / цілі релізу | **Milestones** = фази плану (`docs/DESIGN.md` §12) + issue з міткою `epic` і sub‑issues | [Milestones](https://github.com/Rufics/OpenPID/milestones) |
| Sprint Backlog | поле *Sprint* (iteration, 2 тижні) у Project | вид *Sprint board* |
| Increment | squash‑merge у `master` → release‑plz → тег і реліз із бінарниками | `docs/RELEASING.md` |
| Definition of Done | нижче | кожне issue має цей блок |
| Зв'язок PR ↔ issue | обов'язковий: `Closes #N` / `Part of #N` в описі PR | крок «PR references an issue» у CI; у ruleset `master` — як required status check |
| Закриття PR без злиття | лише з коментарем: причина + посилання на PR‑заміну або issue | вручну; приклад — #107 → #110 |

### Milestones

| Milestone | Зміст | Критерій |
|-----------|-------|----------|
| `v0.2 — Phase 2a: formats` | INAV, доробки ArduPilot, Rotorflight, відсутні фікстури | кожна підтримувана прошивка декодує свої фікстури чисто |
| `v0.3 — Phase 2b: filters & overlays` | `openpid-sim`, Filter Sim, RPM/dyn‑notch overlay 2D/3D | лог із `gyroUnfilt` відтворює власний фільтрований гіро |
| `v0.4 — Phase 2c: Pro parity` | RPM estimator, motor noise, генератор, плеєр, проєкти, експорт, i18n, touch, perf | усі рядки таблиці `docs/DESIGN.md` §2 = ✓ |
| `v1.0 — Phase 3: recommendations` | Recommender, diff/exporter, (опц.) MSP | користувач застосовує рекомендацію |

Issue без milestone = нерозподілений backlog (наприклад, дослідження, що чекають на зразки).

### Мітки

- **Тип** (одна): `type: feature`, `bug`, `type: tech-debt`, `type: test`, `type: ci`, `type: research`, `documentation`; `epic` — парасолькове issue з sub‑issues.
- **Область** (одна або кілька): `area: core | dsp | analysis | sim | gui | cli | fmt-blackbox | fmt-ardupilot | ci | docs`.
- **Пріоритет** (одна): `priority: P0` (блокує спринт/реліз) … `P3` (коли буде час/зразки).
- `blocked` — чекає на щось зовнішнє (зразки логів, доступ, апстрім); `good first issue`, `help wanted` — для спільноти.

Статус, розмір і спринт **не** дублюються мітками: вони живуть у полях Project.

## GitHub Project (Scrum dashboard)

Project створюється один раз скриптом `tools/github-project.sh` (потрібен
`gh auth refresh -s project`; GraphQL недоступний із CI‑/агентних сесій, тому
це робить власник локально). Скрипт створює проєкт, поля, прив'язує його до
репозиторію і додає всі відкриті issues. Поле *Sprint* (iteration) і view‑и
GitHub не дає створити через API — їх налаштовують у UI за цим списком:

| View | Layout | Налаштування |
|------|--------|--------------|
| **Backlog** | Table | group by *Milestone*, sort by *Priority* ↑ потім *Size*; поля: Title, Status, Priority, Size, Sprint, Area, Labels |
| **Sprint board** | Board by *Status* | filter `sprint:@current`; колонки Backlog → Ready → In progress → In review → Done; swimlanes вимкнені |
| **Roadmap** | Roadmap | date field = *Sprint*, group by *Milestone*, zoom = month |
| **Bugs** | Table | filter `label:bug is:open`, sort by Priority |
| **Blocked** | Table | filter `label:blocked is:open` |

Поля: *Status* (вбудоване: Backlog, Ready, In progress, In review, Done),
*Priority* (P0–P3, дзеркало мітки), *Size* (XS ≈ ½ дня, S ≈ 1, M ≈ 2–3, L ≈ 5,
XL = розбити), *Sprint* (iteration, 2 тижні, старт у понеділок), *Area*.

Вбудовані workflow‑и Project (Settings → Workflows), які варто увімкнути:
*Item added to project → Status Backlog*, *Pull request merged → Done*,
*Item closed → Done*, *Auto-add to project* для `is:issue is:open`.
Insights: графік *Burn up* по полю Sprint (Settings → Insights → New chart,
layout Burn up, X = time, group by Status).

## Ритуали (мінімум)

| Ритуал | Коли | Що відбувається |
|--------|------|-----------------|
| Sprint planning | кожні 2 тижні, старт спринту | PO обирає ціль спринту (одне речення в описі iteration), переносить issues з *Ready* у спринт за пріоритетом і розміром |
| Refinement | асинхронно, перед плануванням | у кожного issue з *Backlog*, що претендує на спринт, є Context / Scope / Acceptance і Size → статус *Ready* |
| Daily | асинхронно | коментар у issue або PR: що зроблено, що блокує; нічого окремого |
| Review | злиття PR | PR summary comment (покриття, Sonar), демонстрація через скриншоти/CLI‑вивід у PR |
| Retro | кінець спринту | 3 пункти в issue спринту: що лишити, що змінити, що зробити з блокерами; зміни процесу — редагування цього файлу |

## Definition of Ready

- Context (навіщо, посилання на `docs/DESIGN.md`/ADR), Scope, Acceptance criteria.
- Мітки type / area / priority, milestone, Size.
- Залежності названі (sub‑issue, `blocked`).

## Definition of Done

- PR із зеленим CI: fmt, clippy `-D warnings`, тести на 3 ОС, покриття ≥ 90 %, cargo‑deny, Sonar quality gate; коментар‑підсумок у PR без відкритих зауважень.
- Документація оновлена (`docs/`, README, ADR), якщо змінилися архітектура, залежності або формат.
- Фікстури лише з MIT‑сумісною ліцензією (ADR‑0009).
- Issue закрите посиланням з PR (`Closes #N`), epic оновлений.

## Гілки та коміти

Feature‑гілка → PR → squash‑merge у `master` (ruleset вимагає `CI OK`).
Заголовок PR і коміти — Conventional Commits (`feat(area): …`, `fix: …`, `ci: …`),
з них release‑plz будує CHANGELOG і версію (`docs/RELEASING.md`).
