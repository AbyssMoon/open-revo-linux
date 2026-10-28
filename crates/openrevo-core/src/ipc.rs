//! Протокол между демоном (root) и клиентами (GUI, CLI):
//! одна JSON-строка запроса — одна JSON-строка ответа через Unix-сокет.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::protocol::{ChargeProfile, PerfMode};
use crate::sysinfo::SysInfo;

pub const DEFAULT_SOCKET: &str = "/run/openrevo.sock";

/// Путь к сокету: `$OPENREVO_SOCKET` или системный по умолчанию.
pub fn socket_path() -> String {
    std::env::var("OPENREVO_SOCKET").unwrap_or_else(|_| DEFAULT_SOCKET.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Status,
    SetPerfMode { mode: PerfMode },
    SetChargeProfile { profile: ChargeProfile },
    SetFnLock { enabled: bool },
    /// Повторно применить сохранённые режим и профиль заряда.
    Reapply,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Status {
    pub backend: String,
    /// Сырой `0x0751` (см. [`PerfMode::from_raw`]).
    pub perf_mode_raw: Option<u8>,
    /// Режим, который демон сохранил и восстанавливает.
    #[serde(default)]
    pub perf_mode_saved: Option<PerfMode>,
    /// Сколько режимов включил драйвер (3 — плата в списке, 0 — нет).
    #[serde(default)]
    pub profiles_available: i32,
    pub charge_profile: Option<ChargeProfile>,
    #[serde(default)]
    pub fn_lock: Option<bool>,
    /// Вентиляторы, проценты.
    #[serde(default)]
    pub fan1_pct: Option<u8>,
    #[serde(default)]
    pub fan2_pct: Option<u8>,
    /// Температура CPU, по которой EC управляет вентиляторами, °C.
    #[serde(default)]
    pub ec_cpu_temp: Option<u8>,
    /// Потребление пакета CPU за последние ~2 с, ватты.
    #[serde(default)]
    pub cpu_power_w: Option<f64>,
    pub errors: Vec<String>,
    pub sysinfo: SysInfo,
}

impl Status {
    pub fn perf_mode(&self) -> Option<PerfMode> {
        self.perf_mode_raw.and_then(PerfMode::from_raw)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Response {
    Ok { status: Status },
    Error { message: String },
}

#[derive(Debug, Clone)]
pub enum IpcError {
    /// Демон недоступен.
    Connect(String),
    /// Демон ответил ошибкой (связь есть).
    Daemon(String),
}

impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IpcError::Connect(e) | IpcError::Daemon(e) => f.write_str(e),
        }
    }
}

pub fn request(req: &Request) -> Result<Status, IpcError> {
    match send(req)? {
        Response::Ok { status } => Ok(status),
        Response::Error { message } => Err(IpcError::Daemon(message)),
    }
}

fn send(req: &Request) -> Result<Response, IpcError> {
    let path = socket_path();
    let conn = |e: std::io::Error| IpcError::Connect(format!("нет связи с демоном openrevod ({path}): {e}"));
    let stream = UnixStream::connect(&path).map_err(conn)?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let mut writer = stream.try_clone().map_err(conn)?;
    let mut line = serde_json::to_string(req).map_err(|e| IpcError::Daemon(e.to_string()))?;
    line.push('\n');
    writer.write_all(line.as_bytes()).map_err(conn)?;

    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).map_err(conn)?;
    serde_json::from_str::<Response>(&reply)
        .map_err(|e| IpcError::Connect(format!("непонятный ответ демона: {e}: {reply}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_wire_format() {
        let r = Request::SetPerfMode { mode: PerfMode::Turbo };
        assert_eq!(serde_json::to_string(&r).unwrap(), r#"{"cmd":"set_perf_mode","mode":"turbo"}"#);
        let r: Request = serde_json::from_str(r#"{"cmd":"set_charge_profile","profile":"stationary"}"#).unwrap();
        assert!(matches!(r, Request::SetChargeProfile { profile: ChargeProfile::Stationary }));
    }

    #[test]
    fn old_status_without_new_fields() {
        let s: Status = serde_json::from_str(
            r#"{"backend":"x","perf_mode_raw":16,"charge_profile":null,"errors":[],"sysinfo":{}}"#,
        )
        .unwrap();
        assert_eq!(s.perf_mode(), Some(PerfMode::Turbo));
    }
}
