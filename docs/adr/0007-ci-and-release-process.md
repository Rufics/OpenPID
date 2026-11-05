# ADR-0007: Процес розробки, CI‑перевірки та релізи

Статус: прийнято (2026-11-04)

## Рішення
- **Trunk‑based з feature‑гілками**: `master` захищена ruleset‑ом (`.github/rulesets/master.json`): тільки PR, squash‑merge, 1 схвалення code owner, вирішені треди, лінійна історія, підписані коміти, strict status checks.
- **Conventional Commits** для комітів і заголовків PR (`.commitlintrc.json`, job `commits`); з них release‑plz генерує CHANGELOG, версії (єдина на workspace) і теги `vX.Y.Z`; `release.yml` додає бінарники до GitHub Release.
- **Обов'язкові перевірки**: fmt, clippy з lint‑ами стилю (`docs/STYLE.md`), rustdoc без попереджень, typos, тести на трьох ОС, **покриття ≥ 90 % рядків** усього workspace (`cargo llvm-cov --fail-under-lines`; GUI тестується headless, ADR-0008), cargo‑deny. Job `CI OK` агрегує їх для ruleset‑а.
- **Артефакти**: кожен запуск CI публікує бінарники для трьох ОС і lcov‑звіт (14 днів).
- **Кеш**: `Swatinem/rust-cache` з окремими ключами на job; concurrency скасовує застарілі запуски.
- **SonarCloud** як додаткова (не блокуюча до появи секрета) перевірка з тим самим lcov; після налаштування Quality Gate додається до ruleset.
- **Dependabot** щотижня групованими PR для cargo і actions.

## Чому так
- 90 % — поріг для бібліотечного коду з golden/reference‑тестами (на момент рішення ≈ 92 %); GUI спочатку було виключено; з ADR-0008 воно тестується headless через `egui_kittest` і входить до порогу.
- release‑plz замість release‑please: рідний для Cargo‑workspace, розуміє успадкування версії, не потребує Node.
- Squash‑merge робить заголовок PR єдиним джерелом правди для журналу змін і спрощує bisect.

## Наслідки
- Для роботи release‑plz і Sonar власник додає секрети (`docs/RELEASING.md`).
- Будь‑який новий публічний елемент без документації або `unwrap()` поза тестами ламає CI — свідомо.

## Закріплення версій у CI (доповнення 2026‑11)

- Кожен GitHub Action вказується **повним SHA коміту** з тегом у коментарі (`uses: actions/checkout@3d3c42e… # v7.0.1`). Тег можна переписати, SHA — ні; Dependabot (`.github/dependabot.yml`, екосистема `github-actions`) оновлює SHA і коментар разом.
- Runner‑и — лише датовані образи (`ubuntu-24.04`, `macos-15`, `macos-15-intel`, `windows-2025`), не `*-latest`: зміна образу — свідомий коміт, а не сюрприз від GitHub.
- Покинуті через нестачу runner‑ів job‑и перезапускаються вручну (docs/RELEASING.md), без retry‑action: це нова залежність, яка не лікує причину. macOS збирається на обох пулах (`macos-15` arm64 і `macos-15-intel` x86_64), тож реліз не залежить від одного з них (#44).
- Інструменти, які ставляться в CI, також із версією (`cargo-llvm-cov@0.9.1` через `taiki-e/install-action`; toolchain — `RUST_TOOLCHAIN`, той самий, що в `rust-toolchain.toml`).
- Назви job‑ів у matrix не містять імені runner‑а (`test (linux)`, а не `test (ubuntu-24.04)`), щоб оновлення образу не ламало список обов'язкових перевірок у ruleset‑і.

## Підсумковий коментар у PR (доповнення 2026‑11)

Job `PR summary comment` (`.github/scripts/pr_summary.py`, лише стандартна бібліотека Python) після `coverage` і `sonar` публікує один коментар у PR і оновлює його на кожен push: покриття рядків по crate‑ах з дельтою до базової гілки та найменш покритими файлами, стан quality gate SonarCloud з усіма умовами, лічильники bugs/vulnerabilities/code smells/hotspots і таблицю відкритих зауважень із посиланнями. Коментар інформаційний: він не входить до `CI OK`, а публікується навіть коли gate покриття впав. Для PR із fork‑ів job пропускається (token лише на читання).

