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
