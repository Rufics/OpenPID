# Blackbox: відмінності між версіями Betaflight і покриття тестами

Джерело: `src/main/blackbox/blackbox.c` та `blackbox_fielddefs.h` у тегах
4.2.11, 4.3.2, 4.4.3, 4.5.5, 2025.12.5, 2026.6.2 (порівняно автоматично), а також
парсер офіційного Blackbox Explorer (`blackbox-log-viewer/src/flightlog_parser.js`)
і його новіша TypeScript‑версія у `betaflight-configurator/src/blackbox-viewer/`.

## Бінарний формат

Кодування полів (0, 1, 3, 6, 7, 8, 9, 10) і предиктори (0–11) **не змінювалися**
від 4.2 до 2026.6; `Data version` скрізь 2. Усі відмінності — у наборі полів і
заголовків, тобто на рівні `FirmwareProfile`, а не декодера.

| Версія | Нові / змінені поля кадрів | Нові / змінені заголовки | Що це означає для OpenPID |
|--------|----------------------------|--------------------------|---------------------------|
| 4.2 | базовий набір: `axisP/I/D/F`, `rcCommand`, `setpoint`, `vbatLatest`, `amperageLatest`, `BaroAlt`, `rssi`, `gyroADC`, `accSmooth`, `debug`, `motor`, `servo`, GPS, slow | `motorOutput`, `gyro_scale`, `vbatref` (сантивольти) | base‑профіль |
| 4.3 | — | `fields_disabled_mask`, `gyro_lpf1_*`/`dterm_lpf1_*` (перейменування `*_lowpass_*`), `ff_weight`, `dyn_notch_count`, `rpm_filter_*` | профіль читає обидва варіанти імен |
| 4.4 | `BaroAlt` → `baroAlt` | `blackbox_high_resolution` (×10 для gyro, setpoint, rcCommand), `sample_rate` | масштаб ділиться на 10 при `blackbox_high_resolution:1` |
| 4.5 | `gyroUnfilt[0..2]`, `eRPM[0..7]` | `gyro_lpf1_dyn_expo`, `dterm_lpf1_dyn_expo`, `rc_smoothing_rx_smoothed`, `motor_poles`, `rc_command_scale` (configurator ділить rcCommand на нього) | `gyroUnfilt` → `Gyro{Raw}`, `eRPM` → `Erpm` та похідний `MotorRpm` через `motor_poles` |
| 4.0–4.5, `debug_mode 6` (`GYRO_SCALED`) | `debug[0..2]` = гіро до програмних фільтрів, deg/s | — | похідний `Gyro{Raw}` з `debug[0..2]`, коли `gyroUnfilt` не логується (фікстури 4.2.0, 4.5.0, 4.5.1) |
| 2025.12 | `axisS` (wing), `imuQuaternion[0..2]` (int16 ×1/32767), `GPS_velned` | версії виду `2025.12.x`; `d_max` замість `d_min`; `minthrottle` більше не пишеться (лише `motorOutput`); `DeviceUID`, `motor_kv`, `gyro_debug_axis`; у `fields_disabled_mask` з'явився біт ATTITUDE (8), ACC зсунувся | парсер версій підтримує рік.місяць; `FirmwareVersion` порівнюється лексикографічно, тому `2025.12 > 4.5` |
| 2026.6 | `GPS_time`, `GPS_home_epoch` (G/H‑кадри) | — | лише aux‑таблиці |

Події (`E`‑кадри) теж стабільні: `SYNC_BEEP=0`, `INFLIGHT_ADJUSTMENT=13`,
`LOGGING_RESUME=14`, `DISARM=15`, `FLIGHTMODE=30`, `LOG_END=255`.

## Покриття тестовими логами

Усі логи лежать у `tests/logs/` (Git LFS, ADR-0009) і мають MIT‑сумісну ліцензію;
джерела та примітки — у `tests/logs/README.md`.

| Версія | Файл | Джерело | Ліцензія |
|--------|------|---------|----------|
| 4.2.0 / 4.2.8 / 4.2.11 | `betaflight-4.2.*` | fc-blackbox, blackbox-log | MIT / Apache‑2.0 |
| 4.3.x | **немає** | публічного логу з MIT‑сумісною ліцензією не знайдено (пошук GitHub 2026‑11‑04: лише GPL‑3.0 або без ліцензії) | — |
| 4.4.x | **немає** | те саме | — |
| 4.5.0 | `betaflight-4.5.0-gps-rescue.bbl` | Iteratrix/propwash | MIT |
| 4.5.1 | `betaflight-4.5.1-two-sessions.bbl` | PedroS235/blackbird | MIT |
| 2025.12.2 | `betaflight-2025.12.2-btfl_027.bbl` | Iteratrix/propwash | MIT |
| 2026.6.x | **немає** | публічного логу не знайдено | — |

Тест `versions.rs` вимагає наявності 4.2, 4.5 і 2025.12 і повідомляє про решту.
Логи без MIT‑сумісної ліцензії (GPL, non‑commercial, без ліцензії) не додаються.

Для 2026.6 план: зібрати Betaflight SITL з тегу `2026.6.x`
(`make TARGET=SITL`, ціль має `USE_BLACKBOX_VIRTUAL`, `blackbox_device = VIRTUAL`,
`blackbox_mode = ALWAYS_ON`) і записати лог без польоту — заголовки й набір полів
будуть справжні. У середовищі розробки цієї сесії збирання стороннього коду
заборонено політикою, тому це робиться на машині власника.

## INAV

