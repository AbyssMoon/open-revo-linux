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
    #[serde(default)]
    charge_profile: Option<ChargeProfile>,
    /// Только то, что поставил пользователь: заводское значение не трогаем.
    #[serde(default)]
    charge_limit: Option<u8>,
}

struct Daemon {
    backend: Box<dyn Backend>,
    profiles_available: i32,
    saved: Saved,
    state_path: PathBuf,
    sysinfo: SysInfo,
    cpu_power_w: Option<f64>,
    /// Режим, который демон видел в EC последним, — чтобы отличить смену
    /// клавишей от своей записи.
    last_mode: Option<PerfMode>,
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
        self.last_mode = self.current_mode();
    }

    fn status(&mut self) -> Status {
        fn ok<T>(errors: &mut Vec<String>, r: backend::Result<T>, what: &str) -> Option<T> {
            r.map_err(|e| errors.push(format!("{what}: {e}"))).ok()
        }
        let mut errors = Vec::new();
        let perf_mode_raw = ok(&mut errors, self.backend.perf_mode_raw(), "режим");
        let charge_profile = ok(&mut errors, self.backend.charge_profile(), "профиль заряда");
        let charge_limit = match self.backend.charge_limit() {
            Ok(l) => Some(l),
            // Нет патча 0003 или модель не та — просто не показываем.
            Err(backend::Error::Unsupported(_)) => None,
            Err(e) => {
                errors.push(format!("лимит заряда: {e}"));
                None
            }
        };
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
            Request::SetChargeProfile { profile } => {
                backend::apply_charge_profile(self.backend.as_mut(), profile).map(|()| {
                    log(&format!("профиль заряда «{}»", profile.name()));
                    self.saved.charge_profile = Some(profile);
                    self.save();
                })
            }
            Request::SetChargeLimit { percent } => {
                backend::apply_charge_limit(self.backend.as_mut(), percent).map(|()| {
                    log(&format!("лимит заряда {percent}%"));
                    self.saved.charge_limit = Some(percent);
                    self.save();
                })
            }
            Request::SetFnLock { enabled } => self.backend.set_fn_lock(enabled),
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

    let (backend, profiles_available): (Box<dyn Backend>, i32) = if mock {
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

    let mut daemon = Daemon {
        backend,
        profiles_available,
        saved: saved.unwrap_or_default(),
        state_path,
        sysinfo: SysInfo::read(),
        cpu_power_w: None,
        last_mode: None,
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
    } else {
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
