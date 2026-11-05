# OpenPID — дизайн-проєкт

Статус: v0.4 (2026-11-04) — фази 0 і 1 реалізовані (див. §12); процес розробки та стиль — `CONTRIBUTING.md`, `docs/STYLE.md`, `docs/RELEASING.md`. Мова документа — українська, технічні терміни та ідентифікатори коду — англійською.

OpenPID — кросплатформенний (macOS / Windows / Linux) інструмент на Rust для розбору blackbox-логів політних контролерів, навігації по них та допомоги у налаштуванні PID і фільтрів. Функціональний орієнтир — PIDtoolbox / PIDtoolbox Pro, але весь код (декодер, математика, GUI) пишеться власний.

---

## 1. Цілі та принципи

| # | Принцип | Що це означає на практиці |
|---|---------|---------------------------|
| P1 | **Тільки Rust, мінімум залежностей** | Залежності лише там, де власна реалізація невиправдано дорога або небезпечна (GPU/віконна система, FFT, серіалізація). Кожна залежність фіксується в ADR з обґрунтуванням. Заборонено транзитивно тягнути C/C++ (крім системних API ОС). |
| P2 | **Надійна математика** | `f64` у всіх обчисленнях; одиниці вимірювання у типах; явні маски валідності замість «тихих» NaN; кожен алгоритм має еталонні тести проти незалежної реалізації (NumPy/SciPy‑скрипти в `tools/reference`, лише для розробки). |
| P3 | **Розширюваність без переписування ядра** | Формат логу, прошивка, аналіз, рекомендатор (autotune), експортер — усе підключається через trait‑и та реєстри. Додати новий формат = додати crate, що реалізує `LogFormat`. Ядро (`openpid-core`) не знає про Betaflight. |
| P4 | **Один рушій, багато фронтендів** | GUI, CLI та тести використовують той самий конвеєр. GUI — лише представлення даних, у ньому немає математики. |
| P5 | **Чесність до користувача** | Пошкоджені кадри, пропуски, нестабільний sample‑rate, невідомі поля — показуються, а не маскуються. Будь‑який графік має підпис, з яких параметрів він обчислений. |
| P6 | **Кросплатформенність і сенсорне керування з першого дня** | Один бінарник на платформу, GPU‑рендеринг, мультитач (pinch‑zoom, pan двома пальцями), великі цілі дотику в «touch‑режимі». |

Не‑цілі (на цьому етапі): веб‑версія, мобільні ОС, реальний час з FC через USB (але архітектура цього не блокує — див. §9).

---

## 2. Функціональний обсяг

Зіставлення з таблицею можливостей PIDtoolbox / Pro та план фаз.

| Можливість | PTB | Pro | OpenPID фаза | Примітка |
|------------|:---:|:---:|:---:|----------|
| Log viewer (часові графіки, A/B) | ✓ | ✓ | 1 | Два логи A/B поруч, як у PTB |
| Step response | ✓ | ✓ | 1 | Деконволюція Вінера (див. §6.2) |
| 2D spectral analyzer (PSD) | ✓ | ✓ | 1 | Welch |
| Freq × throttle heatmap | ✓ | ✓ | 1 | §6.4 |
| Freq × time heatmap (STFT) | ✓ | ✓ | 1 | §6.5 |
| PID slider tool | ✓ | ✓ | 1 | Betaflight‑специфіка, реалізація через `FirmwareProfile` |
| Multi‑firmware support | ✓ | ✓ | 0→2 | Betaflight/Emuflight/Rotorflight — фаза 0; ArduPilot — зроблено (ADR‑0010, `docs/ARDUPILOT.md`), далі INAV — фаза 2; KISS/Fettec/QuickSilver — за наявності зразків (рішення Q2) |
| RPM / dyn‑notch overlay 2D | — | ✓ | 2 | Потрібен eRPM (DShot telemetry) + pole count |
| RPM / dyn‑notch overlay 3D | — | ✓ | 2 | 3D‑поверхня freq×throttle×amp |
| RPM estimator | — | ✓ | 2 | Оцінка обертів з піків шуму, коли телеметрії немає |
| Filter sim | — | ✓ | 2 | Прогін `gyroUnfilt` через модель фільтрів прошивки |
| Test signal generator | — | ✓ | 2 | Синтетичні сигнали для перевірки фільтрів/алгоритмів |
| Motor / prop noise tool | — | ✓ | 2 | Спектри по моторах, гармоніки |
| Dynamic spectrum player | — | ✓ | 2 | Анімоване відтворення STFT у часі |
| Рекомендації / autotune | — | — | 3 | Власне розширення, trait `Recommender` |
| Експорт (CSV/JSON/PNG, CLI diff) | част. | част. | 1 | |
| Headless CLI | — | — | 0 | Для CI, скриптів і golden‑тестів |

---

## 3. Архітектура верхнього рівня

