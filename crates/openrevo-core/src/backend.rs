//! Доступ к прошивке: `/dev/tuxedo_io` (ioctl) и sysfs tuxedo_keyboard,
//! либо эмуляция для разработки без ноутбука.

use std::fs::{self, File, OpenOptions};
use std::os::fd::AsRawFd;

use crate::protocol::{ChargeProfile, PerfMode};

pub const TUXEDO_IO: &str = "/dev/tuxedo_io";
const PLATFORM: &str = "/sys/devices/platform/tuxedo_keyboard";

#[derive(Debug, Clone)]
pub enum Error {
    /// Нет драйвера, нет файла, ioctl не прошёл.
    Io(String),
    /// Драйвер есть, но эту функцию для модели не включил.
    Unsupported(String),
    /// Прошивка приняла запись, но значение не поменялось.
    NotApplied(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) | Error::Unsupported(e) | Error::NotApplied(e) => f.write_str(e),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub trait Backend: Send {
    fn describe(&self) -> String;
    /// Сырой `0x0751`.
    fn perf_mode_raw(&mut self) -> Result<u8>;
    fn set_perf_mode(&mut self, mode: PerfMode) -> Result<()>;
    fn charge_profile(&mut self) -> Result<ChargeProfile>;
    fn set_charge_profile(&mut self, profile: ChargeProfile) -> Result<()>;
    fn fn_lock(&mut self) -> Result<bool>;
    fn set_fn_lock(&mut self, enabled: bool) -> Result<()>;
    /// Скорость вентилятора 0 или 1 из EC, 0–200.
    fn fan_raw(&mut self, fan: u8) -> Result<u8>;
    /// Температура CPU, по которой EC крутит вентиляторы, °C.
    fn ec_cpu_temp(&mut self) -> Result<u8>;
}

// --- tuxedo_io ---------------------------------------------------------------

// tuxedo_io_ioctl.h: аргумент объявлен как `int32_t*`, поэтому в номер ioctl
// зашит размер указателя (8 байт), а не int32.
const IOCTL_MAGIC: u64 = 0xEC;
const MAGIC_READ_UW: u64 = IOCTL_MAGIC + 3;
const MAGIC_WRITE_UW: u64 = IOCTL_MAGIC + 4;
const PTR_SIZE: u64 = std::mem::size_of::<*const i32>() as u64;

const fn ior(ty: u64, nr: u64) -> u64 {
    (2 << 30) | (PTR_SIZE << 16) | (ty << 8) | nr
}
const fn iow(ty: u64, nr: u64) -> u64 {
    (1 << 30) | (PTR_SIZE << 16) | (ty << 8) | nr
}

const R_HWCHECK_UW: u64 = ior(IOCTL_MAGIC, 0x06);
const R_UW_FANSPEED: u64 = ior(MAGIC_READ_UW, 0x10);
const R_UW_FANSPEED2: u64 = ior(MAGIC_READ_UW, 0x11);
const R_UW_FAN_TEMP: u64 = ior(MAGIC_READ_UW, 0x12);
const R_UW_MODE: u64 = ior(MAGIC_READ_UW, 0x14);
const R_UW_PROFS_AVAILABLE: u64 = ior(MAGIC_READ_UW, 0x21);
const W_UW_PERF_PROF: u64 = iow(MAGIC_WRITE_UW, 0x18);

pub struct TuxedoIo {
    dev: File,
    profiles: i32,
}

impl TuxedoIo {
    pub fn open() -> Result<Self> {
        let dev = OpenOptions::new().read(true).write(true).open(TUXEDO_IO).map_err(|e| {
            Error::Io(format!(
                "нет {TUXEDO_IO} ({e}): модуль tuxedo_io не загружен или драйвер не собран под это ядро"
            ))
        })?;
        let mut b = TuxedoIo { dev, profiles: 0 };
        if b.read(R_HWCHECK_UW)? != 1 {
            return Err(Error::Io("tuxedo_io не нашёл интерфейс Uniwill: это не ноутбук на платформе Uniwill".into()));
        }
        b.profiles = b.read(R_UW_PROFS_AVAILABLE)?;
        Ok(b)
    }

