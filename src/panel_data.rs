#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PanelChatMode {
    Chat,
    Cowork,
    Code,
}

impl PanelChatMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            PanelChatMode::Chat => "Chat",
            PanelChatMode::Cowork => "Cowork",
            PanelChatMode::Code => "Code",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectFacts {
    pub(crate) id: usize,
    pub(crate) name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConversationFacts {
    pub(crate) id: usize,
    pub(crate) title: String,
    pub(crate) mode: PanelChatMode,
    pub(crate) message_count: usize,
    pub(crate) pinned: bool,
    pub(crate) pending: bool,
    pub(crate) project_id: Option<usize>,
    pub(crate) branch_source_title: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectConversationRow {
    pub(crate) id: usize,
    pub(crate) title: String,
    pub(crate) detail: String,
    pub(crate) pinned: bool,
    pub(crate) pending: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectTreeNode {
    pub(crate) id: usize,
    pub(crate) name: String,
    pub(crate) conversations: Vec<ProjectConversationRow>,
}

pub(crate) fn project_tree(
    projects: &[ProjectFacts],
    conversations: &[ConversationFacts],
) -> Vec<ProjectTreeNode> {
    projects
        .iter()
        .map(|project| ProjectTreeNode {
            id: project.id,
            name: project.name.clone(),
            conversations: conversations
                .iter()
                .filter(|conversation| conversation.project_id == Some(project.id))
                .map(project_conversation_row)
                .collect(),
        })
        .collect()
}

fn project_conversation_row(conversation: &ConversationFacts) -> ProjectConversationRow {
    ProjectConversationRow {
        id: conversation.id,
        title: conversation.title.clone(),
        detail: conversation_detail(conversation),
        pinned: conversation.pinned,
        pending: conversation.pending,
    }
}

fn conversation_detail(conversation: &ConversationFacts) -> String {
    let mut detail = format!(
        "{} · {} message{}",
        conversation.mode.label(),
        conversation.message_count,
        if conversation.message_count == 1 {
            ""
        } else {
            "s"
        }
    );
    if let Some(source_title) = &conversation.branch_source_title {
        detail = format!("{detail} · Branched from {source_title}");
    }
    detail
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(test)]
#[allow(dead_code)]
pub(crate) enum ArtifactFilter {
    All,
    Images,
    Files,
}

#[cfg(test)]
#[allow(dead_code)]
impl ArtifactFilter {
    pub(crate) const ALL: [ArtifactFilter; 3] = [
        ArtifactFilter::All,
        ArtifactFilter::Images,
        ArtifactFilter::Files,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            ArtifactFilter::All => "All",
            ArtifactFilter::Images => "Images",
            ArtifactFilter::Files => "Files",
        }
    }

    fn matches(self, kind: ArtifactKind) -> bool {
        match self {
            ArtifactFilter::All => true,
            ArtifactFilter::Images => kind == ArtifactKind::Image,
            ArtifactFilter::Files => kind == ArtifactKind::File,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(test)]
#[allow(dead_code)]
pub(crate) enum ArtifactKind {
    Image,
    File,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg(test)]
#[allow(dead_code)]
pub(crate) struct ArtifactFacts {
    pub(crate) conversation_id: usize,
    pub(crate) kind: ArtifactKind,
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn filter_artifacts(
    items: &[ArtifactFacts],
    filter: ArtifactFilter,
) -> Vec<ArtifactFacts> {
    items
        .iter()
        .filter(|item| filter.matches(item.kind))
        .cloned()
        .collect()
}
