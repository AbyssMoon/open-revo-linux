#!/bin/bash
# Собирает переносную папку dist/openrevo-portable: скопировал на ноутбук,
# запустил ./run.sh — в системе ничего не устанавливается.
#
#   --no-archive   не упаковывать в tar.xz (так делает scripts/make-rpm.sh)
set -euo pipefail

ARCHIVE=1
[ "${1:-}" = --no-archive ] && ARCHIVE=0

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=../env.sh
source "$ROOT/env.sh"

OUT="$ROOT/dist/openrevo-portable"
if [ -n "$QT_DIR" ]; then
    QT_LIB="$QT_DIR/lib" QT_QML="$QT_DIR/qml" QT_PLUGINS="$QT_DIR/plugins"
    # Из своего Qt берём всё, что лежит в его lib/ (в том числе ICU).
    LIB_RE='.'
else
    # Системный Qt: такая сборка работает только на том же дистрибутиве
    # (годится для проверки пакета на своём ноутбуке). Копируем только сам Qt.
    QT_LIB="$($QMAKE -query QT_INSTALL_LIBS)"
    QT_QML="$($QMAKE -query QT_INSTALL_QML)"
    QT_PLUGINS="$($QMAKE -query QT_INSTALL_PLUGINS)"
    LIB_RE='^libQt6'
    echo "внимание: Qt из системы ($QT_LIB) — сборка только для этого дистрибутива" >&2
fi

cargo build --release --workspace --manifest-path "$ROOT/Cargo.toml"

rm -rf "$OUT"
mkdir -p "$OUT"/{bin,lib,plugins,qml,state}

cp "$ROOT"/target/release/{openrevod,openrevo,openrevo-gui} "$OUT/bin/"

# QML-модули, которые использует интерфейс.
cp -a "$QT_QML/QtQuick" "$QT_QML/QtQml" "$OUT/qml/"
rm -rf "$OUT/qml/QtQuick/"{Particles,Dialogs,LocalStorage,Effects,VectorImage,Shapes,tooling}
# Нужен только стиль Fusion (он опирается на Basic и impl).
rm -rf "$OUT/qml/QtQuick/Controls/"{Imagine,Material,Universal,FluentWinUI3,designer}
# Значок в трее и его меню (Qt.labs.platform).
mkdir -p "$OUT/qml/Qt/labs"
cp -a "$QT_QML/Qt/labs/platform" "$OUT/qml/Qt/labs/"

# Плагины: X11 и Wayland, картинки, методы ввода.
for d in platforms wayland-decoration-client wayland-graphics-integration-client \
         wayland-shell-integration xcbglintegrations imageformats iconengines \
         platforminputcontexts; do
    [ -d "$QT_PLUGINS/$d" ] && cp -a "$QT_PLUGINS/$d" "$OUT/plugins/"
done
# Платформы без графической оболочки (eglfs, linuxfb, vnc) не нужны.
rm -f "$OUT"/plugins/platforms/lib{qeglfs,qlinuxfb,qvnc,qminimalegl,qvkkhrdisplay}.so
mkdir -p "$OUT/plugins/platformthemes"
cp "$QT_PLUGINS"/platformthemes/libqxdgdesktopportal.so "$OUT/plugins/platformthemes/" 2>/dev/null || true

# Библиотеки Qt: замыкание зависимостей всех бинарников и плагинов,
# берём только то, что лежит в каталоге Qt (системные библиотеки не копируем).
mapfile -t objs < <(find "$OUT/bin/openrevo-gui" "$OUT/plugins" "$OUT/qml" -name '*.so*' -o -name openrevo-gui)
declare -A seen=()
queue=("${objs[@]}")
while [ ${#queue[@]} -gt 0 ]; do
    f="${queue[0]}"
    queue=("${queue[@]:1}")
    while read -r lib; do
        name="$(basename "$lib")"
        [ -n "${seen[$name]:-}" ] && continue
        [[ "$name" =~ $LIB_RE ]] || continue
        seen[$name]=1
        cp -L "$lib" "$OUT/lib/$name"
        queue+=("$QT_LIB/$name")
    done < <(LD_LIBRARY_PATH="$QT_LIB" ldd "$f" 2>/dev/null | awk -v q="$QT_LIB/" 'index($3, q) == 1 { print $3 }')
done

# Исходники драйвера — собрать под ядро ноутбука (./build-driver.sh).
mkdir -p "$OUT/driver"
cp -r "$ROOT"/third_party/tuxedo-drivers/src "$OUT/driver/"
cp "$ROOT"/third_party/tuxedo-drivers/{LICENSE,VERSION} "$OUT/driver/"

cp "$ROOT"/packaging/portable/{run.sh,build-driver.sh,README.txt} "$OUT/"
chmod +x "$OUT"/{run.sh,build-driver.sh}

echo
echo "Готово: $OUT ($(du -sh "$OUT" | cut -f1))"
echo "Максимальная требуемая версия glibc:"
objdump -T "$OUT"/bin/* "$OUT"/lib/* 2>/dev/null | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1

[ "$ARCHIVE" = 1 ] || exit 0

# Архив для релиза.
rm -f "$ROOT"/dist/openrevo-portable.tar.{gz,xz}
tar -C "$ROOT/dist" -cf - openrevo-portable | xz -9e -T0 > "$ROOT/dist/openrevo-portable.tar.xz"
echo "Архив: $ROOT/dist/openrevo-portable.tar.xz ($(du -h "$ROOT/dist/openrevo-portable.tar.xz" | cut -f1))"
