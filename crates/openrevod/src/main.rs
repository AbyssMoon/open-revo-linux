//! openrevod — системный демон (запускается от root).
//!
//! * переключает режим, профиль и лимит заряда по запросам клиентов через Unix-сокет;
//! * сохраняет выбор и восстанавливает его после загрузки (tuxedo_io при
//!   загрузке модуля всегда ставит «баланс») и выхода из сна;
//! * замечает, что режим сменили в обход демона (клавишей или прошивкой), и
//!   запоминает новый;
//! * считает потребление CPU по RAPL (счётчик доступен только root).

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use openrevo_core::backend::{self, fan_percent, Backend, Mock, TuxedoIo};
use openrevo_core::ipc::{Request, Response, Status, DEFAULT_SOCKET};
use openrevo_core::power::EnergyCounter;
use openrevo_core::protocol::{self, ChargeProfile, PerfMode};
use openrevo_core::sysinfo::SysInfo;
use serde::{Deserialize, Serialize};

const DEFAULT_STATE: &str = "/var/lib/openrevo/state.json";
const WATCH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    #[serde(default)]
    perf_mode: Option<PerfMode>,
    /// Прошивочный профиль (`charging_profile`). Если драйвер умеет лимит,
    /// профиль из интерфейса — это лимит, а здесь `high_capacity`.
    #[serde(default)]
    charge_profile: Option<ChargeProfile>,
    /// Только то, что поставил пользователь: заводское значение не трогаем.
    #[serde(default)]
    charge_limit: Option<u8>,
    /// Подсветка клавиатуры: tuxedo_keyboard при загрузке её гасит. Яркость
    /// запоминаем и ту, что поставили клавишей Fn.
    #[serde(default)]
    kbd_brightness: Option<u8>,
    #[serde(default)]
    kbd_color: Option<[u8; 3]>,
    #[serde(default)]
    usb_powershare: Option<bool>,
    #[serde(default)]
    ac_auto_boot: Option<bool>,
}

struct Daemon {
    backend: Box<dyn Backend>,
    profiles_available: i32,
    /// Драйвер умеет лимит заряда (патч 0003, плата в списке).
    has_limit: bool,
    /// Вентиляторы на максимум. Не сохраняется: после перезапуска демона и
    /// перезагрузки — снова авто, а после сна включается обратно.
    fan_max: bool,
    saved: Saved,
    state_path: PathBuf,
    sysinfo: SysInfo,
    cpu_power_w: Option<f64>,
    /// Режим, который демон видел в EC последним, — чтобы отличить смену
    /// клавишей от своей записи.
    last_mode: Option<PerfMode>,
    /// То же для яркости подсветки.
    last_kbd: Option<u8>,
}

impl Daemon {
    fn save(&self) {
        if let Some(dir) = self.state_path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let json = serde_json::to_string_pretty(&self.saved).expect("serialize");
        if let Err(e) = fs::write(&self.state_path, json) {
            log(&format!("не удалось сохранить {}: {e}", self.state_path.display()));
        }
    }

    fn current_mode(&mut self) -> Option<PerfMode> {
        self.backend.perf_mode_raw().ok().and_then(PerfMode::from_raw)
    }

