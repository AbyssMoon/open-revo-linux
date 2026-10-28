# source env.sh — локальные тулчейны проекта (ничего не ставится в систему)
_OR_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export RUSTUP_HOME="$_OR_ROOT/.toolchain/rustup"
export CARGO_HOME="$_OR_ROOT/.toolchain/cargo"
export QT_DIR="$(ls -d "$_OR_ROOT"/.toolchain/qt/6.*/gcc_64 2>/dev/null | head -1)"
if [ -n "$QT_DIR" ]; then
    export QMAKE="$QT_DIR/bin/qmake"
    export PATH="$CARGO_HOME/bin:$QT_DIR/bin:$PATH"
    export LD_LIBRARY_PATH="$QT_DIR/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    # libGL.so (dev-симлинк) нужен только линковщику; сама библиотека — системная.
    export RUSTFLAGS="-L native=$_OR_ROOT/.toolchain/linkfix${RUSTFLAGS:+ $RUSTFLAGS}"
else
    # Qt из системы (qt6-qtdeclarative-devel / qt6-declarative-dev) — для
    # разработки; переносная сборка и пакеты собираются только со своим Qt.
    export QMAKE="$(command -v qmake6 || command -v qmake)"
    export PATH="$CARGO_HOME/bin:$PATH"
fi
export CXXFLAGS="-Wno-sfinae-incomplete${CXXFLAGS:+ $CXXFLAGS}"
