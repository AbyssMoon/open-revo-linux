//! Управление ноутбуками MECHREVO (платформа Uniwill / TongFang) в Linux
//! через драйвер tuxedo-drivers с патчами для MECHREVO.

pub mod acpi_events;
pub mod backend;
pub mod ec_window;
pub mod ipc;
pub mod power;
pub mod protocol;
pub mod sysinfo;
