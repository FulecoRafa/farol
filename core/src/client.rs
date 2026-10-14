//! The command line talking to a running farol (`farol toggle`), the way a
//! compositor keybinding calls it: connect, one call, print the answer.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};

/// `$FAROL_SOCKET`, else `farol.sock` in the services dir `tessela run`
/// gives the project (`$XDG_RUNTIME_DIR/tessela/<$FAROL_PROJECT or farol>/services`).
pub fn socket() -> PathBuf {
    if let Some(p) = std::env::var_os("FAROL_SOCKET") {
        return p.into();
    }
    if let Some(dir) = std::env::var_os("TESSELA_SERVICES_DIR") {
        return PathBuf::from(dir).join("farol.sock");
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let project = std::env::var("FAROL_PROJECT").unwrap_or_else(|_| "farol".into());
    runtime.join("tessela").join(project).join("services").join("farol.sock")
}

#[cfg(unix)]
pub fn call(method: &str, args: Value) -> Result<String, String> {
    use std::os::unix::net::UnixStream;
    let path = socket();
    let stream = UnixStream::connect(&path).map_err(|e| format!("{}: {e} (is farol running?)", path.display()))?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
    let mut w = &stream;
    let line = json!({ "id": 1, "op": "call", "method": method, "args": args }).to_string();
    w.write_all(format!("{line}\n").as_bytes()).map_err(|e| e.to_string())?;
    let mut answer = String::new();
    BufReader::new(&stream).read_line(&mut answer).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&answer).map_err(|e| format!("bad answer: {e}"))?;
    if let Some(e) = v.get("error") {
        return Err(e.as_str().unwrap_or_default().to_string());
    }
    Ok(match v.get("result") {
        None | Some(Value::Null) => String::new(),
        Some(r) => r.to_string(),
    })
}

#[cfg(not(unix))]
pub fn call(_method: &str, _args: Value) -> Result<String, String> {
    Err("talking to a running farol needs Unix sockets (not on this OS yet)".into())
}
