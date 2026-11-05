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
    ("Angular rate", "Кутова швидкість"),
    ("Appearance", "Вигляд"),
    ("Axis", "Вісь"),
    (
        "Betaflight CLI to apply (select all, copy):",
        "Команди Betaflight CLI (виділити все, скопіювати):",
    ),
    ("Channels:", "Канали:"),
    ("Clear markers", "Прибрати маркери"),
    (
        "Click or tap the plot to place the cursor, then set the epoch from it.",
        "Клацніть або торкніться графіка, щоб поставити курсор, і виставте епоху з нього.",
    ),
    ("D step", "крок D"),
    ("Dark", "Темна"),
    ("Diagnostics", "Діагностика"),
    ("Dyn notch", "Dyn notch"),
    ("Evidence", "Докази"),
    ("Export", "Експорт"),
    ("Filter Sim", "Симуляція фільтрів"),
    ("Firmware defaults", "Типові значення прошивки"),
    ("Freq × throttle", "Частота × газ"),
    ("Freq × time", "Частота × час"),
    ("LPF headroom", "запас над LPF"),
    ("Language", "Мова"),
    ("Light", "Світла"),
    (
        "Load a Betaflight 4.3+ log into slot A to use the PID sliders.",
        "Завантажте лог Betaflight 4.3+ у слот A, щоб користуватися PID-слайдерами.",
    ),
    ("Log viewer", "Перегляд логу"),
    ("Motor noise", "Шум моторів"),
    (
        "No configuration directory: settings last until the app closes.",
        "Немає теки конфігурації: налаштування діють до закриття програми.",
    ),
    ("Notch", "Notch"),
    ("Open", "Відкрити"),
    ("Open log", "Відкрити лог"),
    ("Open…", "Відкрити…"),
    ("PID roll / pitch / yaw", "PID roll / pitch / yaw"),
    ("PID sliders", "PID-слайдери"),
    ("Pause", "Пауза"),
    ("Play", "Відтворити"),
    ("Plot as PNG…", "Графік як PNG…"),
    ("Project", "Проєкт"),
    ("Proposals", "Пропозиції"),
    ("RPM notch", "RPM notch"),
    ("RPM overlay", "RPM-overlay"),
    ("Reset to defaults", "Скинути до типових"),
    ("Reset to log", "Скинути до логу"),
    ("Reset view", "Скинути вид"),
    (
        "Rules are hints with their evidence, not an autotune: apply one change, fly, compare the new log.",
        "Правила — підказки з доказами, не autotune: застосуйте одну зміну, політайте, порівняйте новий лог.",
    ),
    ("Save as…", "Зберегти як…"),
    ("Settings", "Налаштування"),
    ("Slot", "Слот"),
    ("Spectrum", "Спектр"),
    ("Step response", "Перехідна характеристика"),
    ("Surface 3D", "Поверхня 3D"),
    ("Theme", "Тема"),
    ("To start", "На початок"),
    ("Touch mode", "Сенсорний режим"),
    (
        "Touch mode (larger controls, touch gestures)",
        "Сенсорний режим (більші елементи, жести)",
    ),
    ("Units", "Одиниці"),
    (
        "Units apply to the log viewer traces; analyses stay in deg/s.",
        "Одиниці стосуються кривих перегляду логу; аналізи лишаються в °/с.",
    ),
    ("Y-offset", "Зсув Y"),
    ("axis", "вісь"),
    ("channel", "канал"),
    ("close", "закрити"),
    ("colour max (0 = auto)", "стеля кольору (0 = авто)"),
    (
        "corrupt / desync / skipped bytes",
        "пошкоджені / десинхр. / пропущені байти",
    ),
    ("cutoff", "зріз"),
    (
        "drag: orbit · wheel / pinch: zoom · shift-drag, right-drag or two fingers: pan · double-click: reset",
        "перетягування: обертання · коліщатко / щипок: масштаб · shift-, праве або два пальці: зсув · подвійний клік: скинути",
    ),
    ("empty", "порожній"),
    ("epoch", "епоха"),
    ("filter", "фільтр"),
    ("filter…", "фільтр…"),
    ("frames", "кадри"),
    ("from sliders", "зі слайдерів"),
    ("harmonics", "гармоніки"),
    ("height", "висота"),
    ("height of the colour maximum", "висота стелі кольору"),
    ("high input", "верхній вхід"),
    (
        "into the first empty slot, or a new one",
        "у перший порожній слот або в новий",
    ),
    ("loading", "завантаження"),
    ("logged", "у лозі"),
    ("max", "макс"),
    ("min input", "мін. вхід"),
    ("min segments", "мін. сегментів"),
    ("motor", "мотор"),
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
    (
        "open another log to compare (up to 8)",
        "відкрити ще один лог для порівняння (до 8)",
    ),
    ("overshoot limit", "межа перерегулювання"),
    ("peak prominence", "виразність піка"),
    ("peaks", "піки"),
    ("position", "позиція"),
    ("profile", "профіль"),
    ("prominence", "виразність"),
    ("rpm", "об/хв"),
    ("samples", "семпли"),
    ("segment", "сегмент"),
    ("session", "сесія"),
    ("shading", "затінення"),
    ("show", "показувати"),
    ("smooth", "згладжування"),
    ("solo", "лише цей"),
    ("speed", "швидкість"),
    ("step >", "крок >"),
    (
        "throttle bins with fewer segments are greyed out: they are unsampled, not quiet",
        "біни газу з меншою кількістю сегментів затемнені: там немає даних, а не тиша",
    ),
    ("throttle window ±", "вікно газу ±"),
    ("total", "разом"),
    ("type", "тип"),
    ("unmapped fields", "незіставлені поля"),
    ("window", "вікно"),
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
