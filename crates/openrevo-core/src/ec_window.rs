//! Окно памяти EC (H2RAM) — только чтение.
//!
//! Хост видит первые 4 КиБ памяти EC по физическому адресу `0xFED50000`
//! (в DSDT — `ECMG`, в `/proc/iomem` — `INOU0000`); так же их читает
//! ControlCenter в Windows. Отсюда берём то, чего нет в tuxedo_io: циклы
//! батареи и напряжение, до которого EC заряжает. Отображение только на
//! чтение (`PROT_READ`), записать через него нельзя. Адреса — из разбора
//! прошивки EC 2.12 (ec_reverse), см. docs/RESEARCH.md.

use std::fs::OpenOptions;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;

use crate::protocol::ChargeVoltage;

const BASE: libc::off_t = 0xFED5_0000;
const SIZE: usize = 0x1000;

/// Базовое напряжение заряда от батареи, мВ, big-endian.
const FULL_VOLTAGE: usize = 0x030E;
/// Цель EC после снижения на «износ», мВ, little-endian.
const CHARGE_VOLTAGE: usize = 0x0522;
/// Биты 7:6 — число банок.
const CELLS: usize = 0x0491;
/// Циклы от контроллера батареи, little-endian.
const CYCLES: usize = 0x04A6;
/// Паспортная ёмкость, мА·ч, little-endian — для проверки, что окно то.
const DESIGN_CAPACITY: usize = 0x0402;

pub struct EcWindow {
    ptr: *const u8,
}

// SAFETY: отображение только на чтение, доступ — побайтные volatile-чтения.
unsafe impl Send for EcWindow {}

impl EcWindow {
    pub fn open() -> Result<Self, String> {
        let f = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_SYNC)
            .open("/dev/mem")
            .map_err(|e| format!("/dev/mem: {e}"))?;
        // SAFETY: новое отображение, длина и смещение выровнены по странице.
        let ptr = unsafe {
            libc::mmap(std::ptr::null_mut(), SIZE, libc::PROT_READ, libc::MAP_SHARED, f.as_raw_fd(), BASE)
        };
        if ptr == libc::MAP_FAILED {
            return Err(format!("окно EC {BASE:#x}: {}", std::io::Error::last_os_error()));
        }
        let w = EcWindow { ptr: ptr as *const u8 };
        // Закрытое или чужое окно читается как 0xFF.
        let design = w.le(DESIGN_CAPACITY);
        if design == 0 || design == 0xFFFF {
            return Err(format!("окно EC {BASE:#x} не похоже на MECHREVO (паспортная ёмкость {design:#x})"));
        }
        Ok(w)
    }

    fn byte(&self, addr: usize) -> u8 {
        assert!(addr < SIZE);
        // SAFETY: addr внутри отображения; MMIO — читаем volatile.
        unsafe { self.ptr.add(addr).read_volatile() }
    }

    fn le(&self, addr: usize) -> u16 {
        u16::from(self.byte(addr)) | u16::from(self.byte(addr + 1)) << 8
    }

    fn be(&self, addr: usize) -> u16 {
        u16::from(self.byte(addr)) << 8 | u16::from(self.byte(addr + 1))
    }

    pub fn battery_cycles(&self) -> Option<u16> {
        Some(self.le(CYCLES)).filter(|&c| c < 10_000)
    }

    /// `None`, если значения неправдоподобны (нет батареи, другая прошивка).
    pub fn charge_voltage(&self) -> Option<ChargeVoltage> {
        let cells = match self.byte(CELLS) >> 6 {
            0b11 => 4,
            0b10 => 3,
            _ => return None,
        };
        let full_mv = self.be(FULL_VOLTAGE);
        let charge_mv = self.le(CHARGE_VOLTAGE);
        // 3–4,6 В на банку; цель не выше базы.
        let per_cell = |mv: u16| (3000..=4600).contains(&(u32::from(mv) / cells));
        (per_cell(full_mv) && per_cell(charge_mv) && charge_mv <= full_mv)
            .then_some(ChargeVoltage { charge_mv, full_mv, cells: cells as u8 })
    }
}

impl Drop for EcWindow {
    fn drop(&mut self) {
        // SAFETY: то самое отображение из open().
        unsafe { libc::munmap(self.ptr as *mut libc::c_void, SIZE) };
    }
}
