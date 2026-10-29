//! Значения прошивки: режимы производительности и профили заряда.
//!
//! Режим живёт в регистре EC `0x0751`. tuxedo_io пишет его через
//! `uw_set_performance_profile_v1`: сбрасывает биты `0xb0` и ставит `0xa0`,
//! `0x00` или `0x10`. Профиль заряда — биты 4–5 регистра `0x07A6`, его
//! tuxedo_keyboard отдаёт в sysfs (`charging_profile`). См. docs/HARDWARE.md.

use serde::{Deserialize, Serialize};

/// Биты режима в `0x0751`; остальные (например, `0x40` — «вентиляторы на
/// максимум») к режиму не относятся.
pub const PERF_MODE_MASK: u8 = 0xb0;

/// «Вентиляторы на максимум» в `0x0751`: EC крутит оба на 200/200 и не
/// смотрит на температуру. Смена режима бит не трогает, загрузка tuxedo_io
/// обнуляет весь регистр.
pub const FAN_MAX_BIT: u8 = 0x40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerfMode {
    /// «Офис» в ControlCenter: PL1 25 Вт на WUJIE 15X.
    Office,
    /// «Баланс»: 45 Вт.
    Balanced,
    /// «Турбо»: 65 Вт.
    Turbo,
}

impl PerfMode {
    pub const ALL: [PerfMode; 3] = [PerfMode::Office, PerfMode::Balanced, PerfMode::Turbo];

    /// Аргумент `W_UW_PERF_PROF` (`enum uw_perf_profiles_v1`).
    pub fn ioctl_value(self) -> i32 {
        match self {
            PerfMode::Office => 1,
            PerfMode::Balanced => 2,
            PerfMode::Turbo => 3,
        }
    }

    /// Значение `0x0751`, которое ставит tuxedo_io (только биты режима).
    pub fn ec_bits(self) -> u8 {
        match self {
            PerfMode::Office => 0xa0,
            PerfMode::Balanced => 0x00,
            PerfMode::Turbo => 0x10,
        }
    }

    /// Режим по сырому `0x0751` (`R_UW_MODE`).
    pub fn from_raw(raw: u8) -> Option<Self> {
        let bits = raw & PERF_MODE_MASK;
        Self::ALL.into_iter().find(|m| m.ec_bits() == bits)
    }

    pub fn name(self) -> &'static str {
        match self {
            PerfMode::Office => "office",
            PerfMode::Balanced => "balanced",
            PerfMode::Turbo => "turbo",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "office" | "quiet" | "silent" => Some(PerfMode::Office),
            "balanced" | "balance" => Some(PerfMode::Balanced),
            "turbo" | "performance" => Some(PerfMode::Turbo),
            _ => None,
        }
    }
}

/// Профиль заряда в терминах tuxedo_keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChargeProfile {
    /// Полный заряд.
    #[default]
    HighCapacity,
    Balanced,
    /// Ноутбук почти всегда на зарядке.
    Stationary,
}

impl ChargeProfile {
    pub const ALL: [ChargeProfile; 3] =
        [ChargeProfile::HighCapacity, ChargeProfile::Balanced, ChargeProfile::Stationary];

    /// Строка в `charging_profile` (sysfs).
    pub fn name(self) -> &'static str {
        match self {
            ChargeProfile::HighCapacity => "high_capacity",
            ChargeProfile::Balanced => "balanced",
            ChargeProfile::Stationary => "stationary",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "high_capacity" | "full" | "off" => Some(ChargeProfile::HighCapacity),
            "balanced" => Some(ChargeProfile::Balanced),
            "stationary" => Some(ChargeProfile::Stationary),
            _ => None,
        }
    }

    /// До скольких процентов заряжать, если драйвер умеет лимит заряда: тогда
    /// профиль — это лимит, а прошивочный профиль остаётся `high_capacity`.
    pub fn limit_percent(self) -> u8 {
        match self {
            ChargeProfile::HighCapacity => CHARGE_LIMIT_MAX,
            ChargeProfile::Balanced => 80,
            ChargeProfile::Stationary => 60,
        }
    }

    /// Профиль по лимиту; `None` — лимит задан вручную (`openrevo limit`).
    pub fn from_limit(percent: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.limit_percent() == percent)
    }
}

