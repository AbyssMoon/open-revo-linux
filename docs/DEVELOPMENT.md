# Разработка

Как устроен проект, как его собирать, отлаживать на железе и расширять.
Регистры и замеры — в [HARDWARE.md](HARDWARE.md), как помочь проекту — в
[CONTRIBUTING.md](../CONTRIBUTING.md).

## Архитектура

```
 ┌──────────────┐   ┌──────────────┐
 │ openrevo-gui │   │   openrevo   │   пользователь
 │  (Qt / QML)  │   │    (CLI)     │
 └──────┬───────┘   └──────┬───────┘
        │  JSON по Unix-сокету  │
        └──────────┬───────────┘
            ┌──────┴───────┐
            │  openrevod   │   root
            └──┬────────┬──┘
   /dev/tuxedo_io     /sys/devices/platform/tuxedo_keyboard
   (ioctl: режим,     (charging_profile, fn_lock)
    вентиляторы)
        │                      │
  tuxedo_io.ko ─ tuxedo_keyboard.ko ─ uniwill_wmi.ko → WMI Uniwill → EC
```

| Крейт | Что внутри |
|---|---|
| `openrevo-core` | `protocol` — режимы и профили заряда, разбор `0x0751`; `backend` — `TuxedoIo` (ioctl + sysfs) и `Mock`, запись с проверкой; `ipc` — JSON-протокол; `ec_window` — окно памяти EC (только чтение: циклы, напряжение заряда); `power` — счётчик энергии RAPL; `sysinfo` — DMI и замеры для известных плат; `acpi_events` — события ACPI (netlink). |
| `openrevod` | Демон: обслуживает сокет, хранит настройки (`state.json`), раз в 2 с считает мощность CPU, замечает выход из сна (восстанавливает настройки) и смену режима в обход демона (запоминает её). |
| `openrevo-cli` | Клиент демона и `probe` — прямое чтение через `/dev/tuxedo_io`. |
| `openrevo-gui` | QObject `Controller` (cxx-qt) + QML. Батарею, частоту и температуру читает сам из sysfs — root для этого не нужен. |

Почему демон: `/dev/tuxedo_io`, запись в sysfs драйвера и счётчик RAPL доступны только root,
а настройки нужно восстанавливать без пользователя — после загрузки (`tuxedo_io` при загрузке
всегда ставит «баланс») и после сна.

## Сборка

```sh
./scripts/setup-toolchain.sh   # Rust и Qt 6.8.3 в .toolchain/
source env.sh                  # пути к тулчейну; нужен в каждом новом терминале
cargo build --workspace
cargo test --workspace
```

`env.sh` выставляет `RUSTUP_HOME`, `CARGO_HOME`, `QMAKE`, `LD_LIBRARY_PATH` и
`RUSTFLAGS=-L .toolchain/linkfix` (там симлинк `libGL.so` — системный пакет `-devel`
ставить не нужно). Если Qt в `.toolchain/` нет, берётся системный `qmake6` — так удобно
разрабатывать, но пакеты собираются только со своим Qt 6.8.3. `CXXFLAGS=-Wno-sfinae-incomplete`
глушит предупреждение GCC 16 в заголовках Qt.

Недоступен `download.qt.io` — `QT_MIRROR=https://mirror.yandex.ru/mirrors/qt.io/
./scripts/setup-toolchain.sh` скачает с зеркала, но aqtinstall откажется ставить без
контрольных сумм, а зеркала их обычно не хранят. Тогда — системный Qt (см. README).

Иконки — SVG из `scripts/gen-icons.py`; после правки запустите его и пересоберите GUI
(иконки встроены в ресурсы, список — в `build.rs`).

## Запуск без ноутбука MECHREVO

```sh
export OPENREVO_SOCKET=$XDG_RUNTIME_DIR/openrevo-dev.sock
./target/debug/openrevod --mock --socket $OPENREVO_SOCKET --state /tmp/or-state.json &
./target/debug/openrevo status
./target/debug/openrevo-gui
```

`Mock` ведёт себя как WUJIE 15X с патченым драйвером. Транспорт (номера ioctl, sysfs)
эмуляция не проверяет — изменения в `backend.rs` проверяйте на железе.