    /// Вернуть сохранённые режим, профиль и лимит заряда, если в прошивке другие.
    fn reapply(&mut self, reason: &str) {
        if let Some(mode) = self.saved.perf_mode {
            if self.current_mode() != Some(mode) {
                match backend::apply_perf_mode(self.backend.as_mut(), mode) {
                    Ok(()) => log(&format!("режим «{}» восстановлен ({reason})", mode.name())),
                    Err(e) => log(&format!("режим «{}» не восстановлен ({reason}): {e}", mode.name())),
                }
            }
        }
        if let Some(p) = self.saved.charge_profile {
            if self.backend.charge_profile().ok() != Some(p) {
                match backend::apply_charge_profile(self.backend.as_mut(), p) {
                    Ok(()) => log(&format!("профиль заряда «{}» восстановлен ({reason})", p.name())),
                    Err(e) => log(&format!("профиль заряда «{}» не восстановлен ({reason}): {e}", p.name())),
                }
            }
        }
        if let Some(p) = self.saved.charge_limit {
            let ok = self.backend.charge_limit().is_ok_and(|l| {
                l.percent == p && (l.active || p >= protocol::CHARGE_LIMIT_MAX)
            });
            if !ok {
                match backend::apply_charge_limit(self.backend.as_mut(), p) {
                    Ok(()) => log(&format!("лимит заряда {p}% восстановлен ({reason})")),
                    Err(e) => log(&format!("лимит заряда {p}% не восстановлен ({reason}): {e}")),
                }
            }
        }
        let kbd = self.backend.kbd_backlight().ok();
        if let (Some(c), Some(k)) = (self.saved.kbd_color, kbd) {
            if k.color.is_some_and(|now| now != c) {
                if let Err(e) = self.backend.set_kbd_color(c) {
                    log(&format!("цвет подсветки не восстановлен ({reason}): {e}"));
                }
            }
        }
        if let (Some(b), Some(k)) = (self.saved.kbd_brightness, kbd) {
            if k.brightness != b {
                match self.backend.set_kbd_brightness(b) {
                    Ok(()) => log(&format!("подсветка {b}/{} восстановлена ({reason})", k.max_brightness)),
                    Err(e) => log(&format!("подсветка не восстановлена ({reason}): {e}")),
                }
            }
        }
        self.last_kbd = self.backend.kbd_backlight().ok().map(|k| k.brightness);
        if let Some(v) = self.saved.usb_powershare {
            if self.backend.usb_powershare().ok() == Some(!v) {
                if let Err(e) = self.backend.set_usb_powershare(v) {
                    log(&format!("USB у выключенного ноутбука не восстановлено ({reason}): {e}"));
                }
            }
        }
        if let Some(v) = self.saved.ac_auto_boot {
            if self.backend.ac_auto_boot().ok() == Some(!v) {
                if let Err(e) = self.backend.set_ac_auto_boot(v) {
                    log(&format!("включение от зарядки не восстановлено ({reason}): {e}"));
                }
            }
        }
        if self.fan_max && self.backend.fan_max().ok() == Some(false) {
            match backend::apply_fan_max(self.backend.as_mut(), true) {
                Ok(()) => log(&format!("вентиляторы на максимум восстановлены ({reason})")),
                Err(e) => log(&format!("вентиляторы на максимум не восстановлены ({reason}): {e}")),
            }
        }
        self.last_mode = self.current_mode();
    }

    /// Лимит заряда; прошивочный профиль при этом — `high_capacity`: щадящие
    /// профили прошивки урезают ёмкость, которую батарея считает полной, и
    /// вместе с лимитом батарея недозаряжалась бы вдвойне.
    fn set_charge_limit(&mut self, percent: u8) -> backend::Result<()> {
        if self.backend.charge_profile().ok() != Some(ChargeProfile::HighCapacity) {
            backend::apply_charge_profile(self.backend.as_mut(), ChargeProfile::HighCapacity)?;
        }
        self.saved.charge_profile = Some(ChargeProfile::HighCapacity);
        backend::apply_charge_limit(self.backend.as_mut(), percent)?;
        self.saved.charge_limit = Some(percent);
        self.save();
        Ok(())
    }

    /// Состояние от прошлых версий: профиль без лимита. Переводим в лимит;
    /// `true` — состояние поменялось, его надо применить.
    fn migrate_charge_profile(&mut self) -> bool {
        if !self.has_limit || self.saved.charge_limit.is_some() {
            return false;
        }
        match self.saved.charge_profile {
            Some(p) if p != ChargeProfile::HighCapacity => {
                log(&format!("профиль заряда «{}» теперь задаётся лимитом {}%", p.name(), p.limit_percent()));
                self.saved.charge_profile = Some(ChargeProfile::HighCapacity);
                self.saved.charge_limit = Some(p.limit_percent());
                self.save();
                true
            }
            _ => false,
        }
    }

