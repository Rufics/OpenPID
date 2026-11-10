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

/// English → Ukrainian, sorted by the English key (binary search).
static UA: &[(&str, &str)] = &[
    (
        "0 = as many as the RPM filter removes",
        "0 = стільки, скільки знімає RPM-фільтр",
    ),
    ("3D", "3D"),
    ("< step", "< крок"),
    (
        "A log without eRPM: estimate motor speed from the gyro",
        "Лог без eRPM: оцінити оберти моторів з гіроскопа",
    ),
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
    ("Analysis", "Аналіз"),
    ("Angular rate", "Кутова швидкість"),
    ("Appearance", "Вигляд"),
    ("Auto", "Авто"),
    ("Axis", "Вісь"),
    ("Back", "Назад"),
    (
        "Betaflight CLI to apply (select all, copy):",
        "Команди Betaflight CLI (виділити все, скопіювати):",
    ),
    ("Blackbox Explorer", "Blackbox Explorer"),
    (
        "Blackbox Explorer's line colours and its black-red-white heatmaps",
        "кольори ліній Blackbox Explorer і його чорно-червоно-білі теплові карти",
    ),
    (
        "Blackbox analysis for tuning: noise, filters, step response, advice.",
        "Аналіз blackbox для налаштування: шум, фільтри, step response, поради.",
    ),
    ("Centre", "Центр"),
    ("Channel", "Канал"),
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
    ("Count", "Кількість"),
    ("Cursor", "Курсор"),
    ("Custom", "Власні"),
    ("Cut-off", "Зріз"),
    ("D step", "крок D"),
    ("D-term", "D-складова"),
    ("Dark", "Темна"),
    ("Data", "Дані"),
    ("Debug", "Налагодження"),
    ("Delay", "Затримка"),
    ("Details", "Деталі"),
    ("Diagnostics", "Діагностика"),
    ("Display", "Показ"),
    ("Done", "Готово"),
    ("Drop a log here, or", "Перетягніть лог сюди або"),
    ("Dyn max", "Дин. макс"),
    ("Dyn min", "Дин. мін"),
    ("Dyn notch", "Dyn notch"),
    ("Dynamic notch", "Динамічний notch"),
    ("Evidence", "Докази"),
    ("Expand one axis", "Одна вісь на всю висоту"),
    ("Export", "Експорт"),
    ("F-term", "F-складова"),
    ("Filter Sim", "Симуляція фільтрів"),
    ("Firmware defaults", "Типові значення прошивки"),
    ("First run", "Перший запуск"),
    ("Freestyle", "Фрістайл"),
    ("Freq × throttle", "Частота × газ"),
    ("Freq × time", "Частота × час"),
    ("Full throttle range", "Увесь діапазон газу"),
    ("Go to Advice", "До порад"),
    ("Gyro", "Гіро"),
    ("Gyro prefilt", "Гіро до фільтра"),
    ("Harmonics", "Гармоніки"),
    ("Help", "Довідка"),
    ("Hide the control panel", "Сховати панель керування"),
    ("High input", "Сильний вхід"),
    ("I-term", "I-складова"),
    ("Keyboard shortcuts", "Гарячі клавіші"),
    ("LPF headroom", "запас над LPF"),
    ("Language", "Мова"),
    ("Latency", "Затримка"),
    ("Light", "Світла"),
    (
        "Lines: the motor harmonics, ×1 solid, ×2 and ×3 dashed. Bars: each motor's ×1, ×2, ×3 over the mean of the motors (1 = average); a louder motor stands out on every axis.",
        "Лінії: гармоніки моторів, ×1 суцільна, ×2 і ×3 пунктир. Стовпчики: ×1, ×2, ×3 кожного мотора відносно середнього по моторах (1 = середнє); гучніший мотор видно на кожній осі.",
    ),
    ("Link scales", "Спільна шкала"),
    (
        "Load a Betaflight 4.3+ log into slot A to use the PID sliders.",
        "Завантажте лог Betaflight 4.3+ у слот A, щоб користуватися PID-слайдерами.",
    ),
    ("Log", "Лог"),
    ("Log check", "Перевірка логу"),
    ("Log frequency", "Лог. частота"),
    ("Log viewer", "Перегляд логу"),
    ("Low-pass 1", "Low-pass 1"),
    ("Low-pass 2", "Low-pass 2"),
    ("Markers", "Позначки"),
    ("Max", "Макс"),
    ("Max frequency", "Макс. частота"),
    ("Menu", "Меню"),
    ("Min", "Мін"),
    ("Min input", "Мін. вхід"),
    ("Min segments", "Мін. сегментів"),
    ("Model", "Модель"),
    ("Most active 20 s", "Найактивніші 20 с"),
    ("Motor RPM", "Оберти моторів"),
    ("Motor noise", "Шум моторів"),
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
    (
        "Noise: peaks above 80 Hz are notch candidates.",
        "Шум: піки вище 80 Гц — кандидати на notch.",
    ),
    ("Notch", "Notch"),
    ("Notches", "Нотчі"),
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
    ("P-term", "P-складова"),
    ("PID error", "Помилка PID"),
    ("PID roll / pitch / yaw", "PID roll / pitch / yaw"),
    ("PID sliders", "PID-слайдери"),
    ("PID sum", "Сума PID"),
    ("Pause", "Пауза"),
    ("Peaks", "Піки"),
    ("Per motor", "По моторах"),
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
    ("Project", "Проєкт"),
    ("Prominence", "Виразність"),
    ("Proposals", "Пропозиції"),
    ("Quantities", "Величини"),
    (
        "RMS of sim minus logged filtered gyro, relative to the logged RMS: how well the model matches the flight controller",
        "RMS різниці sim і записаного відфільтрованого гіро відносно записаного RMS: наскільки модель збігається з контролером",
    ),
    ("RPM notch", "RPM notch"),
    ("RPM overlay", "RPM-overlay"),
    ("Racing", "Гонки"),
    ("Range", "Діапазон"),
    ("Raw responses", "Сирі відгуки"),
    ("Recent files", "Нещодавні файли"),
    ("Reset first-run state", "Скинути стан першого запуску"),
    ("Reset to defaults", "Скинути до типових"),
    ("Reset to log", "Скинути до логу"),
    ("Reset view", "Скинути вид"),
    ("Residual", "Залишок"),
    (
        "Rules are hints with their evidence, not an autotune: apply one change, fly, compare the new log.",
        "Правила — підказки з доказами, не autotune: застосуйте одну зміну, політайте, порівняйте новий лог.",
    ),
    ("SP-gyro delay (xcorr)", "Затримка уставка→гіро (xcorr)"),
    ("SR latency (50 %)", "Затримка відгуку (50 %)"),
    ("Save as…", "Зберегти як…"),
    ("Scale", "Шкала"),
    ("Segment", "Сегмент"),
    ("Setpoint", "Уставка"),
    ("Settings", "Налаштування"),
    ("Show the control panel", "Показати панель керування"),
    (
        "Show the tour on the first opened log",
        "Показувати тур при першому відкритому лозі",
    ),
    ("Show tour", "Показати тур"),
    (
        "Single keys work while no text field has the keyboard; Cmd on macOS, Ctrl elsewhere.",
        "Одиночні клавіші працюють, коли фокус не в текстовому полі; Cmd на macOS, Ctrl на інших ОС.",
    ),
    ("Skip", "Пропустити"),
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
    ("Surface 3D", "Поверхня 3D"),
    ("THE FILTER CHAIN", "ЛАНЦЮЖОК ФІЛЬТРІВ"),
    ("Tab", "Вкладка"),
    ("Targets", "Цілі"),
    ("Theme", "Тема"),
    (
        "Thresholds behind the step-response verdicts and the Advice overshoot rule.",
        "Пороги вердиктів step response і правила перерегулювання в Порадах.",
    ),
    ("Throttle", "Газ"),
    ("Throttle window ±", "Вікно газу ±"),
    ("To start", "На початок"),
    ("Touch mode", "Сенсорний режим"),
    (
        "Touch mode (larger controls, touch gestures)",
        "Сенсорний режим (більші елементи, жести)",
    ),
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
    ("User guide", "Посібник користувача"),
    ("Window", "Вікно"),
    ("Y correction", "Корекція Y"),
    ("Y max", "Y макс."),
    ("Y min", "Y мін."),
    ("Y scale ±", "Шкала Y ±"),
    ("Y-offset", "Зсув Y"),
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
        "a logarithmic frequency axis on the full spectrum",
        "логарифмічна вісь частот на повному спектрі",
    ),
    (
        "a marker per motor and harmonic in the motor's colour; off: one per harmonic at the mean motor speed",
        "позначка на кожен мотор і гармоніку кольором мотора; вимкнено: одна на гармоніку при середніх обертах",
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
    ("amplitude", "амплітуда"),
    ("as flown (dashed)", "як у польоті (пунктир)"),
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
    ("channel", "канал"),
    (
        "click a plot to place a marker",
        "клацніть графік, щоб поставити мітку",
    ),
    ("click to fix this end", "клацніть, щоб зафіксувати цю межу"),
    ("click: expand this axis", "клік: ця вісь на всю висоту"),
    (
        "click: show this axis on the plot",
        "клік: показати цю вісь на графіку",
    ),
    ("close", "закрити"),
    ("colour max (0 = auto)", "стеля кольору (0 = авто)"),
    ("copy", "скопіювати"),
    (
        "corrupt / desync / skipped bytes",
        "пошкоджені / десинхр. / пропущені байти",
    ),
    ("cutoff", "зріз"),
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
    (
        "drag: orbit · wheel / pinch: zoom · shift-drag, right-drag or two fingers: pan · double-click: reset",
        "перетягування: обертання · коліщатко / щипок: масштаб · shift-, праве або два пальці: зсув · подвійний клік: скинути",
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
    ("filter", "фільтр"),
    ("filter…", "фільтр…"),
    ("final", "кінцеве"),
    ("final tolerance", "допуск кінцевого"),
    ("first empty slot", "перший порожній слот"),
    ("frames", "кадри"),
    ("frequency (Hz)", "частота (Гц)"),
    ("frequency (Hz, log)", "частота (Гц, лог.)"),
    ("from sliders", "зі слайдерів"),
    ("gyro", "гіро"),
    ("harmonics", "гармоніки"),
    ("height", "висота"),
    ("height of the colour maximum", "висота стелі кольору"),
    ("hide", "сховати"),
    ("high", "висока"),
    ("high input", "верхній вхід"),
    (
        "hover the map for a value",
        "наведіть на карту, щоб побачити значення",
    ),
    (
        "how far a peak must stand above its surroundings to be marked",
        "наскільки пік має підніматися над оточенням, щоб його позначити",
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
        "into the first empty slot, or a new one",
        "у перший порожній слот або в новий",
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
    ("link scales", "спільна шкала"),
    ("loading", "завантаження"),
    ("log ok", "лог ок"),
    ("log shorter than 20 s", "лог коротший за 20 с"),
    ("log too short to trim", "лог закороткий для обрізання"),
    ("log unfit", "лог непридатний"),
    ("log usable", "лог придатний"),
    ("log:", "лог:"),
    ("logged", "у лозі"),
    ("logged filtered", "записаний відфільтрований"),
    ("loudest", "найгучніший"),
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
    ("max", "макс"),
    ("mean of the motors", "середнє по моторах"),
    ("med-high", "вище середньої"),
    ("medium", "середня"),
    ("min input", "мін. вхід"),
    ("min segments", "мін. сегментів"),
    ("min windows ok", "мін. вікон (ok)"),
    ("min windows weak", "мін. вікон (слабко)"),
    ("motor", "мотор"),
    ("motors", "мотори"),
    (
        "motors whose logged speed moved the RPM notches",
        "мотори, чиї записані оберти рухали RPM notch",
    ),
    (
        "multiples of each motor's frequency to read: 1 is the motor itself, 2 and 3 its overtones",
        "кратні частоти мотора, які читати: 1 — сам мотор, 2 і 3 — його обертони",
    ),
    ("new slot, to compare", "новий слот, для порівняння"),
    (
        "no gyro.filtered in the log to validate against",
        "у лозі немає gyro.filtered для перевірки",
    ),
    (
        "no unfiltered gyro in this log",
        "у цьому лозі немає нефільтрованого гіро",
    ),
    (
        "no unfiltered gyro in this log (the filtered one has the motor peaks removed);",
        "у цьому лозі немає нефільтрованого гіро (у відфільтрованому піки моторів прибрано);",
    ),
    (
        "normalise every window on its settled tail before averaging",
        "нормувати кожне вікно за усталеним хвостом перед усередненням",
    ),
    ("not in this log", "немає в цьому лозі"),
    ("not logged", "не записано"),
    ("notches", "notch-и"),
    (
        "notches per motor, at the motor frequency and its multiples; 0 switches them off",
        "notch на мотор, на частоті мотора та її кратних; 0 вимикає їх",
    ),
    (
        "notches that follow the strongest peaks of the gyro; 0 switches them off",
        "notch, що йдуть за найсильнішими піками гіро; 0 вимикає їх",
    ),
    ("note", "примітка"),
    (
        "nothing to change: every rule is satisfied",
        "нічого міняти: усі правила виконані",
    ),
    ("of", "з"),
    ("off", "вимк."),
    ("one axis at full height", "одна вісь на всю висоту"),
    (
        "one colour scale for every log (the largest automatic maximum), so the same colour means the same value",
        "одна шкала кольору для всіх логів (найбільша з автоматичних стель): однаковий колір означає однакове значення",
    ),
    (
        "open another log to compare (up to 8)",
        "відкрити ще один лог для порівняння (до 8)",
    ),
    ("other", "інше"),
    ("overshoot", "перерегулювання"),
    ("overshoot fail", "перерегулювання: межа"),
    ("overshoot limit", "межа перерегулювання"),
    ("overshoot warn", "перерегулювання: увага"),
    ("peak", "пік"),
    ("peak prominence", "виразність піка"),
    ("peaks", "піки"),
    ("phase (deg)", "фаза (°)"),
    (
        "playback speed relative to the flight",
        "швидкість відтворення відносно польоту",
    ),
    ("position", "позиція"),
    (
        "power in decibels (10·log10) instead of unit²/Hz",
        "потужність у децибелах (10·log10) замість одиниця²/Гц",
    ),
    ("prefilt", "до фільтра"),
    ("prefilt (dB)", "до фільтра (дБ)"),
    ("profile", "профіль"),
    ("prominence", "виразність"),
    ("raw", "сирий"),
    ("response", "відгук"),
    ("rest", "спокої"),
    ("rise", "підйом"),
    ("rise max", "підйом макс"),
    ("rpm", "об/хв"),
    ("running", "рахується"),
    ("samples", "семпли"),
    ("segment", "сегмент"),
    ("session", "сесія"),
    ("setpoint", "уставка"),
    ("setpoint minus gyro", "уставка мінус гіро"),
    ("settling", "усталення"),
    ("settling max", "усталення макс"),
    (
        "seven steps through the tuning loop, on the sample log",
        "сім кроків циклом налаштування на зразковому лозі",
    ),
    ("shading", "затінення"),
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
    ("smoothing", "згладжування"),
    ("solo", "лише цей"),
    ("spectrum at", "спектр на"),
    ("speed", "швидкість"),
    ("start", "початок"),
    ("start = cursor", "початок = курсор"),
    ("step >", "крок >"),
    ("sum", "сума"),
    ("target band", "цільова смуга"),
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
        "the log's unfiltered gyro through the modelled chain (sim) against the filtered gyro the flight controller logged",
        "нефільтроване гіро логу через модель ланцюжка (sim) проти відфільтрованого гіро, яке записав польотний контролер",
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
    (
        "the same for the unfiltered gyro: what the filters remove",
        "те саме для нефільтрованого гіро: що прибирають фільтри",
    ),
    (
        "the same target as on the Step tab (Settings → Targets)",
        "той самий поріг, що на вкладці Step (Settings → Targets)",
    ),
    (
        "the static notches, the dynamic-notch band and the RPM filter minimum",
        "статичні нотчі, смуга динамічного нотча й мінімум RPM-фільтра",
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
        "the unfiltered gyro beside the filtered one",
        "нефільтрований гіро поруч із фільтрованим",
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
    ("throttle", "газ"),
    ("throttle (%)", "газ (%)"),
    (
        "throttle bins with fewer segments are dimmed: they are unsampled, not quiet",
        "біни газу з меншою кількістю сегментів затемнено: там немає даних, а не тиша",
    ),
    ("throttle window ±", "вікно газу ±"),
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
    ("unmapped fields", "незіставлені поля"),
    (
        "upper end of the power axis; auto follows the drawn data",
        "верхня межа осі потужності; авто — за показаними даними",
    ),
    (
        "upper end of the response axis; 1 is the setpoint",
        "верхня межа осі відгуку; 1 — це уставка",
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
        "whether the logged throttle moved the dynamic low-pass",
        "чи рухав записаний газ динамічний low-pass",
    ),
    ("whole log", "весь лог"),
    ("whole log, every slot", "весь лог, усі слоти"),
    ("window", "вікно"),
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
        "×1 over the mean of the motors",
        "×1 відносно середнього по моторах",
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
