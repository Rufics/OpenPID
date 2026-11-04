# Стиль коду OpenPID

Основа — [Google Rust Style Guide](https://google.github.io/styleguide/rustguide.html) (адаптовано; правила, що стосуються лише Chromium/Android/Fuchsia, опущено), поверх [офіційного Rust Style Guide](https://doc.rust-lang.org/style-guide/) і [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/). Коли правила суперечать, пріоритет має цей документ.

Легенда колонки «Перевірка»: **auto** — відхиляється CI автоматично (rustfmt / clippy / rustdoc / typos); **review** — на code review.

## 1. Форматування

| Правило | Перевірка |
|---------|-----------|
| Весь код форматується `rustfmt` із `rustfmt.toml` (`max_width = 100`). `#[rustfmt::skip]` — лише для ручного вирівнювання матриць/таблиць або згенерованого коду, з коментарем чому. | auto (`cargo fmt --check`) |
| Hex‑літерали — малими літерами (`0x3f80_0000`), довгі числа розділяються `_`. | auto (`clippy::unreadable_literal`, `clippy::manual_range_contains`…) |

## 2. Організація коду та імпорти

| Правило | Перевірка |
|---------|-----------|
| Малі crate‑и з однією відповідальністю; залежності між ними лише «вниз» (див. `docs/DESIGN.md` §3). | review |
| Файл і leaf‑модуль називаються за головним типом (`series.rs` → `Series`). | review |
| **Без glob‑імпортів** (`use foo::*`), окрім `use super::*` у `mod tests`. | auto (`clippy::wildcard_imports`) |
| Один `use` — одна модуль‑група; типи імпортуються і вживаються без шляху (`Series`, не `series::Series`), вільні функції викликаються з модулем (`mem::take`, не `take`). Винятки: неоднозначні імена (`io::Error`). | review (`imports_granularity = "Module"` у rustfmt — лише nightly) |
| Trait‑и, потрібні в одній функції, імпортуються всередині неї. | review |

## 3. Власність, параметри, повернення

| Правило | Перевірка |
|---------|-----------|
| Уникати `Rc`/`Arc`; використовувати, коли інакше структура володіння стає невиправдано складною (у GUI сесії лога — `Arc<FlightSession>`, бо їх ділять потоки). | review |
| `.clone()` — лише коли потрібна глибока копія, не замість позики. | review (`clippy::redundant_clone`) |
| Приймати `&T`, `&[T]`, `&str`, `Option<&T>` замість `&Box<T>`, `&Vec<T>`, `&String`, `&Option<T>`. | auto (`clippy::ptr_arg`, `clippy::borrowed_box`, `clippy::ref_option`) |
| У бібліотечних crate‑ах віддавати перевагу ітераторам / `impl IntoIterator`; у застосунку (`openpid-app`, `openpid-cli`) `Vec`/`&[T]` прийнятні, якщо спрощують код. | review |
| `enum` замість `bool` у публічних параметрах, якщо значення параметра неочевидне. | review (`clippy::fn_params_excessive_bools`) |

## 4. Помилки та паніки

| Правило | Перевірка |
|---------|-----------|
| `Option` — проста відсутність; `Result` із типом помилки (`thiserror`) — відновлювані помилки з контекстом; `panic!` — лише порушені інваріанти (баг). | review |
| **`unwrap()` заборонено** поза тестами; якщо паніка неминуча — `expect("чому це не може статися")`. | auto (`clippy::unwrap_used`; у тестах дозволено через `clippy.toml`) |
| Віддавати перевагу pattern matching (`if let`, `let … else`) над «перевірив‑і‑unwrap». | review (`clippy::unnecessary_unwrap`) |
| `assert!` з повідомленням‑контекстом, коли причина неочевидна або в повідомленні корисне значення. | review |
| Функції, що можуть панікувати на некоректному вході, документують це у секції `# Panics`. | auto (`clippy::missing_panics_doc`) |
| `todo!()`, `unimplemented!()`, `dbg!()` не потрапляють у `master`. | auto (`clippy::todo`, `clippy::unimplemented`, `clippy::dbg_macro`) |

## 5. Типи та trait‑и

| Правило | Перевірка |
|---------|-----------|
| Newtype замість `type` alias, коли потрібна типобезпека; іменовані поля замість кортежів (крім newtype). | review |
| `#[derive]` замість ручних impl стандартних trait‑ів; реалізовувати стандартні trait‑и (`Default`, `From`, `Display`…) замість власних методів з тією ж семантикою. | auto (`clippy::derivable_impls`, `clippy::should_implement_trait`) |
| Структури опцій реалізують `Default`. Конструктори: `new()`, `new_with_x()`, `try_new()` для `Result`; `from_x()` у парі з `From`. | review (`clippy::new_without_default`) |
| Публічні enum/struct з публічними полями, що будуть розширюватися (напр. `ChannelId`, `EventKind`), помічати `#[non_exhaustive]`, якщо crate використовується ззовні workspace. | review |
| `#[must_use]` на чистих функціях і типах‑результатах, якщо тип ще не `#[must_use]`. | auto (`clippy::must_use_candidate` — warn) |

## 6. Іменування

| Правило | Перевірка |
|---------|-----------|
| Конвенції Rust API Guidelines: `as_`/`to_`/`into_` відповідно до `self`; getter `foo()`, `foo_mut()`, `take_foo()`, `set_foo()`. | auto (`clippy::wrong_self_convention`) |
| Без незрозумілих абревіатур; узвичаєні (`fft`, `psd`, `pid`, `rpm`, `csv`) дозволені. | review |
| Описові імена lifetime‑ів у типах (`'data`), одно‑літерні — лише для простих зв'язків вхід→вихід. | review |
| Extension‑trait‑и мають суфікс `Ext`. | review |

## 7. Документація

| Правило | Перевірка |
|---------|-----------|
| **Кожен публічний елемент документовано** (`///`), кожен модуль має `//!` на початку файлу. Коментар каже те, чого не каже код; тривіальне повторення імені — гірше за відсутність (тоді `#[allow(missing_docs)]` з причиною). | auto (`missing_docs`) |
| Markdown; ідентифікатори у backticks; речення з великої літери та крапкою. | auto (`clippy::doc_markdown`) |
| Посилання в docs не бувають битими; `cargo doc` без попереджень. | auto (`RUSTDOCFLAGS=-D warnings`) |
| Правопис у коді, коментарях і docs. | auto (`typos`) |

## 8. Інше

| Правило | Перевірка |
|---------|-----------|
| **`unsafe` заборонено** у всіх crate‑ах (`#![deny(unsafe_code)]`). Якщо колись стане необхідним — окремий модуль, `// SAFETY:` коментар і обґрунтування в ADR. | auto (`unsafe_code = "deny"`) |
| Лише stable toolchain, без nightly‑фіч. | auto (`rust-toolchain.toml`) |
| Не виконувати код до `main()` (без `ctor`). | review |
| Достатньо явних типів, щоб інферовані були зрозумілі читачеві (особливо для `.into()`, `.parse()`, результатів ланцюжків ітераторів). | review |
| Прямий цикл замість складного ланцюжка комбінаторів, якщо так зрозуміліше. | review |
| Математика — `f64`, одиниці у типах, явні маски валідності (див. ADR‑0003). | review + reference tests |

## Як це перевіряється

- `Cargo.toml` → `[workspace.lints]`: `missing_docs`, `unsafe_code`, `clippy::unwrap_used`, `clippy::wildcard_imports`, `clippy::missing_panics_doc`, `clippy::dbg_macro`, `clippy::todo`, `clippy::unimplemented` плюс `clippy::pedantic`; всі попередження — помилки в CI (`-D warnings`).
- `clippy.toml`: `unwrap`/`expect`/`dbg`/`print` дозволені у тестах.
- Локально перед push: команди з `CONTRIBUTING.md`.
