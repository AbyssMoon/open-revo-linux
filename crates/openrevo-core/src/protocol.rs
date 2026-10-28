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
        }
    }
}
