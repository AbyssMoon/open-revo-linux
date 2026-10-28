//! Потребление CPU по счётчику энергии RAPL.
//!
//! На Ryzen драйвер `intel_rapl` тоже регистрирует домен `package-0`
//! (`intel-rapl:0`); счётчик читается только от root. Лимитов мощности здесь
//! нет: их ставит EC по режиму, а не powercap.

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

fn read_u64(path: &PathBuf) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Каталог домена `package-*` в powercap.
fn package_domain() -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir("/sys/class/powercap")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    dirs.sort();
    dirs.into_iter().find(|d| {
        fs::read_to_string(d.join("name"))
            .map(|n| n.trim().starts_with("package"))
            .unwrap_or(false)
    })
}

pub struct EnergyCounter {
    dir: PathBuf,
}

impl EnergyCounter {
    pub fn open() -> Option<Self> {
        let dir = package_domain()?;
        read_u64(&dir.join("energy_uj"))?;
        Some(EnergyCounter { dir })
    }

    /// Счётчик энергии пакета, мкДж.
    pub fn read(&self) -> Option<(u64, Instant)> {
        read_u64(&self.dir.join("energy_uj")).map(|e| (e, Instant::now()))
    }

    /// Средняя мощность между двумя замерами, ватты.
    pub fn average_w(&self, prev: (u64, Instant), now: (u64, Instant)) -> Option<f64> {
        let dt = now.1.duration_since(prev.1).as_secs_f64();
        if dt < 0.1 {
            return None;
        }
        let de = if now.0 >= prev.0 {
            now.0 - prev.0
        } else {
            // Переполнение счётчика.
            read_u64(&self.dir.join("max_energy_range_uj"))? - prev.0 + now.0
        };
        Some(de as f64 / 1e6 / dt)
    }
}
