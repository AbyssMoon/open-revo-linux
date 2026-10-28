#!/usr/bin/env python3
"""Рисует SVG-иконки интерфейса.

    python3 scripts/gen-icons.py   → crates/openrevo-gui/qml/img/

* mode_*.svg, tray_*.svg — режимы: шкала-«спидометр» с цветом режима и символом
  (стиль micontrol-linux); tray_* — то же на тёмном круге;
* charge_*.svg — профили заряда: батарея, заполненная до уровня профиля;
* fan.svg — вентилятор для карточки «сейчас»;
* nav_*.svg — контурные иконки меню слева (nav_X_on — выбранный пункт).

После правки пересоберите GUI: иконки встроены в ресурсы (список — в build.rs).
"""
import math
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "crates/openrevo-gui/qml/img"
OUT.mkdir(parents=True, exist_ok=True)

# --- режимы ------------------------------------------------------------------

# режим: (доля шкалы, цвет, символ)
MODES = {
    "office": (0.25, "#38bdf8", "moon"),
    "balanced": (0.5, "#34d399", "equal"),
    "turbo": (1.0, "#fbbf24", "bolt"),
}

GLYPHS = {
    # полумесяц: дуга внешней окружности (50,52 r15) и внутренней (57,46 r12)
    "moon": '<path d="M64.64 55.25 A15 15 0 1 1 49.02 37.03 A12 12 0 0 0 64.64 55.25 Z" fill="{c}"/>',
    "equal": '<g stroke="{c}" stroke-width="5" stroke-linecap="round">'
             '<line x1="39" y1="46" x2="61" y2="46"/><line x1="39" y1="58" x2="61" y2="58"/></g>',
    "bolt": '<path d="M54 34 L39 55 L49 55 L45 71 L61 48 L51 48 Z" fill="{c}" '
            'stroke="{c}" stroke-width="2" stroke-linejoin="round"/>',
}

CX, CY, R = 50, 52, 36
START, SWEEP = 135, 270


def point(deg):
    a = math.radians(deg)
    return CX + R * math.cos(a), CY + R * math.sin(a)


def arc(frac):
    x0, y0 = point(START)
    x1, y1 = point(START + SWEEP * frac)
    large = 1 if SWEEP * frac > 180 else 0
    return f"M{x0:.2f} {y0:.2f} A{R} {R} 0 {large} 1 {x1:.2f} {y1:.2f}"


def gauge(frac, color, glyph, background=False):
    ticks = []
    for i in range(7):
        a = math.radians(START + SWEEP * i / 6)
        r1, r2 = R - 9, R - 5
        ticks.append(
            f'<line x1="{CX + r1 * math.cos(a):.2f}" y1="{CY + r1 * math.sin(a):.2f}" '
            f'x2="{CX + r2 * math.cos(a):.2f}" y2="{CY + r2 * math.sin(a):.2f}"/>'
        )
    bg = '<circle cx="50" cy="50" r="49" fill="#1f1f1f"/>' if background else ""
    value = (f'<path d="{arc(frac)}" fill="none" stroke="{color}" stroke-width="7" stroke-linecap="round"/>'
             if frac > 0 else "")
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" width="100" height="100">'
        f"{bg}"
        f'<path d="{arc(1)}" fill="none" stroke="#3a3a3a" stroke-width="7" stroke-linecap="round"/>'
        f"{value}"
        f'<g stroke="#5a5a5a" stroke-width="2" stroke-linecap="round">{"".join(ticks)}</g>'
        f"{GLYPHS[glyph].format(c=color)}"
        "</svg>\n"
    )


for name, (frac, color, glyph) in MODES.items():
    (OUT / f"mode_{name}.svg").write_text(gauge(frac, color, glyph))
    (OUT / f"tray_{name}.svg").write_text(gauge(frac, color, glyph, background=True))
# Режим неизвестен / нет связи с демоном.
(OUT / "tray_unknown.svg").write_text(gauge(0, "#9ca3af", "equal", background=True))

# --- профили заряда ----------------------------------------------------------