```
                 ┌──────────────────────────────────────────────┐
                 │               openpid-app (GUI)              │
                 │  egui/eframe · views · touch · state/cache   │
                 └───────────────┬──────────────────────────────┘
                                 │ тільки типи з core/analysis
┌──────────────┐   ┌─────────────┴────────────┐   ┌──────────────────┐
│ openpid-cli  │──▶│     openpid-analysis     │◀──│  openpid-sim     │
│ (headless)   │   │ step · spectral · heat · │   │ filter sim · gen │
└──────┬───────┘   │ notch overlay · metrics  │   └────────┬─────────┘
       │           └─────────────┬────────────┘            │
       │                         │                         │
       │           ┌─────────────┴────────────┐            │
       │           │       openpid-dsp        │◀───────────┘
       │           │ fft · windows · welch ·  │
       │           │ deconv · filters · resamp│
       │           └─────────────┬────────────┘
       │                         │
       │           ┌─────────────┴────────────┐
       └──────────▶│       openpid-core       │
                   │ FlightLog · Channel ·    │
                   │ units · traits · registry│
                   └─────────────┬────────────┘
                                 │ impl LogFormat / FirmwareProfile
      ┌──────────────┬───────────┴──────────┬──────────────────┐
      │ fmt-blackbox │ fmt-blackbox-csv     │ fmt-ardupilot …  │
      │ (BF/Emu/RF/  │ (вихід blackbox_     │ (фаза 2)         │
      │  INAV binary)│  decode, для тестів) │                  │
      └──────────────┴──────────────────────┴──────────────────┘
```

Cargo workspace:

```
openpid/
├─ Cargo.toml                 # workspace, [workspace.dependencies], lints
├─ crates/
│  ├─ openpid-core/           # доменна модель, trait‑и, реєстри, помилки
│  ├─ openpid-dsp/            # чиста математика, без I/O
│  ├─ openpid-analysis/       # аналізи поверх core+dsp
│  ├─ openpid-sim/            # симуляція фільтрів, генератор сигналів
│  ├─ openpid-fmt-blackbox/   # власний декодер бінарного Blackbox
│  ├─ openpid-fmt-csv/        # CSV від blackbox_decode (тести/сумісність)
│  ├─ openpid-cli/            # бінарник `openpid`
│  └─ openpid-app/            # бінарник `openpid-gui`
├─ tests/
│  ├─ logs/                   # еталонні логи (малі, різні прошивки)
│  └─ golden/                 # очікувані результати аналізів (JSON)
├─ tools/
│  └─ reference/              # Python‑скрипти генерації еталонів (dev‑only)
└─ docs/
   ├─ DESIGN.md
   └─ adr/
```

Правила залежностей між crate‑ами (перевіряються в CI через `cargo deny`/власний скрипт):

- `core` не залежить ні від чого в workspace.
- `dsp` залежить лише від `core` (для типів одиниць) і `rustfft`.
- `fmt-*` залежать лише від `core`.
- `analysis`, `sim` → `core`, `dsp`.
- `app`, `cli` → усе вище; **ніхто не залежить від `app`**.

---

## 4. Доменна модель (`openpid-core`)

### 4.1 Канонічне представлення логу

```rust
/// Один файл може містити кілька польотів (сесій) — як .BBL з кількома логами.
pub struct LogFile {
    pub source: SourceInfo,            // шлях, розмір, хеш
    pub sessions: Vec<FlightSession>,
}

pub struct FlightSession {
    pub id: SessionId,
    pub firmware: FirmwareInfo,        // vendor, version, target, дата
    pub settings: Settings,            // типізовані налаштування (PID, фільтри, rates…)
    pub raw_headers: Vec<(String, String)>, // усі заголовки «як є»
    pub timebase: Timebase,            // номінальний крок, фактичний, jitter, пропуски
    pub channels: ChannelSet,          // усі ряди даних
    pub events: Vec<Event>,            // E‑кадри: arm/disarm, inflight adjust, logging resume…
    pub diagnostics: Diagnostics,      // кількість пошкоджених кадрів, resync, невідомі поля
}
```

**Канал** — єдиний ряд з однією одиницею вимірювання:

```rust
pub struct Channel {
    pub id: ChannelId,
    pub unit: Unit,                    // DegPerSec, Percent, Us, Raw, Rpm, Volt, …
    pub data: Series,                  // f64 + маска валідності + індекс часу
    pub origin: Origin,                // Logged | Derived { from: Vec<ChannelId>, by: &'static str }
}

/// Семантичний ідентифікатор, незалежний від прошивки.
pub enum ChannelId {
    Time,
    LoopIteration,
    Gyro { axis: Axis, stage: GyroStage },     // Raw | Scaled | Filtered
    Setpoint { axis: Axis },
    PidTerm { axis: Axis, term: PidTerm },     // P | I | D | F | S(um)
    PidError { axis: Axis },                   // похідний
    RcCommand { ch: u8 },
    Throttle,                                   // похідний, 0..100 %
    Motor { idx: u8 },
    Erpm { idx: u8 },
    Debug { idx: u8 },                          // сирий debug[n]
    Battery(BatteryField),
    Custom(Arc<str>),                           // усе, що формат не зміг зіставити
}
```

