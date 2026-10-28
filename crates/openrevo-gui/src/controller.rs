//! QObject `Controller`, через который QML общается с демоном openrevod.

use std::fs;
use std::pin::Pin;

use cxx_qt::Threading;
use cxx_qt_lib::QString;
use openrevo_core::acpi_events;
use openrevo_core::ipc::{self, IpcError, Request, Status};
use openrevo_core::protocol::{ChargeProfile, PerfMode};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, connected)]
        #[qproperty(QString, error_text)]
        /// Ошибка последнего действия (демон доступен, но команда не прошла).
        #[qproperty(QString, action_error)]
        #[qproperty(QString, backend)]
        /// Имя режима (`office`, `balanced`, `turbo`) или пусто, если неизвестен.
        #[qproperty(QString, perf_mode)]
        #[qproperty(i32, perf_raw)]
        /// 3 — драйвер включил режимы для платы, 0 — нет.
        #[qproperty(i32, profiles_available)]
        /// `high_capacity`, `balanced`, `stationary` или пусто.
        #[qproperty(QString, charge_profile)]
        /// -1 — не поддерживается, 0 — выключен, 1 — включён.
        #[qproperty(i32, fn_lock)]
        /// Вентиляторы, % (-1 — неизвестно).
        #[qproperty(i32, fan1)]
        #[qproperty(i32, fan2)]
        /// Потребление пакета CPU, Вт (-1 — неизвестно).
        #[qproperty(f64, cpu_power)]
        /// Средняя частота ядер, МГц, и температура (Tctl), °C (-1 — неизвестно).
        #[qproperty(i32, cpu_freq)]
        #[qproperty(i32, cpu_temp)]
        #[qproperty(i32, battery_capacity)]
        /// Мощность заряда (+) или разряда (−), Вт; 0 — не идёт.
        #[qproperty(f64, battery_power)]
        /// Текущая, полная и паспортная ёмкость, Вт·ч (-1 — неизвестно).
        #[qproperty(f64, battery_energy)]
        #[qproperty(f64, battery_energy_full)]
        #[qproperty(f64, battery_energy_design)]
        /// Charging / Discharging / Not charging / Full.
        #[qproperty(QString, battery_status)]
        /// -1 — неизвестно, 0 — от батареи, 1 — от сети.
        #[qproperty(i32, ac_online)]
        #[qproperty(QString, device_info)]
        #[qproperty(bool, is_mechrevo)]
        /// JSON `{режим: [длительный, кратковременный]}` — замеры для этой модели или `{}`.
        #[qproperty(QString, mode_limits)]
        type Controller = super::ControllerRust;

        #[qinvokable]
        fn refresh(self: Pin<&mut Controller>);

        #[qinvokable]
        fn set_perf(self: Pin<&mut Controller>, mode: &QString);

        #[qinvokable]
        fn set_charge(self: Pin<&mut Controller>, profile: &QString);

        #[qinvokable]
        fn request_fn_lock(self: Pin<&mut Controller>, enabled: bool);

        /// Начать принимать запросы от повторных запусков интерфейса.
        #[qinvokable]
        fn listen_instances(self: Pin<&mut Controller>);

        /// Перечитывать состояние сразу по событиям ACPI (питание, клавиши).
        #[qinvokable]
        fn listen_events(self: Pin<&mut Controller>);

        /// Повторный запуск попросил показать окно.
        #[qsignal]
        fn show_requested(self: Pin<&mut Controller>);
    }

    impl cxx_qt::Threading for Controller {}
}

pub struct ControllerRust {
    connected: bool,
    error_text: QString,
    action_error: QString,
    backend: QString,
    perf_mode: QString,
    perf_raw: i32,
    profiles_available: i32,
    charge_profile: QString,
    fn_lock: i32,
    fan1: i32,
    fan2: i32,
    cpu_power: f64,
    cpu_freq: i32,
    cpu_temp: i32,
    battery_capacity: i32,
    battery_power: f64,
    battery_energy: f64,
    battery_energy_full: f64,
    battery_energy_design: f64,
    battery_status: QString,
    ac_online: i32,
    device_info: QString,
    is_mechrevo: bool,
    mode_limits: QString,
}

impl Default for ControllerRust {
    fn default() -> Self {
        Self {
            connected: false,
            error_text: QString::default(),
            action_error: QString::default(),
            backend: QString::default(),
            perf_mode: QString::default(),
            perf_raw: -1,
            profiles_available: 0,
            charge_profile: QString::default(),
            fn_lock: -1,
            fan1: -1,
            fan2: -1,
            cpu_power: -1.0,
            cpu_freq: -1,
            cpu_temp: -1,
            battery_capacity: -1,
            battery_power: 0.0,
            battery_energy: -1.0,
            battery_energy_full: -1.0,
            battery_energy_design: -1.0,
            battery_status: QString::default(),
            ac_online: -1,
            device_info: QString::default(),
            is_mechrevo: false,
            mode_limits: QString::from("{}"),
        }
    }
}