    fn status(&mut self) -> Status {
        fn ok<T>(errors: &mut Vec<String>, r: backend::Result<T>, what: &str) -> Option<T> {
            r.map_err(|e| errors.push(format!("{what}: {e}"))).ok()
        }
        let mut errors = Vec::new();
        let perf_mode_raw = ok(&mut errors, self.backend.perf_mode_raw(), "режим");
        let fw_profile = ok(&mut errors, self.backend.charge_profile(), "профиль заряда");
        let charge_limit = match self.backend.charge_limit() {
            Ok(l) => Some(l),
            // Нет патча 0003 или модель не та — просто не показываем.
            Err(backend::Error::Unsupported(_)) => None,
            Err(e) => {
                errors.push(format!("лимит заряда: {e}"));
                None
            }
        };
        let charge_profile = if self.has_limit {
            charge_limit.and_then(|l| ChargeProfile::from_limit(l.effective_percent()))
        } else {
            fw_profile
        };
        let fan_max = self.backend.fan_max().ok();
        // Есть не на всех моделях — ошибку не показываем.
        let kbd_backlight = self.backend.kbd_backlight().ok();
        let usb_powershare = self.backend.usb_powershare().ok();
        let ac_auto_boot = self.backend.ac_auto_boot().ok();
        // Есть не на всех моделях — ошибку не показываем.
        let fn_lock = self.backend.fn_lock().ok();
        let fan1_pct = self.backend.fan_raw(0).ok().map(fan_percent);
        let fan2_pct = self.backend.fan_raw(1).ok().map(fan_percent);
        let ec_cpu_temp = self.backend.ec_cpu_temp().ok().filter(|&t| t > 0);
        if self.profiles_available < 3 {
            errors.push(format!(
                "драйвер не включил режимы для платы «{}» — переключение режимов недоступно",
                self.sysinfo.board
            ));
        }
        Status {
            backend: self.backend.describe(),
            perf_mode_raw,
            perf_mode_saved: self.saved.perf_mode,
            profiles_available: self.profiles_available,
            charge_profile,
            charge_limit,
            fn_lock,
            fan1_pct,
            fan2_pct,
            fan_max,
            kbd_backlight,
            usb_powershare,
            ac_auto_boot,
            ec_cpu_temp,
            cpu_power_w: self.cpu_power_w,
            errors,
            sysinfo: self.sysinfo.clone(),
        }
    }

    fn handle(&mut self, req: Request) -> Response {
        let result = match req {
            Request::Status => Ok(()),
            Request::SetPerfMode { mode } => backend::apply_perf_mode(self.backend.as_mut(), mode).map(|()| {
                log(&format!("режим «{}»", mode.name()));
                self.last_mode = Some(mode);
                self.saved.perf_mode = Some(mode);
                self.save();
            }),
            Request::SetChargeProfile { profile } if self.has_limit => {
                let percent = profile.limit_percent();
                self.set_charge_limit(percent)
                    .map(|()| log(&format!("профиль заряда «{}»: лимит {percent}%", profile.name())))
            }
            Request::SetChargeProfile { profile } => {
                backend::apply_charge_profile(self.backend.as_mut(), profile).map(|()| {
                    log(&format!("профиль заряда «{}»", profile.name()));
                    self.saved.charge_profile = Some(profile);
                    self.save();
                })
            }
            Request::SetChargeLimit { percent } => {
                self.set_charge_limit(percent).map(|()| log(&format!("лимит заряда {percent}%")))
            }
            Request::SetFnLock { enabled } => self.backend.set_fn_lock(enabled),
            Request::SetFanMax { enabled } => backend::apply_fan_max(self.backend.as_mut(), enabled).map(|()| {
                log(&format!("вентиляторы на максимум: {}", if enabled { "вкл" } else { "выкл" }));
                self.fan_max = enabled;
            }),
            Request::SetKbdBrightness { brightness } => {
                self.backend.set_kbd_brightness(brightness).map(|()| {
                    self.last_kbd = Some(brightness);
                    self.saved.kbd_brightness = Some(brightness);
                    self.save();
                })
            }
            Request::SetKbdColor { color } => self.backend.set_kbd_color(color).map(|()| {
                self.saved.kbd_color = Some(color);
                self.save();
            }),
            Request::SetUsbPowershare { enabled } => self.backend.set_usb_powershare(enabled).map(|()| {
                log(&format!("USB у выключенного ноутбука: {}", if enabled { "вкл" } else { "выкл" }));
                self.saved.usb_powershare = Some(enabled);
                self.save();
            }),
            Request::SetAcAutoBoot { enabled } => self.backend.set_ac_auto_boot(enabled).map(|()| {
                log(&format!("включение от зарядки: {}", if enabled { "вкл" } else { "выкл" }));
                self.saved.ac_auto_boot = Some(enabled);
                self.save();
            }),
            Request::Reapply => {
                self.reapply("по запросу");
                Ok(())
            }
        };
        match result {
            Ok(()) => Response::Ok { status: self.status() },
            Err(e) => {
                log(&format!("ошибка: {e}"));
                Response::Error { message: e.to_string() }
            }
        }
    }
}

