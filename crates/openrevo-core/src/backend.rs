//! Доступ к прошивке: `/dev/tuxedo_io` (ioctl) и sysfs tuxedo_keyboard,
//! либо эмуляция для разработки без ноутбука.

use std::fs::{self, File, OpenOptions};
use std::os::fd::AsRawFd;

use crate::protocol::{ChargeLimit, ChargeProfile, KbdBacklight, PerfMode, CHARGE_LIMIT_MAX, CHARGE_LIMIT_MIN, FAN_MAX_BIT};

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
    fn charge_limit(&mut self) -> Result<ChargeLimit>;
    /// Записать `0x07B9`; соблюдать его EC начнёт только после
    /// [`Backend::unlock_charge_limit`].
    fn set_charge_limit(&mut self, percent: u8) -> Result<()>;
    /// Обход заводской ошибки MECHREVO (патч 0003): EC включает лимит.
    /// Занимает около 2 секунд.
    fn unlock_charge_limit(&mut self) -> Result<()>;
    /// «Вентиляторы на максимум» (бит `0x40` в `0x0751`).
    fn fan_max(&mut self) -> Result<bool>;
    fn set_fan_max(&mut self, enabled: bool) -> Result<()>;
    fn kbd_backlight(&mut self) -> Result<KbdBacklight>;
    fn set_kbd_brightness(&mut self, brightness: u8) -> Result<()>;
    fn set_kbd_color(&mut self, color: [u8; 3]) -> Result<()>;
    /// Питание USB у выключенного ноутбука.
    fn usb_powershare(&mut self) -> Result<bool>;
    fn set_usb_powershare(&mut self, enabled: bool) -> Result<()>;
    /// Включаться при подключении зарядки.
    fn ac_auto_boot(&mut self) -> Result<bool>;
    fn set_ac_auto_boot(&mut self, enabled: bool) -> Result<()>;
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
const W_UW_MODE: u64 = iow(MAGIC_WRITE_UW, 0x12);
const W_UW_PERF_PROF: u64 = iow(MAGIC_WRITE_UW, 0x18);
// Патч 0003.
const R_UW_CHARGE_LIMIT: u64 = ior(MAGIC_READ_UW, 0x40);
const R_UW_CHARGE_LIMIT_ACTIVE: u64 = ior(MAGIC_READ_UW, 0x41);
const W_UW_CHARGE_LIMIT: u64 = iow(MAGIC_WRITE_UW, 0x40);
// _IO: без аргумента, размер в номере 0.
const W_UW_CHARGE_LIMIT_UNLOCK: u64 = (MAGIC_WRITE_UW << 8) | 0x41;

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
            return Err(ioctl_error(req));
        }
        Ok(v)
    }

    fn write(&mut self, req: u64, v: i32) -> Result<()> {
        // SAFETY: драйвер читает из аргумента один int32 (copy_from_user).
        let rc = unsafe { libc::ioctl(self.dev.as_raw_fd(), req as _, &v as *const i32) };
        if rc < 0 {
            return Err(ioctl_error(req));
        }
        Ok(())
    }
}