Скриншоты для README и проверка вёрстки без окна:

```sh
QT_QPA_PLATFORM=offscreen ./target/debug/openrevo-gui --page 1 --screenshot out.png
```

`--page N` — номер вкладки (0 производительность, 1 батарея, 2 клавиатура, 3 система,
4 о программе). Сообщения «No native SystemTrayIcon» в offscreen — норма. Страницу
«Система» в README не выкладывайте: на ней серийный номер.

## Отладка на ноутбуке

| Задача | Команда | root |
|---|---|---|
| Собрать драйвер под текущее ядро | `./scripts/build-driver.sh` → `build/driver/` | нет |
| Прочитать состояние драйвера | `sudo ./target/debug/openrevo probe` | да, только чтение |
| Отладочный демон | `sudo ./scripts/dev-daemon.sh` (не закрывать) | да |
| Клиенты к нему | `OPENREVO_SOCKET=/run/openrevo-dev.sock ./target/debug/openrevo status` | нет |
| Перезапустить демон после пересборки | `touch build/dev-restart` | нет |
| Лог отладочного демона | `build/daemon.log` | нет |

Если стоит пакет, остановите его демон (`sudo systemctl stop openrevod`), чтобы два демона
не боролись за режим.

## IPC

Одна JSON-строка запроса → одна JSON-строка ответа, `\n` в конце. Сокет по умолчанию
`/run/openrevo.sock` (`$OPENREVO_SOCKET` переопределяет), права 0666 — набор команд ограничен.

```json
{"cmd": "status"}
{"cmd": "set_perf_mode", "mode": "turbo"}                 // office|balanced|turbo
{"cmd": "set_charge_profile", "profile": "stationary"}   // high_capacity|balanced|stationary = лимит 100|80|60%
{"cmd": "set_charge_limit", "percent": 70}                // свой лимит 40–100
{"cmd": "set_fan_max", "enabled": true}
{"cmd": "set_kbd_brightness", "brightness": 2}           // 0 — выключить
{"cmd": "set_kbd_color", "color": [0, 0, 255]}
{"cmd": "set_usb_powershare", "enabled": true}
{"cmd": "set_ac_auto_boot", "enabled": false}
{"cmd": "set_fn_lock", "enabled": true}
{"cmd": "reapply"}                                        // восстановить сохранённое
```

```json
{"result": "ok", "status": {
  "backend": "tuxedo_io (3 режима)", "perf_mode_raw": 0, "perf_mode_saved": "balanced",
  "profiles_available": 3, "charge_profile": "stationary",
  "charge_limit": {"percent": 60, "active": true}, "fn_lock": false,
  "fan1_pct": 25, "fan2_pct": 0, "fan_max": false,
  "kbd_backlight": {"brightness": 2, "max_brightness": 4, "color": [255, 255, 255]},
  "usb_powershare": false, "ac_auto_boot": false, "battery_cycles": 121,
  "charge_voltage": {"charge_mv": 16800, "full_mv": 17800, "cells": 4},
  "ec_cpu_temp": 43, "cpu_power_w": 3.1, "errors": [],
  "sysinfo": {"vendor": "MECHREVO", "product": "WUJIE15XA", "board": "WUJIE15-GX5HRXG", …}}}
{"result": "error", "message": "режим «turbo» не включился: EC сообщает 0x00"}
```

- `perf_mode_raw` — сырой `0x0751` (неизвестные значения не теряются).
- `errors` — ошибки чтения отдельных функций; статус при этом всё равно отдаётся.
- Новые поля `Status` помечайте `#[serde(default)]`, чтобы старые клиенты и демоны
  понимали друг друга.

## Драйвер

`third_party/tuxedo-drivers` — не форк, а выборка из релиза TUXEDO с патчами:

- `UPSTREAM` — версия tuxedo-drivers, `SHA256` — архива с GitLab;
- `VERSION` — версия пакета DKMS (`openrevo-drivers`); последняя цифра — номер ревизии
  наших патчей; меняйте её при любом изменении `src/`;
- `patches/` — патчи к исходникам TUXEDO (пути `a/src/…`);
- `src/` — результат `scripts/update-driver.sh`, руками не править.

Обновить tuxedo-drivers:

