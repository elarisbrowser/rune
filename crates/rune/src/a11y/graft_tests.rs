//! Foreign trees grafted under elements: sent once their graft node is, whole to a new client.

use accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};

use crate::{Cx, El, Env, Runtime, col};

const DOC: TreeId = TreeId(accesskit::Uuid::from_u128(0x5eed));

struct Doc {
    grafted: bool,
}

fn view(s: &Doc, _: &mut Cx<Doc>) -> El {
    let doc = crate::el().key("doc").role("document").label("Page").size(200.0, 100.0);
    col().padding(20.0).child(if s.grafted { doc.accessibility_graft(DOC) } else { doc })
}

fn tree(label: &str, whole: bool) -> TreeUpdate {
    let mut root = Node::new(Role::Document);
    root.set_children(vec![NodeId(2)]);
    let mut button = Node::new(Role::Button);
    button.set_label(label);
    TreeUpdate {
        nodes: vec![(NodeId(1), root), (NodeId(2), button)],
        tree: whole.then(|| TreeInfo { root: NodeId(1), toolkit_name: None, toolkit_version: None }),
        tree_id: DOC,
        focus: NodeId(2),
    }
}

#[test]
fn a_grafted_tree_follows_its_graft_node_and_replays_whole_to_a_new_client() {
    let mut rt = Runtime::new(Doc { grafted: false }, view, (300.0, 200.0), Env::new(true, 1.0));
    rt.frame(0.0);
    let mut link = crate::AccessibilityLink::new();
    link.activate();
    link.service(&mut rt, "Doc").expect("activated");
    rt.update(|_, tx| tx.update_accessibility_subtree(tree("Go", true)));
    rt.frame(0.0);
    link.service(&mut rt, "Doc");
    assert!(rt.core_mut().a11y_graft_updates().is_empty(), "no graft node yet");

    rt.update(|s, _| s.grafted = true);
    rt.frame(0.0);
    let update = link.service(&mut rt, "Doc").expect("the graft node appeared");
    let (_, graft) = update.nodes.iter().find(|(_, n)| n.author_id() == Some("doc")).expect("graft node");
    assert_eq!(graft.tree_id(), Some(DOC));
    assert!(graft.children().is_empty());
    assert_eq!(graft.transform().map(|t| t.as_coeffs()[4..].to_vec()), Some(vec![20.0, 20.0]), "placed at its corner");
    let sent = rt.core_mut().a11y_graft_updates();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].tree.is_some() && sent[0].nodes.len() == 2, "the whole tree once its graft node is sent");

    rt.update(|_, tx| tx.update_accessibility_subtree(tree("Stop", false)));
    rt.frame(0.0);
    link.service(&mut rt, "Doc");
    let sent = rt.core_mut().a11y_graft_updates();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].tree.is_none(), "later changes go as pushed");

    drop(link);
    let mut second = crate::AccessibilityLink::new();
    second.activate();
    second.service(&mut rt, "Doc").expect("activated");
    let sent = rt.core_mut().a11y_graft_updates();
    let (_, button) = sent[0].nodes.iter().find(|(id, _)| *id == NodeId(2)).expect("button");
    assert!(sent[0].tree.is_some() && button.label() == Some("Stop"), "a new client gets the latest whole tree");
}

fn focused_view(_: &Doc, _: &mut Cx<Doc>) -> El {
    col().child(
        crate::el().key("doc").role("document").label("Page").size(200.0, 100.0)
            .focusable(true).accessibility_graft(DOC),
    )
}

/// AccessKit panics on focus held by a graft node whose tree it doesn't have yet.
#[test]
fn a_focused_graft_takes_focus_only_once_its_tree_is_sent() {
    let mut rt = Runtime::new(Doc { grafted: true }, focused_view, (300.0, 200.0), Env::new(true, 1.0));
    rt.frame(0.0);
    rt.update(|_, tx| tx.update_accessibility_subtree(tree("Go", true)));
    rt.frame(0.0);
    assert!(rt.accessibility_action("doc", accesskit::Action::Focus, None));
    rt.frame(0.0);
    let mut link = crate::AccessibilityLink::new();
    link.activate();
    let root = link.service(&mut rt, "Doc").expect("activated");
    let (graft_id, _) = root.nodes.iter().find(|(_, n)| n.author_id() == Some("doc")).expect("graft node");
    assert_ne!(root.focus, *graft_id, "focus waits for the tree");
    let sent = rt.core_mut().a11y_graft_updates();
    assert_eq!(sent.len(), 2, "the tree, then the focus moving into it");
    assert_eq!((sent[0].tree_id, sent[1].tree_id), (DOC, TreeId::ROOT));
    assert_eq!(sent[1].focus, *graft_id);
    rt.frame(0.0);
    assert_eq!(link.service(&mut rt, "Doc").map_or(*graft_id, |u| u.focus), *graft_id, "and keeps it");
}

struct Overlay {
    shown: bool,
}

/// A grafted page with a Rune child of its own, such as an IME caret or a hover overlay.
fn overlay_view(s: &Overlay, _: &mut Cx<Overlay>) -> El {
    let mut doc = crate::el().key("doc").role("document").label("Page").size(200.0, 100.0).accessibility_graft(DOC);
    if s.shown {
        doc = doc.child(crate::el().key("caret").role("button").label("Caret").size(4.0, 10.0));
    }
    col().padding(20.0).child(doc)
}

fn labels(update: Option<TreeUpdate>) -> Vec<String> {
    update.map(|u| u.nodes).unwrap_or_default().iter().filter_map(|(_, n)| n.label().map(str::to_owned)).collect()
}

/// A graft node lists none of its own children, so a child Rune sends separately would be left
/// unreachable from the root, which AccessKit rejects. The child stays out of the tree.
#[test]
fn a_child_of_a_graft_node_stays_out_of_the_tree() {
    let mut rt = Runtime::new(Overlay { shown: true }, overlay_view, (300.0, 200.0), Env::new(true, 1.0));
    rt.frame(0.0);
    let mut link = crate::AccessibilityLink::new();
    link.activate();
    assert!(!labels(link.service(&mut rt, "Overlay")).iter().any(|l| l == "Caret"), "present at launch");

    rt.update(|s, _| s.shown = false);
    rt.frame(0.0);
    link.service(&mut rt, "Overlay");
    rt.update(|s, _| s.shown = true);
    rt.frame(0.0);
    assert!(!labels(link.service(&mut rt, "Overlay")).iter().any(|l| l == "Caret"), "added later");
}
