#!/bin/bash
# Запуск Open Revo прямо из этой папки, без установки в систему.
#
#   ./run.sh           — настоящий режим: демон от root (sudo) + интерфейс
#   ./run.sh --mock    — эмуляция прошивки, без root (проверить интерфейс)
#   ./run.sh probe     — только прочитать состояние драйвера (sudo, без записи)
#
# После закрытия окна демон останавливается, а модули драйвера выгружаются,
# если их загружал этот скрипт.
set -uo pipefail

HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
SOCK=/run/openrevo-test.sock
LOG="$HERE/state/daemon.log"
mkdir -p "$HERE/state"

export LD_LIBRARY_PATH="$HERE/lib"
export QT_PLUGIN_PATH="$HERE/plugins"
export QML_IMPORT_PATH="$HERE/qml"
export QML2_IMPORT_PATH="$HERE/qml"

# Порядок загрузки; выгружаются в обратном.
MODULES=(tuxedo_compatibility_check tuxedo_keyboard uniwill_wmi tuxedo_io)
LOADED_MODULES=""
DPID=""

cleanup() {
    trap - EXIT HUP INT TERM
    if [ -n "$DPID" ]; then
        # Сигнал процессу sudo до демона не доходит — останавливаем сам демон.
        sudo pkill -TERM -f -- "bin/openrevod.* --socket $SOCK" 2>/dev/null
        wait "$DPID" 2>/dev/null
        sudo rm -f "$SOCK"
    fi
    if [ "$LOADED_MODULES" = 1 ]; then
        for ((i = ${#MODULES[@]} - 1; i >= 0; i--)); do
            sudo rmmod "${MODULES[$i]}" 2>/dev/null
        done
        echo "Модули драйвера выгружены."
    fi
}

ensure_driver() {
    [ -e /dev/tuxedo_io ] && return 0
    # Драйвер уже есть в системе (пакет) — просто загрузить.
    if modinfo tuxedo_io >/dev/null 2>&1; then
        sudo modprobe -a tuxedo_keyboard uniwill_wmi tuxedo_io && sleep 1 && return 0
    fi
    # Модули, собранные в этой папке (./build-driver.sh).
    local b="$HERE/driver/build"
    if [ -f "$b/tuxedo_io/tuxedo_io.ko" ]; then
        sudo modprobe -a led_class_multicolor sparse_keymap 2>/dev/null
        sudo insmod "$b/tuxedo_compatibility_check/tuxedo_compatibility_check.ko" &&
            sudo insmod "$b/tuxedo_keyboard.ko" &&
            sudo insmod "$b/uniwill_wmi.ko" &&
            sudo insmod "$b/tuxedo_io/tuxedo_io.ko" &&
            LOADED_MODULES=1 && sleep 1 && return 0
    fi
    echo "Нет /dev/tuxedo_io: драйвер не найден." >&2
    echo "Соберите его в этой папке: ./build-driver.sh (см. README.txt)" >&2
    echo "или проверьте интерфейс в эмуляции: ./run.sh --mock" >&2
    return 1
}

start_daemon() {
    # $@ — команда демона; стартует в фоне.
    "$@" --socket "$SOCK" --state "$HERE/state/state.json" </dev/null >"$LOG" 2>&1 &
    DPID=$!
    for _ in $(seq 50); do
        [ -S "$SOCK" ] && return 0
        if [ ! -d "/proc/$DPID" ]; then
            echo "Демон завершился с ошибкой:" >&2
            cat "$LOG" >&2
            DPID=""
            return 1
        fi
        sleep 0.1
    done
    echo "Демон не открыл сокет за 5 секунд, лог: $LOG" >&2
    return 1
}

MODE="${1:-}"
[ $# -gt 0 ] && shift

case "$MODE" in
    --mock)
        SOCK="${XDG_RUNTIME_DIR:-/tmp}/openrevo-mock.sock"
        trap cleanup EXIT HUP INT TERM
        # В эмуляции root не нужен — подменяем sudo на прямой запуск.
        sudo() { "$@"; }
        start_daemon "$HERE/bin/openrevod" --mock || exit 1
        ;;
    probe)
        sudo -v || exit 1
        trap cleanup EXIT HUP INT TERM
        ensure_driver || exit 1
        sudo "$HERE/bin/openrevo" probe
        exit
        ;;
    "")
        echo "Нужны права root для демона."
        sudo -v || exit 1
        trap cleanup EXIT HUP INT TERM
        ensure_driver || exit 1
        start_daemon sudo "$HERE/bin/openrevod" || exit 1
        echo "Демон запущен, лог: $LOG"
        ;;
    *)
        sed -n '2,9p' "$0"
        exit 2
        ;;
esac

# Остальные аргументы передаются интерфейсу (например, --page N).
OPENREVO_SOCKET="$SOCK" "$HERE/bin/openrevo-gui" "$@"
