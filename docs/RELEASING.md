# Релізи та налаштування репозиторію

## Одноразове налаштування на GitHub (робить власник)

1. **Ruleset для `master`.** Settings → Rules → Rulesets → New ruleset → Import a ruleset → файл `.github/rulesets/master.json`. Він вимагає PR зі squash‑злиттям, 1 схвалення code owner, вирішені треди, лінійну історію, підписані коміти, заборону force‑push/видалення та зелені перевірки: `CI OK`, `fmt + clippy + docs + typos`, `conventional commits`, `test (ubuntu/macos/windows)`, `coverage (>= 90% lines)`, `cargo deny`. Перевірки мають «strict» режим — гілка PR має бути оновлена до `master`.
   - Якщо підписані коміти небажані — видаліть правило `required_signatures` з JSON перед імпортом.
   - Поле `integration_id: 15368` — GitHub Actions; при імпорті GitHub підставить правильне значення сам.
2. **Секрети та бот для релізів.**
   - `SONAR_TOKEN` — вже доданий; job `SonarCloud` активний.
   - **Токен для release‑plz — тільки від бота, не від людини.** Workflow `release-plz` автентифікується як GitHub App і сам видає собі одногодинний installation‑токен (`actions/create-github-app-token`); жодних персональних PAT. Навіщо взагалі токен: PR і теги, створені стандартним `GITHUB_TOKEN`, за правилами GitHub не запускають інші workflow (захист від рекурсії), тож release‑PR не отримав би перевірок CI і його не можна було б злити під ruleset‑ом, а тег не запустив би збірку бінарників. Токен App цього обмеження не має.
     Створення (5 хвилин, один раз):
     1. Settings → Developer settings → GitHub Apps → *New GitHub App*. Назва, напр. `openpid-release-bot`; Homepage — URL репозиторію; *Webhook* вимкнути.
     2. Permissions → Repository permissions: **Contents: Read and write**, **Pull requests: Read and write**, **Metadata: Read‑only** (додається автоматично). Більше нічого. *Where can this App be installed* — *Only on this account*.
     3. Create → на сторінці App запам'ятати **App ID**; нижче *Generate a private key* → завантажиться `.pem`.
     4. Install App → встановити на свій акаунт, обравши **тільки репозиторій OpenPID**.
     5. У репозиторії: Settings → Secrets and variables → Actions: змінна `RELEASE_APP_ID` = App ID; секрет `RELEASE_APP_PRIVATE_KEY` = вміст `.pem` цілком. Після цього workflow `release-plz` вмикається сам (без змінної він пропускається і не червоніє).
     Відкликати доступ можна будь‑коли: видалити інсталяцію App або згенерувати новий ключ.
   - Альтернатива «заборонити»: не створювати App — тоді релізи робляться вручну (`git tag vX.Y.Z && git push --tags` з робочої машини запускає `release.yml`), а CHANGELOG ведеться руками. Автоматика вимкнена, але нічого не ламається.
3. **Settings → General → Pull Requests**: лишити увімкненим тільки *Allow squash merging*, встановити *Default commit message: Pull request title and description*, увімкнути *Automatically delete head branches*.
4. **Settings → Actions → General**: *Workflow permissions: Read repository contents*. Опцію *Allow GitHub Actions to create and approve pull requests* вмикати **не потрібно**: App‑токен не підпадає під це обмеження.
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
