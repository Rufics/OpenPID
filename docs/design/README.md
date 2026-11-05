# Дизайн-рішення інтерфейсу

Кожна помітна зміна інтерфейсу оформлюється документом `docs/design/<тема>/README.md`:
зауваження замовника → причина (виміряна, не вгадана) → варіанти, що відрізняються
підходом → оцінка за матрицею → рішення → вимоги до реалізації → критерії приймання →
тести. Документ пишеться **до** реалізації і є технічним завданням для розробника;
після реалізації до нього додаються розділ «Реалізація» і знімок результату.

## Матриця оцінки варіантів

Оцінка 1–5 за кожним критерієм × вага, максимум 55. Рекомендується варіант із
найвищим балом; за рівності — дешевший. Рішення замовника головніше за бали.

| Критерій | Вага | Що міряємо |
|---|---|---|
| Ефективність задач | 3 | скільки дій і секунд до результату в циклі «лог → зміна налаштувань» |
| Видимість | 2 | чи помітно функцію без документації |
| Канонічність | 2 | чи так роблять PIDtoolbox, Blackbox Explorer, Adobe/MS/VS Code |
| Клавіатура й доступність | 2 | керування без миші, accessible names (kittest-перевірка увімкнена) |
| Масштабування / touch | 1 | 1280×800 і touch-режим, 44 pt цілі |
| Вартість | 1 | розмір реалізації (XS ≈ ½ дня … L ≈ 5 днів) |

## Спринт UX-1 (milestone v1.1, епік [#98](https://github.com/Rufics/OpenPID/issues/98))

| Тема | Документ | Issue |
|---|---|---|
| N логів замість A/B | [multi-slot](multi-slot/README.md) | [#105](https://github.com/Rufics/OpenPID/issues/105) |
| Перший запуск: тур і стартовий екран | [onboarding-tour](onboarding-tour/README.md) | [#104](https://github.com/Rufics/OpenPID/issues/104) |
| Вікно аналізу для кожного логу | [analysis-window](analysis-window/README.md) | [#102](https://github.com/Rufics/OpenPID/issues/102) |
| Чи придатний лог | [log-fitness](log-fitness/README.md) | [#100](https://github.com/Rufics/OpenPID/issues/100) |
| Колірна шкала heatmap | [colour-scale](colour-scale/README.md) | [#99](https://github.com/Rufics/OpenPID/issues/99) |
| Step response: вердикти | [step-verdicts](step-verdicts/README.md) | [#103](https://github.com/Rufics/OpenPID/issues/103) |
| Гарячі клавіші | [hotkeys](hotkeys/README.md) | [#101](https://github.com/Rufics/OpenPID/issues/101) |

Спільні правила для всіх тем спринту:

- **Мова**: усі нові підписи через `tr(…)` з українським перекладом у `i18n.rs`;
  шрифт egui не має гліфів ▶ ⏸ → ● U+2011 — лише ASCII і базова латиниця/кирилиця.
- **Доступність**: кожен інтерактивний віджет має accessible name (`labelled_by`),
  інакше headless-тест падає.
- **Стан**: параметри інструментів — у `Tools` і в `Project` (`#[serde(default)]`),
  налаштування користувача — у `Prefs`; жодної математики в `openpid-app`.
- **Тести**: кожна тема — headless-тест kittest на реальній фікстурі + unit-тести
  чистої логіки в `openpid-analysis`/`openpid-core`; покриття ≥ 90 % лишається.
- **Документація**: USER-GUIDE і скриншоти (`tools/screenshots.sh`) оновлюються в тому ж PR.