Ключова ідея: **`fmt-*` crate зіставляє поля конкретної прошивки з `ChannelId`**, а всі аналізи працюють лише з `ChannelId`. Наприклад, у Betaflight 4.x `gyroUnfilt[0]` → `Gyro{Roll, Raw}`, а в старих версіях ті самі дані лежать у `debug[0..2]` при `debug_mode=GYRO_SCALED` — це знання живе у `FirmwareProfile`, не в аналізах.

### 4.2 Timebase

Blackbox не гарантує рівномірний крок: є jitter, пропущені кадри (dropped frames), `I`/`P` інтервали, resync після пошкодження. Політика:

- Зберігаємо **фактичні** мітки часу (`time` у мкс, i64) для кожного кадру.
- `Timebase` містить номінальний крок (з `looptime × pid_process_denom × P-interval`), медіанний фактичний крок, max‑jitter та список інтервалів‑прогалин.
- Для спектральних аналізів ряд **ресемплюється** на рівномірну сітку (лінійна інтерполяція в межах допуску, прогалини більші за `gap_tolerance` позначаються як невалідні й виключаються з сегментів). Це робиться один раз у `analysis::Prepared` і кешується.
- Усе це видно користувачу у панелі «Діагностика логу».

### 4.3 Налаштування (`Settings`)

Типізована структура з опціональними полями (`Option<T>`), яку заповнює `FirmwareProfile`:

```rust
pub struct Settings {
    pub pid: PerAxis<PidGains>,          // P, I, D, D_min/D_max, F
    pub gyro_filters: FilterChain,       // lowpass1/2 (тип, cutoff, dyn min/max), notch, dyn‑notch, rpm‑notch
    pub dterm_filters: FilterChain,
    pub rates: RatesProfile,
    pub motor: MotorConfig,              // протокол, poles, kv?
    pub loop_rates: LoopRates,           // gyro rate, pid rate, log rate
    pub extra: BTreeMap<String, String>, // усе, що не зіставлено
}
```

Усе зіставлення — через явні таблиці `header_name → setter`, щоб додавання нового параметра було локальною зміною.

### 4.4 Trait‑и розширення

```rust
/// Формат файлу логу.
pub trait LogFormat: Send + Sync {
    fn id(&self) -> &'static str;                       // "blackbox-binary"
    fn sniff(&self, head: &[u8], name: &str) -> Confidence;
    fn open(&self, src: &dyn ByteSource, opts: &OpenOptions) -> Result<LogFile, FormatError>;
}

/// Семантика конкретної прошивки/версії: мапінг полів, одиниці, дефолти.
pub trait FirmwareProfile: Send + Sync {
    fn matches(&self, headers: &RawHeaders) -> Option<Score>;
    fn map_field(&self, field: &str, headers: &RawHeaders) -> Option<(ChannelId, Unit, Scale)>;
    fn settings(&self, headers: &RawHeaders) -> Settings;
    fn derive(&self, session: &mut FlightSession);      // throttle %, pid error, gyro stage тощо
    fn filter_model(&self, s: &Settings) -> Option<FilterChainModel>; // для Filter Sim
    fn slider_model(&self) -> Option<Box<dyn SliderModel>>;          // для PID slider tool
}

/// Обчислення поверх сесії. Чиста функція від (дані, параметри).
pub trait Analysis: Send + Sync {
    type Params: Clone + Hash + Serialize + Default;
    type Output: Serialize;
    fn id(&self) -> &'static str;
    fn requires(&self) -> &'static [Requirement];      // які ChannelId потрібні
    fn run(&self, ctx: &AnalysisCtx, p: &Self::Params) -> Result<Self::Output, AnalysisError>;
}

/// Рекомендації змін налаштувань (autotune та подібне) — фаза 3.
pub trait Recommender: Send + Sync {
    fn id(&self) -> &'static str;
    fn recommend(&self, s: &FlightSession, results: &ResultStore) -> Vec<Proposal>;
}

/// Експорт у зовнішній вигляд (CLI diff прошивки, CSV, JSON, PNG).
pub trait Exporter: Send + Sync {
    fn id(&self) -> &'static str;
    fn export(&self, what: &Exportable, w: &mut dyn Write) -> Result<(), ExportError>;
}
```

Реєстри (`FormatRegistry`, `ProfileRegistry`, `AnalysisRegistry`, …) заповнюються статично в `openpid-app`/`openpid-cli` через `inventory`‑подібний власний механізм (простий `fn builtin() -> Vec<Box<dyn …>>`, без макросів і без динамічного завантаження). Динамічні плагіни (WASM / cdylib) свідомо відкладені: вони додають ABI‑поверхню і залежності; за потреби їх можна додати пізніше як ще один `LogFormat`/`Analysis` adapter без зміни trait‑ів.