```sh
./scripts/update-driver.sh 4.26.0      # скачает, запишет SHA256 — сверьте с GitLab
echo 4.26.0.1 > third_party/tuxedo-drivers/VERSION
./scripts/build-driver.sh              # собирается ли под текущее ядро
```

CI проверяет, что патчи ложатся на исходники и что `src/` совпадает с результатом
`update-driver.sh`, и собирает драйвер под ядра Ubuntu и Fedora.

Установка в пакетах — `packaging/common/dkms-setup.sh`: собирает под все ядра с заголовками,
удаляет из DKMS старые версии и перезагружает модули (при этом `tuxedo_io` сбрасывает режим —
демон его восстанавливает).

## Как добавить функцию

1. Найдите её в tuxedo-drivers (`tuxedo_io_ioctl.h`, sysfs `tuxedo_keyboard`) и проверьте
   на железе. Запишите находку в HARDWARE.md.
2. `backend.rs` — метод трейта `Backend`, реализация в `TuxedoIo` и в `Mock`. Для записи —
   перечитайте значение после неё.
3. `ipc.rs` — поле `Status` (`#[serde(default)]`) и/или вариант `Request`.
4. `openrevod` — заполнить поле в `status()`, обработать запрос в `handle()`.
5. `openrevo-cli` — строка в `print_status` / команда, при необходимости — в `probe`.
6. `openrevo-gui` — `#[qproperty]` в `controller.rs` (имя `foo_bar` в QML станет `fooBar`;
   `#[qinvokable]` не может называться как сеттер свойства `set_*`), затем QML.
7. Тест на `Mock`; проверка на железе.

## Как добавить модель

- Режимы: плата должна попасть в проверки `GXxHRXx`-подобной платы в
  `patches/0002-…` (или в отдельный патч), затем `./scripts/update-driver.sh` и новая
  ревизия в `VERSION`.
- Замеры мощности (`SysInfo::mode_limits`) добавляйте только по реальным замерам на этой
  плате и показывайте только для неё.

## Релиз

Релиз собирает CI (`.github/workflows/release.yml`) по тегу `v*`:

1. Обновите версию в `Cargo.toml` (`workspace.package.version`), раздел в `CHANGELOG.md`
   и `%changelog` в `packaging/rpm/openrevo.spec`, закоммитьте.
2. Проверьте на ноутбуке: `./scripts/make-rpm.sh` (или `make-deb.sh`), установка пакета,
   `openrevo status`.
3. `git tag vX.Y.Z && git push origin main vX.Y.Z`.

Дальше CI сам: переносная сборка и `.deb` на Ubuntu 24.04 (glibc 2.39 — запускается и на
дистрибутивах постарше Fedora), из той же сборки RPM в контейнере `fedora:44`, релиз на GitHub
с `openrevo-portable.tar.xz`, `openrevo-X.Y.Z-1.fc44.x86_64.rpm` и `openrevo_X.Y.Z_amd64.deb` —
через `GITHUB_TOKEN`, отдельный токен не нужен. Описание релиза — раздел `## X.Y.Z` из
CHANGELOG.md. Тег, не совпадающий с версией в `Cargo.toml`, сборку останавливает.

Файлы пакетов: общие (юнит, `dkms.conf`, `dkms-setup.sh`, ярлыки, обёртка GUI) —
`packaging/common/`, спек — `packaging/rpm/`, `control` и скрипты установки — `packaging/deb/`.
Список `Depends` в `.deb` ведётся вручную: при новых зависимостях Qt проверьте `.deb` в
контейнере `ubuntu:24.04` (`apt install ./dist/openrevo_*.deb`, затем `ldd` на
`/opt/openrevo/bin/openrevo-gui`).

## Известные проблемы и долги

- `status` отдаёт серийный номер ноутбука через сокет с правами 0666 — любой локальный
  пользователь может его прочитать.
- Режимы от батареи не проверены: возможно, прошивка не даёт «Турбо» без сети.
- Если есть клавиша смены режима, демон узнаёт о ней только опросом EC (раз в 2 с).
- `run.sh` запускает GUI; выход — только через меню трея. Без трея (среда без
  StatusNotifierItem) закрытие окна завершает программу.
