#!/bin/bash
# Отладочный запуск на ноутбуке: sudo ./scripts/dev-daemon.sh
# Запускает target/debug/openrevod на /run/openrevo-dev.sock с настройками в
# build/dev-state, лог — build/daemon.log. Ctrl+C — остановить.
# Драйвер должен быть загружен (пакет или ./scripts/build-driver.sh + insmod).
# Пока он работает, клиенты обходятся без root:
#   OPENREVO_SOCKET=/run/openrevo-dev.sock ./target/debug/openrevo status
#   OPENREVO_SOCKET=/run/openrevo-dev.sock ./target/debug/openrevo-gui
# Перезапуск после пересборки: touch build/dev-restart
set -uo pipefail
ROOT="$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)"
SOCK=/run/openrevo-dev.sock
[ "$(id -u)" = 0 ] || { echo "нужен root: sudo $0" >&2; exit 1; }
[ -e /dev/tuxedo_io ] || { echo "нет /dev/tuxedo_io: загрузите драйвер (modprobe tuxedo_io)" >&2; exit 1; }

pkill -f "openrevod --socket $SOCK" && sleep 0.5
mkdir -p "$ROOT/build/dev-state"
PID=""
cleanup() {
    [ -n "$PID" ] && kill "$PID" 2>/dev/null && wait "$PID" 2>/dev/null
    rm -f "$SOCK"
    chown -R "${SUDO_UID:-0}:${SUDO_GID:-0}" "$ROOT/build/dev-state" "$ROOT/build/daemon.log" 2>/dev/null
}
trap cleanup EXIT

while true; do
    # Без конвейера: $! должен быть PID самого демона.
    "$ROOT/target/debug/openrevod" --socket "$SOCK" \
        --state "$ROOT/build/dev-state/state.json" >>"$ROOT/build/daemon.log" 2>&1 &
    PID=$!
    echo "демон запущен; лог $ROOT/build/daemon.log"
    while kill -0 "$PID" 2>/dev/null && [ ! -e "$ROOT/build/dev-restart" ]; do sleep 0.5; done
    kill "$PID" 2>/dev/null
    wait "$PID" 2>/dev/null
    PID=""
    if [ -e "$ROOT/build/dev-restart" ]; then
        rm -f "$ROOT/build/dev-restart"
        echo "--- перезапуск после пересборки"
        continue
    fi
    break
done