### 4.5 Чому це «ООП» у Rust

Ми дотримуємося принципів, а не синтаксису спадкування:

- **Інверсія залежностей** — ядро залежить від абстракцій (trait), а конкретні прошивки/формати — від ядра.
- **Open/Closed** — нові формати, аналізи, рекомендатори додаються новими типами; існуючий код не змінюється.
- **Single responsibility** — crate = одна відповідальність (формат, математика, аналіз, GUI).
- **Композиція замість спадкування** — `FirmwareProfile` для Emuflight/Rotorflight *делегує* профілю Betaflight і перевизначає лише відмінності.
- **Типи як документація** — одиниці вимірювання, осі, стадії фільтрації закодовані в enum‑ах; неможливо випадково подати сирий gyro замість фільтрованого.

---

## 5. Декодер Blackbox (`openpid-fmt-blackbox`)

Власна реалізація за публічною специфікацією (Betaflight «Blackbox Logging Internals»; еталонні реалізації — `blackbox-tools/parser.c`, `blackbox-log-viewer/flightlog_parser.js`).

- **Вхід**: `&[u8]` (через `memmap2` для великих файлів або повне читання) — zero‑copy, без проміжних рядків.
- **Контейнер**: один файл → N логів, межа — маркер заголовка `H Product:Blackbox flight data recorder by Nicholas Sherlock`. Підтримати `.BBL/.BFL/.TXT` і дампи SD‑карти з «сміттям» між логами.
- **Заголовки**: `H Field I name/signed/predictor/encoding`, `H Field P predictor/encoding`, `H Field S …`, `H Field G/H …`, `I interval`, `P interval`, `P ratio`, `Data version`, `minthrottle`, `vbatref`, `gyro_scale`, `motorOutput` та всі інші — зберігаються сирими та передаються `FirmwareProfile`.
- **Кадри**: `I` (intra), `P` (inter), `S` (slow), `G`/`H` (GPS), `E` (events). Предиктори 0–10, кодування 0–10 (SignedVB, UnsignedVB, Neg14, EliasDelta u/s, TAG8_8SVB, TAG2_3S32, TAG8_4S16, Null, TAG2_3SVariable). Кожне кодування — окрема функція з property‑тестами (кодер у тестах → декодер → ідентичність).
- **Стійкість**: валідація кадру по наступному байту (має бути початком відомого кадру), перевірка монотонності `loopIteration`/`time`, resync на наступний `I`‑кадр, облік статистики пошкоджень у `Diagnostics`. Фаззинг (`cargo fuzz`) на декодері — обов'язковий, бо вхід недовірений.
- **Події**: sync beep, inflight adjustment, logging resume, disarm, flight mode flags → `Event`.
- **Вихід**: колонки `i64` → після мапінгу профілем стають `Channel` з `f64` та одиницями (масштаб застосовується один раз, у `f64`, без проміжного округлення).

Профілі (`openpid-fmt-blackbox::profiles`): `betaflight` (4.0–4.6+, різні debug_mode), `emuflight`, `rotorflight` (делегують BF з перевизначенням), `inav` (інші масштаби gyro, інші імена). Профіль обирається за `Firmware revision`, бо `Firmware type` завжди `Cleanflight`.

Окремий crate `openpid-fmt-csv` читає CSV від офіційного `blackbox_decode` — використовується для **конформанс‑тестів** нашого декодера (той самий лог → ті самі значення) і як fallback для екзотичних логів.

---

## 6. Математика (`openpid-dsp`, `openpid-analysis`)

### 6.1 Політика чисел

- Усі ряди — `f64`. Сирі лічильники (`loopIteration`, `time`) — `i64`/`u64` до останнього моменту.
- `Series` = `Vec<f64>` + `BitMask` валідності; операції через маску, без NaN‑«сентинелів».
- FFT — `rustfft` (чистий Rust, детермінований, MIT/Apache). Обгортка `dsp::fft::Planner` кешує плани за довжиною. Довжини — довільні (rustfft не вимагає степеня двійки), але для швидкості стандартні сегменти беруться найближчим «гладким» числом (2^a·3^b·5^c).
- Вікна: Hann (за замовчуванням, як у PTB), Hamming, Blackman‑Harris, Tukey(α). Нормування вікна (coherent gain / ENBW) враховується **явно** в PSD, щоб амплітуди між різними довжинами сегментів були порівнянні.
- Кожна функція `dsp` має тест проти еталону, згенерованого `tools/reference/*.py` (NumPy/SciPy) і збереженого як fixture; допуск — відносна похибка ≤ 1e‑9 для лінійних операцій, ≤ 1e‑6 для ітеративних.
- Додатково: property‑тести (Парсеваль для PSD, лінійність деконволюції, імпульс → крок через cumsum).

### 6.2 Step response

