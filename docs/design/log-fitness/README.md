# Чи придатний лог: перевірка після відкриття

Зауваження замовника (05.11.2026): пропрацювати й реалізувати концепт «чи
придатний лог» — користувач має одразу бачити, що цей лог може показати і чого ні.

## Причина

Про відсутність `gyroUnfilt` чи eRPM користувач дізнається лише на вкладці Filter
Sim або Motor noise з сірого напису. Про замалу активність стіків — тільки з
«16 / 150 windows» у Step response. Що `debug_mode` чи частоту логу треба змінити
*до* наступного польоту, ніхто не каже.

## Варіанти

| | Варіант | Плюси | Мінуси | Бали |
|---|---|---|---|---|
| R0 | Як є: написи на вкладках | — | пізно і розкидано | 22 |
| R1 | Банер під шапкою на всіх вкладках, згортається | помітно | займає місце постійно | 38 |
| R2 | Картка лише у Diagnostics | не заважає | ніхто не ходить у Diagnostics | 30 |
| R3 | Спливне вікно після відкриття | гарантовано побачать | модальність дратує при кожному лозі | 33 |
| R4 | Окрема вкладка «Log check» | місце для деталей | ще одна вкладка з 12 | 34 |
| R5 | **Бейджі у чипі слота** («1 kHz · 40 s · raw ✓ · rpm ✓ · sticks !») з підказками + повна картка у Diagnostics + блок у `openpid info` | завжди видно, не займає місця; деталі за кліком | треба компактні позначки | **49** |

Рекомендація: **R5**.

## Рішення замовника (05.11.2026)

R5.

## Вимоги до реалізації

**Чиста логіка** `openpid-analysis::fitness::check(session: &FlightSession) -> Fitness`:
`Fitness { checks: Vec<Check>, overall: Level }`, `Check { id: &'static str,
level: Level (Ok | Warn | Fail), title, detail, fix: Option<String> }`. Перевірки
(пороги — константи з коментарями):

| id | Ok | Warn | Fail | fix (підказка) |
|---|---|---|---|---|
| `rate` | ≥ 1 kHz | 500–999 Hz | < 500 Hz | «set blackbox_sample_rate = 1/1» |
| `duration` | ≥ 20 s | 5–20 s | < 5 s | «log a longer flight» |
| `raw_gyro` | є `gyro.raw.*` | — | немає | «set debug_mode = GYRO_SCALED» (BF < 4.3) / «enable gyroUnfilt field» |
| `erpm` | є `motor_rpm.*` | немає, але є estimator | — | «set dshot_telemetry = ON, blackbox log motor rpm» |
| `frames` | corrupt < 0.5 % | 0.5–5 % | > 5 % | «check SD card / flash speed» |
| `gaps` | 0 | ≤ 3 | > 3 або > 1 s | — |
| `sticks` | ≥ 10 step-вікон на roll і pitch | 5–9 | < 5 | «fly sharper stick inputs for step response» |
| `throttle` | ≥ 50 % бінів газу з ≥ 3 сегментами | 25–50 % | < 25 % | «vary throttle through the range» |
| `debug_mode` | відомий | — | — | інформаційно: що саме в debug[0..7] |

`overall` = найгірший рівень серед `rate`, `duration`, `frames`; решта — лише
попередження. Кількість step-вікон рахується дешевим проходом (та сама логіка
відбору вікон, що в `StepAnalysis`, винесена у `step::count_windows`).

**GUI.**
- Чип слота: після тривалості — бейджі `raw ✓/✗`, `rpm ✓/✗`, `sticks ✓/!/✗` і
  загальний кружок (зелений/жовтий/червоний); hover — список перевірок із текстом.
- Diagnostics: картка «Log check» над таблицею: рядок на перевірку (іконка
  ASCII `[ok] [!] [x]`, назва, пояснення, fix у моноширинному полі з кнопкою Copy).
- Вкладки, яким бракує даних (Filter Sim, Motor noise), показують той самий `fix`.

**CLI.** `openpid info` друкує блок `fitness:` із тими самими рядками;
`--json` додає `fitness` до `SessionSummary`.

**Поведінка.** Перевірка рахується один раз при відкритті (у worker) і кешується
в `Loaded.fitness`.

## Критерії приймання

- 2025.12: усі перевірки Ok (є raw gyro і eRPM); 4.2.0: `erpm` Warn із fix, `raw_gyro` Ok
  (GYRO_SCALED); 4.5.1 сесія 0 (3 с): `duration` Fail, `sticks` Fail.
- Hover на чипі показує список; Diagnostics показує картку; `openpid info` і JSON містять блок.
- Unit-тести `fitness::check` на трьох фікстурах і на синтетиці; headless-тест бейджів.

## Розмір

S (≈ 1–1½ дня).
