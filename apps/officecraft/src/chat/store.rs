//! Chats kept with their project (docs/ai-chat.md): `.officecraft/chats/<id>.json`, the whole
//! exchange as the API saw it, so a chat carries on where it was left, on any Mac the project
//! folder is on.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// One chat.
#[derive(Clone, Debug, PartialEq)]
pub struct Chat {
    pub id: String,
    pub title: String,
    pub created: i64,
    pub updated: i64,
    /// The Messages API conversation: user, system notes, assistant replies, tool results.
    pub messages: Vec<Value>,
}

impl Chat {
    pub fn new(now: i64) -> Chat {
        // Seconds and a counter: unique within a project, and sorts by time.
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Chat { id: format!("{now:010}-{n:04}"), title: String::new(), created: now, updated: now, messages: Vec::new() }
    }

    /// The title, from the first question when there isn't one.
    pub fn titled(&mut self) {
        if !self.title.is_empty() {
            return;
        }
        let first = self.messages.iter().find(|m| m["role"] == "user").map(user_text).unwrap_or_default();
        let one_line = first.split_whitespace().collect::<Vec<_>>().join(" ");
        self.title = if one_line.chars().count() > 60 { one_line.chars().take(57).collect::<String>() + "…" } else { one_line };
    }
}

/// The text the person typed in a user message (not tool results).
pub fn user_text(m: &Value) -> String {
    match &m["content"] {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter(|p| p["type"] == "text").filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// The project's chat folder.
pub fn dir(root: &Path) -> PathBuf {
    root.join(officecraft_core::project::DIR).join("chats")
}

fn file(root: &Path, id: &str) -> PathBuf {
    dir(root).join(format!("{id}.json"))
}

/// Writes the chat (all at once: a crash never leaves half a file).
pub fn save(root: &Path, chat: &Chat) -> Result<(), String> {
    std::fs::create_dir_all(dir(root)).map_err(|e| e.to_string())?;
    let v = json!({
        "version": 1,
        "id": chat.id,
        "title": chat.title,
        "created": chat.created,
        "updated": chat.updated,
        "messages": chat.messages,
    });
    let text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    crate::settings::write_atomic(&file(root, &chat.id), text.as_bytes()).map_err(|e| e.to_string())
}

pub fn load(root: &Path, id: &str) -> Option<Chat> {
    let v: Value = serde_json::from_slice(&std::fs::read(file(root, id)).ok()?).ok()?;
    Some(Chat {
        id: v["id"].as_str()?.to_string(),
        title: v["title"].as_str().unwrap_or_default().to_string(),
        created: v["created"].as_i64().unwrap_or(0),
        updated: v["updated"].as_i64().unwrap_or(0),
        messages: v["messages"].as_array().cloned().unwrap_or_default(),
    })
}

/// The project's chats, newest first: id, title, last changed.
pub fn list(root: &Path) -> Vec<(String, String, i64)> {
    let mut out: Vec<(String, String, i64)> = std::fs::read_dir(dir(root))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let id = name.strip_suffix(".json")?.to_string();
            let c = load(root, &id)?;
            Some((c.id, c.title, c.updated))
        })
        .collect();
    out.sort_by(|a, b| b.2.cmp(&a.2).then(b.0.cmp(&a.0)));
    out
}

pub fn delete(root: &Path, id: &str) -> Result<(), String> {
    std::fs::remove_file(file(root, id)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn chats_are_kept_with_the_project_newest_first() {
        let root = std::env::temp_dir().join(format!("oc-chats-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut a = Chat::new(100);
        a.messages.push(json!({"role": "user", "content": [{"type": "text", "text": "What does   the Q3 report say about revenue in the north and south regions this year?"}]}));
        a.titled();
        assert_eq!(a.title.chars().count(), 58);
        assert!(a.title.ends_with('…') && !a.title.contains("  "));
        save(&root, &a).unwrap();
        let mut b = Chat::new(200);
        b.updated = 200;
        b.messages.push(json!({"role": "user", "content": "Short"}));
        b.titled();
        save(&root, &b).unwrap();
        assert_eq!(load(&root, &a.id).unwrap(), a);
        let ids: Vec<String> = list(&root).into_iter().map(|(id, ..)| id).collect();
        assert_eq!(ids, vec![b.id.clone(), a.id.clone()]);
        assert!(root.join(".officecraft/chats").is_dir());
        delete(&root, &b.id).unwrap();
        assert_eq!(list(&root).len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_typed_text_counts_as_what_the_person_said() {
        assert_eq!(user_text(&json!({"role": "user", "content": [{"type": "tool_result", "content": "x"}]})), "");
        assert_eq!(user_text(&json!({"role": "user", "content": "hi"})), "hi");
    }
}