fn log(msg: &str) {
    // stderr попадает в journald при запуске через systemd.
    eprintln!("{msg}");
}

fn usage() -> ! {
    eprintln!(
        "использование: openrevod [--mock] [--socket ПУТЬ] [--state ПУТЬ]\n\
         \n  --mock     эмуляция прошивки (для разработки без ноутбука)\n\
         \x20 --socket   путь к сокету (по умолчанию {DEFAULT_SOCKET})\n\
         \x20 --state    файл настроек (по умолчанию {DEFAULT_STATE})"
    );
    std::process::exit(2)
}

/// `uniwill_wmi` поднимает интерфейс примерно через секунду после загрузки
/// модуля, а до этого tuxedo_io отвечает «не Uniwill» — ждём.
const DRIVER_WAIT: Duration = Duration::from_secs(15);

fn open_driver() -> backend::Result<TuxedoIo> {
    let start = Instant::now();
    loop {
        match TuxedoIo::open() {
            Ok(b) => return Ok(b),
            Err(e) if start.elapsed() >= DRIVER_WAIT => return Err(e),
            Err(_) => thread::sleep(Duration::from_millis(500)),
        }
    }
}

/// Время с загрузки, включая сон (в отличие от `Instant`, который сон не считает).
fn boottime() -> Option<f64> {
    fs::read_to_string("/proc/uptime")
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// Следит за сном, сменой режима в обход демона и потреблением CPU.
fn watcher(daemon: Arc<Mutex<Daemon>>) {
    let start = Instant::now();
    let base = boottime().unwrap_or(0.0);
    let mut slept = 0.0;
    let counter = EnergyCounter::open();
    let mut energy = counter.as_ref().and_then(EnergyCounter::read);
    loop {
        thread::sleep(WATCH_INTERVAL);

        let now_energy = counter.as_ref().and_then(EnergyCounter::read);
        let cpu_power = match (&counter, energy, now_energy) {
            (Some(c), Some(a), Some(b)) => c.average_w(a, b),
            _ => None,
        };
        energy = now_energy;

        let mut resumed = false;
        if let Some(now) = boottime() {
            let total_sleep = (now - base) - start.elapsed().as_secs_f64();
            resumed = total_sleep - slept > 1.0;
            slept = total_sleep;
        }

        let mut d = daemon.lock().unwrap();
        d.cpu_power_w = cpu_power;
        if resumed {
            d.reapply("выход из сна");
            continue;
        }
        let last = d.last_mode;
        if let Some(m) = d.current_mode().filter(|&m| Some(m) != last) {
            log(&format!("режим сменился в обход демона: «{}»", m.name()));
            d.last_mode = Some(m);
            d.saved.perf_mode = Some(m);
            d.save();
        }
        // Яркость, поставленную клавишей Fn, тоже вернём после загрузки.
        let last_kbd = d.last_kbd;
        if let Some(b) = d.backend.kbd_backlight().ok().map(|k| k.brightness).filter(|&b| Some(b) != last_kbd) {
            d.last_kbd = Some(b);
            if last_kbd.is_some() {
                d.saved.kbd_brightness = Some(b);
                d.save();
            }
        }
    }
}

fn serve(stream: UnixStream, daemon: Arc<Mutex<Daemon>>) {
    let Ok(mut writer) = stream.try_clone() else { return };
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { break };
        let resp = match serde_json::from_str::<Request>(&line) {
            Ok(req) => daemon.lock().unwrap().handle(req),
            Err(e) => Response::Error { message: format!("неверный запрос: {e}") },
        };
        let mut out = serde_json::to_string(&resp).expect("serialize");
        out.push('\n');
        if writer.write_all(out.as_bytes()).is_err() {
            break;
        }
    }
}

