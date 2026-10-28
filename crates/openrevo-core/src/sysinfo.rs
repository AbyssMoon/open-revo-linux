//! Сведения о системе из DMI.

use std::fs;

use serde::{Deserialize, Serialize};

use crate::protocol::PerfMode;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SysInfo {
    pub vendor: String,
    pub product: String,
    pub board: String,
    pub sku: String,
    pub bios_version: String,
    pub bios_date: String,
    pub ec_version: String,
    /// Читается только от root, поэтому заполняется демоном.
    pub serial: String,
}

/// Лимит мощности CPU в режиме, замеренный от сети: (длительный, кратковременный), Вт.
pub type ModeLimits = (u32, u32);

fn dmi(name: &str) -> String {
    fs::read_to_string(format!("/sys/class/dmi/id/{name}"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Плата MECHREVO WUJIE 15X (Ryzen 7 8845HS) — та же платформа, что TUXEDO
/// InfinityBook Pro Gen9 AMD (`GXxHRXx`).
pub const BOARD_WUJIE15X: &str = "WUJIE15-GX5HRXG";

impl SysInfo {
    pub fn read() -> Self {
        Self {
            vendor: dmi("sys_vendor"),
            product: dmi("product_name"),
            board: dmi("board_name"),
            sku: dmi("product_sku"),
            bios_version: dmi("bios_version"),
            bios_date: dmi("bios_date"),
            ec_version: dmi("ec_firmware_release"),
            serial: dmi("product_serial"),
        }
    }

    pub fn is_mechrevo(&self) -> bool {
        self.vendor.to_lowercase().contains("mechrevo")
    }

    pub fn is_wujie15x(&self) -> bool {
        self.board == BOARD_WUJIE15X
    }

    /// Замеры stress-ng на WUJIE 15X от сети (docs/HARDWARE.md). Для других
    /// моделей не показываем: таблицы мощности у плат разные.
    pub fn mode_limits(&self, mode: PerfMode) -> Option<ModeLimits> {
        if !self.is_wujie15x() {
            return None;
        }
        Some(match mode {
            PerfMode::Office => (25, 30),
            PerfMode::Balanced => (45, 50),
            PerfMode::Turbo => (65, 70),
        })
    }
}