#[derive(Default)]
struct PowerSupply {
    capacity: Option<i32>,
    status: Option<String>,
    ac: Option<bool>,
    /// Вт, со знаком: + заряд, − разряд.
    power: Option<f64>,
    energy: Option<f64>,
    energy_full: Option<f64>,
    energy_design: Option<f64>,
}

/// Батарея и сетевое питание из `/sys/class/power_supply` (root не нужен).
/// Батарея этого ноутбука отдаёт заряд в мкА·ч, поэтому энергия считается
/// через паспортное напряжение, если `energy_*` нет.
fn power_supply() -> PowerSupply {
    let mut ps = PowerSupply::default();
    let read = |p: std::path::PathBuf| fs::read_to_string(p).ok().map(|s| s.trim().to_string());
    let num = |p: std::path::PathBuf| read(p).and_then(|v| v.parse::<f64>().ok());
    for e in fs::read_dir("/sys/class/power_supply").into_iter().flatten().flatten() {
        let dir = e.path();
        match read(dir.join("type")).as_deref() {
            Some("Battery") if ps.capacity.is_none() => {
                ps.capacity = read(dir.join("capacity")).and_then(|v| v.parse().ok());
                ps.status = read(dir.join("status"));
                let watts = num(dir.join("power_now")).map(|v| v / 1e6).or_else(|| {
                    Some(num(dir.join("current_now"))? * num(dir.join("voltage_now"))? / 1e12)
                });
                let sign = match ps.status.as_deref() {
                    Some("Discharging") => -1.0,
                    Some("Charging") => 1.0,
                    _ => 0.0,
                };
                ps.power = watts.map(|w| w.abs() * sign);
                let volts = num(dir.join("voltage_min_design")).map(|v| v / 1e6);
                let wh = |energy: &str, charge: &str| {
                    num(dir.join(energy))
                        .map(|v| v / 1e6)
                        .or_else(|| Some(num(dir.join(charge))? / 1e6 * volts?))
                };
                ps.energy = wh("energy_now", "charge_now");
                ps.energy_full = wh("energy_full", "charge_full");
                ps.energy_design = wh("energy_full_design", "charge_full_design");
            }
            Some("Mains") => {
                let online = read(dir.join("online")).as_deref() == Some("1");
                ps.ac = Some(ps.ac.unwrap_or(false) || online);
            }
            _ => {}
        }
    }
    ps
}

/// Средняя текущая частота ядер, МГц.
fn cpu_freq_mhz() -> Option<i32> {
    let (mut sum, mut n) = (0u64, 0u64);
    for e in fs::read_dir("/sys/devices/system/cpu").ok()?.flatten() {
        let p = e.path().join("cpufreq/scaling_cur_freq");
        if let Some(v) = fs::read_to_string(p).ok().and_then(|s| s.trim().parse::<u64>().ok()) {
            sum += v;
            n += 1;
        }
    }
    (n > 0).then(|| (sum / n / 1000) as i32)
}

/// Температура CPU (`k10temp`, Tctl), °C.
fn cpu_temp_c() -> Option<i32> {
    for e in fs::read_dir("/sys/class/hwmon").ok()?.flatten() {
        let dir = e.path();
        if fs::read_to_string(dir.join("name")).ok()?.trim() == "k10temp" {
            let v: i32 = fs::read_to_string(dir.join("temp1_input")).ok()?.trim().parse().ok()?;
            return Some(v / 1000);
        }
    }
    None
}

impl qobject::Controller {
    fn update_power(mut self: Pin<&mut Self>) {
        self.as_mut().set_cpu_freq(cpu_freq_mhz().unwrap_or(-1));
        self.as_mut().set_cpu_temp(cpu_temp_c().unwrap_or(-1));
        let ps = power_supply();
        self.as_mut().set_battery_capacity(ps.capacity.unwrap_or(-1));
        self.as_mut()
            .set_battery_status(QString::from(ps.status.unwrap_or_default().as_str()));
        self.as_mut().set_ac_online(ps.ac.map_or(-1, i32::from));
        self.as_mut().set_battery_power(ps.power.unwrap_or(0.0));
        self.as_mut().set_battery_energy(ps.energy.unwrap_or(-1.0));
        self.as_mut()
            .set_battery_energy_full(ps.energy_full.unwrap_or(-1.0));
        self.as_mut()
            .set_battery_energy_design(ps.energy_design.unwrap_or(-1.0));
    }