Метод — **Wiener‑деконволюція** вхід (setpoint) → вихід (gyro filtered), як у сучасних PTB/PID‑Analyzer, а не евристика «кінець маневру» зі старого PTB.

1. Епоха (вибраний користувачем діапазон часу) ділиться на вікна довжиною `L` (за замовч. 2 с) з кроком `L/superpos` (superpos = 16 → 125 мс).
2. Кожне вікно: `x = setpoint·w`, `y = gyro·w` (Hann), zero‑pad до гладкої довжини.
3. `H = Y·conj(X) / (|X|² + λ(f))`, де `λ(f)` — регуляризація: `reg_low` (0.1) нижче `cutoff` (за замовч. 25 Гц), `reg_high` (1e8) вище, між ними raised‑cosine перехід ±20 % у log‑шкалі. Обидва сигнали множаться на Hann перед FFT: без цього неспостережуваний хвіст відгуку за межею запису занижує підсилення (≈10 % для 4‑с запису і τ = 20 мс — перевірено на синтетичній моделі). Довжина FFT — 5‑smooth число ≥ 1.5·len, щоб циклічна згортка не накладалась. Імпульсна відповідь `h = IFFT(H)`, крокова `s = cumsum(h)` на перші 500 мс.
4. Відбір вікон: `max|setpoint| ≥ threshold` (за замовч. 20 °/с, окремо «low/high input» як у PID‑Analyzer з порогом 500 °/с), виключення вікон з прогалинами/пошкодженнями.
5. Агрегація: зважене середнє (вага = амплітуда входу) **та** 2D‑гістограма (час × амплітуда) для показу розкиду; опційна Y‑offset корекція (нормування на стале значення в хвості) і згладжування (PTB «smoothing 1/20/40/60»).
6. Метрики: rise time (10–90 %), overshoot %, settling time (±5 %), delay (час першого перетину 50 % усередненої кривої) і **latency** — зсув максимуму крос‑кореляції setpoint↔gyro по сирих сигналах епохи (`openpid_dsp::xcorr::lag_of_max`, пошук до 100 мс, параболічне уточнення піка). Для білого входу це транспортна затримка осі; для плавного стік‑входу пік захоплює й частину власного відставання об'єкта (на логах: 10–17 мс проти 50 %‑delay 18–22 мс), тому величину читають поруч із delay. Варіант із «відбілюванням» через перші різниці відкинуто: на реальних логах шум гіроскопа домінує в похідній (r ≈ 0.1) і пік стрибає від 4 до 37 мс залежно від згладжування. Обидві величини показуються поруч: delay залежить від фільтрів і PID, latency — транспортна затримка осі. Y‑offset: кожне вікно нормується на середнє останньої чверті відгуку, якщо воно > 0.2 (інакше вікно лишається як є).

Параметри (`StepParams`): `window_s, superpos, cutoff_hz, min_input, high_input, resp_len_s, y_offset, smoothing, axis`.

### 6.3 2D spectral analyzer (PSD)

Welch: сегменти `nfft` (за замовч. 1024 при 4 kHz… параметризовано у секундах), overlap 50 %, Hann, усереднення періодограм, одиниці — `(°/s)²/Hz` з опцією dB та лінійної. Канали: gyro raw/filtered, D‑term, PID error, setpoint, motor, debug[n]. Порівняння A/B на одній осі.

### 6.4 Freq × throttle heatmap

Як у PTB `PTthrSpec`, але без MATLAB‑артефактів:

1. Сегменти по 200 мс (параметр), Hann, FFT, однобічна амплітуда `|Y|/N·2`.
2. Для кожного сегмента — середній throttle (%).
3. Для кожного throttle‑біна `t ∈ 0..100` усереднюються спектри сегментів з `|thr − t| ≤ half_window` (за замовч. 6 %) — ковзне вікно по throttle.
4. Згладжування 2D box‑фільтром (параметри `thr_smooth=8`, `freq_smooth=segment_len/100`), обрізка до `max_freq`.
5. Результат — `Grid2D { x: throttle, y: freq, z: amp }` + кількість сегментів на бін (показується, щоб порожні біни не виглядали як «тиша»).

### 6.5 Freq × time heatmap (STFT)

STFT з `nfft`, hop, Hann → `Grid2D { x: time, y: freq, z: amp/dB }`. Це ж джерело для «Dynamic spectrum player» (фаза 2) — плеєр просто проходить по стовпцях.

### 6.6 Фільтри та overlay (фаза 2)

