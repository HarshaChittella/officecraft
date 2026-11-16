//! The link between the MCP server (its own process) and the running Craftworks window: a
//! Unix socket in Craftworks' settings folder, and a token written there at each launch,
//! readable only by you (mode 0600, as GridCraft's `control_auth` does). Requests are JSON
//! lines; the window runs them on its main thread against the open tab, so an AI's edit is an
//! ordinary command you watch happen and can undo.
//!
//! ```text
//! → {"token": "…", "id": 1, "op": "tabs"}
//! ← {"id": 1, "ok": [{"path": "/p/report.docx", "kind": "Document", "dirty": false}]}
//! → {"token": "…", "id": 2, "op": "call", "path": "/p/report.docx", "method": "engine.execute", "params": {…}}
//! ← {"id": 2, "ok": {…}}   or   {"id": 2, "error": "…"}
//! ```

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

const SOCKET: &str = "live.sock";
const TOKEN: &str = "live.token";
/// The longest request or reply line accepted.
const MAX_LINE: u64 = 64 * 1024 * 1024;
/// How long a request waits for the window.
const TIMEOUT: Duration = Duration::from_secs(60);

/// Craftworks' settings folder: `CRAFTWORKS_SETTINGS_DIR`, else the usual place.
pub fn settings_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("CRAFTWORKS_SETTINGS_DIR") {
        return Some(PathBuf::from(d));
    }
    let home = std::env::home_dir()?;
    Some(if cfg!(target_os = "macos") { home.join("Library/Application Support/Craftworks") } else { home.join(".config/craftworks") })
}

/// Whether a backend request only reads: inspecting, reading ranges, listing commands, or an
/// `engine.execute` of a command whose name says it reads (`slide.inspect`, `cell.get`,
/// `range.read`, `edit.find`…). Everything else counts as an edit.
pub fn is_read_only(method: &str, params: &Value) -> bool {
    match method {
        "document.inspect" | "engine.commands" | "sheet.read" | "ui.render" | "render.page" | "ui.parity" | "ping" | "ui.inspect"
        | "ui.screenshot" => true,
        // PdfCraft's tools (H7) say themselves whether they only read.
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            pdfcraft_automation::tools().iter().any(|t| t.name == name && t.read_only)
        }
        "engine.execute" | "command" => {
            let id = params.get("command").and_then(Value::as_str).unwrap_or_default();
            let last = id.rsplit('.').next().unwrap_or(id).to_ascii_lowercase();
            ["inspect", "get", "read", "list", "describe", "state", "query", "find", "count", "info", "search", "evaluate"]
                .iter()
                .any(|w| last.starts_with(w))
        }
        _ => false,
    }
}

/// A request from the MCP server, for the window's main thread.
pub struct Request {
    pub op: String,
    pub args: Value,
    reply: Sender<Result<Value, String>>,
}

impl Request {
    pub fn answer(self, r: Result<Value, String>) {
        let _ = self.reply.send(r);
    }
}

/// The window's end: listens, checks the token, and queues requests for the main thread.
pub struct LiveServer {
    rx: Receiver<Request>,
    socket: PathBuf,
    token_file: PathBuf,
}

fn random_token() -> std::io::Result<String> {
    let mut b = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// Equal strings, compared in time that doesn't depend on where they differ.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl LiveServer {
    /// Starts listening in `dir`. `wake` is called when a request arrives.
    pub fn start(dir: &Path, wake: impl Fn() + Send + Sync + 'static) -> std::io::Result<LiveServer> {
        std::fs::create_dir_all(dir)?;
        let socket = dir.join(SOCKET);
        let token_file = dir.join(TOKEN);
        let _ = std::fs::remove_file(&socket);
        let token = random_token()?;
        {
            let _ = std::fs::remove_file(&token_file);
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&token_file)?;
            f.write_all(token.as_bytes())?;
        }
        let listener = UnixListener::bind(&socket)?;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
        let (tx, rx) = channel::<Request>();
        let wake = std::sync::Arc::new(wake);
        std::thread::Builder::new().name("craftworks-live".into()).spawn(move || {
            for conn in listener.incoming().flatten() {
                let (tx, token, wake) = (tx.clone(), token.clone(), wake.clone());
                let _ = std::thread::Builder::new().name("craftworks-live-conn".into()).spawn(move || serve(conn, &token, &tx, &*wake));
            }
        })?;
        Ok(LiveServer { rx, socket, token_file })
    }

    /// The requests waiting for the main thread.
    pub fn drain(&self) -> Vec<Request> {
        self.rx.try_iter().collect()
    }
}

impl Drop for LiveServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_file(&self.token_file);
    }
}

