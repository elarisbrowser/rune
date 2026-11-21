//! Foreign accessibility trees an app grafts under its elements (`El::accessibility_graft`), as
//! AccessKit grafts one tree into another: content Rune doesn't lay out, such as a document
//! another engine draws into a surface. The app pushes each tree's updates with
//! `Tx::update_accessibility_subtree`; Rune keeps every tree's latest nodes, so an adapter that
//! starts listening later, or a graft node that comes back, gets the whole tree. A tree's
//! updates go out only once its graft node has, as AccessKit requires. Actions on its nodes go
//! to `App::on_accessibility_action`.

use std::collections::HashSet;

use accesskit::{ActionRequest, Affine, Node as AkNode, NodeId as AkId, Rect as AkRect, TreeId, TreeInfo, TreeUpdate};

use super::tree::rect;
use crate::runtime::{HashMap, RuntimeCore};

/// Every grafted tree's latest nodes, the updates not yet sent, and actions for the app.
#[derive(Default)]
pub(crate) struct Grafts {
    trees: HashMap<TreeId, Graft>,
    pending: Vec<TreeUpdate>,
    actions: Vec<ActionRequest>,
}

#[derive(Default)]
struct Graft {
    nodes: HashMap<AkId, AkNode>,
    info: Option<TreeInfo>,
    focus: Option<AkId>,
}

impl Grafts {
    /// Merges `update` into its tree, and queues it to send when `listening`. An update that
    /// carries `TreeInfo` holds the whole tree and replaces what was kept.
    pub(crate) fn push(&mut self, update: TreeUpdate, listening: bool) {
        let graft = self.trees.entry(update.tree_id).or_default();
        if let Some(info) = &update.tree {
            graft.nodes.clear();
            graft.info = Some(info.clone());
        }
        graft.nodes.extend(update.nodes.iter().cloned());
        graft.focus = Some(update.focus);
        if listening { self.pending.push(update); }
    }

    /// Forgets `tree` (`Tx::remove_accessibility_subtree`).
    pub(crate) fn remove(&mut self, tree: TreeId) {
        self.trees.remove(&tree);
        self.pending.retain(|u| u.tree_id != tree);
    }

    pub(crate) fn queue_action(&mut self, request: ActionRequest) { self.actions.push(request); }

    pub(crate) fn take_actions(&mut self) -> Vec<ActionRequest> { std::mem::take(&mut self.actions) }

    /// The whole of `tree` as one update: the nodes its root still reaches.
    fn full(&self, tree: TreeId) -> Option<TreeUpdate> {
        let graft = self.trees.get(&tree)?;
        let info = graft.info.clone()?;
        let mut nodes = Vec::new();
        let mut stack = vec![info.root];
        let mut seen = HashSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) { continue; }
            let Some(node) = graft.nodes.get(&id) else { continue };
            stack.extend(node.children().iter().copied());
            nodes.push((id, node.clone()));
        }
        let focus = graft.focus.filter(|f| seen.contains(f)).unwrap_or(info.root);
        Some(TreeUpdate { nodes, tree: Some(info), tree_id: tree, focus })
    }
}

impl RuntimeCore {
    /// The foreign tree node `i` hosts, when it is a graft node.
    pub(super) fn a11y_graft_tree(&self, i: usize) -> Option<TreeId> {
        self.tree.nodes[i].a11y.more.as_deref().and_then(|m| m.graft)
    }

    /// Makes node `i` the graft node of its foreign tree, when it hosts one: its tree is its
    /// content, placed from its top-left corner. Returns whether it did.
    pub(super) fn a11y_graft_node(&self, i: usize, node: &mut AkNode) -> bool {
        let Some(tree) = self.a11y_graft_tree(i) else { return false };
        let r = node.bounds().unwrap_or_else(|| rect(self.paint.drawn[i], [0.0; 2]));
        node.set_tree_id(tree);
        node.set_children(Vec::<AkId>::new());
        node.set_transform(Affine::translate((r.x0, r.y0)));
        node.set_bounds(AkRect { x0: 0.0, y0: 0.0, x1: r.width(), y1: r.height() });
        true
    }

    /// Node `i` is a graft node whose tree the adapter doesn't hold yet. AccessKit refuses focus
    /// on such a node; its tree goes out after the root's update, then `a11y_graft_updates`
    /// moves the focus in.
    pub(super) fn a11y_graft_waiting(&self, i: usize) -> bool {
        self.tree.nodes[i].a11y.more.as_deref().and_then(|m| m.graft)
            .is_some_and(|graft| !self.host.a11y().is_some_and(|t| t.grafted.contains(&graft)))
    }

    /// The foreign tree updates to send after this turn's own: a tree whose graft node is new
    /// to the adapter goes whole, the rest as pushed. Trees whose graft nodes aren't sent wait.
    pub(crate) fn a11y_graft_updates(&mut self) -> Vec<TreeUpdate> {
        let pending = std::mem::take(&mut self.host.a11y_grafts.pending);
        let Some(tree) = self.host.a11y() else { return Vec::new() };
        let present = tree.grafts_sent();
        let mut out = Vec::new();
        let mut whole = HashSet::new();
        for &id in &present {
            if !tree.grafted.contains(&id) && let Some(update) = self.host.a11y_grafts.full(id) {
                whole.insert(id);
                out.push(update);
            }
        }
        out.extend(pending.into_iter().filter(|u| present.contains(&u.tree_id) && !whole.contains(&u.tree_id)));
        let Some(tree) = self.host.a11y_mut() else { return out };
        tree.grafted.retain(|id| present.contains(id));
        let arrived = !whole.is_empty();
        tree.grafted.extend(whole);
        // A focused graft node held the focus back until its tree was out: hand it over now.
        if arrived {
            let focus = self.a11y_focus_now();
            if focus != super::tree::ROOT {
                out.push(TreeUpdate { nodes: Vec::new(), tree: None, tree_id: TreeId::ROOT, focus });
            }
        }
        out
    }
}
