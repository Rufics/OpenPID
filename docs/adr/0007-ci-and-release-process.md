# ADR-0007: Процес розробки, CI‑перевірки та релізи

Статус: прийнято (2026-11-04)

## Рішення
- **Trunk‑based з feature‑гілками**: `master` захищена ruleset‑ом (`.github/rulesets/master.json`): тільки PR, squash‑merge, 1 схвалення code owner, вирішені треди, лінійна історія, підписані коміти, strict status checks.
- **Conventional Commits** для комітів і заголовків PR (`.commitlintrc.json`, job `commits`); з них release‑plz генерує CHANGELOG, версії (єдина на workspace) і теги `vX.Y.Z`; `release.yml` додає бінарники до GitHub Release.
- **Обов'язкові перевірки**: fmt, clippy з lint‑ами стилю (`docs/STYLE.md`), rustdoc без попереджень, typos, тести на трьох ОС, **покриття ≥ 85 % рядків** без GUI‑crate (`cargo llvm-cov --fail-under-lines`), cargo‑deny. Job `CI OK` агрегує їх для ruleset‑а.
- **Артефакти**: кожен запуск CI публікує бінарники для трьох ОС і lcov‑звіт (14 днів).
- **Кеш**: `Swatinem/rust-cache` з окремими ключами на job; concurrency скасовує застарілі запуски.
- **SonarCloud** як додаткова (не блокуюча до появи секрета) перевірка з тим самим lcov; після налаштування Quality Gate додається до ruleset.
- **Dependabot** щотижня групованими PR для cargo і actions.

## Чому так
- 85 % — реалістичний поріг для бібліотечного коду з golden/reference‑тестами (на момент рішення ≈ 92 %); GUI виключено, бо його не можна тестувати headless без значних інвестицій (`egui_kittest`), це окреме рішення на пізніше.
- release‑plz замість release‑please: рідний для Cargo‑workspace, розуміє успадкування версії, не потребує Node.
- Squash‑merge робить заголовок PR єдиним джерелом правди для журналу змін і спрощує bisect.

## Наслідки
- Для роботи release‑plz і Sonar власник додає секрети (`docs/RELEASING.md`).
- Будь‑який новий публічний елемент без документації або `unwrap()` поза тестами ламає CI — свідомо.
