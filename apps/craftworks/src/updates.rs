//! Updates in the local rebuild mode (docs/updates.md, milestone 1). Craftworks is built from
//! the four Craft apps' source, so a new WordCraft, GridCraft, DeckCraft or PdfCraft release
//! means rebuilding Craftworks, not downloading theirs. On a Mac that has the Craftworks
//! checkout this build came from, the app asks `scripts/upstream.py status` whether any of them
//! has a newer release (once a day, and on Craftworks ▸ Check for Updates…), shows "Update
//! available" in the title row, and on request runs `upstream.py sync`: it moves our changes
//! onto the releases, builds, tests, runs the acceptance check, installs to /Applications and
//! commits. Then "Restart to update". Without the checkout there is nothing to rebuild with,
//! and the feature stays out of sight.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};

/// What this binary was built from: the four apps' releases and the shared libraries.
pub const BUILT_FROM: &str = include_str!("../../../vendor/versions.json");

/// How often the app looks for releases by itself.
const CHECK_EVERY_SECS: i64 = 20 * 60 * 60;

/// One app with a newer release.
#[derive(Clone, Debug, PartialEq)]
pub struct AppUpdate {
    pub name: String,
    pub have: String,
    pub latest: String,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Idle,
    Checking,
    /// Newer releases are out.
    Available(Vec<AppUpdate>),
    /// The rebuild is running; the stage it is at.
    Updating(String),
    /// Installed: a restart picks it up.
    Ready,
    Failed(String),
}

enum Msg {
    Checked(Result<(Vec<AppUpdate>, Vec<(String, String)>), String>),
    Stage(String),
    Log(PathBuf),
    Installed(PathBuf),
    Done(Result<String, String>),
}

pub struct Updates {
    /// The checkout this build came from, when it is still on this Mac.
    pub source: Option<PathBuf>,
    pub state: State,
    pub popover_open: bool,
    pub pill_rect: Option<egui::Rect>,
    /// The last rebuild's log.
    pub log: Option<PathBuf>,
    /// Where the new build was installed (what a restart opens).
    pub installed: Option<PathBuf>,
    /// A check the user asked for: its answer is said even when there is nothing new.
    manual: bool,
    /// Messages for the user (toasts), taken by the app each frame.
    pub notices: Vec<(bool, String)>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    waker: Option<egui::Context>,
}

impl Default for Updates {
    fn default() -> Self {
        let (tx, rx) = channel();
        Updates {
            source: None,
            state: State::Idle,
            popover_open: false,
            pill_rect: None,
            log: None,
            installed: None,
            manual: false,
            notices: Vec::new(),
            tx,
            rx,
            waker: None,
        }
    }
}

/// The checkout at `root`, if it is one Craftworks can rebuild itself from.
fn usable_source(root: &Path) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    (root.join("scripts/upstream.py").is_file() && root.join(".git").exists() && root.join("vendor/versions.json").is_file()).then_some(root)
}

/// The search path for the scripts: an app started from Finder gets only the system's.
fn search_path() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    [
        format!("{home}/.cargo/bin"),
        "/opt/homebrew/bin".into(),
        "/usr/local/bin".into(),
        "/Library/Frameworks/Python.framework/Versions/Current/bin".into(),
        std::env::var("PATH").unwrap_or_default(),
        "/usr/bin:/bin".into(),
    ]
    .join(":")
}

fn python(root: &Path) -> Command {
    let mut c = Command::new("python3");
    c.arg(root.join("scripts/upstream.py")).current_dir(root).env("PATH", search_path()).stdin(Stdio::null());
    c
}

/// The versions this binary was built from, by app name (for the "up to date" message).
pub fn built_from() -> Vec<(String, String)> {
    let v: serde_json::Value = serde_json::from_str(BUILT_FROM).unwrap_or_default();
    [("wordcraft", "WordCraft"), ("gridcraft", "GridCraft"), ("deckcraft", "DeckCraft"), ("pdfcraft", "PdfCraft")]
        .iter()
        .map(|(id, name)| ((*name).to_string(), v["apps"][id]["release"].as_str().unwrap_or("?").to_string()))
        .collect()
}

