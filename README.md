# OpenPID

Кросплатформенний інструмент на Rust для аналізу blackbox‑логів політних контролерів і допомоги в налаштуванні PID та фільтрів (функціональний орієнтир — PIDtoolbox / PIDtoolbox Pro, код — повністю власний). Ліцензія — MIT.

## Стан

Фаза 0 (фундамент) реалізована:

- `openpid-core` — доменна модель (`FlightSession`, `Channel`, `ChannelId`, `Series`, `Timebase`, `Settings`) і trait‑и розширення (`LogFormat`, `FirmwareProfile`, `Analysis`).
- `openpid-fmt-blackbox` — власний декодер бінарного Blackbox (усі кодування та предиктори, I/P/S/G/H/E‑кадри, валідація, ресинхронізація, rollover часу, події) та профілі прошивок Betaflight, EmuFlight, Rotorflight, INAV (базовий) і generic.
- `openpid-fmt-csv` — читання CSV‑експорту `blackbox_decode`.
- `openpid-cli` — `openpid info` та `openpid decode`.

Подальший план — у [`docs/DESIGN.md`](docs/DESIGN.md) (§12).

## Збірка та використання

```sh
cargo build --release
./target/release/openpid info  path/to/LOG.BFL            # огляд сесій, прошивки, частот, каналів
./target/release/openpid info  path/to/LOG.BFL --json     # те саме у JSON
./target/release/openpid decode path/to/LOG.BBL --session 1 --out log1.csv   # сирі цілі, як blackbox_decode
./target/release/openpid decode path/to/LOG.BBL --scaled                     # канонічні канали у фізичних одиницях
```

## Тести

```sh
cargo test --workspace            # unit, golden, конформанс, стійкість до пошкоджень
tools/fetch-community-logs.sh     # додаткові логи спільноти (Rotorflight, EmuFlight 0.4, BF 4.3) у tests/logs/external
OPENPID_UPDATE_GOLDEN=1 cargo test -p openpid-fmt-blackbox --test fixtures   # оновити golden‑файли після свідомої зміни
```

Якщо поруч із логом у `tests/logs/` покласти `<лог>.<сесія>.csv`, згенерований офіційним `blackbox_decode --index <сесія>`, тест `csv_conformance` порівняє наш сирий декод із ним поле в поле.

## Документи

- [`docs/DESIGN.md`](docs/DESIGN.md) — дизайн‑проєкт
- [`docs/adr/`](docs/adr/) — архітектурні рішення
- [`docs/OPEN-QUESTIONS.md`](docs/OPEN-QUESTIONS.md) — питання та ухвалені рішення
- [`tests/logs/README.md`](tests/logs/README.md) — походження тестових логів
