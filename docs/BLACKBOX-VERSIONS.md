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

| Версія | Файл | Джерело | Ліцензія джерела | Де лежить |
|--------|------|---------|------------------|-----------|
| 4.2.0 / 4.2.8 / 4.2.11 | `betaflight-4.2.*` | fc-blackbox, blackbox-log | MIT/Apache‑2.0 | `tests/logs/` (закомічено) |
| 4.3.0 | `betaflight-4.3.0-LOG00001.BFL` | gimbal-ghost | GPL‑3.0 | `tests/logs/external/` |
| 4.4.3 | `betaflight-4.4.3-btfl_001.bbl` | vlad-avr/BBLHelper | немає | external |
| 4.4.2 (SITL‑симуляція) | `betaflight-4.4.2-simulation-demo.bbl` | k-vanier/iz_pid | немає | external |
| 4.5.1 | `betaflight-4.5.1-LOG00001.BFL`, `…LOG00022.BFL` | aprskalo1/betaflight-blackbox-analyzer | немає | external |
| 4.5.2 | `betaflight-4.5.2-Run2.bbl` | jrwrodgers/bbl_decoder | немає | external |
| 2025.12.0‑RC4 | `betaflight-2025.12.0-RC4-short.bbl` | mdimitri/bbx-analyzer | non‑commercial | external |
| 2026.6.x | **немає** | публічного логу не знайдено (пошук GitHub 2026‑11‑04) | — | — |

Логи без MIT‑сумісної ліцензії не комітяться; `tools/fetch-community-logs.sh`
завантажує їх у git‑ігнорований каталог, а тест `versions.rs` перевіряє
кожну наявну версію і повідомляє, яких бракує.

Для 2026.6 план: зібрати Betaflight SITL з тегу `2026.6.x`
(`make TARGET=SITL`, ціль має `USE_BLACKBOX_VIRTUAL`, `blackbox_device = VIRTUAL`,
`blackbox_mode = ALWAYS_ON`) і записати лог без польоту — заголовки й набір полів
будуть справжні. У середовищі розробки цієї сесії збирання стороннього коду
заборонено політикою, тому це робиться на машині власника.
