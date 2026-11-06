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
    ("< step", "< крок"),
    (
        "A log without eRPM: estimate motor speed from the gyro",
        "Лог без eRPM: оцінити оберти моторів з гіроскопа",
    ),
    ("Acceleration", "Прискорення"),
    ("Add log", "Додати лог"),
    ("Advice", "Поради"),
    (
        "Advice with its evidence and ready CLI commands. Copy and apply.",
        "Поради з доказами й готові команди CLI. Скопіюйте й застосуйте.",
    ),
    ("All", "Увесь"),
    ("Angular rate", "Кутова швидкість"),
    ("Appearance", "Вигляд"),
    ("Auto", "Авто"),
    ("Axis", "Вісь"),
    ("Back", "Назад"),
    (
        "Betaflight CLI to apply (select all, copy):",
        "Команди Betaflight CLI (виділити все, скопіювати):",
    ),
    (
        "Blackbox analysis for tuning: noise, filters, step response, advice.",
        "Аналіз blackbox для налаштування: шум, фільтри, step response, поради.",
    ),
    ("Channels:", "Канали:"),
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
    ("Copy", "Копіювати"),
    ("Custom", "Власні"),
    ("D step", "крок D"),
    ("Dark", "Темна"),
    ("Diagnostics", "Діагностика"),
    ("Done", "Готово"),
    ("Drop a log here, or", "Перетягніть лог сюди або"),
    ("Dyn notch", "Dyn notch"),
    ("Evidence", "Докази"),
    ("Export", "Експорт"),
    ("Filter Sim", "Симуляція фільтрів"),
    ("Firmware defaults", "Типові значення прошивки"),
    ("First run", "Перший запуск"),
    ("Freestyle", "Фрістайл"),
    ("Freq × throttle", "Частота × газ"),
    ("Freq × time", "Частота × час"),
    ("Go to Advice", "До порад"),
    ("Help", "Довідка"),
    ("Keyboard shortcuts", "Гарячі клавіші"),
    ("LPF headroom", "запас над LPF"),
    ("Language", "Мова"),
    ("Light", "Світла"),
    (
        "Load a Betaflight 4.3+ log into slot A to use the PID sliders.",
        "Завантажте лог Betaflight 4.3+ у слот A, щоб користуватися PID-слайдерами.",
    ),
    ("Log check", "Перевірка логу"),
    ("Log viewer", "Перегляд логу"),
    ("Most active 20 s", "Найактивніші 20 с"),
    ("Motor noise", "Шум моторів"),
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
    ("Open", "Відкрити"),
    ("Open log", "Відкрити лог"),
    ("Open sample log", "Відкрити зразковий лог"),
    ("Open…", "Відкрити…"),
    ("PID roll / pitch / yaw", "PID roll / pitch / yaw"),
    ("PID sliders", "PID-слайдери"),
    ("Pause", "Пауза"),
    ("Play", "Відтворити"),
    ("Player", "Плеєр"),
    ("Plot as PNG…", "Графік як PNG…"),
    ("Preset", "Пресет"),
    ("Previous tab", "Попередня вкладка"),
    ("Project", "Проєкт"),
    ("Proposals", "Пропозиції"),
    ("RPM notch", "RPM notch"),
    ("RPM overlay", "RPM-overlay"),
    ("Racing", "Гонки"),
    ("Recent files", "Нещодавні файли"),
    ("Reset first-run state", "Скинути стан першого запуску"),
    ("Reset to defaults", "Скинути до типових"),
    ("Reset to log", "Скинути до логу"),
    ("Reset view", "Скинути вид"),
    (
        "Rules are hints with their evidence, not an autotune: apply one change, fly, compare the new log.",
        "Правила — підказки з доказами, не autotune: застосуйте одну зміну, політайте, порівняйте новий лог.",
    ),
    ("Save as…", "Зберегти як…"),
    ("Settings", "Налаштування"),
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
    ("Slot", "Слот"),
    ("Spectrum", "Спектр"),
    ("Start the tour", "Почати тур"),
    ("Step", "Крок"),
    ("Step response", "Перехідна характеристика"),
    (
        "Stick response: overshoot and delay, judged against your targets.",
        "Відгук на стік: перерегулювання і затримка проти ваших цілей.",
    ),
    ("Surface 3D", "Поверхня 3D"),
    ("Tab", "Вкладка"),
    ("Targets", "Цілі"),
    ("Theme", "Тема"),
    (
        "Thresholds behind the step-response verdicts and the Advice overshoot rule.",
        "Пороги вердиктів step response і правила перерегулювання в Порадах.",
    ),
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
    ("Units", "Одиниці"),
    (
        "Units apply to the log viewer traces; analyses stay in deg/s.",
        "Одиниці стосуються кривих перегляду логу; аналізи лишаються в °/с.",
    ),
    ("User guide", "Посібник користувача"),
    ("Y-offset", "Зсув Y"),
    (
        "Your logs live here. Drop another one to compare.",
        "Тут ваші логи. Перетягніть ще один, щоб порівняти.",
    ),
    (
        "a synthetic 30 s flight: no file needed",
        "синтетичний політ 30 с, файл не потрібен",
    ),
    ("axis", "вісь"),
    ("channel", "канал"),
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
    ("delay", "затримка"),
    (
        "drag a handle or the window; double-click = whole log",
        "тягніть ручку або вікно; подвійний клік = весь лог",
    ),
    (
        "drag: orbit · wheel / pinch: zoom · shift-drag, right-drag or two fingers: pan · double-click: reset",
        "перетягування: обертання · коліщатко / щипок: масштаб · shift-, праве або два пальці: зсув · подвійний клік: скинути",
    ),
    ("empty", "порожній"),
    ("end", "кінець"),
    ("epoch", "епоха"),
    ("filter", "фільтр"),
    ("filter…", "фільтр…"),
    ("final", "кінцеве"),
    ("final tolerance", "допуск кінцевого"),
    ("first empty slot", "перший порожній слот"),
    ("frames", "кадри"),
    ("from sliders", "зі слайдерів"),
    ("harmonics", "гармоніки"),
    ("height", "висота"),
    ("height of the colour maximum", "висота стелі кольору"),
    ("high input", "верхній вхід"),
    (
        "hover the map for a value",
        "наведіть на карту, щоб побачити значення",
    ),
    (
        "into the first empty slot, or a new one",
        "у перший порожній слот або в новий",
    ),
    (
        "keys: Right / Enter, Left, Esc",
        "клавіші: → / Enter, ←, Esc",
    ),
    ("latency", "лаг"),
    ("link scales", "спільна шкала"),
    ("loading", "завантаження"),
    ("log shorter than 20 s", "лог коротший за 20 с"),
    ("log too short to trim", "лог закороткий для обрізання"),
    ("logged", "у лозі"),
    ("max", "макс"),
    ("min input", "мін. вхід"),
    ("min segments", "мін. сегментів"),
    ("min windows ok", "мін. вікон (ok)"),
    ("min windows weak", "мін. вікон (слабко)"),
    ("motor", "мотор"),
    ("new slot, to compare", "новий слот, для порівняння"),
    (
        "normalise every window on its settled tail before averaging",
        "нормувати кожне вікно за усталеним хвостом перед усередненням",
    ),
    ("notches", "notch-и"),
    ("note", "примітка"),
    (
        "nothing to change: every rule is satisfied",
        "нічого міняти: усі правила виконані",
    ),
    ("of", "з"),
    (
        "one colour scale for every log (the largest automatic maximum), so the same colour means the same value",
        "одна шкала кольору для всіх логів (найбільша з автоматичних стель): однаковий колір означає однакове значення",
    ),
    (
        "open another log to compare (up to 8)",
        "відкрити ще один лог для порівняння (до 8)",
    ),
    ("overshoot", "перерегулювання"),
    ("overshoot fail", "перерегулювання: межа"),
    ("overshoot limit", "межа перерегулювання"),
    ("overshoot warn", "перерегулювання: увага"),
    ("peak prominence", "виразність піка"),
    ("peaks", "піки"),
    ("position", "позиція"),
    ("profile", "профіль"),
    ("prominence", "виразність"),
    ("rise", "підйом"),
    ("rise max", "підйом макс"),
    ("rpm", "об/хв"),
    ("samples", "семпли"),
    ("segment", "сегмент"),
    ("session", "сесія"),
    ("settling", "усталення"),
    ("settling max", "усталення макс"),
    (
        "seven steps through the tuning loop, on the sample log",
        "сім кроків циклом налаштування на зразковому лозі",
    ),
    ("shading", "затінення"),
    ("show", "показувати"),
    ("smooth", "згладжування"),
    ("solo", "лише цей"),
    ("speed", "швидкість"),
    ("start", "початок"),
    ("step >", "крок >"),
    ("target band", "цільова смуга"),
    (
        "the same target as on the Step tab (Settings → Targets)",
        "той самий поріг, що на вкладці Step (Settings → Targets)",
    ),
    (
        "throttle bins with fewer segments are greyed out: they are unsampled, not quiet",
        "біни газу з меншою кількістю сегментів затемнені: там немає даних, а не тиша",
    ),
    ("throttle window ±", "вікно газу ±"),
    ("timeline", "таймлайн"),
    ("total", "разом"),
    ("type", "тип"),
    ("unmapped fields", "незіставлені поля"),
    ("whole log", "весь лог"),
    ("whole log, every slot", "весь лог, усі слоти"),
    ("window", "вікно"),
    ("windows", "вікна"),
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
