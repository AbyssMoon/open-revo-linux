#!/bin/bash
# Обновляет third_party/tuxedo-drivers/src из исходников TUXEDO:
# скачивает архив версии UPSTREAM, сверяет SHA256, накладывает patches/*.patch
# и оставляет только модули, которые нужны Open Revo.
#
#   ./scripts/update-driver.sh            — пересобрать src из текущих UPSTREAM/SHA256
#   ./scripts/update-driver.sh 4.26.0     — перейти на новую версию (SHA256 запишется
#                                           сам — сверьте его с GitLab вручную)
#
# После обновления поменяйте VERSION (версия модуля в DKMS) и проверьте сборку:
#   ./scripts/build-driver.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TD="$ROOT/third_party/tuxedo-drivers"
UPSTREAM="${1:-$(cat "$TD/UPSTREAM")}"
URL="https://gitlab.com/tuxedocomputers/development/packages/tuxedo-drivers/-/archive/v$UPSTREAM/tuxedo-drivers-v$UPSTREAM.tar.gz"
WORK="$ROOT/build/driver-update"

rm -rf "$WORK"
mkdir -p "$WORK"
curl -sSfL -o "$WORK/src.tar.gz" "$URL"

if [ $# -eq 0 ]; then
    echo "$(cat "$TD/SHA256")  $WORK/src.tar.gz" | sha256sum -c --quiet
else
    echo "$UPSTREAM" > "$TD/UPSTREAM"
    sha256sum "$WORK/src.tar.gz" | cut -d' ' -f1 > "$TD/SHA256"
    echo "новая версия $UPSTREAM, SHA256 $(cat "$TD/SHA256")"
fi

tar -C "$WORK" -xzf "$WORK/src.tar.gz"
UP="$WORK/tuxedo-drivers-v$UPSTREAM"
for p in "$TD"/patches/*.patch; do
    patch -d "$UP" -Np1 --no-backup-if-mismatch -i "$p"
done

rm -rf "$TD/src"
mkdir -p "$TD/src"
S="$UP/src"
cp "$S"/clevo_acpi.c "$S"/clevo_wmi.c "$S"/clevo_interfaces.h "$S"/clevo_keyboard.h "$S"/clevo_leds.h \
   "$S"/tuxedo_keyboard.c "$S"/tuxedo_keyboard_common.h \
   "$S"/uniwill_wmi.c "$S"/uniwill_interfaces.h "$S"/uniwill_keyboard.h "$S"/uniwill_leds.h \
   "$TD/src/"
cp -r "$S"/tuxedo_io "$S"/tuxedo_compatibility_check "$TD/src/"
# Только наши модули: остальные (nb04, nb05, ite…) уже есть в ядре или не нужны,
# а копии из DKMS подменили бы модули ядра.
cat > "$TD/src/Kbuild" <<'EOF'
obj-m += clevo_acpi.o
obj-m += clevo_wmi.o
obj-m += tuxedo_keyboard.o
obj-m += uniwill_wmi.o

obj-y += tuxedo_compatibility_check/
obj-y += tuxedo_io/
EOF
cp "$UP/LICENSE" "$TD/LICENSE"
cp "$UP/files/usr/lib/modprobe.d/tuxedo-drivers-backlist-upstream-conflicts.conf" "$TD/blacklist.conf"

echo "готово: $TD/src (tuxedo-drivers $UPSTREAM + $(ls "$TD"/patches/*.patch | wc -l) патча)"
