//! One accessibility tree for a window that shows engines from other egui contexts
//! (PLAN 8.1; spiked in Phase 2).
//!
//! The window's egui context builds its AccessKit tree; [`Graft`], an egui plugin on that
//! context, appends the shown engine's tree to it after the pass (`output_hook`), hung under a
//! "tab content" node the chrome creates. Node ids are 64-bit hashes of egui ids, so the two
//! trees only collide on egui's fixed root id: the engine's root is renamed ([`ENGINE_ROOT`]),
//! and screen-reader actions aimed at it are renamed back ([`engine_action`]).

use egui::accesskit::{self, NodeId, Role, TreeUpdate};
use std::sync::{Arc, Mutex};

/// The engine root's id in the window's tree (any value no egui hash will produce in practice).
pub const ENGINE_ROOT: NodeId = NodeId(0x00C0_FFEE_7AB5_0001);

/// What the app hands the plugin each frame.
#[derive(Debug, Default)]
pub struct Shared {
    /// The node the engine's tree hangs under (the chrome's tab content).
    pub host: Option<NodeId>,
    /// The shown engine's last tree.
    pub engine: Option<TreeUpdate>,
    /// Whether the window produced a tree (a screen reader or other client is listening):
    /// then the engines are asked for theirs too.
    pub active: bool,
    /// The last merged tree's size and labels (for `--check`).
    pub last_nodes: usize,
    pub last_labels: Vec<String>,
    /// The window's repaint delay at the end of its last frame (for `--check`'s idle report).
    pub window_repaint_delay: Option<std::time::Duration>,
}

/// The plugin. Register with `ctx.add_plugin(Graft::new(shared.clone()))`.
pub struct Graft {
    shared: Arc<Mutex<Shared>>,
}

impl Graft {
    pub fn new(shared: Arc<Mutex<Shared>>) -> Self {
        Graft { shared }
    }
}

impl egui::Plugin for Graft {
    fn debug_name(&self) -> &'static str {
        "craftworks_a11y_graft"
    }

    fn output_hook(&mut self, _ctx: &egui::Context, output: &mut egui::FullOutput) {
        let Ok(mut s) = self.shared.lock() else { return };
        s.active = output.platform_output.accesskit_update.is_some();
        s.window_repaint_delay = output.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay);
        let engine = s.engine.take();
        if let (Some(window), Some(host), Some(engine)) = (output.platform_output.accesskit_update.as_mut(), s.host, engine) {
            graft(window, host, engine);
            s.last_nodes = window.nodes.len();
            s.last_labels = window.nodes.iter().filter_map(|(_, n)| n.label().map(str::to_string)).take(4000).collect();
        }
    }
}

/// Appends `engine`'s nodes to `window` under node `host`. The engine's root becomes
/// [`ENGINE_ROOT`], a group; when the engine has focus, so does the window's tree.
pub fn graft(window: &mut TreeUpdate, host: NodeId, engine: TreeUpdate) {
    let root = engine.tree.as_ref().map(|t| t.root);
    let rename = |id: NodeId| if Some(id) == root { ENGINE_ROOT } else { id };
    let mut engine_has_focus = false;
    for (id, mut node) in engine.nodes {
        if Some(id) == root {
            node.set_role(Role::Group);
            if node.label().is_none() {
                node.set_label("Tab content");
            }
        }
        if id == engine.focus && Some(id) != root {
            engine_has_focus = true;
        }
        window.nodes.push((rename(id), node));
    }
    if let Some((_, h)) = window.nodes.iter_mut().find(|(id, _)| *id == host) {
        h.push_child(ENGINE_ROOT);
    }
    if engine_has_focus && window.focus == window.tree.as_ref().map_or(window.focus, |t| t.root) {
        window.focus = rename(engine.focus);
    }
}

/// A screen reader's action for the window, as the engine should see it: aimed at
/// [`ENGINE_ROOT`] it goes to the engine's real root; other ids are left alone (each context
/// ignores ids it doesn't know).
pub fn engine_action(mut req: accesskit::ActionRequest, engine_root: NodeId) -> accesskit::ActionRequest {
    if req.target_node == ENGINE_ROOT {
        req.target_node = engine_root;
    }
    req
}

#[cfg(test)]
mod tests {
    use super::*;
    use accesskit::{Node, TreeId, TreeInfo};

    fn node(role: Role, label: &str, children: &[u64]) -> Node {
        let mut n = Node::new(role);
        n.set_label(label);
        for c in children {
            n.push_child(NodeId(*c));
        }
        n
    }

    #[test]
    fn the_engine_tree_hangs_under_the_tab_content_node() {
        let root = NodeId(1);
        let mut window = TreeUpdate {
            nodes: vec![
                (root, node(Role::Window, "Craftworks", &[10, 20])),
                (NodeId(10), node(Role::TabList, "Tabs", &[])),
                (NodeId(20), node(Role::Group, "Tab content", &[])),
            ],
            tree: Some(TreeInfo::new(root)),
            tree_id: TreeId::ROOT,
            focus: root,
        };
        // The engine's root has the same id as the window's (egui's fixed root id).
        let engine = TreeUpdate {
            nodes: vec![(root, node(Role::Window, "", &[100])), (NodeId(100), node(Role::Button, "Bold", &[]))],
            tree: Some(TreeInfo::new(root)),
            tree_id: TreeId::ROOT,
            focus: NodeId(100),
        };
        graft(&mut window, NodeId(20), engine);
        let ids: Vec<u64> = window.nodes.iter().map(|(id, _)| id.0).collect();
        assert_eq!(ids.iter().filter(|i| **i == 1).count(), 1, "one root: the engine's was renamed");
        let host = window.nodes.iter().find(|(id, _)| *id == NodeId(20)).map(|(_, n)| n.children().to_vec()).unwrap_or_default();
        assert_eq!(host, vec![ENGINE_ROOT]);
        let er = window.nodes.iter().find(|(id, _)| *id == ENGINE_ROOT).map(|(_, n)| n.clone());
        assert_eq!(er.as_ref().map(|n| n.role()), Some(Role::Group));
        assert_eq!(er.as_ref().map(|n| n.children().to_vec()), Some(vec![NodeId(100)]));
        assert_eq!(window.focus, NodeId(100), "the engine's focused button has the window's focus");
    }

    #[test]
    fn actions_on_the_renamed_root_go_back_to_the_engine_root() {
        let req = |n| accesskit::ActionRequest { action: accesskit::Action::Focus, target_tree: TreeId::ROOT, target_node: n, data: None };
        assert_eq!(engine_action(req(ENGINE_ROOT), NodeId(1)).target_node, NodeId(1));
        assert_eq!(engine_action(req(NodeId(55)), NodeId(1)).target_node, NodeId(55));
    }
}