- `openpid-sim` (ADR‑0012): PT1/PT2/PT3, biquad LPF/notch (формули Betaflight/RBJ) як машини станів `update(x)` з аналітичною `response(f)`, зібрані в `Chain`. Усі — у `f64`, з тестами частотної відповіді проти аналітичних формул і узгодженості `update` ↔ `response`.
- `ChainModel::from_settings` збирає ланцюг gyro або D‑term із `Settings` (+ сирі заголовки для expo, fade, weights) і проганяє сигнал під керуванням `Drive` (газ, RPM моторів, центри dyn notch): lpf1 із газозалежним зрізом `min + (max−min)·(t(1−t)·expo + t)`, що перераховується раз на 5 мс із кроком 1 % газу; RPM‑notch на гармоніках `k·rpm/60` зі згладженою частотою, fade‑in над `min_hz`, ваговими коефіцієнтами гармонік і пропуском гармонік вище 0,48·fs; dyn notch, що стежить за найсильнішими піками STFT (≈100 мс, крок 16 мс, PT1 4 Гц) у смузі `min..max`. Порядок: RPM → статичні notch → lpf1 → lpf2 → dyn notch. Обмеження моделі: працює на частоті логу, а не гіроскопа (на 1 кГц лишається смуга до 500 Гц, гармоніки RPM вище не моделюються), трекер dyn notch — наближення SDFT прошивки, крива expo‑0 для < 4.2 — лінійна. На фікстурі Betaflight 2025.12 (gyroUnfilt у логу) залишок `gyro.sim − gyro.filtered` становить 5–13 % RMS проти 27–75 % без фільтрів (тест у crate).
- **Filter Sim** (`FilterSimAnalysis`, вкладка Filter Sim, `openpid run filter-sim --set …`): `gyro.raw → ChainModel(settings) → gyro.sim`, порівняння з `gyro.filtered` з логу (метрика валідації: залишок у % RMS, затримка за крос‑кореляцією) і з «what‑if» ланцюгом (`FilterOverrides`: зрізи й типи LPF1/LPF2, динамічний діапазон, RPM‑ і dyn‑notch, статичний notch; 0 вимикає стадію). Виводи: обидва ряди в часі, PSD raw/logged/sim/as‑flown, опис стадій. Потребує нефільтрованого гіро в логі (`gyroUnfilt`, `debug_mode GYRO_SCALED`, batch sampler ArduPilot).
- **RPM/dyn‑notch overlay** (`RpmOverlay` у виводі freq×throttle): з `motor_rpm.*` (eRPM і `motor_poles`) — медіана RPM кожного мотора на кожен бін газу (ті самі сегменти й вікно, що й спектри); GUI малює гармоніки `k·rpm/60` (перемикачі на мотор, кількість гармонік, за замовчуванням — скільки знімає RPM‑фільтр), смуги статичних notch, смугу dyn notch і мінімум RPM‑фільтра; у JSON CLI `overlay` є завжди, коли є RPM. Перевірено на фікстурі 4.5.1: фундаментальна лінія лежить на хребті спектра нефільтрованого гіро у 93 % бінів. 3D — та сама сітка як поверхня (#26).
- **RPM estimator** (`RpmEstimator`, `openpid run rpm`): коли eRPM відсутній — кандидати з найсильніших піків STFT нефільтрованого гіро у смузі `rpm_min..rpm_max` (і як 1‑ша…3‑тя гармоніка), динамічне програмування обирає неперервний у часі шлях (штраф за відносний стрибок і за вищу гармоніку), достовірність = перевищення піка над медіаною смуги. При чотирьох моторах на близьких обертах оцінка стежить за їх середнім: медіанна похибка 2,9 % на фікстурі 4.5.1 і 2,0 % на 2025.12 (проти логованого eRPM, рахується у виводі як `median_error`). `gyro.filtered` непридатний — RPM‑фільтр уже зняв піки.

### 6.7 Кеш та інкрементальність

`ResultStore` кешує `Output` за ключем `(session_id, epoch, analysis_id, hash(params))`. Важкі обчислення виконуються у фоновому потоці (`std::thread` + канал; `rayon` — лише якщо вимірювання покажуть потребу), GUI отримує прогрес і не блокується.

---

## 7. GUI (`openpid-app`)

### 7.1 Технологія

**egui + eframe (wgpu backend) + egui_plot** — ADR‑0002. Чистий Rust, один бінарник на macOS/Windows/Linux, GPU‑рендеринг, вбудована підтримка мультитачу (`MultiTouchInfo`: pinch‑zoom, two‑finger pan, rotation), активна спільнота, стабільні релізи.

Розглянуті альтернативи: `iced` 0.14 (Elm‑архітектура, менше готових компонентів для графіків), `slint` 1.18 (власна DSL‑мова розмітки, ліцензія GPLv3 / royalty‑free з обмеженнями / комерційна), `tauri` (веб‑стек — суперечить P1).

### 7.2 Структура застосунку

```
AppState
├─ Workspace { logs: [LoadedLog], active: {A, B}, epoch: {A, B} }
├─ ResultStore (кеш аналізів)
├─ ViewState (поточна вкладка, параметри кожного інструмента, zoom/pan)
└─ Prefs (тема, touch‑режим, одиниці, мова)

Views (кожен — struct, що реалізує trait View { fn ui(&mut self, ctx, state) })
├─ LogViewer      — часові графіки, вибір епохи, курсор‑синхронізація A/B
├─ StepResponse   — криві + метрики + гістограма розкиду
├─ Spectral2D     — PSD, A/B overlay, маркери частот
├─ HeatmapThr     — freq×throttle, overlay notch/RPM
├─ HeatmapTime    — freq×time, плеєр (фаза 2)
├─ SliderTool     — PID slider ↔ gains
├─ FilterSim      — (фаза 2)
├─ Diagnostics    — заголовки, статистика кадрів, прогалини
└─ Settings
```

- Heatmap рендериться у текстуру (`ColorImage` → `TextureHandle`) один раз на зміну даних; осі/overlay малюються поверх через `egui_plot`. Це тримає 60 fps навіть при 100×1000 сітках.
- Усі параметри інструментів — звичайні `struct`и, серіалізуються у «проєкт» (`.openpid.json`): відкрив — отримав ті самі графіки.
- **Touch‑режим**: автовмикання при першій touch‑події або вручну; збільшує розміри контролів (≥ 44 pt), додає жести: pinch — zoom по осі, два пальці — pan, довгий дотик — курсор‑вимірювач, двотап — reset zoom.
- Теми: темна (за замовч., у стилі PTB) і світла; локалізація UA/EN через просту таблицю рядків.
- **Settings** (фаза 1): тема, touch‑режим, одиниці (deg/s ↔ rad/s, g ↔ m/s²) зберігаються у `prefs.json` в каталозі конфігурації платформи (`~/.config/openpid`, `~/Library/Application Support/openpid`, `%APPDATA%\openpid`; перевизначення — `OPENPID_CONFIG_DIR`). Невідомі поля ігноруються, відсутні — за замовчуванням, тому версії можуть ділити файл. Одиниці застосовуються до трас log viewer; аналізи лишаються в deg/s.

### 7.3 Робочий процес користувача

1. Відкрити файл (діалог `rfd` або drag‑and‑drop) → список сесій у файлі → вибрати.
2. Log viewer: побачити весь політ, вибрати епоху (у PTB — поля «start/end sec»; у нас — ще й перетягування меж на графіку).
3. Перейти на інструмент; параметри змінюються — результат перераховується у фоні.
4. Завантажити лог B, порівняти.
5. Експорт: PNG графіка, CSV/JSON результатів, (фаза 3) diff налаштувань.

---

## 8. CLI (`openpid-cli`)

```
openpid info  <log>                      # сесії, прошивка, діагностика
openpid decode <log> --session 1 --csv   # сумісний з blackbox_decode CSV
openpid run step <log> --axis roll --epoch 5..60 --json
openpid run psd  <log> --channel gyro.filtered.roll
openpid run thr-heatmap <log> --png out.png
```

`openpid gen [<spec.json>] [--out synthetic.csv]` (`openpid-sim::SignalSpec`): синтетична сесія з відомою «правдою» — профіль газу (константа/рампа/синус), стік (кроки/чирп/синус), об'єкт першого порядку із затримкою, гармоніки моторів, прив'язані до газу, білий/рожевий шум і тони, PT1‑«прошивковий» фільтр, джитер і пропуски. Записується як масштабований CSV із канонічними ключами колонок, який `openpid` відкриває як звичайний лог (`gen --example` друкує спец за замовчуванням). На цьому ґрунтуються генераторні тести аналізів (`crates/openpid-analysis/tests/synthetic.rs`).

CLI використовує ті самі реєстри, що й GUI, і є основою golden‑тестів у CI (вихід порівнюється з `tests/golden/*.json`).

---

## 9. Інтеграція з autotune / пристроєм (фаза 3, архітектурний задел)

- `Recommender` отримує `FlightSession` + результати аналізів і повертає `Vec<Proposal>` (`{ path: "pid.roll.d", from, to, rationale, confidence }`).
- `Exporter("betaflight-cli")` перетворює `Proposal` у `set …` команди / diff.
- Пізніше — `Device` trait (MSP через serial) для читання/запису налаштувань і завантаження логів з FC. Залежність на serial‑порт (`serialport` — має C‑код на деяких ОС) — предмет окремого ADR, коли дійдемо.

Жодна з цих частин не вимагає змін у `core`: усе — нові реалізації існуючих trait‑ів.

---

## 10. Залежності (початковий список, ADR‑0001)

| Crate | Навіщо | Чому не самому |
|-------|--------|----------------|
| `eframe`/`egui`/`egui_plot` | GUI, вікна, GPU | Власний UI‑тулкіт — роки роботи |
| `wgpu`, `winit` | транзитивно через eframe | те саме |
| `rustfft` | FFT | власний FFT можливий, але rustfft — еталонно перевірений, чистий Rust, SIMD; ризик помилок у власному вищий, ніж користь |
| `serde` + `serde_json` | проєкти, golden‑тести, експорт | де‑факто стандарт, чистий Rust |
| ~~`memmap2`~~ | zero‑copy великих логів | не знадобився: логи читаються в пам'ять цілком (аудит 2026‑11‑05, вилучено з workspace) |
| `rfd` | нативні діалоги файлів | платформні API |
| `thiserror` | типи помилок | лише derive‑макрос; можна й без нього |
| `clap` (тільки cli) | парсинг аргументів | опційно; можна замінити ручним парсером |

Dev‑only: `proptest`, `criterion`, `cargo-fuzz`, `insta` (snapshot). Усе інше — додається лише через ADR. `cargo deny` перевіряє ліцензії (allow: MIT/Apache‑2.0/BSD/Zlib/MPL‑2.0 — узгодити з Q1) та дублікати.

---

## 11. Якість і тестування

- **Unit**: кодування blackbox (roundtrip), предиктори, вікна, FFT‑обгортка, фільтри (частотна відповідь vs аналітика).
- **Reference**: `tools/reference/*.py` генерує fixtures (JSON) для step/PSD/heatmap на синтетичних сигналах і на еталонних логах; Rust‑тести порівнюють з допусками.
- **Conformance**: наш декодер vs CSV `blackbox_decode` на `tests/logs/*` (усі канали, усі кадри).
- **Golden**: вихід CLI на еталонних логах зафіксований у `tests/golden`; зміна алгоритму = свідоме оновлення golden з описом у PR.
- **Fuzz**: декодер blackbox, парсер заголовків.
- **Bench**: `criterion` для декодера (МБ/с) і для step/heatmap (мс на 60 с логу при 8 kHz).
- **CI**: GitHub Actions матриця macOS/Windows/Linux: `fmt`, `clippy -D warnings`, тести, `cargo deny`, збірка релізних бінарників. Цілі продуктивності: лог 8 kHz × 3 хв декодується < 1 с, усі аналізи фази 1 — < 2 с на ноутбуці.
- **Lints**: `#![deny(unsafe_code)]` у всіх crate‑ах, крім `fmt-blackbox` (mmap) із задокументованими `unsafe`‑блоками; `clippy::pedantic` вибірково.

---

## 12. План фаз

| Фаза | Зміст | Критерій готовності |
|------|-------|---------------------|
| 0 — Фундамент ✅ | workspace, `core` модель і trait‑и, `fmt-blackbox` (BF/Emu/RF), `fmt-csv`, `cli info/decode`, конформанс‑ і robustness‑тести, CI | Конформанс з незалежним декодером `blackbox-log` на всіх тестових логах; CSV‑конформанс з `blackbox_decode` вмикається, щойно з'являться CSV‑фікстури |
| 1 — Паритет з PTB (free) ✅ | `dsp` базис, step, PSD, heatmaps, GUI з 7 вкладками, A/B, touch, PID slider tool (Betaflight simplified tuning, звірено на логах 4.5 і 2025.12). Аудит 2026‑11‑05 — перенесено у backlog (milestone v0.4, якщо не вказано інше): проєкти (#31), експорт PNG/CSV (#32), звірка дефолтів з PTB (#33), touch‑жести §7.2 (#35), тема/локалізація (#34), вкладка Settings і збережені Prefs (#53), маркери частот у спектрі (#51), перетягування меж епохи на графіку (#52), лічильник сегментів на heatmap (#54), Y‑offset/latency step response (#50), property‑тести деконволюції (#55, v0.3), CSV‑фікстури `blackbox_decode` з фази 0 (#49, v0.2) | Користувач відтворює свій звичний PTB‑воркфлоу повністю в OpenPID |
| 2 — Паритет з Pro | фільтри/sim, notch/RPM overlay 2D/3D, RPM estimator, генератор, motor‑noise, spectrum player; формати ArduPilot → INAV → KISS… | Усі рядки таблиці §2 = «так» |
| 3 — Розширення | `Recommender`, CLI diff, (опц.) MSP | Перша рекомендація, яку користувач реально застосовує |

---

## 13. Ризики

| Ризик | Пом'якшення |
|-------|-------------|
| Нюанси формату між версіями прошивок (імена полів, масштаби, debug_mode) | Профілі + конформанс до `blackbox_decode`; збирати бібліотеку тестових логів від спільноти |
| Відсутність публічної специфікації для KISS/Fettec/QuickSilver/Ardupilot | Окремі `fmt-*` crate‑и, кожен — після отримання зразків; Ardupilot (DataFlash `FMT`‑самоописний) — найкраще документований |
| Розбіжність наших графіків із PTB «на око» через інші параметри згладжування | Параметри винесені в UI з PTB‑сумісними дефолтами; документуємо різницю |
| Продуктивність egui на великих масивах точок | Децимація (min/max per pixel) для line‑plot, текстури для heatmap |
| Ліцензійна чистота | Жодного копіювання коду з GPL‑джерел; лише власні реалізації за описом алгоритму |

---

## 14. Відкриті питання

Див. `docs/OPEN-QUESTIONS.md`. Відповіді на Q1–Q5 потрібні до початку фази 0.