    /// Результат команды: ошибка демона показывается отдельно, состояние перечитывается.
    fn apply_action(mut self: Pin<&mut Self>, result: Result<Status, IpcError>) {
        match result {
            Err(IpcError::Daemon(e)) => {
                self.as_mut().set_action_error(QString::from(e.as_str()));
                let status = ipc::request(&Request::Status);
                self.apply(status);
            }
            other => {
                self.as_mut().set_action_error(QString::default());
                self.apply(other);
            }
        }
    }

    fn apply(mut self: Pin<&mut Self>, result: Result<Status, IpcError>) {
        self.as_mut().update_power();
        let s = match result {
            Ok(s) => s,
            Err(e) => {
                self.as_mut().set_connected(false);
                self.as_mut().set_error_text(QString::from(e.to_string().as_str()));
                return;
            }
        };
        self.as_mut().set_connected(true);
        self.as_mut()
            .set_error_text(QString::from(s.errors.join("\n").as_str()));
        self.as_mut().set_backend(QString::from(s.backend.as_str()));

        self.as_mut().set_perf_raw(s.perf_mode_raw.map_or(-1, i32::from));
        self.as_mut()
            .set_perf_mode(QString::from(s.perf_mode().map_or("", PerfMode::name)));
        self.as_mut().set_profiles_available(s.profiles_available);
        self.as_mut().set_charge_profile(QString::from(
            s.charge_profile.map_or("", ChargeProfile::name),
        ));
        self.as_mut().set_fn_lock(s.fn_lock.map_or(-1, i32::from));
        self.as_mut().set_fan1(s.fan1_pct.map_or(-1, i32::from));
        self.as_mut().set_fan2(s.fan2_pct.map_or(-1, i32::from));
        self.as_mut().set_cpu_power(s.cpu_power_w.unwrap_or(-1.0));
        // Без k10temp — температура, по которой EC крутит вентиляторы.
        if *self.cpu_temp() < 0 {
            let t = s.ec_cpu_temp.map_or(-1, i32::from);
            self.as_mut().set_cpu_temp(t);
        }

        let i = &s.sysinfo;
        let mut info = format!(
            "Производитель: {}\nМодель: {}\nПлата: {}\nBIOS: {} ({})\nEC: {}",
            i.vendor, i.product, i.board, i.bios_version, i.bios_date, i.ec_version
        );
        if !i.serial.is_empty() {
            info.push_str(&format!("\nСерийный номер: {}", i.serial));
        }
        self.as_mut().set_device_info(QString::from(info.as_str()));
        self.as_mut().set_is_mechrevo(i.is_mechrevo());
        let limits: serde_json::Map<String, serde_json::Value> = PerfMode::ALL
            .into_iter()
            .filter_map(|m| i.mode_limits(m).map(|(a, b)| (m.name().to_string(), serde_json::json!([a, b]))))
            .collect();
        let limits = serde_json::Value::Object(limits).to_string();
        self.as_mut().set_mode_limits(QString::from(limits.as_str()));
    }

    pub fn refresh(self: Pin<&mut Self>) {
        self.apply(ipc::request(&Request::Status));
    }

    pub fn set_perf(self: Pin<&mut Self>, mode: &QString) {
        let Some(mode) = PerfMode::from_name(&mode.to_string()) else {
            return;
        };
        self.apply_action(ipc::request(&Request::SetPerfMode { mode }));
    }

    pub fn set_charge(self: Pin<&mut Self>, profile: &QString) {
        let Some(profile) = ChargeProfile::from_name(&profile.to_string()) else {
            return;
        };
        self.apply_action(ipc::request(&Request::SetChargeProfile { profile }));
    }

    pub fn request_fn_lock(self: Pin<&mut Self>, enabled: bool) {
        self.apply_action(ipc::request(&Request::SetFnLock { enabled }));
    }

    pub fn listen_instances(self: Pin<&mut Self>) {
        let thread = self.qt_thread();
        crate::single_instance::listen(move || {
            let _ = thread.queue(|ctl| ctl.show_requested());
        });
    }

    pub fn listen_events(self: Pin<&mut Self>) {
        let thread = self.qt_thread();
        std::thread::spawn(move || {
            // Без событий интерфейс просто живёт на опросе по таймеру.
            let Ok(listener) = acpi_events::Listener::open() else { return };
            while let Ok(events) = listener.next() {
                let relevant = events
                    .iter()
                    .any(|e| e.device_class == "wmi" || e.device_class == "ac_adapter");
                if relevant && thread.queue(|ctl| ctl.refresh()).is_err() {
                    break;
                }
            }
        });
    }
}