    /// Сколько режимов драйвер включил для этой платы (0 — плата не в списке).
    pub fn profiles_available(&self) -> i32 {
        self.profiles
    }

    fn read(&mut self, req: u64) -> Result<i32> {
        let mut v: i32 = 0;
        // SAFETY: драйвер пишет в аргумент один int32 (copy_to_user sizeof(result)).
        let rc = unsafe { libc::ioctl(self.dev.as_raw_fd(), req as _, &mut v as *mut i32) };
        if rc < 0 {
            return Err(Error::Io(format!("ioctl {req:#x}: {}", std::io::Error::last_os_error())));
        }
        Ok(v)
    }

    fn write(&mut self, req: u64, v: i32) -> Result<()> {
        // SAFETY: драйвер читает из аргумента один int32 (copy_from_user).
        let rc = unsafe { libc::ioctl(self.dev.as_raw_fd(), req as _, &v as *const i32) };
        if rc < 0 {
            return Err(Error::Io(format!("ioctl {req:#x}: {}", std::io::Error::last_os_error())));
        }
        Ok(())
    }
}

fn sysfs_read(path: &str) -> Result<String> {
    fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .map_err(|e| Error::Io(format!("{path}: {e}")))
}

fn sysfs_write(path: &str, value: &str) -> Result<()> {
    fs::write(path, value).map_err(|e| Error::Io(format!("{path}: {e}")))
}

fn charging_profile_path() -> String {
    format!("{PLATFORM}/charging_profile/charging_profile")
}

fn fn_lock_path() -> String {
    format!("{PLATFORM}/fn_lock")
}

impl Backend for TuxedoIo {
    fn describe(&self) -> String {
        format!("tuxedo_io ({} режима)", self.profiles)
    }

    fn perf_mode_raw(&mut self) -> Result<u8> {
        Ok(self.read(R_UW_MODE)? as u8)
    }

    fn set_perf_mode(&mut self, mode: PerfMode) -> Result<()> {
        // Без своей платы в списке драйвер молча игнорирует запись.
        if self.profiles < 3 {
            return Err(Error::Unsupported(
                "драйвер не включил режимы для этой платы (нужен патч с её именем, см. docs/HARDWARE.md)".into(),
            ));
        }
        self.write(W_UW_PERF_PROF, mode.ioctl_value())
    }

    fn charge_profile(&mut self) -> Result<ChargeProfile> {
        let path = charging_profile_path();
        let s = sysfs_read(&path).map_err(|_| Error::Unsupported("драйвер не поддерживает профили заряда для этой модели".into()))?;
        ChargeProfile::from_name(&s).ok_or_else(|| Error::Io(format!("неизвестный профиль заряда «{s}»")))
    }

    fn set_charge_profile(&mut self, profile: ChargeProfile) -> Result<()> {
        sysfs_write(&charging_profile_path(), profile.name())
    }

    fn fn_lock(&mut self) -> Result<bool> {
        Ok(sysfs_read(&fn_lock_path())? == "1")
    }

    fn set_fn_lock(&mut self, enabled: bool) -> Result<()> {
        sysfs_write(&fn_lock_path(), if enabled { "1" } else { "0" })
    }

    fn fan_raw(&mut self, fan: u8) -> Result<u8> {
        let req = if fan == 0 { R_UW_FANSPEED } else { R_UW_FANSPEED2 };
        Ok(self.read(req)?.clamp(0, 255) as u8)
    }

