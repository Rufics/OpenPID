## Що змінено

<!-- Коротко: що і навіщо. Заголовок PR — у форматі Conventional Commits, напр. `feat(dsp): add Tukey window`. -->

## Як перевірено

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cargo deny check`
- [ ] Покриття не впало нижче 85 % (job `coverage`)

## Документація

- [ ] Оновлено `docs/` / README / ADR, якщо змінилася архітектура, залежності або формат
