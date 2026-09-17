use std::collections::HashMap;

use gpui_component::dock::{
    DockAreaState, PaneTree, PanelBuilder, PanelId, PanelInfo, PanelSource, PanelState, RootKind,
};
use serde_json::json;

#[derive(Default)]
struct SavedPanels(HashMap<PanelId, PanelState>);

impl PanelBuilder for SavedPanels {
    fn build(&mut self, state: &PanelState, _: &PanelInfo) -> PanelId {
        let id = PanelId::from_u64(self.0.len() as u64 + 1);
        self.0.insert(id, state.clone());
        id
    }
}

impl PanelSource for SavedPanels {
    fn panel_name(&self, _: PanelId) -> &'static str {
        "ClaudeConversationPanel"
    }

    fn is_visible(&self, _: PanelId) -> bool {
        true
    }

    fn dump(&self, id: PanelId) -> PanelState {
        self.0[&id].clone()
    }
}

#[test]
fn legacy_conversation_layout_preserves_tabs_split_sizes_and_side_docks() {
    // The schema written before DockItem/TabPanel became DockLayout/TabGroup.
    let conversation = |id, title| {
        json!({
            "panel_name": "ClaudeConversationPanel",
            "children": [],
            "info": {"panel": {"conversation_id": id, "title": title}}
        })
    };
    let tabs = |children, active_index| {
        json!({
            "panel_name": "TabPanel",
            "children": children,
            "info": {"tabs": {"active_index": active_index}}
        })
    };
    let legacy = json!({
        "version": 1,
        "center": {
            "panel_name": "StackPanel",
            "children": [
                tabs(vec![conversation(7, "First"), conversation(9, "Second")], 1),
                tabs(vec![conversation(12, "Split")], 0)
            ],
            "info": {"stack": {"sizes": [640.0, 420.0], "axis": 0}}
        },
        "left_dock": {
            "panel": tabs(vec![json!({
                "panel_name": "ClaudeProjectsPanel",
                "children": [],
                "info": {"panel": null}
            })], 0),
            "placement": "left",
            "size": 240.0,
            "open": false
        }
    });
    let state: DockAreaState = serde_json::from_value(legacy.clone()).unwrap();
    let mut panels = SavedPanels::default();
    let tree = PaneTree::from_state(&state.center, RootKind::Split, &mut panels);
    assert_eq!(panels.0.len(), 3);
    assert_eq!(tree.to_state(&panels), state.center);
    assert_eq!(serde_json::to_value(state).unwrap(), legacy);
}
