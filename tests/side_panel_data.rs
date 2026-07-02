#![allow(dead_code)]

#[path = "../src/panel_data.rs"]
mod panel_data;

use panel_data::{
    ArtifactFacts, ArtifactFilter, ArtifactKind, ConversationFacts, PanelChatMode, ProjectFacts,
    filter_artifacts, project_tree,
};

fn conversation(id: usize, title: &str, mode: PanelChatMode) -> ConversationFacts {
    ConversationFacts {
        id,
        title: title.to_string(),
        mode,
        message_count: 1,
        pinned: false,
        pending: false,
        project_id: None,
        branch_source_title: None,
    }
}

#[test]
fn project_tree_places_conversations_under_their_project() {
    let projects = vec![
        ProjectFacts {
            id: 10,
            name: "Launch".to_string(),
        },
        ProjectFacts {
            id: 20,
            name: "Empty".to_string(),
        },
    ];
    let mut launch_chat = conversation(1, "Pinned chat", PanelChatMode::Chat);
    launch_chat.project_id = Some(10);
    launch_chat.pinned = true;
    let mut launch_code = conversation(2, "Code fix", PanelChatMode::Code);
    launch_code.project_id = Some(10);
    let unassigned = conversation(3, "Loose chat", PanelChatMode::Cowork);

    let tree = project_tree(&projects, &[launch_chat, launch_code, unassigned]);

    assert_eq!(tree.len(), 2);
    assert_eq!(tree[0].name, "Launch");
    assert_eq!(tree[0].conversations.len(), 2);
    assert_eq!(tree[0].conversations[0].id, 1);
    assert_eq!(tree[0].conversations[0].title, "Pinned chat");
    assert!(tree[0].conversations[0].pinned);
    assert_eq!(tree[1].name, "Empty");
    assert!(tree[1].conversations.is_empty());
}

#[test]
fn artifacts_filter_by_type_and_keep_source_ids() {
    let artifacts = vec![
        ArtifactFacts {
            conversation_id: 1,
            kind: ArtifactKind::Image,
        },
        ArtifactFacts {
            conversation_id: 2,
            kind: ArtifactKind::Image,
        },
        ArtifactFacts {
            conversation_id: 2,
            kind: ArtifactKind::File,
        },
    ];

    assert_eq!(filter_artifacts(&artifacts, ArtifactFilter::All).len(), 3);
    assert_eq!(
        filter_artifacts(&artifacts, ArtifactFilter::Images).len(),
        2
    );
    let files = filter_artifacts(&artifacts, ArtifactFilter::Files);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].conversation_id, 2);
}
