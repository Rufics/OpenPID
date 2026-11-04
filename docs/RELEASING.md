# Релізи та налаштування репозиторію

## Одноразове налаштування на GitHub (робить власник)

1. **Ruleset для `master`.** Settings → Rules → Rulesets → New ruleset → Import a ruleset → файл `.github/rulesets/master.json`. Він вимагає PR зі squash‑злиттям, 1 схвалення code owner, вирішені треди, лінійну історію, підписані коміти, заборону force‑push/видалення та зелені перевірки: `CI OK`, `fmt + clippy + docs + typos`, `conventional commits`, `test (ubuntu/macos/windows)`, `coverage (>= 85% lines)`, `cargo deny`. Перевірки мають «strict» режим — гілка PR має бути оновлена до `master`.
   - Якщо підписані коміти небажані — видаліть правило `required_signatures` з JSON перед імпортом.
   - Поле `integration_id: 15368` — GitHub Actions; при імпорті GitHub підставить правильне значення сам.
2. **Секрети** (Settings → Secrets and variables → Actions):
   - `SONAR_TOKEN` — токен проєкту на sonarcloud.io (проєкт імпортується з GitHub; ключі у `sonar-project.properties`). Без нього job `SonarCloud` пропускається з повідомленням.
   - `RELEASE_PLZ_TOKEN` — fine‑grained PAT з правами *Contents: write* і *Pull requests: write* на цей репозиторій, щоб release‑PR запускав CI. Без нього release‑plz працює через `GITHUB_TOKEN`, але PR не триґерить перевірки.
3. **Settings → General → Pull Requests**: лишити увімкненим тільки *Allow squash merging*, встановити *Default commit message: Pull request title and description*, увімкнути *Automatically delete head branches*.
4. **Settings → Actions → General**: *Workflow permissions: Read repository contents*, увімкнути *Allow GitHub Actions to create and approve pull requests* (потрібно release‑plz).
5. **Dependabot** уже налаштований (`.github/dependabot.yml`): щотижневі згруповані PR для cargo і GitHub Actions. Settings → Code security: увімкнути Dependabot alerts.
6. **SonarCloud**: після першого аналізу встановити Quality Gate «Sonar way» і увімкнути *Pull request decoration*; Sonar стане ще однією перевіркою, яку можна додати до ruleset (`SonarCloud Code Analysis`).

## Як виходить реліз

```
feature branch ──PR──▶ master ──release-plz──▶ PR "chore: release vX.Y.Z" ──merge──▶ tag vX.Y.Z
                                                                                     └─▶ GitHub Release + CHANGELOG
                                                                                     └─▶ release.yml: бінарники для
                                                                                         Linux x86_64, macOS arm64/x86_64, Windows x86_64
```

Версія єдина для всього workspace (`[workspace.package].version`), тег — `v<version>`, журнал — `CHANGELOG.md` з Conventional Commits. Публікації на crates.io немає (`publish = false`).

## Артефакти CI

Кожен запуск CI збирає `openpid` і `openpid-gui` для трьох ОС (job `build`) і звіт покриття (`coverage-lcov`); завантажити їх можна зі сторінки запуску (Actions → run → Artifacts), зберігаються 14 днів.

## Кеш

`Swatinem/rust-cache` кешує `~/.cargo` і `target/` за ключем `Cargo.lock` + toolchain окремо для job‑ів `check`, `test`, `coverage`, `build-<os>`; повторні запуски перезбирають лише змінені crate‑и.