    fn ec_cpu_temp(&mut self) -> Result<u8> {
        Ok(self.read(R_UW_FAN_TEMP)?.clamp(0, 255) as u8)
    }
}

// --- эмуляция ----------------------------------------------------------------

/// Ведёт себя как WUJIE 15X с патченым драйвером.
pub struct Mock {
    mode_raw: u8,
    charge: ChargeProfile,
    fn_lock: bool,
    tick: u32,
}

impl Default for Mock {
    fn default() -> Self {
        // tuxedo_io при загрузке ставит «баланс».
        Mock { mode_raw: 0x00, charge: ChargeProfile::HighCapacity, fn_lock: false, tick: 0 }
    }
}

impl Backend for Mock {
    fn describe(&self) -> String {
        "mock (эмуляция WUJIE 15X)".into()
    }

    fn perf_mode_raw(&mut self) -> Result<u8> {
        Ok(self.mode_raw)
    }

    fn set_perf_mode(&mut self, mode: PerfMode) -> Result<()> {
        self.mode_raw = (self.mode_raw & !crate::protocol::PERF_MODE_MASK) | mode.ec_bits();
        Ok(())
    }

    fn charge_profile(&mut self) -> Result<ChargeProfile> {
        Ok(self.charge)
    }

    fn set_charge_profile(&mut self, profile: ChargeProfile) -> Result<()> {
        self.charge = profile;
        Ok(())
    }

    fn fn_lock(&mut self) -> Result<bool> {
        Ok(self.fn_lock)
    }

    fn set_fn_lock(&mut self, enabled: bool) -> Result<()> {
        self.fn_lock = enabled;
        Ok(())
    }

    fn fan_raw(&mut self, fan: u8) -> Result<u8> {
        self.tick = self.tick.wrapping_add(1);
        let base = match PerfMode::from_raw(self.mode_raw) {
            Some(PerfMode::Office) => 40,
            Some(PerfMode::Turbo) => 140,
            _ => 80,
        };
        Ok(base + (self.tick % 10) as u8 + fan * 5)
    }

    fn ec_cpu_temp(&mut self) -> Result<u8> {
        Ok(48)
    }
}

/// Записать режим и убедиться, что EC его принял.
pub fn apply_perf_mode(b: &mut dyn Backend, mode: PerfMode) -> Result<()> {
    b.set_perf_mode(mode)?;
    let raw = b.perf_mode_raw()?;
    if PerfMode::from_raw(raw) != Some(mode) {
        return Err(Error::NotApplied(format!(
            "режим «{}» не включился: EC сообщает 0x{raw:02x}",
            mode.name()
        )));
    }
    Ok(())
}

pub fn apply_charge_profile(b: &mut dyn Backend, profile: ChargeProfile) -> Result<()> {
    b.set_charge_profile(profile)?;
    let now = b.charge_profile()?;
    if now != profile {
        return Err(Error::NotApplied(format!(
            "профиль заряда «{}» не включился: стоит «{}»",
            profile.name(),
            now.name()
        )));
    }
    Ok(())
}

/// Скорость вентилятора в процентах (EC считает от 0 до 200).
pub fn fan_percent(raw: u8) -> u8 {
    ((raw as u32 * 100 + 100) / 200).min(100) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers_match_python_probe() {
        // Сверено с вызовами из Python на ноутбуке (см. docs/HARDWARE.md).
        assert_eq!(R_UW_MODE, 0x8008_ef14);
        assert_eq!(W_UW_PERF_PROF, 0x4008_f018);
    }

    #[test]
    fn mock_round_trip() {
        let mut m = Mock::default();
        for mode in PerfMode::ALL {
            apply_perf_mode(&mut m, mode).unwrap();
        }
        apply_charge_profile(&mut m, ChargeProfile::Stationary).unwrap();
        assert_eq!(m.charge_profile().unwrap(), ChargeProfile::Stationary);
    }

    #[test]
    fn fan_percent_range() {
        assert_eq!(fan_percent(0), 0);
        assert_eq!(fan_percent(100), 50);
        assert_eq!(fan_percent(200), 100);
        assert_eq!(fan_percent(255), 100);
    }
}
