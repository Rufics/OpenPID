# Step response: вердикти замість сирих метрик

Зауваження замовника (05.11.2026): допрацювати Step response — запропонувати, як
користувачу зрозуміти результат.

## Причина

Рядок метрик моноширинним шрифтом («16 / 150 windows · rise 18 ms · delay 11 ms ·
latency 11.1 ms · overshoot 31 % · settling — ms · final 0.83») не каже, чи це
добре. На короткому лозі 4.5.1 середня крива шумить, а «16 вікон» ніяк не
попереджає про ненадійність. Користувач PTB звик до візуальної оцінки «крива
біля 1 без горба», але не до чисел.

## Варіанти

| | Варіант | Плюси | Мінуси | Бали |
|---|---|---|---|---|
| R0 | Як є | — | числа без оцінки | 24 |
| R1 | **Бейджі з кольором** під графіком: `overshoot 31 % [too high]`, `rise 18 ms [ok]`… | миттєво зрозуміло; дешево | без пояснення порогів | 46 |
| R2 | **Цільова смуга на графіку**: зелена 0,95–1,05 після 100 мс, червона лінія 1,15 | видно «горб» очима; канон PTB («біля 1») | лише для overshoot/settling | 45 |
| R3 | Речення-вердикт + посилання на Advice («Roll overshoots 31 %: Advice пропонує D 40 → 44») | зв'язує з дією | текст тьмяніє без бейджів | 40 |
| R4 | **Таблиця трьох осей** roll/pitch/yaw з бейджами, без перемикання осі | бачиш усе зразу; A/B у рядках | три аналізи замість одного (кеш є; 3 × 22 мс) | 47 |
| R5 | Єдиний бал «goodness 0–100» як у деяких тюнерах | один погляд | приховує причину; спірна формула | 30 |

Рекомендація: **R1 + R2 + R4**, R3 — як рядок під таблицею, коли Advice має пропозицію.

## Рішення замовника (05.11.2026)

R1 + R2 + R4 + рядок R3.

## Вимоги до реалізації

**Пороги** — єдине джерело для Step, Advice і майбутнього порівняння A/B:
`openpid-analysis::targets::StepTargets { overshoot_warn: 15, overshoot_fail: 30,
rise_ms_max: 60, settling_ms_max: 120, final_tol: 0.10, min_windows_ok: 10,
min_windows_weak: 5 }` + пресети `Freestyle` (ці), `Racing` (rise 40, overshoot 20),
`Cinematic` (overshoot 8, settling 100). `RecommendParams.overshoot_max_pct` береться
з `StepTargets` (прибрати дублювання). Пресет і правки — у Settings → «Targets», у
`Prefs` і `Project`.

**Вердикти** `step::verdicts(metrics, windows_used, targets) -> Vec<Verdict>`:
`Verdict { metric, value_text, level: Ok|Warn|Fail|Unknown, note }`. Надійність:
`windows_used ≥ 10` — ok, 5–9 — «weak: N windows», < 5 — «unreliable» (усі інші
вердикти стають Unknown із сірим кольором). `final_value` поза 1 ± tol → «gain
mismatch: enable Y-offset or check setpoint scaling».

**GUI.**
- Над графіком замість рядка метрик — таблиця: рядки roll / pitch / yaw (усі три
  рахуються, активна вісь підсвічена і малюється; клік по рядку перемикає вісь),
  стовпці windows · rise · delay · latency · overshoot · settling · final; клітинка
  = значення + бейдж кольором (зелений/жовтий/червоний/сірий, ASCII-мітка `ok`/`!`/`x`
  поруч для колірно-безпечності). Для кількох слотів — рядок на слот і вісь.
- На графіку: смуга 1 ± tol (зелена, альфа 30) від `settling_ms_max` до кінця;
  горизонтальна лінія `1 + overshoot_warn/100` (жовта пунктирна) і `1 + overshoot_fail/100`
  (червона пунктирна); підписи ліворуч.
- Рядок під таблицею: якщо Advice для цієї сесії має пропозицію по PID осі —
  «Advice: roll D 40 -> 44 (overshoot 31 %)» з кнопкою «Go to Advice».
- Hover на бейджі — поріг і пояснення.

**CLI.** `openpid run step` додає `verdicts` у JSON.

## Критерії приймання

- 4.2.0 roll: overshoot 35 % → Fail, pitch 37 % → Fail; 2025.12 pitch 6 % → Ok, roll
  21 % → Warn; 4.5.1 сесія 1 (16 вікон) → надійність Ok, yaw (0 вікон) → Unknown.
- Зміна пресету в Settings змінює бейджі без перерахунку аналізу (вердикти — чиста
  функція від метрик).
- Headless-тест: таблиця трьох осей, клік по рядку перемикає вісь, бейджі, смуга на
  графіку (перевірка через стан плоту), рядок Advice.
- Unit-тести `verdicts` на крайніх значеннях і `None`.

## Розмір

M (≈ 2 дні).

## Реалізація (06.11.2026, #103)

- `openpid-analysis::targets`: `StepTargets` (+ `Hash`, serde default), пресети
  `TargetPreset::{Freestyle, Racing, Cinematic}` і `matching()`, `verdicts()` —
  чиста функція (7 вердиктів у порядку стовпців), `overall()`. Unit-тести на
  крайніх значеннях, `None` і замалій кількості вікон; перевірка на реальних
  логах у `tests/real_logs.rs`.
- `RecommendParams.overshoot_max_pct` замінено на `targets: StepTargets`
  (правило D спрацьовує вище `overshoot_warn_pct`); старі проєкти читаються
  (serde default, зайве поле ігнорується).
- Пороги живуть у `Prefs.targets` (Settings → Targets: пресети, поля, «Custom»,
  коли жоден пресет не збігається) і в `Project.recommend.targets`; під час
  роботи — `tools.recommend.targets`. Поле «overshoot limit» на Advice редагує
  той самий поріг.
- Step tab: таблиця `egui::Grid` трьох осей × слотів (усі три осі рахуються через
  кеш), `App.step_view` зберігає вердикти, смугу й рядок Advice для тестів; смуга
  1 ± tol від `settling_ms_max` до кінця відповіді (`Polygon` з фіксованими
  координатами — в auto-bounds не впливає), пунктирні `HLine` warn/fail; рядок
  Advice читає кешований результат recommender-а (запускає його для першого
  видимого слота).
- CLI: `run step --json` додає `verdicts` (пороги з `--project`, інакше
  Freestyle).

Відхилення: `delay` і `latency` інформаційні (зелені без порогу, з приміткою);
`settling = None` — Warn («never settles»), а не Unknown; у 4.5.1 сесія 1 yaw має
55 вікон (не 0, як припускав документ) — критерій перевірено на roll (16 вікон →
Ok за кількістю, overshoot 30.5 % → Fail, final 0.8 → gain mismatch).
