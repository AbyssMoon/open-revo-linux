#!/bin/bash
# Собирает модули драйвера под текущее ядро в build/driver (root не нужен):
# проверка, что патченый tuxedo-drivers собирается. Нужны заголовки ядра
# (kernel-devel / linux-headers-$(uname -r)), gcc и make.
#
#   ./scripts/build-driver.sh [ВЕРСИЯ_ЯДРА]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
KVER="${1:-$(uname -r)}"
KDIR="/lib/modules/$KVER/build"
# В контейнере без самого ядра есть только kernel-devel.
[ -d "$KDIR" ] || KDIR="/usr/src/kernels/$KVER"
OUT="$ROOT/build/driver"

[ -d "$KDIR" ] || { echo "нет заголовков ядра $KVER ($KDIR)" >&2; exit 1; }
rm -rf "$OUT"
mkdir -p "$OUT"
cp -r "$ROOT"/third_party/tuxedo-drivers/src/. "$OUT/"
make -C "$KDIR" M="$OUT" -j"$(nproc)" modules
echo
echo "Готово:"
find "$OUT" -name '*.ko' -printf '  %P\n' | sort