Той самий бінарний формат і ті самі кодування/предиктори, що й у Betaflight; відмінності — у наборі полів (`axisRate`, `fwAlt*`, `fwPos*`, `nav*`, `servo[]`, `attitude[]`, `accVib`) і в `S`‑кадрах (предиктор 1 для `rxUpdateRate`/`escTemperature`). Фікстури: 3.0.1 і 9.0.1 (перший MiB).

Розслідування #11 (2026‑11‑04): повний лог INAV 9.0.1 (7,8 МБ) показував 1590 «corrupt» кадрів і 211 КБ пропущених байтів. Карта втрат синхронізації (`cargo run -p openpid-fmt-blackbox --example frame_dump -- <log>`) показала: дві прогалини з нулями по ~0,5 КБ посередині (SD‑буфер) і **~150 КБ після фактичного кінця логу з байтами іншого файлу** (пари `A3 95` — це заголовки ArduPilot DataFlash, що лишилися на карті). Декодер читав правильно; «corrupt» рахувалися спроби декодувати сміття по одному байту. Тепер парсер зупиняється після 16 КіБ без жодного прийнятого кадру (`GIVE_UP_BYTES`), а хвіст записується в `diagnostics.notes` як `trailing bytes`. Обрізана фікстура цього хвоста не містить і декодується з 1 corrupt‑кадром.

### Відображення полів INAV (профіль `inav`)

| Поле | Канал | Одиниця |
|------|-------|---------|
| `axisRate[i]` | `setpoint.*` | deg/s (на 9.0.1 перевірено: `axisRate − gyroADC == pidError`) |
| `gyroADC[i]`, `accSmooth[i]` | `gyro.filtered.*`, `acc.*` | deg/s, g (`acc_1G`) |
| `axisP/I/D/F[i]`, `pidSum[i]`, `pidError[i]` | `pid.*`, `pid_error.*` | raw, deg/s |
| `vbat`, `amperage` | `battery.voltage/current` | V, A (сантиодиниці ×0.01) |
| `motor[i]` | `motor.i` | % за `motorOutput` |
| `rcCommand[i]` | `rc.i` | raw (±500, газ 1000…2000) |
| `attitude[i]` | `custom:attitude.roll/pitch/yaw` | deg (децидеградуси ×0.1) |
| `servo[i]`, `rcData[i]` | `custom:servo.i`, `custom:rc_raw.i` | µs |
| `BaroAlt`, `AirSpeed` | `custom:BaroAlt`, `custom:airspeed` | cm, cm/s |
| `navPos/navVel/navAcc/navTgtPos/navTgtVel[i]`, `navEPH/EPV`, `navSurf` | `custom:nav.*` | cm, cm/s, cm/s² |
| `navState`, `navFlags`, `navTgtHdg` | `custom:nav.state/flags/target_heading` | flags / raw |
| `fwAlt*`, `fwPos*` | `custom:fw_alt.*`, `custom:fw_pos.*` | raw (fixed‑wing контролери) |

Налаштування: `rollPID` = P,I,D[,FF]; `looptime` → gyro/pid Гц, `P interval` → частота логу; `gyro_lpf_hz`/`gyro_lpf_type`, `dterm_lpf_hz`/`dterm_lpf_type`, `dterm_lpf2_hz`, `gyro_notch_hz/cutoff`; `dynamicGyroNotchMinHz`/`dynamicGyroNotchQ` (Q ×0.01) → dyn notch; `rpm_gyro_*` → RPM notch; `motor_pwm_protocol` (ті самі коди, що в Betaflight), `motorOutput`.

## Rotorflight

Той самий контейнер, що й Betaflight, плюс події та поля гелікоптера. Розслідування #7 (2026‑11‑04): усі 11 «corrupt» кадрів фікстури 4.3.0 були **подіями `E` з кодом 50** (`FLIGHT_LOG_EVENT_GOVSTATE`, один байт стану губернатора), яких парсер не знав; після кожної губилися P‑кадри до наступного I‑кадру (82 desync, 9 розривів часу). Тепер парсер читає події Rotorflight 50/51/52 (governor / rescue / airborne state, 1 байт), 100 (custom data: довжина + байти) і 101 (custom string, `\0`‑термінований) і віддає їх як `EventKind::Other { name, data }`. Фікстура декодується без corrupt/desync і без розривів.

| Поле | Канал | Одиниця |
|------|-------|---------|
| `motor[i]` | `motor.i` | % (лог у 0,1 %: 0…1000, **не** PWM) |
| `setpoint[3]`, `mixer[3]` | `custom:collective`, `custom:mixer.collective` | % (±1000 → ±100) |
| `mixer[0..2]` | `custom:mixer.roll/pitch/yaw` | % |
| `headspeed`, `tailspeed` | `custom:headspeed`, `custom:tailspeed` | rpm |
| `attitude[i]` | `custom:attitude.*` | deg (×0.1) |
| `Vbat`, `Ibat` | `battery.voltage/current` | V, A (×0.01) |
| `rcCommand[3]` | `rc.3` — стік колективу (±500) | raw |
| `rcCommand[4]` | → похідний `throttle` (0…1000 → %) | % |

`motor_poles` у 4.3 містить адресу пам'яті (`0x2000xxxx`) замість кількості полюсів — профіль відкидає значення поза 2…64. Заголовки `gyro_rpm_filter_bank_*`, `gyro_decimation_hz`, `rates_type`, `rate_limits`, `P ratio`, `Board information` потрапляють у `settings.extra`.

