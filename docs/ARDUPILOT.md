# ArduPilot DataFlash: як лог стає сесією OpenPID

Crate `openpid-fmt-ardupilot` читає бінарні логи ArduPilot (`.bin` з SD‑карти
або завантажені через MAVLink). Формат самоописний: пакети `FMT` задають ім'я,
довжину, назви та типи колонок кожного іншого пакета; `FMTU`/`UNIT`/`MULT`
(з 4.0) додають одиниці та множники. Реалізацію зроблено за описом формату
(`libraries/AP_Logger/LogStructure.h`), без запозичення коду.

## Часова база

Повідомлення пишуться кожне зі своєю частотою: `IMU` — сотні Гц, `RATE`/`PIDR` —
10…400 Гц, `BAT` — 10 Гц. OpenPID тримає всі канали на одній часовій базі, тому:

- базою стає найчастіше з `IMU` (екземпляр 0), `RATE`, `PIDR`;
- решта повідомлень «утримується» (sample‑and‑hold): у кожній точці бази канал
  несе останнє значення, залоговане не пізніше за цю точку, а до першого
  значення він недійсний (validity mask).

Наслідок: для step response потрібні setpoint і gyro з близькими частотами.
У логах, де `RATE`/`PIDR` пишуться в 10 Гц (стандартний `LOG_BITMASK`),
відгук буде сходинковим; для тюнінгу вмикайте швидке логування
(`LOG_BITMASK` біт `ATTITUDE_FAST`, `PID`).

## Відображення полів

| Поле | Канал | Одиниця | Примітка |
|------|-------|---------|----------|
| `IMU.GyrX/Y/Z` | `gyro.filtered.*` | deg/s | рад/с × 57.3; у `IMU` гіро вже після `INS_GYRO_FILTER` |
| `IMU.AccX/Y/Z` | `acc.*` | g | м/с² ÷ 9.80665 |
| `RATE.RDes/PDes/YDes` | `setpoint.*` | deg/s | Copter < 3.4: сантиградуси/с × 0.01 |
| `RATE.R/P/Y`, `ROut/…` | `custom:rate.*`, `custom:rate_out.*` | deg/s, — | |
| `PIDR/PIDP/PIDY.Tar` | `setpoint.*` (якщо немає `RATE`) або `custom:pid_target.*` | deg/s | Copter: рад/с × 57.3; Plane: уже deg/s |
| `PID*.Err` | `pid_error.*` | deg/s | те саме масштабування |
| `PID*.P/I/D/FF` | `pid.p/i/d/f.*` | raw | вихід контролера |
| `RCOU.C1…` | Copter: `motor.N` (%, за `MOT_PWM_MIN/MAX`); Plane: `custom:servo.N` (µs) | | |
| `RCIN.C1…` | `rc.N` | µs | |
| `CTUN.ThO` / `ThrOut` | `throttle` | % | Copter 4.x: 0…1 × 100; 3.x: 0…1000 × 0.1; Plane: уже % |
| `BAT`/`CURR` | `battery.voltage/current/consumed` | V, A, mAh | `CURR` (3.x) у сантивольтах/сантиамперах |
| `ESC[i].RPM` | `motor_rpm.i` | rpm | фізичні оберти з телеметрії ESC (не eRPM) |
| `ATT.*` | `custom:attitude.*` | deg | |
| інші колонки експортованих повідомлень | `custom:MSG.Col` | з `FMTU` | множник із `MULT`; без `FMTU` — за символом формату (`c`/`e` → ×0.01, `L` → ×1e‑7) |

Експортуються `IMU`, `RATE`, `PIDR/P/Y`, `RCOU`, `RCIN`, `CTUN`, `ATT`,
`BAT`/`CURR`, `ESC` (усі екземпляри). Решта (GPS, EKF, BARO, …) лишається
поза каналами; їх перелік і кількість — у `diagnostics.notes`. `MODE` та `EV`
стають подіями.

## Налаштування з `PARM`

| Що | Параметри |
|----|-----------|
| PID по осях | Copter: `ATC_RAT_{RLL,PIT,YAW}_{P,I,D,FF}` (3.x: `RATE_*`); Plane: `{RLL,PTCH,YAW}_RATE_*` (без yaw‑контролера — нулі) |
| Частоти | `SCHED_LOOP_RATE` → pid_hz; `INS_GYRO_RATE` → gyro_hz (1/2/4/8 кГц); log_hz — частота часової бази |
| Фільтр гіро | `INS_GYRO_FILTER` → lowpass1 (biquad 2‑го порядку) |
| D‑term | `*_FLTD` → dterm lowpass1 (PT1) |
| Harmonic notch | `INS_HNTCH_*`: режим 2/3 (RPM/ESC) → `rpm_notch`; 4 (FFT) → `dyn_notch`; 0/1 (статичний/за газом) → статичний `notch`; усі параметри — в `settings.extra` |
| Мотори | `MOT_PWM_MIN/MAX`, `MOT_PWM_TYPE`, `FRAME_CLASS` → кількість |

Усі `PARM` і рядки `MSG` доступні як raw headers (вкладка Diagnostics).

## Тестові логи

`tests/logs/ardupilot-*.bin` (MIT, джерела в `tests/logs/README.md`): Copter 3.3
(старий формат з `TimeMS`, без `FMTU`), Plane 4.3.5, QuadPlane 4.4.4 з
телеметрією ESC та harmonic notch. Перші 20 рядків ключових повідомлень
звірено з незалежним Python‑декодером (`tests/golden/*.rows.json`).

## Відомі обмеження

- Один лог = одна сесія (перезапуски логера всередині файлу не розділяються).
- Копія гіро «до фільтра» (batch sampler `ISBH/ISBD`) не читається; `gyro.raw`
  відсутній, тож Filter Sim для ArduPilot поки недоступний.
- QuadPlane у VTOL‑режимі використовує `Q_A_RAT_*`; відображення обирається за
  типом машини (Plane), тому в VTOL‑сегментах масштаб `PIDR.Tar` може бути
  в рад/с.
