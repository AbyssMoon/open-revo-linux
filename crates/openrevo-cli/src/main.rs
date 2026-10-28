//! openrevo — консольный клиент демона и проверка драйвера.

use openrevo_core::backend::{fan_percent, Backend, Mock, TuxedoIo};
use openrevo_core::ipc::{self, Request, Status};
use openrevo_core::protocol::{ChargeLimit, ChargeProfile, PerfMode, CHARGE_LIMIT_MAX, CHARGE_LIMIT_MIN};
use openrevo_core::sysinfo::SysInfo;

const USAGE: &str = "\
использование: openrevo КОМАНДА

через демон openrevod:
  status                                  текущее состояние
  perf office|balanced|turbo              режим производительности
  charge high_capacity|balanced|stationary
                                          профиль заряда батареи
  limit 40..100|off                       лимит заряда, % (off = 100, без лимита)
  fnlock on|off|toggle                    Fn Lock
  reapply                                 повторно применить сохранённые настройки

напрямую через /dev/tuxedo_io (нужен root, демон не нужен, только чтение):
  probe [--mock]                          проверить драйвер и прочитать регистры

переменная OPENREVO_SOCKET задаёт путь к сокету демона";

fn die(msg: impl std::fmt::Display) -> ! {
    eprintln!("ошибка: {msg}");
    std::process::exit(1)
}

fn onoff(v: Option<bool>) -> &'static str {
    match v {
        Some(true) => "вкл",
        Some(false) => "выкл",
        None => "—",
    }
}

fn mode_line(raw: Option<u8>) -> String {
    match raw {
        Some(v) => match PerfMode::from_raw(v) {
            Some(m) => format!("{} (EC 0x{v:02x})", m.name()),
            None => format!("неизвестный (EC 0x{v:02x})"),
        },
        None => "—".into(),
    }
}

fn limit_line(l: ChargeLimit) -> String {
    if l.effective() {
        format!("{}%", l.percent)
    } else if l.percent < CHARGE_LIMIT_MAX {
        format!("нет (в EC {}%, но EC его не соблюдает)", l.percent)
    } else {
        "нет".into()
    }
}

fn print_status(s: &Status) {
    let i = &s.sysinfo;
    println!("Бэкенд:          {}", s.backend);
    println!("Устройство:      {} {} (плата {})", i.vendor, i.product, i.board);
    println!("BIOS / EC:       {} ({}) / {}", i.bios_version, i.bios_date, i.ec_version);
    println!("Режим:           {}", mode_line(s.perf_mode_raw));
    if let Some(m) = s.perf_mode() {
        if let Some((pl1, pl2)) = i.mode_limits(m) {
            println!("                 от сети CPU до {pl1} Вт, кратковременно {pl2} Вт");
        }
    }
    let charge = s.charge_profile.map_or("—", ChargeProfile::name);
    println!("Профиль заряда:  {charge}");
    if let Some(l) = s.charge_limit {
        println!("Лимит заряда:    {}", limit_line(l));
    }
    println!("Fn Lock:         {}", onoff(s.fn_lock));
    let pct = |v: Option<u8>| v.map_or("—".to_string(), |v| format!("{v}%"));
    println!("Вентиляторы:     {} / {}", pct(s.fan1_pct), pct(s.fan2_pct));
    if let Some(t) = s.ec_cpu_temp {
        println!("Температура EC:  {t} °C");
    }
    if let Some(p) = s.cpu_power_w {
        println!("Мощность CPU:    {p:.1} Вт");
    }
    for e in &s.errors {
        println!("! {e}");
    }
}