fn ioctl_error(req: u64) -> Error {
    let e = std::io::Error::last_os_error();
    match e.raw_os_error() {
        // Драйвер без патча 0003 (ENOTTY) или плата не в его списке (EOPNOTSUPP).
        Some(libc::ENOTTY) | Some(libc::EOPNOTSUPP) => {
            Error::Unsupported(format!("ioctl {req:#x}: драйвер не поддерживает это для этой модели ({e})"))
        }
        _ => Error::Io(format!("ioctl {req:#x}: {e}")),
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

/// Подсветка клавиатуры: `rgb:kbd_backlight` или `white:kbd_backlight`.
fn kbd_led_dir() -> Result<String> {
    let none = || Error::Unsupported("нет подсветки клавиатуры в /sys/class/leds".into());
    let mut dirs: Vec<String> = fs::read_dir("/sys/class/leds")
        .map_err(|_| none())?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(":kbd_backlight"))
        .collect();
    dirs.sort();
    dirs.first().map(|n| format!("/sys/class/leds/{n}")).ok_or_else(none)
}

fn sysfs_bool(path: &str) -> Result<bool> {
    let s = sysfs_read(path).map_err(|_| Error::Unsupported(format!("нет {path}")))?;
    Ok(s == "1")
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

    fn charge_limit(&mut self) -> Result<ChargeLimit> {
        Ok(ChargeLimit {
            percent: self.read(R_UW_CHARGE_LIMIT)?.clamp(0, 127) as u8,
            active: self.read(R_UW_CHARGE_LIMIT_ACTIVE)? != 0,
        })
    }

    fn set_charge_limit(&mut self, percent: u8) -> Result<()> {
        self.write(W_UW_CHARGE_LIMIT, percent as i32)
    }

    fn unlock_charge_limit(&mut self) -> Result<()> {
        // SAFETY: _IO без аргумента.
        let rc = unsafe { libc::ioctl(self.dev.as_raw_fd(), W_UW_CHARGE_LIMIT_UNLOCK as _) };
        if rc < 0 {
            return Err(ioctl_error(W_UW_CHARGE_LIMIT_UNLOCK));
        }
        Ok(())
    }

    fn fan_max(&mut self) -> Result<bool> {
        Ok(self.perf_mode_raw()? & FAN_MAX_BIT != 0)
    }

    fn kbd_backlight(&mut self) -> Result<KbdBacklight> {
        let dir = kbd_led_dir()?;
        let num = |f: &str| -> Result<u8> {
            let s = sysfs_read(&format!("{dir}/{f}"))?;
            s.parse().map_err(|_| Error::Io(format!("{dir}/{f}: «{s}»")))
        };
        // multi_intensity есть только у RGB: «R G B».
        let color = sysfs_read(&format!("{dir}/multi_intensity")).ok().and_then(|s| {
            let v: Vec<u8> = s.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            <[u8; 3]>::try_from(v).ok()
        });
        Ok(KbdBacklight { brightness: num("brightness")?, max_brightness: num("max_brightness")?, color })
    }

    fn set_kbd_brightness(&mut self, brightness: u8) -> Result<()> {
        sysfs_write(&format!("{}/brightness", kbd_led_dir()?), &brightness.to_string())
    }

    fn set_kbd_color(&mut self, [r, g, b]: [u8; 3]) -> Result<()> {
        let path = format!("{}/multi_intensity", kbd_led_dir()?);
        if fs::metadata(&path).is_err() {
            return Err(Error::Unsupported("подсветка одноцветная".into()));
        }
        // led-class-multicolor сразу применяет цвет с текущей яркостью.
        sysfs_write(&path, &format!("{r} {g} {b}"))
    }

    fn usb_powershare(&mut self) -> Result<bool> {
        sysfs_bool(&format!("{PLATFORM}/usb_powershare/usb_powershare"))
    }

    fn set_usb_powershare(&mut self, enabled: bool) -> Result<()> {
        sysfs_write(&format!("{PLATFORM}/usb_powershare/usb_powershare"), if enabled { "1" } else { "0" })
    }

    fn ac_auto_boot(&mut self) -> Result<bool> {
        sysfs_bool(&format!("{PLATFORM}/ac_auto_boot/ac_auto_boot"))
    }

    fn set_ac_auto_boot(&mut self, enabled: bool) -> Result<()> {
        sysfs_write(&format!("{PLATFORM}/ac_auto_boot/ac_auto_boot"), if enabled { "1" } else { "0" })
    }

    fn set_fan_max(&mut self, enabled: bool) -> Result<()> {
        // W_UW_MODE пишет весь 0x0751: биты режима берём текущие.
        let raw = self.perf_mode_raw()?;
        let next = if enabled { raw | FAN_MAX_BIT } else { raw & !FAN_MAX_BIT };
        self.write(W_UW_MODE, next as i32)
    }
}

// --- эмуляция ----------------------------------------------------------------

/// Ведёт себя как WUJIE 15X с патченым драйвером.
pub struct Mock {
    mode_raw: u8,
    charge: ChargeProfile,
    fn_lock: bool,
    /// Как на заводе: 0x07B9 пишется, но EC его не соблюдает.
    limit: ChargeLimit,
    kbd: KbdBacklight,
    usb_powershare: bool,
    ac_auto_boot: bool,
    tick: u32,
}

impl Default for Mock {
    fn default() -> Self {
        // tuxedo_io при загрузке ставит «баланс».
        Mock {
            mode_raw: 0x00,
            charge: ChargeProfile::HighCapacity,
            fn_lock: false,
            limit: ChargeLimit { percent: 100, active: false },
            // tuxedo_keyboard при загрузке гасит подсветку.
            kbd: KbdBacklight { brightness: 0, max_brightness: 4, color: Some([255, 255, 255]) },
            usb_powershare: false,
            ac_auto_boot: false,
            tick: 0,
        }
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
        if self.mode_raw & FAN_MAX_BIT != 0 {
            return Ok(200);
        }
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

    fn charge_limit(&mut self) -> Result<ChargeLimit> {
        Ok(self.limit)
    }

    fn set_charge_limit(&mut self, percent: u8) -> Result<()> {
        if !(1..=100).contains(&percent) {
            return Err(Error::Io("ioctl: Invalid argument".into()));
        }
        self.limit.percent = percent;
        Ok(())
    }

    fn unlock_charge_limit(&mut self) -> Result<()> {
        self.limit.active = true;
        Ok(())
    }

    fn fan_max(&mut self) -> Result<bool> {
        Ok(self.mode_raw & FAN_MAX_BIT != 0)
    }

    fn set_fan_max(&mut self, enabled: bool) -> Result<()> {
        self.mode_raw = if enabled { self.mode_raw | FAN_MAX_BIT } else { self.mode_raw & !FAN_MAX_BIT };
        Ok(())
    }

    fn kbd_backlight(&mut self) -> Result<KbdBacklight> {
        Ok(self.kbd)
    }

    fn set_kbd_brightness(&mut self, brightness: u8) -> Result<()> {
        if brightness > self.kbd.max_brightness {
            return Err(Error::Io("brightness: Invalid argument".into()));
        }
        self.kbd.brightness = brightness;
        Ok(())
    }

    fn set_kbd_color(&mut self, color: [u8; 3]) -> Result<()> {
        self.kbd.color = Some(color);
        Ok(())
    }

    fn usb_powershare(&mut self) -> Result<bool> {
        Ok(self.usb_powershare)
    }

    fn set_usb_powershare(&mut self, enabled: bool) -> Result<()> {
        self.usb_powershare = enabled;
        Ok(())
    }

    fn ac_auto_boot(&mut self) -> Result<bool> {
        Ok(self.ac_auto_boot)
    }

    fn set_ac_auto_boot(&mut self, enabled: bool) -> Result<()> {
        self.ac_auto_boot = enabled;
        Ok(())
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

/// Поставить лимит заряда и, если EC его ещё не соблюдает, включить обходом.
/// 100 — без лимита: обход не нужен.
pub fn apply_charge_limit(b: &mut dyn Backend, percent: u8) -> Result<()> {
    if !(CHARGE_LIMIT_MIN..=CHARGE_LIMIT_MAX).contains(&percent) {
        return Err(Error::Io(format!(
            "лимит заряда {percent}%: допустимо от {CHARGE_LIMIT_MIN} до {CHARGE_LIMIT_MAX}"
        )));
    }
    b.set_charge_limit(percent)?;
    if percent < CHARGE_LIMIT_MAX && !b.charge_limit()?.active {
        b.unlock_charge_limit()?;
    }
    let now = b.charge_limit()?;
    if now.percent != percent {
        return Err(Error::NotApplied(format!(
            "лимит заряда {percent}% не записался: EC сообщает {}%",
            now.percent
        )));
    }
    if percent < CHARGE_LIMIT_MAX && !now.active {
        return Err(Error::NotApplied(format!(
            "лимит заряда {percent}% записан, но EC его не включил (0x0742 бит 2 = 0)"
        )));
    }
    Ok(())
}

pub fn apply_fan_max(b: &mut dyn Backend, enabled: bool) -> Result<()> {
    b.set_fan_max(enabled)?;
    if b.fan_max()? != enabled {
        return Err(Error::NotApplied(format!(
            "вентиляторы на максимум: EC не {}",
            if enabled { "включил" } else { "выключил" }
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
        // _IOR(0xef, 0x40, int32_t*), _IO(0xf0, 0x41).
        assert_eq!(R_UW_CHARGE_LIMIT, 0x8008_ef40);
        assert_eq!(W_UW_CHARGE_LIMIT_UNLOCK, 0x0000_f041);
        assert_eq!(W_UW_MODE, 0x4008_f012);
    }

    #[test]
    fn fan_max_keeps_mode() {
        let mut m = Mock::default();
        apply_perf_mode(&mut m, PerfMode::Turbo).unwrap();
        apply_fan_max(&mut m, true).unwrap();
        // Смена режима бит вентиляторов не сбрасывает, и наоборот.
        apply_perf_mode(&mut m, PerfMode::Office).unwrap();
        assert!(m.fan_max().unwrap());
        apply_fan_max(&mut m, false).unwrap();
        assert_eq!(PerfMode::from_raw(m.perf_mode_raw().unwrap()), Some(PerfMode::Office));
    }

    #[test]
    fn charge_limit_unlocks_once() {
        let mut m = Mock::default();
        apply_charge_limit(&mut m, 80).unwrap();
        assert_eq!(m.charge_limit().unwrap(), ChargeLimit { percent: 80, active: true });
        apply_charge_limit(&mut m, 100).unwrap();
        assert!(!m.charge_limit().unwrap().effective());
        assert!(apply_charge_limit(&mut m, 10).is_err());
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
