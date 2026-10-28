// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The registry as a tree, which is how a unit keeps it: who sits behind
//! whose switch ports, as their topology reports say, in the order a unit
//! lists them.

use std::collections::{HashMap, HashSet};

use crate::wire::DeviceUid;

/// Where one device sits.
#[derive(Clone, Debug, Default)]
struct Node {
    parent: Option<DeviceUid>,
    children: Vec<DeviceUid>,
}

/// Every registered device, this client included, and where it sits.
///
/// Nothing is ever placed back where it was: a device added, moved or left
/// behind by a forgotten parent goes to the end of the list it joins.
#[derive(Clone, Debug, Default)]
pub(crate) struct Tree {
    roots: Vec<DeviceUid>,
    nodes: HashMap<DeviceUid, Node>,
}

impl Tree {
    /// A tree holding only this client, as a unit's registry holds the unit
    /// itself from the start.
    pub(crate) fn new(own: DeviceUid) -> Self {
        let mut tree = Self::default();
        tree.add(own);
        tree
    }

    /// Whether `uid` is registered.
    pub(crate) fn contains(&self, uid: DeviceUid) -> bool {
        self.nodes.contains_key(&uid)
    }

    /// Registers `uid` at the end of the root.
    pub(crate) fn add(&mut self, uid: DeviceUid) {
        if self.nodes.insert(uid, Node::default()).is_none() {
            self.roots.push(uid);
        }
    }

    /// The list `parent`'s children are kept in, the root's for `None`.
    fn list_mut(&mut self, parent: Option<DeviceUid>) -> Option<&mut Vec<DeviceUid>> {
        match parent {
            Some(p) => self.nodes.get_mut(&p).map(|n| &mut n.children),
            None => Some(&mut self.roots),
        }
    }

    /// Moves `uid` to the end of `parent`'s children, or of the root.
    fn place(&mut self, uid: DeviceUid, parent: Option<DeviceUid>) {
        let Some(old) = self.nodes.get(&uid).map(|n| n.parent) else {
            return;
        };
        if let Some(list) = self.list_mut(old) {
            list.retain(|&u| u != uid);
        }
        if let Some(list) = self.list_mut(parent) {
            list.push(uid);
        }
        if let Some(node) = self.nodes.get_mut(&uid) {
            node.parent = parent;
        }
    }

    /// Whether `uid` sits anywhere below `ancestor`.
    fn is_below(&self, uid: DeviceUid, ancestor: DeviceUid) -> bool {
        let mut at = self.nodes.get(&uid).and_then(|n| n.parent);
        // A walk longer than the tree is a cycle, which placing never makes.
        for _ in 0..=self.nodes.len() {
            match at {
                Some(p) if p == ancestor => return true,
                Some(p) => at = self.nodes.get(&p).and_then(|n| n.parent),
                None => return false,
            }
        }
        false
    }

    /// Forgets `uid`, moving its children to the end of its parent's.
    pub(crate) fn remove(&mut self, uid: DeviceUid) {
        let Some(node) = self.nodes.remove(&uid) else {
            return;
        };
        if let Some(list) = self.list_mut(node.parent) {
            list.retain(|&u| u != uid);
        }
        for child in node.children {
            if let Some(n) = self.nodes.get_mut(&child) {
                n.parent = node.parent;
            }
            if let Some(list) = self.list_mut(node.parent) {
                list.push(child);
            }
        }
    }

    /// Applies a topology list `sender` reported, as a unit does with one
    /// that differs from the last.
    ///
    /// Each listed device is moved under the sender unless it is anywhere
    /// below it already. One not registered is skipped, and so is one the
    /// sender sits below, the sender itself included: moving it would take
    /// the sender along into a loop the root no longer reaches. Any direct
    /// child of the sender the list does not name moves to the root.
    pub(crate) fn apply_topology(&mut self, sender: DeviceUid, listed: &[DeviceUid]) {
        if !self.contains(sender) {
            return;
        }
        for &uid in listed {
            // One not registered is no node, which placing skips.
            if uid == sender || self.is_below(uid, sender) || self.is_below(sender, uid) {
                continue;
            }
            self.place(uid, Some(sender));
        }
        let named: HashSet<DeviceUid> = listed.iter().copied().collect();
        let children = self
            .nodes
            .get(&sender)
            .map(|n| n.children.clone())
            .unwrap_or_default();
        for child in children {
            if !named.contains(&child) {
                self.place(child, None);
            }
        }
    }

    /// Every device depth first, a parent before its children, with how deep
    /// it sits: zero at the root.
    pub(crate) fn walk(&self) -> Vec<(DeviceUid, usize)> {
        let mut out = Vec::with_capacity(self.nodes.len());
        let mut stack: Vec<(DeviceUid, usize)> = self.roots.iter().rev().map(|&u| (u, 0)).collect();
        while let Some((uid, depth)) = stack.pop() {
            // Bounded by the tree's size, so a cycle cannot run away.
            if out.len() > self.nodes.len() {
                break;
            }
            out.push((uid, depth));
            if let Some(node) = self.nodes.get(&uid) {
                stack.extend(node.children.iter().rev().map(|&c| (c, depth + 1)));
            }
        }
        out
    }
}
