//! UI language (DESIGN §7.2, #34): a string table keyed by the English text,
//! no dependency. `tr("Settings")` returns the text in the current language
//! and falls back to the English key, so a missing entry is never an error.
//!
//! The current language is thread-local: the GUI draws on one thread, and the
//! headless tests run one app per test thread without affecting each other.

use serde::{Deserialize, Serialize};
use std::cell::Cell;

/// Languages of the interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lang {
    /// English (the keys of the table).
    #[default]
    En,
    /// Ukrainian.
    Ua,
}

impl Lang {
    /// Every language, for the Settings switch.
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Ua];

    /// The language's own name.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ua => "Українська",
        }
    }
}

thread_local! {
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::En) };
}

/// Set the language this thread draws in.
pub fn set(lang: Lang) {
    CURRENT.with(|c| c.set(lang));
}

/// The language this thread draws in.
pub fn current() -> Lang {
    CURRENT.with(Cell::get)
}

/// `en` in the current language (the English text when there is no entry).
pub fn tr(en: &'static str) -> &'static str {
    match current() {
        Lang::En => en,
        Lang::Ua => UA
            .binary_search_by_key(&en, |(k, _)| k)
            .map_or(en, |i| UA[i].1),
    }
}

/// [`tr`] for a text that is not a literal of the app: a label a firmware
/// profile wrote. The English text when there is no entry.
pub fn tr_text(en: &str) -> String {
    match current() {
        Lang::En => en.to_string(),
        Lang::Ua => UA
            .binary_search_by_key(&en, |(k, _)| k)
            .map_or_else(|_| en.to_string(), |i| UA[i].1.to_string()),
    }
}

/// English → Ukrainian, sorted by the English key (binary search).
static UA: &[(&str, &str)] = &[
    ("1 change proposed", "запропоновано 1 зміну"),
    ("< step", "< крок"),
    ("Acceleration", "Прискорення"),
    ("Add log", "Додати лог"),
    (
        "Add log: open another log to compare (up to 8), or drop it on the window",
        "Додати лог: відкрити ще один для порівняння (до 8) або перетягнути на вікно",
    ),
    ("Advice", "Поради"),
    (
        "Advice with its evidence and ready CLI commands. Copy and apply.",
        "Поради з доказами й готові команди CLI. Скопіюйте й застосуйте.",
    ),
    ("All", "Увесь"),
    ("All headers", "Усі заголовки"),
    ("Analysis", "Аналіз"),
    ("Angular rate", "Кутова швидкість"),
    ("Anti-gravity", "Anti-gravity"),
    ("Appearance", "Вигляд"),
    ("Auto", "Авто"),
    ("Axes", "Осі"),
    ("Axis", "Вісь"),
    ("Back", "Назад"),
    ("Battery", "Батарея"),
    ("Blackbox Explorer", "Blackbox Explorer"),
    (
        "Blackbox Explorer's line colours and its black-red-white heatmaps",
        "кольори ліній Blackbox Explorer і його чорно-червоно-білі теплові карти",
    ),
    (
        "Blackbox analysis for tuning: noise, filters, step response, advice.",
        "Аналіз blackbox для налаштування: шум, фільтри, step response, поради.",
    ),
    ("Board", "Плата"),
    ("Centre", "Центр"),
    ("Changed", "Змінено"),
    ("Channel", "Канал"),
    ("Checked", "Перевірено"),
    ("Checks without a proposal", "Перевірки без пропозиції"),
    (
        "Choose the analysis window: drag the handles, or press Auto.",
        "Оберіть вікно аналізу: перетягніть межі або натисніть Auto.",
    ),
    ("Cinematic", "Кіно"),
    ("Clear markers", "Прибрати маркери"),
    (
        "Click or tap the plot to place the cursor, then set the epoch from it.",
        "Клацніть або торкніться графіка, щоб поставити курсор, і виставте епоху з нього.",
    ),
    ("Close the help window", "Закрити вікно довідки"),
    (
        "Close the last opened slot",
        "Закрити останній відкритий слот",
    ),
    ("Colour", "Колір"),
    ("Colour max", "Макс. кольору"),
    ("Colour-blind safe", "Для дальтоніків"),
    ("Control panel", "Панель керування"),
    ("Copy", "Копіювати"),
    ("Copy the table", "Скопіювати таблицю"),
    ("Corrupt frames", "Пошкоджені кадри"),
    ("Count", "Кількість"),
    ("Craft name", "Назва апарата"),
    ("Cursor", "Курсор"),
    ("Custom", "Власні"),
    ("Cut-off", "Зріз"),
    ("D gain", "Підсилення D"),
    ("D step", "крок D"),
    ("D-max gain", "Підсилення D-max"),
    ("D-term", "D-складова"),
    ("D-term RPM filter", "RPM-фільтр D-term"),
    ("D-term dynamic notch", "Динамічний notch D-term"),
    ("D-term filter", "Фільтр D-term"),
    ("D-term filter sliders", "Повзунки фільтра D-term"),
    ("D-term low-pass 1", "Low-pass 1 D-term"),
    ("D-term low-pass 2", "Low-pass 2 D-term"),
    ("D-term notch 1", "Notch 1 D-term"),
    ("D-term notch 2", "Notch 2 D-term"),
    ("Dark", "Темна"),
    ("Data", "Дані"),
    ("Debug", "Налагодження"),
    ("Debug mode", "Режим debug"),
    ("Decoder", "Декодер"),
    ("Delay", "Затримка"),
    ("Desynchronised frames", "Розсинхронізовані кадри"),
    ("Details", "Деталі"),
    ("Differences only", "Лише відмінності"),
    ("Display", "Показ"),
    ("Done", "Готово"),
    ("Drop a log here, or", "Перетягніть лог сюди або"),
    ("Dyn max", "Дин. макс"),
    ("Dyn min", "Дин. мін"),
    ("Dyn notch", "Dyn notch"),
    ("Dynamic notch", "Динамічний notch"),
    ("ESC protocol", "Протокол ESC"),
    ("Export", "Експорт"),
    ("F-term", "F-складова"),
    ("Feedforward", "Feedforward"),
    ("Feedforward gain", "Підсилення feedforward"),
    ("Filter Sim", "Симуляція фільтрів"),
    ("Filter sliders", "Повзунки фільтрів"),
    ("Final tolerance", "Допуск кінцевого"),
    ("Firmware", "Прошивка"),
    ("Firmware date", "Дата прошивки"),
    ("Firmware defaults", "Типові значення прошивки"),
    ("First run", "Перший запуск"),
    ("Flight modes", "Режими польоту"),
    ("Frames", "Кадри"),
    ("Freestyle", "Фрістайл"),
    ("Freq × throttle", "Частота × газ"),
    ("Freq × time", "Частота × час"),
    ("Go to Advice", "До порад"),
    ("Gyro", "Гіро"),
    ("Gyro filter", "Фільтр гіроскопа"),
    ("Gyro low-pass", "Low-pass гіроскопа"),
    ("Gyro low-pass 1", "Low-pass 1 гіроскопа"),
    ("Gyro low-pass 2", "Low-pass 2 гіроскопа"),
    ("Gyro notch 1", "Notch 1 гіроскопа"),
    ("Gyro notch 2", "Notch 2 гіроскопа"),
    ("Gyro prefilt", "Гіро до фільтра"),
    ("Gyro rate", "Частота гіроскопа"),
    ("Harmonics", "Гармоніки"),
    ("Help", "Довідка"),
    ("Hide the control panel", "Сховати панель керування"),
    ("High input", "Сильний вхід"),
    ("I gain", "Підсилення I"),
    ("I-term", "I-складова"),
    ("I-term relax", "I-term relax"),
    ("I-term windup", "I-term windup"),
    ("Keyboard shortcuts", "Гарячі клавіші"),
    ("LPF headroom", "запас над LPF"),
    ("Language", "Мова"),
    ("Latency", "Затримка"),
    ("Light", "Світла"),
    ("Link scales", "Спільна шкала"),
    ("Log", "Лог"),
    ("Log check", "Перевірка логу"),
    ("Log date", "Дата логу"),
    ("Log frequency", "Лог. частота"),
    ("Log viewer", "Перегляд логу"),
    ("Logging rate", "Частота запису"),
    ("Logs", "Логи"),
    ("Low-pass 1", "Low-pass 1"),
    ("Low-pass 2", "Low-pass 2"),
    ("Master multiplier", "Загальний множник"),
    ("Max", "Макс"),
    ("Max frequency", "Макс. частота"),
    ("Menu", "Меню"),
    ("Min", "Мін"),
    ("Min input", "Мін. вхід"),
    ("Min segments", "Мін. сегментів"),
    ("Min windows ok", "Мін. вікон (ok)"),
    ("Min windows weak", "Мін. вікон (слабко)"),
    ("Mode changes", "Зміни режиму"),
    ("Model", "Модель"),
    ("Most active 20 s", "Найактивніші 20 с"),
    ("Motor KV", "KV моторів"),
    ("Motor RPM", "Оберти моторів"),
    ("Motor idle", "Холостий хід моторів"),
    ("Motor output", "Вихід на мотори"),
    ("Motor poles", "Полюси мотора"),
    ("Motors", "Мотори"),
    ("Next", "Далі"),
    ("Next tab", "Наступна вкладка"),
    (
        "No configuration directory: settings last until the app closes.",
        "Немає теки конфігурації: налаштування діють до закриття програми.",
    ),
    (
        "Noise over throttle, and the motor harmonics (RPM overlay).",
        "Шум по газу і гармоніки моторів (RPM overlay).",
    ),
    ("Noise peak", "Пік шуму"),
    (
        "Noise: peaks above 80 Hz are notch candidates.",
        "Шум: піки вище 80 Гц — кандидати на notch.",
    ),
    ("Notch", "Notch"),
    ("Notches", "Нотчі"),
    ("Note", "Примітка"),
    ("ON THE LOG", "НА ЛОЗІ"),
    (
        "Okabe-Ito line colours and viridis heatmaps, readable with colour-vision deficiencies",
        "кольори ліній Okabe–Ito і теплові карти viridis, розрізнювані при порушеннях кольорового зору",
    ),
    ("Open", "Відкрити"),
    ("Open log", "Відкрити лог"),
    ("Open sample log", "Відкрити зразковий лог"),
    ("Open…", "Відкрити…"),
    ("Other channels", "Інші канали"),
    ("Overlays", "Накладки"),
    ("Overshoot", "Перерегулювання"),
    ("Overshoot fail", "Перерегулювання: межа"),
    ("Overshoot limit", "Межа перерегулювання"),
    ("Overshoot warn", "Перерегулювання: увага"),
    ("P & I gain", "Підсилення P та I"),
    ("P-term", "P-складова"),
    ("P:D ratio", "Співвідношення P:D"),
    ("P:I ratio", "Співвідношення P:I"),
    ("PID error", "Помилка PID"),
    ("PID loop rate", "Частота PID-циклу"),
    ("PID mode", "Режим PID"),
    ("PID pitch", "PID pitch"),
    ("PID roll", "PID roll"),
    ("PID sliders", "PID-слайдери"),
    ("PID sum", "Сума PID"),
    ("PID yaw", "PID yaw"),
    ("Pause", "Пауза"),
    ("Peak prominence", "Виразність піка"),
    ("Peaks", "Піки"),
    ("Per motor", "По моторах"),
    ("Pitch D gain", "D по pitch"),
    ("Pitch P & I gain", "P та I по pitch"),
    ("Play", "Відтворити"),
    ("Player", "Плеєр"),
    ("Plot as PNG…", "Графік як PNG…"),
    ("Plot colours", "Кольори графіків"),
    ("Position", "Позиція"),
    ("Prefilt", "До фільтра"),
    ("Prefilt (dotted)", "До фільтра (пунктир)"),
    ("Prefilt beside it", "До фільтра поруч"),
    ("Preset", "Пресет"),
    ("Previous tab", "Попередня вкладка"),
    ("Profile", "Профіль"),
    ("Project", "Проєкт"),
    ("Prominence", "Виразність"),
    ("Quantities", "Величини"),
    (
        "RMS of sim minus logged filtered gyro, relative to the logged RMS: how well the model matches the flight controller",
        "RMS різниці sim і записаного відфільтрованого гіро відносно записаного RMS: наскільки модель збігається з контролером",
    ),
    ("RPM filter", "RPM-фільтр"),
    ("RPM notch", "RPM notch"),
    ("RPM overlay", "RPM-overlay"),
    ("Racing", "Гонки"),
    ("Range", "Діапазон"),
    ("Rate limit", "Обмеження швидкості"),
    ("Rates", "Rates"),
    ("Raw responses", "Сирі відгуки"),
    ("Recent files", "Нещодавні файли"),
    ("Reset first-run state", "Скинути стан першого запуску"),
    ("Reset to defaults", "Скинути до типових"),
    ("Reset to log", "Скинути до логу"),
    ("Residual", "Залишок"),
    ("Rise max", "Підйом макс"),
    ("Roll:pitch ratio", "Співвідношення roll:pitch"),
    ("Rules", "Правила"),
    (
        "Rules are hints with their evidence, not an autotune: apply one change, fly, compare the new log.",
        "Правила — підказки з доказами, не autotune: застосуйте одну зміну, політайте, порівняйте новий лог.",
    ),
    ("SP-gyro delay (xcorr)", "Затримка уставка→гіро (xcorr)"),
    ("SR latency (50 %)", "Затримка відгуку (50 %)"),
    ("Sample step", "Крок семплів"),
    ("Samples", "Семпли"),
    ("Save as…", "Зберегти як…"),
    ("Saved in", "Збережено в"),
    ("Scale", "Шкала"),
    ("Search", "Пошук"),
    ("Sections", "Розділи"),
    ("Segment", "Сегмент"),
    ("Setpoint", "Уставка"),
    ("Settings", "Налаштування"),
    ("Settling max", "Усталення макс"),
    ("Setup", "Налаштування апарата"),
    ("Setup info", "Конфігурація"),
    ("Show", "Показати"),
    ("Show on", "Показати на"),
    ("Show the control panel", "Показати панель керування"),
    (
        "Show the tour on the first opened log",
        "Показувати тур при першому відкритому лозі",
    ),
    ("Show tour", "Показати тур"),
    ("Simplified tuning", "Спрощене налаштування"),
    (
        "Single keys work while no text field has the keyboard; Cmd on macOS, Ctrl elsewhere.",
        "Одиночні клавіші працюють, коли фокус не в текстовому полі; Cmd на macOS, Ctrl на інших ОС.",
    ),
    ("Skip", "Пропустити"),
    ("Skipped bytes", "Пропущені байти"),
    ("Sliders", "Повзунки"),
    ("Slot", "Слот"),
    ("Smoothing", "Згладжування"),
    ("Spectrum", "Спектр"),
    ("Speed", "Швидкість"),
    ("Start the tour", "Почати тур"),
    ("Static notch", "Статичний notch"),
    ("Step", "Крок"),
    ("Step response", "Перехідна характеристика"),
    (
        "Stick response: overshoot and delay, judged against your targets.",
        "Відгук на стік: перерегулювання і затримка проти ваших цілей.",
    ),
    ("Sticks", "Стіки"),
    ("THE FILTER CHAIN", "ЛАНЦЮЖОК ФІЛЬТРІВ"),
    ("TPA", "TPA"),
    ("Tab", "Вкладка"),
    ("Target", "Ціль збірки"),
    ("Targets", "Цілі"),
    ("Theme", "Тема"),
    (
        "These sliders scale the gains the log flew with; this firmware has no sliders of its own, so type the result into its configurator.",
        "Ці повзунки масштабують коефіцієнти, з якими літав лог; власних повзунків у цієї прошивки немає, тож впишіть результат у її конфігуратор.",
    ),
    (
        "Thresholds behind the step-response verdicts and the Advice overshoot rule.",
        "Пороги вердиктів step response і правила перерегулювання в Порадах.",
    ),
    ("Throttle", "Газ"),
    ("Throttle boost", "Throttle boost"),
    ("Throttle curve", "Крива газу"),
    ("Throttle window ±", "Вікно газу ±"),
    ("To start", "На початок"),
    ("Touch mode", "Сенсорний режим"),
    ("Tour", "Тур"),
    ("Trim take-off and landing", "Обрізати зліт і посадку"),
    (
        "Try new filters without flying.",
        "Перевірте нові фільтри без польоту.",
    ),
    ("Type", "Тип"),
    ("Unfiltered", "Без фільтрів"),
    ("Units", "Одиниці"),
    (
        "Units apply to the log viewer traces; analyses stay in deg/s.",
        "Одиниці стосуються кривих перегляду логу; аналізи лишаються в °/с.",
    ),
    ("Unmapped fields", "Незіставлені поля"),
    ("User guide", "Посібник користувача"),
    ("Window", "Вікно"),
    ("Y correction", "Корекція Y"),
    ("Y max", "Y макс."),
    ("Y min", "Y мін."),
    ("Y scale ±", "Шкала Y ±"),
    ("Y-offset", "Зсув Y"),
    ("Yaw ratio", "Співвідношення yaw"),
    (
        "Your logs live here. Drop another one to compare.",
        "Тут ваші логи. Перетягніть ще один, щоб порівняти.",
    ),
    ("Z max", "Z макс."),
    ("Z min", "Z мін."),
    ("Zoom to the flown range", "Лише діапазон польоту"),
    (
        "a coloured line per motor and harmonic instead of one line at the mean of the motors",
        "кольорова лінія на кожен мотор і гармоніку замість однієї лінії за середнім моторів",
    ),
    (
        "a dashed line where the flight modes change, with the modes from then on, on the throttle / motors row",
        "пунктир там, де змінюються режими польоту, з режимами від цього моменту, на рядку газу й моторів",
    ),
    (
        "a fresh install starts the seven-step tour when its first log opens",
        "після встановлення тур із семи кроків починається, щойно відкрито перший лог",
    ),
    (
        "a logarithmic frequency axis on the full spectrum",
        "логарифмічна вісь частот на повному спектрі",
    ),
    (
        "a moving average of the power over 0, 2, 4, 8, 16 or 32 Hz; medium (8 Hz) by default",
        "ковзне середнє потужності по 0, 2, 4, 8, 16 або 32 Гц; за замовчуванням середнє (8 Гц)",
    ),
    (
        "a moving average over the mean response: off, 10, 20 or 40 ms; more smoothing hides noise, and a little of the overshoot",
        "ковзне середнє середнього відгуку: вимк., 10, 20 або 40 мс; сильніше згладжування ховає шум і трохи перерегулювання",
    ),
    (
        "a synthetic 30 s flight: no file needed",
        "синтетичний політ 30 с, файл не потрібен",
    ),
    (
        "a term of the PID output, in the firmware's units, on the axis rows",
        "складова виходу PID в одиницях прошивки, у рядках осей",
    ),
    ("all", "усі"),
    (
        "all three axes, or one at full height; the key of the shown axis brings all three back",
        "усі три осі або одна на всю висоту; клавіша показаної осі повертає всі три",
    ),
    ("amplitude", "амплітуда"),
    (
        "an axis that overshoots more gets more D; the same target as on the Step tab (Settings, Targets)",
        "вісь, що перерегульовує більше, отримує більше D; та сама ціль, що й на вкладці Step (Налаштування, Цілі)",
    ),
    (
        "an overshoot above this fails the axis",
        "перерегулювання вище цього — вісь не проходить",
    ),
    (
        "an overshoot above this is a warning, and the Advice rule proposes more D",
        "перерегулювання вище цього — попередження, і правило Порад пропонує більше D",
    ),
    ("as flown", "як у польоті"),
    ("as flown (dashed)", "як у польоті (пунктир)"),
    ("as flown:", "як у польоті:"),
    ("at", "при"),
    ("auto", "авто"),
    ("axes", "осі"),
    ("axis", "вісь"),
    (
        "back to the automatic range",
        "повернути автоматичний діапазон",
    ),
    (
        "back to the filters the log flew with",
        "повернутися до фільтрів, з якими літав лог",
    ),
    ("below 100 Hz (1 s segments)", "нижче 100 Гц (сегменти 1 с)"),
    ("changes proposed", "змін запропоновано"),
    ("channel", "канал"),
    (
        "click a plot to place a marker",
        "клацніть графік, щоб поставити мітку",
    ),
    (
        "click: this axis at full height",
        "клік: ця вісь на всю висоту",
    ),
    ("close", "закрити"),
    (
        "commands to apply (select all, copy)",
        "команди для застосування (виділити все, скопіювати)",
    ),
    ("confidence", "впевненість"),
    ("copy", "скопіювати"),
    (
        "copy the rows shown as tab-separated text, to paste into a spreadsheet or a message",
        "скопіювати показані рядки як текст із табуляцією, щоб вставити в таблицю чи повідомлення",
    ),
    (
        "dB (20·log10 of the amplitude) shows weak and strong lines at once, as the reference tool; linear shows the strongest only",
        "dB (20·log10 амплітуди) показує слабкі й сильні лінії разом, як в еталонному інструменті; лінійна — лише найсильніші",
    ),
    ("debug", "налагодження"),
    ("delay", "затримка"),
    ("delay (ms)", "затримка (мс)"),
    ("delay at 100 Hz", "затримка на 100 Гц"),
    (
        "drag a handle or the window; double-click = whole log",
        "тягніть ручку або вікно; подвійний клік = весь лог",
    ),
    ("drives LPF1", "керує LPF1"),
    (
        "each throttle bin averages the segments within this distance of it",
        "кожен бін газу усереднює сегменти в межах цієї відстані від нього",
    ),
    ("empty", "порожній"),
    ("end", "кінець"),
    ("end = cursor", "кінець = курсор"),
    ("epoch", "епоха"),
    ("error", "помилка"),
    (
        "every header of the log, as the firmware wrote it",
        "усі заголовки логу, як їх записала прошивка",
    ),
    ("every section is hidden", "усі розділи приховано"),
    (
        "every setting on this tab back to its default; the recent files stay",
        "усі налаштування цієї вкладки до типових; список недавніх файлів лишається",
    ),
    (
        "every slider back to the position the log flew with",
        "усі повзунки назад у позиції, з якими літав лог",
    ),
    (
        "every slider to the position the firmware ships with",
        "усі повзунки в заводські позиції прошивки",
    ),
    ("filter", "фільтр"),
    ("filter…", "фільтр…"),
    ("final", "кінцеве"),
    ("first empty slot", "перший порожній слот"),
    ("fix", "як виправити"),
    (
        "forget that the tour was seen, so the next first log starts it again",
        "забути, що тур уже переглянуто: наступний перший лог почне його знову",
    ),
    ("frequency (Hz)", "частота (Гц)"),
    ("frequency (Hz, log)", "частота (Гц, лог.)"),
    ("from the sliders", "з повзунків"),
    ("gyro", "гіро"),
    ("headers", "заголовків"),
    ("hide", "сховати"),
    ("high", "висока"),
    (
        "hover the map for a value",
        "наведіть на карту, щоб побачити значення",
    ),
    (
        "how far a noise peak must stand above the spectrum around it to count",
        "наскільки пік шуму має підніматися над спектром довкола, щоб його врахувати",
    ),
    (
        "how far a peak must stand above its surroundings to be marked",
        "наскільки пік має підніматися над оточенням, щоб його позначити",
    ),
    (
        "how far from 1 the settled response may be: 0.10 is within 10 %",
        "наскільки усталений відгук може відрізнятися від 1: 0.10 — у межах 10 %",
    ),
    (
        "how many automatic peaks to mark on each solid line, from 20 Hz up; 0 = none",
        "скільки автоматичних піків позначати на кожній суцільній лінії, від 20 Гц; 0 = жодного",
    ),
    (
        "how many harmonics per motor; 0 = as many as the RPM filter removes",
        "скільки гармонік на мотор; 0 = стільки, скільки знімає RPM-фільтр",
    ),
    (
        "how much D is raised in one proposal; small steps keep the motors cool",
        "на скільки D піднімається за одну пропозицію; малі кроки бережуть мотори від перегріву",
    ),
    (
        "how much quieter the band above the gyro low-pass must be than the low band before its cut-off is raised",
        "наскільки тихішою за низьку смугу має бути смуга над low-pass гіроскопа, щоб підняти його частоту зрізу",
    ),
    (
        "how sure the rule is, from how far the measurement is past its threshold",
        "наскільки правило впевнене: за тим, як далеко вимір за порогом",
    ),
    (
        "how the craft was set up, in plain words: firmware, rates, PID gains, filters, motors",
        "як налаштовано апарат, простими словами: прошивка, частоти, PID, фільтри, мотори",
    ),
    (
        "how wide the smoothing of the curve is, as in the reference tool; medium by default. The same level is a wider band for a shorter log",
        "наскільки широке згладжування кривої, як в еталонному інструменті; за замовчуванням середнє. Той самий рівень — ширша смуга для коротшого логу",
    ),
    ("judged", "оцінено"),
    (
        "keys: Right / Enter, Left, Esc",
        "клавіші: → / Enter, ←, Esc",
    ),
    (
        "lag of the sim behind the logged gyro; negative: the sim leads",
        "відставання sim від записаного гіро; від’ємне: sim випереджає",
    ),
    (
        "larger controls and touch gestures (pinch, two-finger pan, long press); it also switches itself on at the first touch",
        "більші елементи керування й сенсорні жести (щипок, прокрутка двома пальцями, довге натискання); вмикається й сам за першого дотику",
    ),
    ("latency", "лаг"),
    ("latency (ms)", "затримка (мс)"),
    (
        "length of each averaged piece of the full spectrum: longer gives finer frequency steps and less averaging",
        "довжина кожного усередненого шматка повного спектра: довший дає дрібніший крок частоти й менше усереднення",
    ),
    (
        "length of each piece of the log the response is computed on; longer windows see slow settling, shorter ones give more windows",
        "довжина кожного шматка логу, на якому рахується відгук; довші вікна бачать повільне встановлення, коротші дають більше вікон",
    ),
    (
        "length of each spectrum along the log: longer gives finer frequency steps and coarser time steps",
        "довжина кожного спектра вздовж логу: довший дає дрібніший крок частоти й грубший крок часу",
    ),
    (
        "length of each spectrum along the log: longer gives finer frequency steps, fewer segments per throttle bin",
        "довжина кожного спектра вздовж логу: довший дає дрібніший крок частоти й менше сегментів на бін газу",
    ),
    (
        "length of each spectrum segment: longer gives finer frequency steps and fewer averages",
        "довжина сегмента спектра: довший дає дрібніший крок частоти і менше усереднень",
    ),
    ("linear", "лінійна"),
    ("loading", "завантаження"),
    ("log ok", "лог ок"),
    ("log shorter than 20 s", "лог коротший за 20 с"),
    ("log too short to trim", "лог закороткий для обрізання"),
    ("log unfit", "лог непридатний"),
    ("log usable", "лог придатний"),
    ("log:", "лог:"),
    ("logged", "у лозі"),
    ("logged filtered", "записаний відфільтрований"),
    ("low", "низька"),
    ("low-med", "нижче середньої"),
    (
        "low-pass on setpoint and gyro before the deconvolution: noise above it is ignored",
        "фільтр нижніх частот на уставці й гіро перед деконволюцією: шум вище нього ігнорується",
    ),
    (
        "lower end of the power axis; auto follows the drawn data",
        "нижня межа осі потужності; авто — за показаними даними",
    ),
    ("magnitude (dB)", "амплітуда (дБ)"),
    ("markers", "мітки"),
    ("med-high", "вище середньої"),
    ("medium", "середня"),
    ("motors", "мотори"),
    (
        "motors whose logged speed moved the RPM notches",
        "мотори, чиї записані оберти рухали RPM notch",
    ),
    ("name or value", "назва або значення"),
    ("new slot, to compare", "новий слот, для порівняння"),
    (
        "no PID gains in this log, so there is nothing for sliders to scale",
        "у цьому лозі немає PID-коефіцієнтів, тож повзункам нічого масштабувати",
    ),
    (
        "no gyro.filtered in the log to validate against",
        "у лозі немає gyro.filtered для перевірки",
    ),
    ("no log shown", "жоден лог не показано"),
    ("no mode", "без режиму"),
    (
        "no ready commands for this firmware; set the values by hand",
        "для цієї прошивки готових команд немає; встановіть значення вручну",
    ),
    (
        "no row mentions this text",
        "жоден рядок не містить цього тексту",
    ),
    ("no sliders for this log", "для цього логу повзунків немає"),
    (
        "no unfiltered gyro in this log",
        "у цьому лозі немає нефільтрованого гіро",
    ),
    (
        "normalise every window on its settled tail before averaging",
        "нормувати кожне вікно за усталеним хвостом перед усередненням",
    ),
    ("not in this log", "немає в цьому лозі"),
    (
        "not judged: the text says why",
        "не оцінено: у тексті сказано чому",
    ),
    ("not logged", "не записано"),
    (
        "notches per motor, at the motor frequency and its multiples; 0 switches them off",
        "notch на мотор, на частоті мотора та її кратних; 0 вимикає їх",
    ),
    (
        "notches that follow the strongest peaks of the gyro; 0 switches them off",
        "notch, що йдуть за найсильнішими піками гіро; 0 вимикає їх",
    ),
    ("nothing to change", "нічого змінювати"),
    (
        "nothing to change: every rule that could be judged is satisfied",
        "нічого змінювати: кожне правило, яке вдалося оцінити, виконано",
    ),
    (
        "nothing: the sliders are as flown",
        "нічого: повзунки як у польоті",
    ),
    ("of", "з"),
    ("off", "вимк."),
    (
        "one colour scale for every log (the largest automatic maximum), so the same colour means the same value",
        "одна шкала кольору для всіх логів (найбільша з автоматичних стель): однаковий колір означає однакове значення",
    ),
    (
        "only the rows the logs do not agree in: the tinted ones",
        "лише рядки, у яких логи різняться: підфарбовані",
    ),
    (
        "only the rows whose name or value contains this text, any case",
        "лише рядки, назва або значення яких містить цей текст, без урахування регістру",
    ),
    (
        "open a second log to compare the setups",
        "відкрийте другий лог, щоб порівняти налаштування",
    ),
    ("other", "інше"),
    ("overshoot", "перерегулювання"),
    ("peak", "пік"),
    ("phase (deg)", "фаза (°)"),
    (
        "playback speed relative to the flight",
        "швидкість відтворення відносно польоту",
    ),
    ("position", "позиція"),
    (
        "position in % (100 = as the firmware ships)",
        "позиція у % (100 = як у заводській прошивці)",
    ),
    (
        "position in % (100 = the gains as flown)",
        "позиція у % (100 = коефіцієнти як у польоті)",
    ),
    ("power density", "щільність потужності"),
    (
        "power in decibels (10·log10) instead of unit²/Hz",
        "потужність у децибелах (10·log10) замість одиниця²/Гц",
    ),
    ("prefilt", "до фільтра"),
    ("raw", "сирий"),
    (
        "ready thresholds for a flying style; editing a value below makes them custom",
        "готові пороги під стиль польоту; зміна значення нижче робить їх власними",
    ),
    ("response", "відгук"),
    ("rest", "спокої"),
    ("rise", "підйом"),
    ("rows differ", "рядків різняться"),
    ("rpm", "об/хв"),
    ("running", "рахується"),
    ("session", "сесія"),
    ("setpoint", "уставка"),
    ("setpoint minus gyro", "уставка мінус гіро"),
    ("settling", "усталення"),
    (
        "seven steps through the tuning loop, on the sample log",
        "сім кроків циклом налаштування на зразковому лозі",
    ),
    ("show", "показувати"),
    (
        "show only the channels whose name contains this",
        "показати лише канали, назва яких містить цей текст",
    ),
    ("show this motor's lines", "показати лінії цього мотора"),
    ("sim", "sim"),
    ("sim as flown", "sim як у польоті"),
    ("sim as flown (dashed)", "sim як у польоті (пунктир)"),
    ("smooth", "згладжування"),
    ("smoothed curve", "згладжена крива"),
    (
        "smoothed curve: one transform of the whole log with its decibels smoothed, as the reference tool draws it; broad noise shows well, a steady tone almost vanishes. power density: averaged segments (Welch); noise reads its true density and a steady tone stands out",
        "згладжена крива: одне перетворення всього логу зі згладженими децибелами, як малює еталонний інструмент; широкий шум видно добре, сталий тон майже зникає. щільність потужності: усереднені сегменти (Велч); шум читається за справжньою щільністю, сталий тон виразний",
    ),
    ("smoothing", "згладжування"),
    ("solo", "лише цей"),
    (
        "something is off; the text says what to look at",
        "щось не так; у тексті сказано, на що глянути",
    ),
    ("spectrum at", "спектр на"),
    ("start", "початок"),
    ("start = cursor", "початок = курсор"),
    ("step >", "крок >"),
    ("sum", "сума"),
    (
        "the amplitude drawn in the brightest colour; auto: the 99th percentile above 20 Hz",
        "амплітуда найяскравішого кольору; авто: 99-й перцентиль вище 20 Гц",
    ),
    (
        "the analysis window of the docked log ends here",
        "вікно аналізу закріпленого логу закінчується тут",
    ),
    (
        "the analysis window of the docked log starts here",
        "вікно аналізу закріпленого логу починається тут",
    ),
    (
        "the axis whose gyro runs through the chain",
        "вісь, чиє гіро проходить через ланцюжок",
    ),
    (
        "the colours of the interface: dark or light",
        "кольори інтерфейсу: темні або світлі",
    ),
    (
        "the colours of the logs' lines and of the heatmaps",
        "кольори ліній логів і теплових карт",
    ),
    ("the cut-off at full throttle", "зріз на повному газі"),
    (
        "the cut-off at zero throttle when the throttle moves it",
        "зріз на нульовому газі, коли газ його рухає",
    ),
    (
        "the cut-off frequency; 0 switches the filter off",
        "частота зрізу; 0 вимикає фільтр",
    ),
    (
        "the debug values of the log's debug mode, in a row of their own",
        "значення debug-режиму логу, в окремому рядку",
    ),
    (
        "the filtered gyro, what the PID controller sees",
        "відфільтроване гіро, те, що бачить PID-регулятор",
    ),
    (
        "the flight modes active at the pointer or the cursor, on the same card",
        "режими польоту під вказівником або курсором, на тій самій картці",
    ),
    (
        "the frequency the notch removes; 0 switches it off",
        "частота, яку вирізає notch; 0 вимикає його",
    ),
    (
        "the gyro before the software filters, where the noise shows",
        "гіро до програмних фільтрів, де видно шум",
    ),
    (
        "the highest frequency the notches follow",
        "найвища частота, за якою йдуть notch",
    ),
    (
        "the individual responses behind the mean, as a density",
        "окремі відгуки за середнім, як щільність",
    ),
    (
        "the language of the interface, switched at once; units and numbers do not change",
        "мова інтерфейсу, перемикається одразу; одиниці й числа не змінюються",
    ),
    (
        "the log's unfiltered gyro through the modelled chain (sim) against the filtered gyro the flight controller logged",
        "нефільтроване гіро логу через модель ланцюжка (sim) проти відфільтрованого гіро, яке записав польотний контролер",
    ),
    (
        "the logs differ in this row",
        "у цьому рядку логи різняться",
    ),
    (
        "the logs do not differ in the sections shown",
        "у показаних розділах логи не різняться",
    ),
    (
        "the longest rise time (10 to 90 %) that is still fine",
        "найдовший час наростання (від 10 до 90 %), який ще прийнятний",
    ),
    (
        "the longest time to settle within the final tolerance that is still fine",
        "найдовший час встановлення в межах кінцевого допуску, який ще прийнятний",
    ),
    (
        "the low-pass topology: PT1 is gentle and quick; PT2, PT3 and biquad cut harder and delay more",
        "тип low-pass: PT1 м’який і швидкий; PT2, PT3 і biquad ріжуть сильніше і затримують більше",
    ),
    (
        "the lower edge of the notch: closer to the centre is narrower",
        "нижній край notch: ближче до центру — вужчий",
    ),
    (
        "the lowest frequency the notches follow",
        "найнижча частота, за якою йдуть notch",
    ),
    ("the lowest notch centre", "найнижчий центр notch"),
    (
        "the motor outputs, in the row under the axes (%)",
        "виходи моторів, у рядку під осями (%)",
    ),
    (
        "the motor speeds from the ESC telemetry, in a row of their own",
        "оберти моторів з телеметрії ESC, в окремому рядку",
    ),
    (
        "the motors' speed and its harmonics over the map: noise that follows them is motor noise",
        "оберти моторів та їх гармоніки поверх карти: шум, що йде за ними, — це шум моторів",
    ),
    (
        "the notch quality: higher is narrower, with less delay and less removed",
        "добротність notch: вища — вужчий, з меншою затримкою і меншим вирізанням",
    ),
    ("the rate the pilot asked for", "швидкість, яку задав пілот"),
    (
        "the response of each filter at a typical moment of the flight: the mean throttle, the mean motor speeds, the median dynamic-notch centres",
        "відгук кожного фільтра в типовий момент польоту: середній газ, середні оберти моторів, медіанні центри динамічних notch",
    ),
    (
        "the roll, pitch and yaw rows span ± this rate; the same on every row, so the axes compare",
        "рядки roll, pitch і yaw охоплюють ± цю швидкість; однаково в кожному рядку, щоб осі можна було порівняти",
    ),
    ("the rule is satisfied", "правило виконано"),
    (
        "the same for the unfiltered gyro: what the filters remove",
        "те саме для нефільтрованого гіро: що прибирають фільтри",
    ),
    (
        "the static notches, the dynamic-notch band and the RPM filter minimum",
        "статичні нотчі, смуга динамічного нотча й мінімум RPM-фільтра",
    ),
    (
        "the sticks at the pointer or the cursor, at the bottom right of the throttle / motors row",
        "стіки під вказівником або курсором, у правому нижньому куті рядка газу й моторів",
    ),
    (
        "the tab with what this proposal was read from",
        "вкладка з даними, з яких зроблено цю пропозицію",
    ),
    (
        "the throttle axis from the first to the last sampled bin instead of 0–100 %",
        "вісь газу від першого до останнього біна з даними замість 0–100 %",
    ),
    (
        "the throttle, in the row under the axes (%)",
        "газ, у рядку під осями (%)",
    ),
    (
        "the time of the cursor; the values at it are on every row",
        "час курсора; значення в ньому показано в кожному рядку",
    ),
    (
        "the time the cursor and the spectra show",
        "момент, який показують курсор і спектри",
    ),
    (
        "the top of the frequency axes; auto: half the logging rate",
        "верх осей частоти; auto: половина частоти логування",
    ),
    (
        "the unfiltered gyro beside the filtered one; the gyro only",
        "нефільтрований гіро поруч із фільтрованим; лише для гіро",
    ),
    (
        "the unfiltered gyro in a column of its own, on the same colour scale; the gyro only",
        "нефільтрований гіро в окремій колонці, на тій самій шкалі кольору; лише для гіро",
    ),
    (
        "the unfiltered gyro, where the motor noise shows; off: the filtered gyro",
        "нефільтрований гіро, де видно шум моторів; вимкнено: фільтрований гіро",
    ),
    (
        "the unit of the accelerometer traces in the log viewer",
        "одиниця кривих акселерометра в перегляді логу",
    ),
    (
        "the unit of the gyro and setpoint traces in the log viewer",
        "одиниця кривих гіроскопа та уставки в перегляді логу",
    ),
    (
        "the value drawn black; auto: the background noise of the maps",
        "значення, що малюється чорним; авто: фоновий шум карт",
    ),
    (
        "the value drawn brightest; auto: the strongest lines of the maps",
        "значення, що малюється найяскравішим; авто: найсильніші лінії карт",
    ),
    (
        "the verdict table: every log and axis against the targets",
        "таблиця вердиктів: кожен лог і вісь проти цілей",
    ),
    (
        "this log has no such row",
        "у цьому лозі такого рядка немає",
    ),
    ("throttle", "газ"),
    ("throttle (%)", "газ (%)"),
    (
        "throttle bins with fewer segments are dimmed: they are unsampled, not quiet",
        "біни газу з меншою кількістю сегментів затемнено: там немає даних, а не тиша",
    ),
    ("time (ms)", "час (мс)"),
    ("time (s)", "час (с)"),
    ("timeline", "таймлайн"),
    ("top of the frequency axis", "верх осі частот"),
    (
        "top of the frequency axis, at most half the logging rate",
        "верх осі частот, не більше половини частоти запису",
    ),
    ("top peak", "головний пік"),
    ("total", "разом"),
    ("total as flown", "разом, як у польоті"),
    ("type", "тип"),
    (
        "under the proposals, what every other rule saw: satisfied, or not judged and why",
        "під пропозиціями — що побачило кожне інше правило: виконано або не оцінено й чому",
    ),
    (
        "upper end of the power axis; auto follows the drawn data",
        "верхня межа осі потужності; авто — за показаними даними",
    ),
    (
        "upper end of the response axis; 1 is the setpoint",
        "верхня межа осі відгуку; 1 — це уставка",
    ),
    ("verdict", "вердикт"),
    (
        "walk through the tabs now, on the log that is open",
        "пройти вкладками зараз, на відкритому лозі",
    ),
    (
        "what the Latency plots show: the time the mean response takes to reach 50 % (SR latency), or the delay from setpoint to gyro by cross-correlation",
        "що показують графіки затримки: час, за який середній відгук сягає 50 %, або затримку від уставки до гіро за взаємною кореляцією",
    ),
    (
        "what the rows show, on roll, pitch and yaw",
        "що показують ряди, на roll, pitch і yaw",
    ),
    (
        "what was read from the file: samples, frames, and what could not be read",
        "що прочитано з файлу: семпли, кадри і те, що прочитати не вдалося",
    ),
    ("what-if", "що-якщо"),
    (
        "whether the log is fit for analysis: its rate, length, what was logged, and the fix where something is missing",
        "чи придатний лог для аналізу: частота, тривалість, що записано, і як виправити, якщо чогось бракує",
    ),
    (
        "whether the logged throttle moved the dynamic low-pass",
        "чи рухав записаний газ динамічний low-pass",
    ),
    ("whole log", "весь лог"),
    ("whole log, every slot", "весь лог, усі слоти"),
    ("windows", "вікна"),
    (
        "windows above this rate form the \"high input\" group, drawn apart from the low one",
        "вікна вище цієї швидкості утворюють групу «сильний вхід», окремо від слабкого",
    ),
    (
        "windows whose stick input stays below this rate are skipped: too small a step to judge",
        "вікна, де вхід стіків не перевищує цієї швидкості, пропускаються: замалий крок для оцінки",
    ),
    (
        "with at least this many step windows the verdicts are trusted",
        "коли вікон кроку не менше за це, вердиктам можна довіряти",
    ),
    (
        "with fewer step windows than this the verdicts are greyed out",
        "коли вікон кроку менше за це, вердикти сірі",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_and_unique() {
        for w in UA.windows(2) {
            assert!(w[0].0 < w[1].0, "{:?} before {:?}", w[0].0, w[1].0);
        }
        assert!(UA.iter().all(|(k, v)| !k.is_empty() && !v.is_empty()));
    }

    #[test]
    fn translates_and_falls_back() {
        set(Lang::En);
        assert_eq!(tr("Settings"), "Settings");
        set(Lang::Ua);
        assert_eq!(tr("Settings"), "Налаштування");
        assert_eq!(tr("no such key"), "no such key");
        assert_eq!(current(), Lang::Ua);
        set(Lang::En);
        assert_eq!(Lang::Ua.native_name(), "Українська");
        assert_eq!(Lang::ALL.len(), 2);
    }
}
