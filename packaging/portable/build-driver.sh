#!/bin/bash
# Собирает драйвер под текущее ядро в driver/build (root не нужен).
# Нужны заголовки ядра (kernel-devel / linux-headers-$(uname -r)), gcc и make.
set -euo pipefail
HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
KDIR="/lib/modules/$(uname -r)/build"
OUT="$HERE/driver/build"

[ -d "$KDIR" ] || { echo "нет заголовков ядра: поставьте kernel-devel или linux-headers-$(uname -r)" >&2; exit 1; }
rm -rf "$OUT"
cp -r "$HERE/driver/src" "$OUT"
make -C "$KDIR" M="$OUT" -j"$(nproc)" modules
echo
echo "Готово: модули для ядра $(uname -r) в $OUT"