fn serve(conn: UnixStream, token: &str, tx: &Sender<Request>, wake: &dyn Fn()) {
    let Ok(mut out) = conn.try_clone() else { return };
    let mut lines = BufReader::new(conn.take(u64::MAX));
    loop {
        let mut line = String::new();
        match (&mut lines).take(MAX_LINE).read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let msg: Value = match serde_json::from_str(line.trim()) {
            Ok(v) => v,
            Err(e) => {
                let _ = writeln!(out, "{}", json!({"id": null, "error": format!("not JSON: {e}")}));
                continue;
            }
        };
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        if !msg.get("token").and_then(Value::as_str).is_some_and(|t| same(t, token)) {
            let _ = writeln!(out, "{}", json!({"id": id, "error": "wrong token"}));
            return;
        }
        let op = msg.get("op").and_then(Value::as_str).unwrap_or_default().to_string();
        let (rtx, rrx) = channel();
        if tx.send(Request { op, args: msg, reply: rtx }).is_err() {
            return;
        }
        wake();
        let reply = match rrx.recv_timeout(TIMEOUT) {
            Ok(Ok(v)) => json!({"id": id, "ok": v}),
            Ok(Err(e)) => json!({"id": id, "error": e}),
            Err(_) => json!({"id": id, "error": "Craftworks didn't answer in time"}),
        };
        if writeln!(out, "{reply}").is_err() {
            return;
        }
    }
}

/// The MCP server's end: a connection to the running window, if there is one.
pub struct LiveClient {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    token: String,
    next: u64,
}

impl LiveClient {
    /// Connects to the window whose settings folder is `dir`. `None` when it isn't running.
    pub fn connect(dir: &Path) -> Option<LiveClient> {
        let token = std::fs::read_to_string(dir.join(TOKEN)).ok()?.trim().to_string();
        let stream = UnixStream::connect(dir.join(SOCKET)).ok()?;
        stream.set_read_timeout(Some(TIMEOUT + Duration::from_secs(5))).ok()?;
        let writer = stream.try_clone().ok()?;
        Some(LiveClient { reader: BufReader::new(stream), writer, token, next: 0 })
    }

    /// Sends `op` with `args` (an object) and waits for the answer.
    pub fn request(&mut self, op: &str, args: Value) -> Result<Value, String> {
        self.next += 1;
        let mut msg = if args.is_object() { args } else { json!({}) };
        if let Some(o) = msg.as_object_mut() {
            o.insert("token".into(), json!(self.token));
            o.insert("id".into(), json!(self.next));
            o.insert("op".into(), json!(op));
        }
        writeln!(self.writer, "{msg}").map_err(|e| format!("Craftworks went away: {e}"))?;
        let mut line = String::new();
        (&mut self.reader).take(MAX_LINE).read_line(&mut line).map_err(|e| format!("Craftworks went away: {e}"))?;
        let v: Value = serde_json::from_str(line.trim()).map_err(|e| format!("a garbled answer from Craftworks: {e}"))?;
        if v.get("id") != Some(&json!(self.next)) {
            return Err("an answer to another request from Craftworks".into());
        }
        match (v.get("ok"), v.get("error")) {
            (Some(ok), _) => Ok(ok.clone()),
            (_, Some(e)) => Err(e.as_str().unwrap_or("error").to_string()),
            _ => Err("an empty answer from Craftworks".into()),
        }
    }

    /// The open tabs' files and kinds.
    pub fn tabs(&mut self) -> Result<Vec<(PathBuf, String)>, String> {
        let v = self.request("tabs", json!({}))?;
        Ok(v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| Some((PathBuf::from(t.get("path")?.as_str()?), t.get("kind")?.as_str()?.to_string())))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        // Unix socket paths are short (~104 bytes): keep the folder near the root.
        let d = PathBuf::from(format!("/tmp/cwl-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn requests_need_the_token_and_reach_the_main_thread() {
        let dir = scratch("t");
        let server = LiveServer::start(&dir, || {}).unwrap();
        let mode = std::fs::metadata(dir.join(TOKEN)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "only you can read the token");
        // The "main thread": answer whatever comes.
        let main = std::thread::spawn(move || {
            for _ in 0..200 {
                for r in server.drain() {
                    let echo = json!({"op": r.op, "path": r.args.get("path")});
                    r.answer(Ok(echo));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            drop(server);
        });
        let mut c = LiveClient::connect(&dir).unwrap();
        let v = c.request("call", json!({"path": "/p/a.docx"})).unwrap();
        assert_eq!(v, json!({"op": "call", "path": "/p/a.docx"}));
        // A wrong token is refused and the connection closed.
        let mut bad = LiveClient::connect(&dir).unwrap();
        bad.token = "0".repeat(64);
        assert_eq!(bad.request("tabs", json!({})), Err("wrong token".to_string()));
        main.join().unwrap();
        assert!(!dir.join(SOCKET).exists() && !dir.join(TOKEN).exists(), "both go when the window does");
        assert!(LiveClient::connect(&dir).is_none(), "no window, no connection");
    }

    #[test]
    fn reads_and_edits_are_told_apart() {
        assert!(is_read_only("document.inspect", &json!({})));
        assert!(is_read_only("engine.execute", &json!({"command": "slide.inspect"})));
        assert!(is_read_only("engine.execute", &json!({"command": "edit.findNext"})));
        assert!(!is_read_only("engine.execute", &json!({"command": "cell.set"})));
        assert!(!is_read_only("engine.execute", &json!({"command": "text.insert"})));
        assert!(!is_read_only("ui.click", &json!({})));
        assert!(is_read_only("tools/call", &json!({"name": "doc_info"})));
        assert!(!is_read_only("tools/call", &json!({"name": "page_rotate"})));
    }

    #[test]
    fn tokens_compare_in_full() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd") && !same("abc", "ab"));
    }
}