/// Лимит заряда, % (`0x07B9`). 100 — без лимита.
pub const CHARGE_LIMIT_MIN: u8 = 40;
pub const CHARGE_LIMIT_MAX: u8 = 100;

/// Лимит заряда и соблюдает ли его EC (`0x0742` бит 2). Без обхода из
/// патча 0003 EC лимит не соблюдает, сколько ни пиши в `0x07B9`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChargeLimit {
    pub percent: u8,
    pub active: bool,
}

impl ChargeLimit {
    /// Ограничивает ли лимит заряд на самом деле.
    pub fn effective(self) -> bool {
        self.active && self.percent < CHARGE_LIMIT_MAX
    }

    /// До скольких процентов батарея зарядится на самом деле: 100, если EC
    /// лимит не соблюдает или он не задан (0 на заводе).
    pub fn effective_percent(self) -> u8 {
        if self.effective() && self.percent > 0 {
            self.percent
        } else {
            CHARGE_LIMIT_MAX
        }
    }
}

/// Подсветка клавиатуры (`/sys/class/leds/*:kbd_backlight`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KbdBacklight {
    /// 0 — выключена.
    pub brightness: u8,
    pub max_brightness: u8,
    /// Цвет, если подсветка RGB; `None` — одноцветная.
    pub color: Option<[u8; 3]>,
}

/// Готовые цвета подсветки: название и RGB.
pub const KBD_COLORS: [(&str, [u8; 3]); 8] = [
    ("white", [255, 255, 255]),
    ("red", [255, 0, 0]),
    ("orange", [255, 96, 0]),
    ("yellow", [255, 200, 0]),
    ("green", [0, 255, 0]),
    ("cyan", [0, 255, 255]),
    ("blue", [0, 0, 255]),
    ("purple", [160, 0, 255]),
];

/// Цвет по названию из [`KBD_COLORS`] или `RRGGBB` / `#RRGGBB`.
pub fn parse_kbd_color(s: &str) -> Option<[u8; 3]> {
    if let Some((_, c)) = KBD_COLORS.iter().find(|(n, _)| *n == s) {
        return Some(*c);
    }
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let ch = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([ch(0)?, ch(2)?, ch(4)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perf_mode_from_ec() {
        assert_eq!(PerfMode::from_raw(0x00), Some(PerfMode::Balanced));
        assert_eq!(PerfMode::from_raw(0xa0), Some(PerfMode::Office));
        assert_eq!(PerfMode::from_raw(0x10), Some(PerfMode::Turbo));
        // «Вентиляторы на максимум» не меняет режим.
        assert_eq!(PerfMode::from_raw(0x50), Some(PerfMode::Turbo));
        // Биты 0x80 без 0x20 tuxedo_io не ставит.
        assert_eq!(PerfMode::from_raw(0x80), None);
    }

    #[test]
    fn names_round_trip() {
        for m in PerfMode::ALL {
            assert_eq!(PerfMode::from_name(m.name()), Some(m));
        }
        for p in ChargeProfile::ALL {
            assert_eq!(ChargeProfile::from_name(p.name()), Some(p));
            assert_eq!(ChargeProfile::from_limit(p.limit_percent()), Some(p));
        }
    }

    #[test]
    fn kbd_colors() {
        assert_eq!(parse_kbd_color("red"), Some([255, 0, 0]));
        assert_eq!(parse_kbd_color("#00ff80"), Some([0, 255, 128]));
        assert_eq!(parse_kbd_color("00FF80"), Some([0, 255, 128]));
        assert_eq!(parse_kbd_color("12345"), None);
        assert_eq!(parse_kbd_color("гг0000"), None);
    }

    #[test]
    fn effective_limit() {
        // Завод: 0x07B9 = 0, EC лимит не соблюдает.
        assert_eq!(ChargeLimit { percent: 0, active: false }.effective_percent(), 100);
        assert_eq!(ChargeLimit { percent: 60, active: false }.effective_percent(), 100);
        assert_eq!(ChargeLimit { percent: 60, active: true }.effective_percent(), 60);
        assert_eq!(ChargeProfile::from_limit(70), None);
    }
}
