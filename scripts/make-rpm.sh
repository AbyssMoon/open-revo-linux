#!/bin/bash
# Собирает RPM для Fedora → dist/openrevo-<версия>-1.<dist>.x86_64.rpm.
# Root не нужен; ставить: sudo dnf install dist/openrevo-*.rpm
#
#   --no-build   взять уже собранную dist/openrevo-portable
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUNDLE="$ROOT/dist/openrevo-portable"
TOP="$ROOT/build/rpm"

[ "${1:-}" = --no-build ] || "$ROOT/scripts/make-portable.sh" --no-archive
[ -x "$BUNDLE/bin/openrevo-gui" ] || { echo "нет $BUNDLE — запустите без --no-build" >&2; exit 1; }

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
DRV_VER="$(cat "$ROOT/third_party/tuxedo-drivers/VERSION")"

rm -rf "$TOP"
rpmbuild -bb "$ROOT/packaging/rpm/openrevo.spec" \
    --define "_topdir $TOP" \
    --define "or_version $VERSION" \
    --define "drv_version $DRV_VER" \
    --define "bundle $BUNDLE" \
    --define "pkgsrc $ROOT/packaging/common" \
    --define "drvsrc $ROOT/third_party/tuxedo-drivers" \
    --define "icon $ROOT/crates/openrevo-gui/qml/img/tray_balanced.svg" \
    --define "licfile $ROOT/LICENSE"

rm -f "$ROOT"/dist/openrevo-*.rpm
cp "$TOP"/RPMS/x86_64/openrevo-*.rpm "$ROOT/dist/"
RPM="$(ls "$ROOT"/dist/openrevo-*.rpm)"
echo
echo "Готово: $RPM ($(du -h "$RPM" | cut -f1))"
echo "Установка: sudo dnf install $RPM"