impl Updates {
    /// Finds the checkout this build came from. `None` (no updates shown) when it is gone.
    pub fn new() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        Updates { source: usable_source(&root), ..Updates::default() }
    }

    pub fn enabled(&self) -> bool {
        self.source.is_some()
    }

    /// The daily check, at most once every 20 hours (the time of the last one is kept).
    pub fn check_if_due(&mut self, ctx: &egui::Context) {
        let Some(stamp) = crate::settings::dir().map(|d| d.join("update-check.json")) else { return };
        let last = std::fs::read_to_string(&stamp)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v["last"].as_i64())
            .unwrap_or(0);
        let now = chrono::Local::now().timestamp();
        if now - last >= CHECK_EVERY_SECS && self.enabled() {
            let _ = crate::settings::write_atomic(&stamp, serde_json::json!({ "last": now }).to_string().as_bytes());
            self.check(ctx, false);
        }
    }

    /// Asks `upstream.py status` about the releases, on a thread. `manual`: Check for Updates…
    pub fn check(&mut self, ctx: &egui::Context, manual: bool) {
        let Some(root) = self.source.clone() else {
            if manual {
                self.notices.push((true, "Updates rebuild Craftworks from its source, which isn't on this Mac".into()));
            }
            return;
        };
        if matches!(self.state, State::Checking | State::Updating(_) | State::Ready) {
            if manual {
                self.popover_open = true;
            }
            return;
        }
        self.manual = manual;
        self.state = State::Checking;
        self.waker = Some(ctx.clone());
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Checked(status(&root)));
            ctx.request_repaint();
        });
    }

    /// Runs `upstream.py sync` on a thread, reporting its stages.
    pub fn start_update(&mut self, ctx: &egui::Context) {
        let Some(root) = self.source.clone() else { return };
        if matches!(self.state, State::Updating(_)) {
            return;
        }
        self.state = State::Updating("Starting".into());
        self.log = None;
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = sync(&root, &tx, &ctx);
            let _ = tx.send(Msg::Done(result));
            ctx.request_repaint();
        });
    }

    /// Picks up what the threads said.
    pub fn tick(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Checked(Ok((apps, have))) => {
                    if apps.is_empty() {
                        self.state = State::Idle;
                        if self.manual {
                            let list: Vec<String> = have.iter().map(|(n, v)| format!("{n} {v}")).collect();
                            self.notices.push((false, format!("Craftworks is up to date ({})", list.join(", "))));
                        }
                    } else {
                        self.state = State::Available(apps);
                        if self.manual {
                            self.popover_open = true;
                        }
                    }
                }
                Msg::Checked(Err(e)) => {
                    self.state = State::Idle;
                    if self.manual {
                        self.notices.push((true, format!("Couldn't check for updates: {e}")));
                    } else {
                        log::info!("update check: {e}");
                    }
                }
                Msg::Stage(s) => self.state = State::Updating(s),
                Msg::Log(p) => self.log = Some(p),
                Msg::Installed(p) => self.installed = Some(p),
                Msg::Done(Ok(summary)) => {
                    if self.installed.is_some() {
                        self.state = State::Ready;
                        self.popover_open = true;
                    } else {
                        self.state = State::Idle;
                        self.notices.push((false, format!("Craftworks: {summary}")));
                    }
                }
                Msg::Done(Err(e)) => {
                    self.state = State::Failed(e);
                    self.popover_open = true;
                }
            }
        }
    }

    /// The pill's text, when there is something to show.
    pub fn pill_label(&self) -> Option<String> {
        match &self.state {
            State::Available(_) => Some("Update available".into()),
            State::Updating(_) => Some("Updating Craftworks…".into()),
            State::Ready => Some("Restart to update".into()),
            State::Failed(_) => Some("Update failed".into()),
            State::Idle | State::Checking => None,
        }
    }

    pub fn busy(&self) -> bool {
        matches!(self.state, State::Updating(_))
    }

    /// The bundle a restart should open: the one just installed.
    pub fn relaunch_target(&self) -> PathBuf {
        self.installed.clone().unwrap_or_else(|| PathBuf::from("/Applications/Craftworks.app"))
    }
}

fn status(root: &Path) -> Result<(Vec<AppUpdate>, Vec<(String, String)>), String> {
    let mut child = python(root)
        .args(["status", "--json", "--installed", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("couldn't run python3 ({e})"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(BUILT_FROM.as_bytes());
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|_| String::from_utf8_lossy(&out.stderr).trim().to_string())?;
    if !v["ok"].as_bool().unwrap_or(false) {
        return Err(v["error"].as_str().unwrap_or("unknown error").to_string());
    }
    let apps = v["apps"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|x| x["newer"].as_bool().unwrap_or(false))
                .map(|x| AppUpdate {
                    name: x["name"].as_str().unwrap_or_default().into(),
                    have: x["have"].as_str().unwrap_or_default().into(),
                    latest: x["latest"].as_str().unwrap_or_default().into(),
                    url: x["url"].as_str().unwrap_or_default().into(),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((apps, built_from()))
}

fn sync(root: &Path, tx: &Sender<Msg>, ctx: &egui::Context) -> Result<String, String> {
    let mut child = python(root).arg("sync").stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| format!("couldn't run python3 ({e})"))?;
    let mut result = Err("the update stopped without saying why".to_string());
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(s) = line.strip_prefix("STAGE ") {
                let text = s.split_once(": ").map_or(s, |(_, t)| t);
                let _ = tx.send(Msg::Stage(text.to_string()));
            } else if let Some(p) = line.strip_prefix("LOG ") {
                let _ = tx.send(Msg::Log(PathBuf::from(p)));
            } else if let Some(p) = line.strip_prefix("INSTALLED ") {
                let _ = tx.send(Msg::Installed(PathBuf::from(p)));
            } else if let Some(r) = line.strip_prefix("RESULT ok ") {
                result = Ok(r.to_string());
            } else if let Some(r) = line.strip_prefix("RESULT failed ") {
                result = Err(r.split_once(": ").map_or(r, |(_, m)| m).to_string());
            }
            ctx.request_repaint();
        }
    }
    let _ = child.wait();
    result
}

/// Opens the new build once this one has gone (a restart after an update).
pub fn relaunch_after_quit(bundle: &Path) {
    let script = format!("sleep 1; /usr/bin/open -n '{}'", bundle.display().to_string().replace('\'', "'\\''"));
    if let Err(e) = Command::new("/bin/sh").args(["-c", &script]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        log::warn!("couldn't arrange the restart: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_knows_what_it_was_built_from() {
        let v = built_from();
        assert_eq!(v.len(), 4);
        assert!(v.iter().all(|(_, ver)| ver.split('.').count() == 3), "{v:?}");
    }

    #[test]
    fn a_folder_without_the_script_is_no_source() {
        assert!(usable_source(Path::new("/")).is_none());
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert!(usable_source(&root).is_some(), "this checkout is one");
    }

    #[test]
    fn the_pill_says_where_the_update_is() {
        let mut u = Updates::default();
        assert_eq!(u.pill_label(), None);
        u.state = State::Available(vec![]);
        assert_eq!(u.pill_label().as_deref(), Some("Update available"));
        u.state = State::Ready;
        assert_eq!(u.pill_label().as_deref(), Some("Restart to update"));
    }
}
