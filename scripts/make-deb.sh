#!/bin/bash
# Собирает .deb для Ubuntu 24.04+ / Debian 13+ → dist/openrevo_<версия>_amd64.deb.
# Root не нужен, нужен dpkg-deb. Ставить: sudo apt install ./dist/openrevo_*.deb
#
#   --no-build   взять уже собранную dist/openrevo-portable
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUNDLE="$ROOT/dist/openrevo-portable"
COMMON="$ROOT/packaging/common"
TD="$ROOT/third_party/tuxedo-drivers"
PKG="$ROOT/build/deb/openrevo"

[ "${1:-}" = --no-build ] || "$ROOT/scripts/make-portable.sh" --no-archive
[ -x "$BUNDLE/bin/openrevo-gui" ] || { echo "нет $BUNDLE — запустите без --no-build" >&2; exit 1; }

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
DRV_VER="$(cat "$TD/VERSION")"

rm -rf "$PKG"
install -d "$PKG/opt/openrevo" "$PKG/usr/bin" "$PKG/var/lib/openrevo"
cp -a "$BUNDLE"/{bin,lib,plugins,qml} "$PKG/opt/openrevo/"
install -Dm755 "$COMMON/dkms-setup.sh" "$PKG/opt/openrevo/libexec/dkms-setup.sh"

install -Dm755 "$COMMON/openrevo-gui" "$PKG/usr/bin/openrevo-gui"
ln -s ../../opt/openrevo/bin/openrevo "$PKG/usr/bin/openrevo"

install -Dm644 "$COMMON/openrevod.service" "$PKG/usr/lib/systemd/system/openrevod.service"
install -Dm644 "$COMMON/modules-load.conf" "$PKG/usr/lib/modules-load.d/openrevo.conf"
install -Dm644 "$TD/blacklist.conf" "$PKG/usr/lib/modprobe.d/openrevo.conf"

install -Dm644 "$COMMON/openrevo.desktop" "$PKG/usr/share/applications/openrevo.desktop"
install -Dm644 "$COMMON/openrevo-autostart.desktop" "$PKG/etc/xdg/autostart/openrevo.desktop"
install -Dm644 "$ROOT/crates/openrevo-gui/qml/img/tray_balanced.svg" \
    "$PKG/usr/share/icons/hicolor/scalable/apps/openrevo.svg"
install -Dm644 "$ROOT/LICENSE" "$PKG/usr/share/doc/openrevo/copyright"
install -Dm644 "$TD/LICENSE" "$PKG/usr/share/doc/openrevo/LICENSE.tuxedo-drivers"

S="$PKG/usr/src/openrevo-drivers-$DRV_VER"
install -d "$S"
cp -a "$TD/src/." "$S/"
sed "s/@VERSION@/$DRV_VER/" "$COMMON/dkms.conf" > "$S/dkms.conf"

install -d "$PKG/DEBIAN"
sed -e "s/@VERSION@/$VERSION/" -e "s/@SIZE@/$(du -sk "$PKG" | cut -f1)/" \
    "$ROOT/packaging/deb/control" > "$PKG/DEBIAN/control"
for f in postinst prerm postrm; do
    sed "s/@DRV_VER@/$DRV_VER/" "$ROOT/packaging/deb/$f" > "$PKG/DEBIAN/$f"
    chmod 755 "$PKG/DEBIAN/$f"
done
echo /etc/xdg/autostart/openrevo.desktop > "$PKG/DEBIAN/conffiles"

DEB="$ROOT/dist/openrevo_${VERSION}_amd64.deb"
rm -f "$ROOT"/dist/openrevo_*.deb
dpkg-deb --root-owner-group -Zxz --build "$PKG" "$DEB"
echo
echo "Готово: $DEB ($(du -h "$DEB" | cut -f1))"
echo "Установка: sudo apt install $DEB"