fn probe(mock: bool) {
    let i = SysInfo::read();
    println!("DMI: {} {} / плата {} / BIOS {} ({}) / EC {}", i.vendor, i.product, i.board, i.bios_version, i.bios_date, i.ec_version);
    let (mut b, profiles): (Box<dyn Backend>, i32) = if mock {
        (Box::new(Mock::default()), 3)
    } else {
        match TuxedoIo::open() {
            Ok(b) => {
                let n = b.profiles_available();
                (Box::new(b), n)
            }
            Err(e) => die(e),
        }
    };
    println!("бэкенд: {}", b.describe());
    println!("режимов в драйвере: {profiles}{}", if profiles < 3 { " — плата не в списке драйвера, режимы недоступны" } else { "" });
    match b.perf_mode_raw() {
        Ok(v) => println!("0x0751 (режим): {}", mode_line(Some(v))),
        Err(e) => println!("0x0751 (режим): ошибка: {e}"),
    }
    match b.charge_profile() {
        Ok(p) => println!("профиль заряда: {}", p.name()),
        Err(e) => println!("профиль заряда: {e}"),
    }
    match b.charge_limit() {
        Ok(l) => println!(
            "лимит заряда: 0x07B9 = {}%, 0x0742 бит 2 = {} — {}",
            l.percent,
            l.active as u8,
            limit_line(l)
        ),
        Err(e) => println!("лимит заряда: {e}"),
    }
    match b.fn_lock() {
        Ok(v) => println!("fn_lock: {}", onoff(Some(v))),
        Err(e) => println!("fn_lock: {e}"),
    }
    for fan in 0..2 {
        match b.fan_raw(fan) {
            Ok(v) => println!("вентилятор {}: {v}/200 ({}%)", fan + 1, fan_percent(v)),
            Err(e) => println!("вентилятор {}: {e}", fan + 1),
        }
    }
    match b.ec_cpu_temp() {
        Ok(v) => println!("температура CPU по EC: {v} °C"),
        Err(e) => println!("температура CPU по EC: {e}"),
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        println!("{USAGE}");
        return;
    }
    let mock = match args.iter().position(|a| a == "--mock") {
        Some(i) => {
            args.remove(i);
            true
        }
        None => false,
    };
    let cmd = args.remove(0);
    let arg = |i: usize| args.get(i).map(String::as_str);

    let req = match cmd.as_str() {
        "status" => Request::Status,
        "perf" => {
            let s = arg(0).unwrap_or_else(|| die(USAGE));
            Request::SetPerfMode {
                mode: PerfMode::from_name(s).unwrap_or_else(|| die(format!("неизвестный режим: {s}"))),
            }
        }
        "charge" => {
            let s = arg(0).unwrap_or_else(|| die(USAGE));
            Request::SetChargeProfile {
                profile: ChargeProfile::from_name(s).unwrap_or_else(|| die(format!("неизвестный профиль: {s}"))),
            }
        }
        "limit" => {
            let s = arg(0).unwrap_or_else(|| die(USAGE));
            let percent = match s {
                "off" | "none" => CHARGE_LIMIT_MAX,
                _ => s
                    .trim_end_matches('%')
                    .parse()
                    .ok()
                    .filter(|p| (CHARGE_LIMIT_MIN..=CHARGE_LIMIT_MAX).contains(p))
                    .unwrap_or_else(|| die(format!("лимит заряда — от {CHARGE_LIMIT_MIN} до {CHARGE_LIMIT_MAX} или off"))),
            };
            Request::SetChargeLimit { percent }
        }
        "fnlock" => Request::SetFnLock {
            enabled: match arg(0) {
                Some("on") => true,
                Some("off") => false,
                Some("toggle") => match ipc::request(&Request::Status) {
                    Ok(s) => !s.fn_lock.unwrap_or_else(|| die("Fn Lock не поддерживается")),
                    Err(e) => die(e),
                },
                _ => die(USAGE),
            },
        },
        "reapply" => Request::Reapply,
        "probe" => {
            probe(mock);
            return;
        }
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            return;
        }
        other => die(format!("неизвестная команда: {other}\n\n{USAGE}")),
    };

    match ipc::request(&req) {
        Ok(status) => print_status(&status),
        Err(e) => die(e),
    }
}