# профиль: (доля заполнения, цвет)
CHARGE = {
    "high_capacity": (1.0, "#fbbf24"),
    "balanced": (0.8, "#34d399"),
    "stationary": (0.6, "#38bdf8"),
}


def battery(frac, color):
    # Корпус 16..80 по x, 30..70 по y; заливка внутри с отступом 6.
    x0, x1 = 22, 74
    fill = (x1 - x0) * frac
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" width="100" height="100">'
        '<rect x="16" y="30" width="64" height="40" rx="8" fill="none" stroke="#5a5a5a" stroke-width="5"/>'
        '<rect x="82" y="42" width="7" height="16" rx="3" fill="#5a5a5a"/>'
        f'<rect x="{x0}" y="36" width="{fill:.1f}" height="28" rx="4" fill="{color}"/>'
        f'<line x1="{x0 + (x1 - x0) * frac:.1f}" y1="24" x2="{x0 + (x1 - x0) * frac:.1f}" y2="76" '
        f'stroke="{color}" stroke-width="3" stroke-linecap="round" opacity="{0 if frac >= 1 else 1}"/>'
        "</svg>\n"
    )


for name, (frac, color) in CHARGE.items():
    (OUT / f"charge_{name}.svg").write_text(battery(frac, color))

# --- вентилятор --------------------------------------------------------------

blades = []
for i in range(4):
    a = 90 * i
    blades.append(
        f'<path d="M50 50 C44 38 46 24 56 22 C64 21 66 32 58 44 Z" fill="#8a8a8a" transform="rotate({a} 50 50)"/>'
    )
(OUT / "fan.svg").write_text(
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" width="100" height="100">'
    + "".join(blades)
    + '<circle cx="50" cy="50" r="7" fill="#1f1f1f" stroke="#8a8a8a" stroke-width="3"/></svg>\n'
)

# --- меню слева --------------------------------------------------------------

# Серый читается и на светлой, и на тёмной теме; выбранный — цвет выделения Fusion.
NAV_COLORS = {"": "#7b8189", "_on": "#2f7fc1"}

NAV = {
    # шкала-спидометр со стрелкой
    "perf": '<path d="M4.6 17.5 A8.5 8.5 0 1 1 19.4 17.5"/>'
            '<path d="M12 13.5 L16 9"/><circle cx="12" cy="13.5" r="1.3" fill="{c}"/>',
    # батарея с молнией
    "battery": '<rect x="2.5" y="7" width="17" height="10" rx="2.5"/>'
               '<path d="M21.5 10.5 V13.5"/>'
               '<path d="M11.8 9 L9 12.3 H12.2 L10.2 15" stroke-width="1.6"/>',
    # клавиатура
    "keys": '<rect x="2.5" y="6" width="19" height="12" rx="2.5"/>'
            '<path d="M6 9.5 H6.01 M9.3 9.5 H9.31 M12.6 9.5 H12.61 M15.9 9.5 H15.91 M18 9.5 H18.01'
            ' M7.5 12.2 H7.51 M10.8 12.2 H10.81 M14.1 12.2 H14.11 M17 12.2 H17.01 M8 15 H16" stroke-width="1.9"/>',
    # микросхема процессора
    "system": '<rect x="6" y="6" width="12" height="12" rx="2"/><rect x="9.5" y="9.5" width="5" height="5" rx="0.8"/>'
              '<path d="M9.5 3 V6 M14.5 3 V6 M9.5 18 V21 M14.5 18 V21'
              ' M3 9.5 H6 M3 14.5 H6 M18 9.5 H21 M18 14.5 H21"/>',
    "about": '<circle cx="12" cy="12" r="9"/><path d="M12 11 V16.5"/>'
             '<circle cx="12" cy="7.8" r="1.1" fill="{c}" stroke="none"/>',
}


def nav(body, color):
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" '
            f'fill="none" stroke="{color}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">'
            f'{body.format(c=color)}</svg>\n')


for name, body in NAV.items():
    for suffix, color in NAV_COLORS.items():
        (OUT / f"nav_{name}{suffix}.svg").write_text(nav(body, color))

print("готово:", OUT)