fn main() {
    let mut mock = false;
    let mut socket = DEFAULT_SOCKET.to_string();
    let mut state = DEFAULT_STATE.to_string();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--mock" => mock = true,
            "--socket" => socket = args.next().unwrap_or_else(|| usage()),
            "--state" => state = args.next().unwrap_or_else(|| usage()),
            _ => usage(),
        }
    }

    let (mut backend, profiles_available): (Box<dyn Backend>, i32) = if mock {
        (Box::new(Mock::default()), 3)
    } else {
        let b = open_driver().unwrap_or_else(|e| {
            log(&e.to_string());
            std::process::exit(1);
        });
        let n = b.profiles_available();
        (Box::new(b), n)
    };

    let state_path = PathBuf::from(state);
    let saved: Option<Saved> = fs::read_to_string(&state_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    let first_run = saved.is_none();

    let has_limit = !matches!(backend.charge_limit(), Err(backend::Error::Unsupported(_)));
    let mut daemon = Daemon {
        backend,
        profiles_available,
        has_limit,
        fan_max: false,
        saved: saved.unwrap_or_default(),
        state_path,
        sysinfo: SysInfo::read(),
        cpu_power_w: None,
        last_mode: None,
        last_kbd: None,
    };
    log(&format!("бэкенд: {}", daemon.backend.describe()));
    if !mock && !daemon.sysinfo.is_mechrevo() {
        log(&format!(
            "внимание: производитель «{}» — это не MECHREVO, проверено только на WUJIE 15X",
            daemon.sysinfo.vendor
        ));
    }

    if first_run {
        // Первый запуск: запоминаем то, что уже стоит, а не сбрасываем.
        daemon.saved.perf_mode = daemon.current_mode();
        daemon.saved.charge_profile = daemon.backend.charge_profile().ok();
        daemon.save();
        daemon.last_mode = daemon.current_mode();
        log("первый запуск: настройки взяты из прошивки");
    }
    if daemon.migrate_charge_profile() || !first_run {
        daemon.reapply("запуск");
    }

    let daemon = Arc::new(Mutex::new(daemon));
    {
        let d = daemon.clone();
        thread::spawn(move || watcher(d));
    }

    let _ = fs::remove_file(&socket);
    if let Some(dir) = Path::new(&socket).parent() {
        let _ = fs::create_dir_all(dir);
    }
    let listener = UnixListener::bind(&socket).unwrap_or_else(|e| {
        log(&format!("не удалось открыть сокет {socket}: {e}"));
        std::process::exit(1);
    });
    // Клиенты работают от обычного пользователя; набор команд ограничен.
    let _ = fs::set_permissions(&socket, fs::Permissions::from_mode(0o666));
    log(&format!("слушаю {socket}"));

    for stream in listener.incoming().flatten() {
        let d = daemon.clone();
        thread::spawn(move || serve(stream, d));
    }
}
