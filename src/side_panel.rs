//! Side-dock panels. The Projects panel is static; Artifacts renders a live
//! rollup of images and file-like outputs from every conversation.
use gpui::prelude::FluentBuilder as _;
use gpui::{InteractiveElement as _, StatefulInteractiveElement as _, *};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::{DialogFooter, DialogHeader, DialogTitle},
    dock::{Panel, PanelEvent},
    h_flex,
    input::{Input, InputState},
    notification::Notification,
    popover::{Popover, PopoverState},
    scroll::ScrollableElement as _,
    v_flex,
};
use std::collections::HashSet;

use crate::ClaudeApp;
use crate::chat_view::{
    self, ArtifactHighlightKind, ArtifactHighlightTarget, ImageAttachment, MessageBlock,
};
use crate::menus::menu_item;
use crate::models::{ChatMode, Conversation, Project, current_time_ms};
use crate::panel_data::{
    ConversationFacts, PanelChatMode, ProjectConversationRow, ProjectFacts, ProjectTreeNode,
    project_tree,
};
use crate::theme::{
    border_color, file_chip_bg, hover_bg, sidebar_bg, text_2, text_3, text_color, white_color,
};

pub(crate) const PROJECTS_PANEL_NAME: &str = "ClaudeProjectsPanel";
pub(crate) const ARTIFACTS_PANEL_NAME: &str = "ClaudeArtifactsPanel";

pub(crate) struct SidePanel {
    focus_handle: FocusHandle,
    name: &'static str,
    title: SharedString,
    kind: SidePanelKind,
    artifact_type_filter: ArtifactTypeFilter,
    artifact_conversation_filter: Option<usize>,
    artifact_time_filter: ArtifactTimeFilter,
    collapsed_project_ids: HashSet<usize>,
}

enum SidePanelKind {
    Projects { app: WeakEntity<ClaudeApp> },
    Artifacts { app: WeakEntity<ClaudeApp> },
}

#[derive(Clone)]
struct ArtifactItem {
    conversation_id: usize,
    message_ix: usize,
    target: ArtifactHighlightTarget,
    order: usize,
    title: SharedString,
    detail: SharedString,
    conversation: SharedString,
    file_type: ArtifactFileType,
    source: ArtifactSource,
    created_at_ms: Option<u64>,
    kind: ArtifactPayload,
}

#[derive(Clone)]
enum ArtifactPayload {
    Image(ImageAttachment),
    File,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArtifactFileType {
    Image,
    Document,
    File,
}

impl ArtifactFileType {
    fn label(self) -> &'static str {
        match self {
            ArtifactFileType::Image => "Image",
            ArtifactFileType::Document => "Document",
            ArtifactFileType::File => "File",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArtifactSource {
    Uploaded,
    Generated,
}

impl ArtifactSource {
    fn label(self) -> &'static str {
        match self {
            ArtifactSource::Uploaded => "Uploaded",
            ArtifactSource::Generated => "Generated",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArtifactTypeFilter {
    All,
    Images,
    Documents,
    Files,
}

impl ArtifactTypeFilter {
    const ALL: [ArtifactTypeFilter; 4] = [
        ArtifactTypeFilter::All,
        ArtifactTypeFilter::Images,
        ArtifactTypeFilter::Documents,
        ArtifactTypeFilter::Files,
    ];

    fn label(self) -> &'static str {
        match self {
            ArtifactTypeFilter::All => "All",
            ArtifactTypeFilter::Images => "Images",
            ArtifactTypeFilter::Documents => "Documents",
            ArtifactTypeFilter::Files => "Files",
        }
    }

    fn short_label(self) -> &'static str {
        match self {
            ArtifactTypeFilter::All => "All",
            ArtifactTypeFilter::Images => "Images",
            ArtifactTypeFilter::Documents => "Docs",
            ArtifactTypeFilter::Files => "Files",
        }
    }

    fn matches(self, file_type: ArtifactFileType) -> bool {
        match self {
            ArtifactTypeFilter::All => true,
            ArtifactTypeFilter::Images => file_type == ArtifactFileType::Image,
            ArtifactTypeFilter::Documents => file_type == ArtifactFileType::Document,
            ArtifactTypeFilter::Files => file_type == ArtifactFileType::File,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArtifactTimeFilter {
    All,
    Last24Hours,
    Last7Days,
    Older,
    Unknown,
}

impl ArtifactTimeFilter {
    const ALL: [ArtifactTimeFilter; 5] = [
        ArtifactTimeFilter::All,
        ArtifactTimeFilter::Last24Hours,
        ArtifactTimeFilter::Last7Days,
        ArtifactTimeFilter::Older,
        ArtifactTimeFilter::Unknown,
    ];

    fn label(self) -> &'static str {
        match self {
            ArtifactTimeFilter::All => "All time",
            ArtifactTimeFilter::Last24Hours => "24h",
            ArtifactTimeFilter::Last7Days => "7d",
            ArtifactTimeFilter::Older => "Older",
            ArtifactTimeFilter::Unknown => "Unknown",
        }
    }

    fn short_label(self) -> &'static str {
        match self {
            ArtifactTimeFilter::All => "All",
            ArtifactTimeFilter::Last24Hours => "24h",
            ArtifactTimeFilter::Last7Days => "7d",
            ArtifactTimeFilter::Older => "Older",
            ArtifactTimeFilter::Unknown => "No date",
        }
    }

    fn matches(self, created_at_ms: Option<u64>, now_ms: u64) -> bool {
        const DAY_MS: u64 = 24 * 60 * 60 * 1000;
        match self {
            ArtifactTimeFilter::All => true,
            ArtifactTimeFilter::Last24Hours => {
                created_at_ms.is_some_and(|created| created >= now_ms.saturating_sub(DAY_MS))
            }
            ArtifactTimeFilter::Last7Days => {
                created_at_ms.is_some_and(|created| created >= now_ms.saturating_sub(7 * DAY_MS))
            }
            ArtifactTimeFilter::Older => {
                created_at_ms.is_some_and(|created| created < now_ms.saturating_sub(7 * DAY_MS))
            }
            ArtifactTimeFilter::Unknown => created_at_ms.is_none(),
        }
    }
}

#[derive(Clone)]
struct ArtifactConversationOption {
    id: usize,
    title: SharedString,
    count: usize,
}

fn conversation_title(conversation: &Conversation) -> SharedString {
    if conversation.title.is_empty() {
        "Untitled conversation".into()
    } else {
        conversation.title.clone()
    }
}

fn conversation_mode(conversation: &Conversation) -> ChatMode {
    conversation
        .messages
        .last()
        .map(|message| message.mode)
        .unwrap_or(ChatMode::Chat)
}

fn panel_mode(mode: ChatMode) -> PanelChatMode {
    match mode {
        ChatMode::Chat => PanelChatMode::Chat,
        ChatMode::Cowork => PanelChatMode::Cowork,
        ChatMode::Code => PanelChatMode::Code,
    }
}

fn conversation_facts(conversation: &Conversation) -> ConversationFacts {
    ConversationFacts {
        id: conversation.id,
        title: conversation_title(conversation).to_string(),
        mode: panel_mode(conversation_mode(conversation)),
        message_count: conversation.messages.len(),
        pinned: conversation.pinned,
        pending: conversation.pending,
        project_id: conversation.project_id,
        branch_source_title: conversation
            .branch_origin
            .as_ref()
            .map(|origin| origin.source_title.to_string()),
    }
}

fn project_facts(project: &Project) -> ProjectFacts {
    ProjectFacts {
        id: project.id,
        name: project.name.clone(),
    }
}

impl SidePanel {
    pub(crate) fn projects(
        app: WeakEntity<ClaudeApp>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            name: PROJECTS_PANEL_NAME,
            title: "Projects".into(),
            kind: SidePanelKind::Projects { app },
            artifact_type_filter: ArtifactTypeFilter::All,
            artifact_conversation_filter: None,
            artifact_time_filter: ArtifactTimeFilter::All,
            collapsed_project_ids: HashSet::new(),
        }
    }

    pub(crate) fn artifacts(
        app: WeakEntity<ClaudeApp>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            name: ARTIFACTS_PANEL_NAME,
            title: "Artifacts".into(),
            kind: SidePanelKind::Artifacts { app },
            artifact_type_filter: ArtifactTypeFilter::All,
            artifact_conversation_filter: None,
            artifact_time_filter: ArtifactTimeFilter::All,
            collapsed_project_ids: HashSet::new(),
        }
    }
}

impl Panel for SidePanel {
    fn panel_name(&self) -> &'static str {
        self.name
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.title.clone()
    }
}

impl EventEmitter<PanelEvent> for SidePanel {}

impl Focusable for SidePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SidePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match &self.kind {
            SidePanelKind::Projects { app } => self
                .render_projects(app.clone(), window, cx)
                .into_any_element(),
            SidePanelKind::Artifacts { app } => self
                .render_artifacts(app.clone(), window, cx)
                .into_any_element(),
        };

        v_flex()
            .size_full()
            .gap_0()
            .bg(sidebar_bg())
            .track_focus(&self.focus_handle)
            .child(
                div()
                    .px_4()
                    .pt_4()
                    .pb_3()
                    .text_size(px(13.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(text_color())
                    .child(self.title.clone()),
            )
            .child(content)
    }
}

impl SidePanel {
    fn render_artifacts(
        &self,
        app: WeakEntity<ClaudeApp>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let weak = app.clone();
        let all_artifacts = app
            .upgrade()
            .map(|app| Self::collect_artifacts(&app.read(cx).conversations))
            .unwrap_or_default();
        let now_ms = current_time_ms();
        let artifacts = Self::filter_artifacts(
            &all_artifacts,
            self.artifact_type_filter,
            self.artifact_conversation_filter,
            self.artifact_time_filter,
            now_ms,
        );
        let count = artifacts.len();
        let total_count = all_artifacts.len();

        v_flex()
            .flex_1()
            .min_h_0()
            .gap_0()
            .child(
                h_flex()
                    .px_4()
                    .pb_2()
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(12.)).text_color(text_3()).child(
                        if count == total_count {
                            format!("{count} item{}", if count == 1 { "" } else { "s" })
                        } else {
                            format!("{count} of {total_count} items")
                        },
                    )),
            )
            .child(self.render_artifact_filters(&all_artifacts, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .px_3()
                    .pb_3()
                    .map(|this| {
                        if artifacts.is_empty() {
                            this.child(Self::render_empty_artifacts(total_count > 0))
                        } else {
                            this.child(v_flex().gap_2().children(
                                artifacts.into_iter().enumerate().map(|(ix, artifact)| {
                                    Self::render_artifact_item(ix, artifact, weak.clone(), now_ms)
                                }),
                            ))
                        }
                    }),
            )
    }

    fn render_projects(
        &self,
        app: WeakEntity<ClaudeApp>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let weak = app.clone();
        let (projects, conversations) = app
            .upgrade()
            .map(|app| {
                let app = app.read(cx);
                (app.projects.clone(), app.conversations.clone())
            })
            .unwrap_or_default();
        let project_facts = projects.iter().map(project_facts).collect::<Vec<_>>();
        let facts = conversations
            .iter()
            .map(conversation_facts)
            .collect::<Vec<_>>();
        let tree = project_tree(&project_facts, &facts);
        let project_count = tree.len();
        let mut tree_list = v_flex().gap_1();
        for project in tree {
            tree_list = tree_list.child(self.render_project_tree_node(project, weak.clone(), cx));
        }

        v_flex()
            .flex_1()
            .min_h_0()
            .gap_0()
            .child(
                h_flex()
                    .px_4()
                    .pt_1()
                    .pb_2()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(text_3())
                            .child("PROJECT TREE"),
                    )
                    .child(div().text_size(px(12.)).text_color(text_3()).child(format!(
                        "{} project{}",
                        project_count,
                        if project_count == 1 { "" } else { "s" }
                    )))
                    .child(
                        Button::new("project-panel-new")
                            .ghost()
                            .small()
                            .icon(IconName::Plus)
                            .tooltip("New project")
                            .on_click({
                                let app = app.clone();
                                cx.listener(move |_, _, window, cx| {
                                    Self::open_new_project_dialog(app.clone(), window, cx);
                                })
                            }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .px_3()
                    .pb_3()
                    .map(|this| {
                        if project_count == 0 {
                            this.child(Self::render_empty_projects())
                        } else {
                            this.child(tree_list)
                        }
                    }),
            )
    }

    fn open_new_project_dialog(
        app: WeakEntity<ClaudeApp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Project name"));
        window.open_dialog(cx, move |dialog, _, _| {
            let input = input.clone();
            let app = app.clone();
            dialog.w(px(420.)).p_0().content(move |content, _, cx| {
                content
                    .child(
                        DialogHeader::new()
                            .px_5()
                            .py_4()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(DialogTitle::new().child("New project")),
                    )
                    .child(
                        v_flex().px_5().py_4().gap_2().child(
                            div()
                                .h(px(40.))
                                .px_3()
                                .rounded_md()
                                .border_1()
                                .border_color(border_color())
                                .bg(white_color())
                                .flex()
                                .items_center()
                                .child(
                                    Input::new(&input)
                                        .appearance(false)
                                        .bordered(false)
                                        .focus_bordered(false),
                                ),
                        ),
                    )
                    .child(
                        DialogFooter::new()
                            .px_5()
                            .py_3p5()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .child(Button::new("cancel-new-project").label("Cancel").on_click(
                                |_, window, cx| {
                                    window.close_dialog(cx);
                                },
                            ))
                            .child(
                                Button::new("save-new-project")
                                    .primary()
                                    .label("Create")
                                    .on_click({
                                        let input = input.clone();
                                        let app = app.clone();
                                        move |_, window, cx| {
                                            let name = input.read(cx).value().to_string();
                                            if let Some(app) = app.upgrade() {
                                                let created = app.update(cx, |app, cx| {
                                                    app.create_project(&name, cx).is_some()
                                                });
                                                if created {
                                                    window.close_dialog(cx);
                                                    window.push_notification(
                                                        Notification::success("Project created"),
                                                        cx,
                                                    );
                                                }
                                            }
                                        }
                                    }),
                            ),
                    )
            })
        });
    }

    fn render_project_tree_node(
        &self,
        project: ProjectTreeNode,
        app: WeakEntity<ClaudeApp>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let project_id = project.id;
        let collapsed = self.collapsed_project_ids.contains(&project_id);
        let count = project.conversations.len();

        v_flex()
            .id(("project-tree-node", project_id))
            .rounded_lg()
            .border_1()
            .border_color(border_color())
            .bg(white_color())
            .overflow_hidden()
            .child(
                h_flex()
                    .id(("project-tree-header", project_id))
                    .items_center()
                    .gap_2()
                    .px_2p5()
                    .py_2()
                    .cursor_pointer()
                    .hover(|this| this.bg(hover_bg()))
                    .child(
                        Icon::new(if collapsed {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronDown
                        })
                        .size_3p5()
                        .text_color(text_3()),
                    )
                    .child(Icon::new(IconName::Folder).size_4().text_color(text_2()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(text_color())
                            .child(project.name),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(text_3())
                            .child(count.to_string()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.collapsed_project_ids.contains(&project_id) {
                            this.collapsed_project_ids.remove(&project_id);
                        } else {
                            this.collapsed_project_ids.insert(project_id);
                        }
                        cx.notify();
                    })),
            )
            .when(!collapsed, |this| {
                if project.conversations.is_empty() {
                    this.child(
                        div()
                            .px_9()
                            .pb_3()
                            .text_size(px(12.))
                            .text_color(text_3())
                            .child("No conversations"),
                    )
                } else {
                    this.child(
                        v_flex().pb_1().children(
                            project
                                .conversations
                                .into_iter()
                                .map(|row| Self::render_project_row(row, app.clone())),
                        ),
                    )
                }
            })
    }

    fn filter_artifacts(
        items: &[ArtifactItem],
        type_filter: ArtifactTypeFilter,
        conversation_filter: Option<usize>,
        time_filter: ArtifactTimeFilter,
        now_ms: u64,
    ) -> Vec<ArtifactItem> {
        items
            .iter()
            .filter(|item| type_filter.matches(item.file_type))
            .filter(|item| {
                conversation_filter
                    .map(|conversation_id| item.conversation_id == conversation_id)
                    .unwrap_or(true)
            })
            .filter(|item| time_filter.matches(item.created_at_ms, now_ms))
            .cloned()
            .collect()
    }

    fn extension_label(name: &str) -> Option<String> {
        name.rsplit_once('.').and_then(|(_, extension)| {
            let extension = extension.trim();
            (!extension.is_empty() && extension.len() <= 8).then(|| extension.to_ascii_uppercase())
        })
    }

    fn file_type_from_name(name: &str) -> ArtifactFileType {
        let extension = Self::extension_label(name).map(|extension| extension.to_ascii_lowercase());
        match extension.as_deref() {
            Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg") => {
                ArtifactFileType::Image
            }
            Some("pdf" | "doc" | "docx" | "txt" | "md" | "rtf") => ArtifactFileType::Document,
            _ => ArtifactFileType::File,
        }
    }

    fn artifact_detail(
        file_type: ArtifactFileType,
        name: &str,
        detail: Option<&str>,
    ) -> SharedString {
        let mut parts = vec![file_type.label().to_string()];
        let detail = detail.map(str::trim).filter(|detail| !detail.is_empty());
        if let Some(extension) = Self::extension_label(name)
            && !detail
                .is_some_and(|detail| detail.to_ascii_uppercase().starts_with(extension.as_str()))
        {
            parts.push(extension);
        }
        if let Some(detail) = detail {
            parts.push(detail.to_string());
        }
        parts.join(" · ").into()
    }

    fn generated_file_detail(
        file_type: ArtifactFileType,
        name: &str,
        tool_title: &str,
        step_title: &str,
    ) -> SharedString {
        let mut parts = vec![file_type.label().to_string()];
        if let Some(extension) = Self::extension_label(name) {
            parts.push(extension);
        }
        if !tool_title.trim().is_empty() {
            parts.push(tool_title.to_string());
        }
        if !step_title.trim().is_empty() {
            parts.push(step_title.to_string());
        }
        parts.join(" · ").into()
    }

    fn collect_artifacts(conversations: &[Conversation]) -> Vec<ArtifactItem> {
        let mut items = Vec::new();
        let mut order = 0;
        for conversation in conversations {
            let conversation_title = conversation_title(conversation);

            for (message_ix, message) in conversation.messages.iter().enumerate() {
                for (attachment_ix, attachment) in message.attachments.iter().enumerate() {
                    let file_type = if attachment.is_document() {
                        ArtifactFileType::Document
                    } else {
                        ArtifactFileType::Image
                    };
                    let kind = if file_type == ArtifactFileType::Image {
                        ArtifactPayload::Image(attachment.clone())
                    } else {
                        ArtifactPayload::File
                    };
                    items.push(ArtifactItem {
                        conversation_id: conversation.id,
                        message_ix,
                        target: ArtifactHighlightTarget {
                            message_ix,
                            kind: ArtifactHighlightKind::Attachment { attachment_ix },
                        },
                        order,
                        title: attachment.title.clone(),
                        detail: Self::artifact_detail(
                            file_type,
                            attachment.title.as_ref(),
                            Some(attachment.detail.as_ref()),
                        ),
                        conversation: conversation_title.clone(),
                        file_type,
                        source: ArtifactSource::Uploaded,
                        created_at_ms: message.created_at_ms,
                        kind,
                    });
                    order += 1;
                }

                if let Some(blocks) = &message.blocks {
                    for (block_ix, block) in blocks.iter().enumerate() {
                        match block {
                            MessageBlock::GeneratedImage(image) => {
                                let attachment = ImageAttachment {
                                    title: image.title.clone(),
                                    url: image.url.clone(),
                                    detail: image.size.clone(),
                                    path: None,
                                    parsed_text: None,
                                    parse_error: None,
                                };
                                items.push(ArtifactItem {
                                    conversation_id: conversation.id,
                                    message_ix,
                                    target: ArtifactHighlightTarget {
                                        message_ix,
                                        kind: ArtifactHighlightKind::GeneratedImage { block_ix },
                                    },
                                    order,
                                    title: image.title.clone(),
                                    detail: Self::artifact_detail(
                                        ArtifactFileType::Image,
                                        image.title.as_ref(),
                                        Some(image.size.as_ref()),
                                    ),
                                    conversation: conversation_title.clone(),
                                    file_type: ArtifactFileType::Image,
                                    source: ArtifactSource::Generated,
                                    created_at_ms: message.created_at_ms,
                                    kind: ArtifactPayload::Image(attachment),
                                });
                                order += 1;
                            }
                            MessageBlock::Tool(tool) => {
                                for (step_ix, step) in tool.steps.iter().enumerate() {
                                    if let Some(file_chip) = &step.file_chip {
                                        let file_type =
                                            Self::file_type_from_name(file_chip.as_ref());
                                        items.push(ArtifactItem {
                                            conversation_id: conversation.id,
                                            message_ix,
                                            target: ArtifactHighlightTarget {
                                                message_ix,
                                                kind: ArtifactHighlightKind::ToolFile {
                                                    tool_ix: block_ix,
                                                    step_ix,
                                                },
                                            },
                                            order,
                                            title: file_chip.clone(),
                                            detail: Self::generated_file_detail(
                                                file_type,
                                                file_chip.as_ref(),
                                                tool.title.as_ref(),
                                                step.title.as_ref(),
                                            ),
                                            conversation: conversation_title.clone(),
                                            file_type,
                                            source: ArtifactSource::Generated,
                                            created_at_ms: message.created_at_ms,
                                            kind: ArtifactPayload::File,
                                        });
                                        order += 1;
                                    }
                                }
                            }
                            MessageBlock::Thinking(_) | MessageBlock::Markdown(_) => {}
                        }
                    }
                }
            }
        }
        items.sort_by(|a, b| {
            b.created_at_ms
                .cmp(&a.created_at_ms)
                .then_with(|| b.order.cmp(&a.order))
        });
        items
    }

    fn render_project_row(
        row: ProjectConversationRow,
        app: WeakEntity<ClaudeApp>,
    ) -> impl IntoElement {
        let id = row.id;
        let row_title = row.title.clone();
        let menu_app = app.clone();

        h_flex()
            .id(("project-conversation", id))
            .items_center()
            .gap_2()
            .px_2()
            .py_1p5()
            .rounded_md()
            .cursor_pointer()
            .text_color(text_2())
            .hover(|this| this.bg(hover_bg()).text_color(text_color()))
            .child(
                div()
                    .size_5()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(text_3())
                    .child(
                        Icon::new(if row.pinned {
                            IconName::StarFill
                        } else {
                            IconName::Inbox
                        })
                        .size_3p5(),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .truncate()
                            .child(row.title),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(text_3())
                            .truncate()
                            .child(row.detail),
                    ),
            )
            .when(row.pending, |this| {
                this.child(
                    div()
                        .size_2()
                        .rounded_full()
                        .bg(text_color())
                        .flex_shrink_0(),
                )
            })
            .child(
                Popover::new(format!("project-row-menu-{id}"))
                    .anchor(Anchor::BottomRight)
                    .p_0()
                    .trigger(
                        Button::new(format!("project-row-menu-btn-{id}"))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Ellipsis)
                            .tooltip("Conversation actions"),
                    )
                    .content(move |_, _, cx| {
                        Self::project_row_menu_content(
                            id,
                            row_title.clone().into(),
                            menu_app.clone(),
                            cx,
                        )
                        .into_any_element()
                    }),
            )
            .on_click(move |_, window, cx| {
                if let Some(app) = app.upgrade() {
                    app.update(cx, |app, cx| app.select_conversation(id, window, cx));
                }
            })
    }

    fn project_row_menu_content(
        id: usize,
        title: SharedString,
        app: WeakEntity<ClaudeApp>,
        cx: &mut Context<PopoverState>,
    ) -> Stateful<Div> {
        let weak_open = app.clone();
        let weak_rename = app.clone();
        let weak_remove = app.clone();

        v_flex()
            .id("project-row-menu-content")
            .w(px(220.))
            .py_1()
            .child(
                div()
                    .px_3p5()
                    .py_2()
                    .text_size(px(12.))
                    .text_color(text_3())
                    .truncate()
                    .child(title),
            )
            .child(menu_item(
                "project-row-open",
                IconName::Inbox,
                "Open conversation",
                move |window, cx| {
                    if let Some(app) = weak_open.upgrade() {
                        app.update(cx, |app, cx| app.select_conversation(id, window, cx));
                    }
                },
                cx,
            ))
            .child(menu_item(
                "project-row-rename",
                IconName::Replace,
                "Rename conversation",
                move |window, cx| {
                    if let Some(app) = weak_rename.upgrade() {
                        app.update(cx, |app, cx| app.begin_rename_conversation(id, window, cx));
                    }
                },
                cx,
            ))
            .child(div().h(px(1.)).bg(border_color()).my_1())
            .child(menu_item(
                "project-row-remove",
                IconName::Minus,
                "Remove from project",
                move |window, cx| {
                    if let Some(app) = weak_remove.upgrade() {
                        app.update(cx, |app, cx| {
                            app.remove_conversation_from_project(id, window, cx)
                        });
                    }
                },
                cx,
            ))
    }

    fn render_empty_projects() -> impl IntoElement {
        v_flex()
            .items_center()
            .justify_center()
            .gap_2()
            .px_4()
            .py_10()
            .text_center()
            .child(
                div()
                    .size_9()
                    .rounded_full()
                    .bg(file_chip_bg())
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(text_3())
                    .child(Icon::new(IconName::Folder).size_4()),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(text_color())
                    .child("No projects yet"),
            )
            .child(
                div()
                    .max_w(px(210.))
                    .text_size(px(12.))
                    .line_height(relative(1.45))
                    .text_color(text_3())
                    .child("Use Add to project from a chat title to create one."),
            )
    }

    fn artifact_conversation_options(
        artifacts: &[ArtifactItem],
    ) -> Vec<ArtifactConversationOption> {
        let mut options = Vec::<ArtifactConversationOption>::new();
        for artifact in artifacts {
            if let Some(option) = options
                .iter_mut()
                .find(|option| option.id == artifact.conversation_id)
            {
                option.count += 1;
            } else {
                options.push(ArtifactConversationOption {
                    id: artifact.conversation_id,
                    title: artifact.conversation.clone(),
                    count: 1,
                });
            }
        }
        options
    }

    fn selected_conversation_filter_label(&self, artifacts: &[ArtifactItem]) -> SharedString {
        let Some(selected_id) = self.artifact_conversation_filter else {
            return "All conversations".into();
        };

        Self::artifact_conversation_options(artifacts)
            .into_iter()
            .find(|option| option.id == selected_id)
            .map(|option| option.title)
            .unwrap_or_else(|| "Conversation".into())
    }

    fn render_artifact_filters(
        &self,
        artifacts: &[ArtifactItem],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let conversation_label = self.selected_conversation_filter_label(artifacts);
        let conversation_options = Self::artifact_conversation_options(artifacts);
        let selected_conversation = self.artifact_conversation_filter;
        let panel = cx.entity().downgrade();

        v_flex()
            .px_3()
            .pb_2()
            .gap_1p5()
            .child(
                h_flex()
                    .w_full()
                    .gap_0p5()
                    .rounded_lg()
                    .bg(file_chip_bg())
                    .p_0p5()
                    .children(
                        ArtifactTypeFilter::ALL
                            .into_iter()
                            .map(|filter| self.render_artifact_type_filter_chip(filter, cx)),
                    ),
            )
            .child(
                Popover::new("artifact-conversation-filter")
                    .anchor(Anchor::BottomLeft)
                    .p_0()
                    .trigger(
                        Button::new("artifact-conversation-filter-button")
                            .w_full()
                            .outline()
                            .small()
                            .compact()
                            .icon(IconName::Inbox)
                            .label(conversation_label)
                            .tooltip("Source conversation"),
                    )
                    .content(move |_, _, _| {
                        Self::artifact_conversation_filter_menu(
                            conversation_options.clone(),
                            selected_conversation,
                            panel.clone(),
                        )
                        .into_any_element()
                    }),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_0p5()
                    .rounded_lg()
                    .bg(file_chip_bg())
                    .p_0p5()
                    .children(
                        ArtifactTimeFilter::ALL
                            .into_iter()
                            .map(|filter| self.render_artifact_time_filter_chip(filter, cx)),
                    ),
            )
    }

    fn render_artifact_type_filter_chip(
        &self,
        filter: ArtifactTypeFilter,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let selected = self.artifact_type_filter == filter;
        h_flex()
            .id(format!("artifact-type-filter-{}", filter.label()))
            .h(px(24.))
            .flex_1()
            .min_w_0()
            .items_center()
            .justify_center()
            .rounded_md()
            .border_1()
            .border_color(if selected {
                border_color()
            } else {
                file_chip_bg()
            })
            .cursor_pointer()
            .text_size(px(11.5))
            .text_color(if selected { text_color() } else { text_2() })
            .when(selected, |this| {
                this.bg(white_color()).font_weight(FontWeight::SEMIBOLD)
            })
            .hover(|this| this.bg(white_color()).text_color(text_color()))
            .child(div().truncate().child(filter.short_label()))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.artifact_type_filter = filter;
                cx.notify();
            }))
    }

    fn render_artifact_time_filter_chip(
        &self,
        filter: ArtifactTimeFilter,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let selected = self.artifact_time_filter == filter;
        h_flex()
            .id(format!("artifact-time-filter-{}", filter.label()))
            .h(px(24.))
            .flex_1()
            .min_w_0()
            .items_center()
            .justify_center()
            .rounded_md()
            .border_1()
            .border_color(if selected {
                border_color()
            } else {
                file_chip_bg()
            })
            .cursor_pointer()
            .text_size(px(11.))
            .text_color(if selected { text_color() } else { text_2() })
            .when(selected, |this| {
                this.bg(white_color()).font_weight(FontWeight::SEMIBOLD)
            })
            .hover(|this| this.bg(white_color()).text_color(text_color()))
            .child(div().truncate().child(filter.short_label()))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.artifact_time_filter = filter;
                cx.notify();
            }))
    }

    fn artifact_conversation_filter_menu(
        options: Vec<ArtifactConversationOption>,
        selected: Option<usize>,
        panel: WeakEntity<SidePanel>,
    ) -> impl IntoElement {
        let all_panel = panel.clone();
        let mut content = v_flex()
            .id("artifact-conversation-filter-menu")
            .w(px(260.))
            .max_h(px(340.))
            .overflow_y_scrollbar()
            .py_1()
            .child(Self::artifact_conversation_filter_row(
                "artifact-conversation-all",
                IconName::Inbox,
                "All conversations".into(),
                None,
                selected.is_none(),
                all_panel,
            ));

        for option in options {
            let label = format!("{} ({})", option.title, option.count);
            content = content.child(Self::artifact_conversation_filter_row(
                ("artifact-conversation", option.id),
                if selected == Some(option.id) {
                    IconName::Check
                } else {
                    IconName::Inbox
                },
                label.into(),
                Some(option.id),
                selected == Some(option.id),
                panel.clone(),
            ));
        }

        content
    }

    fn artifact_conversation_filter_row(
        id: impl Into<ElementId>,
        icon: IconName,
        label: SharedString,
        value: Option<usize>,
        selected: bool,
        panel: WeakEntity<SidePanel>,
    ) -> Stateful<Div> {
        h_flex()
            .id(id)
            .px_3p5()
            .py_2p5()
            .gap_2p5()
            .items_center()
            .cursor_pointer()
            .text_size(px(13.))
            .text_color(text_color())
            .hover(|this| this.bg(hover_bg()))
            .when(selected, |this| this.bg(file_chip_bg()))
            .child(Icon::new(icon).size_3p5().text_color(text_2()))
            .child(div().flex_1().min_w_0().truncate().child(label))
            .on_click(move |_, _, cx| {
                if let Some(panel) = panel.upgrade() {
                    panel.update(cx, |panel, cx| {
                        panel.artifact_conversation_filter = value;
                        cx.notify();
                    });
                }
            })
    }

    fn artifact_time_label(created_at_ms: Option<u64>, now_ms: u64) -> String {
        const MINUTE_MS: u64 = 60 * 1000;
        const HOUR_MS: u64 = 60 * MINUTE_MS;
        const DAY_MS: u64 = 24 * HOUR_MS;

        let Some(created_at_ms) = created_at_ms else {
            return "Unknown time".to_string();
        };

        let elapsed = now_ms.saturating_sub(created_at_ms);
        if elapsed < MINUTE_MS {
            "Just now".to_string()
        } else if elapsed < HOUR_MS {
            format!("{}m ago", elapsed / MINUTE_MS)
        } else if elapsed < DAY_MS {
            format!("{}h ago", elapsed / HOUR_MS)
        } else {
            format!("{}d ago", elapsed / DAY_MS)
        }
    }

    fn render_empty_artifacts(filtered: bool) -> impl IntoElement {
        v_flex()
            .items_center()
            .justify_center()
            .gap_2()
            .px_4()
            .py_10()
            .text_center()
            .child(
                div()
                    .size_9()
                    .rounded_full()
                    .bg(file_chip_bg())
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(text_3())
                    .child(Icon::new(IconName::File).size_4()),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(text_color())
                    .child(if filtered {
                        "No matching artifacts"
                    } else {
                        "No artifacts yet"
                    }),
            )
            .child(
                div()
                    .max_w(px(210.))
                    .text_size(px(12.))
                    .line_height(relative(1.45))
                    .text_color(text_3())
                    .child(if filtered {
                        "Adjust the filters to see more items."
                    } else {
                        "Images, documents, and generated files appear here."
                    }),
            )
    }

    fn render_artifact_item(
        ix: usize,
        artifact: ArtifactItem,
        app: WeakEntity<ClaudeApp>,
        now_ms: u64,
    ) -> impl IntoElement {
        let conversation_id = artifact.conversation_id;
        let message_ix = artifact.message_ix;
        let target = artifact.target;
        let title = artifact.title.clone();
        let title_for_copy = artifact.title.clone();
        let detail = artifact.detail.clone();
        let meta = format!(
            "{} · {} · Msg {}",
            artifact.source.label(),
            Self::artifact_time_label(artifact.created_at_ms, now_ms),
            message_ix + 1
        );
        let preview = match artifact.kind.clone() {
            ArtifactPayload::Image(image) => Some(image),
            ArtifactPayload::File => None,
        };
        let open_app = app.clone();

        h_flex()
            .id(("artifact-item", ix))
            .gap_2()
            .items_center()
            .p_2()
            .rounded_lg()
            .border_1()
            .border_color(border_color())
            .bg(white_color())
            .cursor_pointer()
            .hover(|this| this.bg(hover_bg()))
            .child(Self::render_artifact_preview(ix, artifact.kind.clone()))
            .child(
                v_flex()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(text_color())
                            .truncate()
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(text_2())
                            .truncate()
                            .child(detail),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(text_3())
                            .truncate()
                            .child(meta),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .flex_shrink_0()
                    .child(
                        Button::new(format!("artifact-copy-{ix}"))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Copy)
                            .tooltip("Copy name")
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    title_for_copy.to_string(),
                                ));
                                window.push_notification(
                                    Notification::info("Artifact name copied"),
                                    cx,
                                );
                            }),
                    )
                    .child(
                        Button::new(format!("artifact-open-conversation-{ix}"))
                            .ghost()
                            .xsmall()
                            .icon(IconName::ExternalLink)
                            .tooltip("Open source message")
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                if let Some(app) = open_app.upgrade() {
                                    app.update(cx, |app, cx| {
                                        app.select_conversation_artifact(
                                            conversation_id,
                                            target,
                                            window,
                                            cx,
                                        )
                                    });
                                }
                            }),
                    )
                    .when_some(preview, |this, preview| {
                        this.child(
                            Button::new(format!("artifact-preview-{ix}"))
                                .ghost()
                                .xsmall()
                                .icon(IconName::GalleryVerticalEnd)
                                .tooltip("Preview image")
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    Self::open_image_preview(preview.clone(), window, cx);
                                }),
                        )
                    }),
            )
            .on_click(move |_, window, cx| {
                if let Some(app) = app.upgrade() {
                    app.update(cx, |app, cx| {
                        app.select_conversation_artifact(conversation_id, target, window, cx)
                    });
                }
            })
    }

    fn render_artifact_preview(ix: usize, kind: ArtifactPayload) -> impl IntoElement {
        match kind {
            ArtifactPayload::Image(image) => {
                let preview = image.clone();
                div()
                    .id(("artifact-image", ix))
                    .size(px(48.))
                    .rounded_md()
                    .overflow_hidden()
                    .border_1()
                    .border_color(border_color())
                    .bg(file_chip_bg())
                    .flex_shrink_0()
                    .cursor_pointer()
                    .child(chat_view::render_attachment_image(&image))
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        Self::open_image_preview(preview.clone(), window, cx);
                    })
                    .into_any_element()
            }
            ArtifactPayload::File => div()
                .id(("artifact-file", ix))
                .size(px(48.))
                .rounded_md()
                .border_1()
                .border_color(border_color())
                .bg(file_chip_bg())
                .flex()
                .items_center()
                .justify_center()
                .flex_shrink_0()
                .text_color(text_2())
                .child(Icon::new(IconName::File).size_4())
                .into_any_element(),
        }
    }

    fn open_image_preview(image: ImageAttachment, window: &mut Window, cx: &mut App) {
        window.open_dialog(cx, move |dialog, window, _| {
            let viewport_size = window.viewport_size();
            let width = if viewport_size.width >= px(760.) {
                px(680.)
            } else {
                (viewport_size.width - px(32.)).max(px(300.))
            };
            let image_height = if viewport_size.height >= px(680.) {
                px(460.)
            } else {
                (viewport_size.height - px(160.)).max(px(240.))
            };
            let title = image.title.clone();
            let detail = image.detail.clone();
            let image = image.clone();

            dialog
                .w(width)
                .margin_top(px(28.))
                .p_0()
                .close_button(false)
                .overlay(true)
                .overlay_closable(true)
                .content(move |content, _, _| {
                    content.gap_0().child(
                        v_flex()
                            .rounded_xl()
                            .overflow_hidden()
                            .bg(white_color())
                            .child(
                                div()
                                    .relative()
                                    .w_full()
                                    .h(image_height)
                                    .bg(file_chip_bg())
                                    .child(chat_view::render_attachment_image_fit(
                                        &image,
                                        ObjectFit::Contain,
                                    ))
                                    .child(
                                        div()
                                            .id("artifact-preview-close")
                                            .absolute()
                                            .top_0()
                                            .right_0()
                                            .m_3()
                                            .size_8()
                                            .rounded_full()
                                            .bg(white_color().opacity(0.92))
                                            .border_1()
                                            .border_color(border_color())
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor_pointer()
                                            .text_color(text_2())
                                            .hover(|this| {
                                                this.bg(white_color()).text_color(text_color())
                                            })
                                            .child(Icon::new(IconName::Close).size_4())
                                            .on_click(|_, window, cx| {
                                                cx.stop_propagation();
                                                window.close_dialog(cx);
                                            }),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .gap_0p5()
                                    .px_4()
                                    .py_3()
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(text_color())
                                            .truncate()
                                            .child(title.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(text_3())
                                            .truncate()
                                            .child(detail.clone()),
                                    ),
                            ),
                    )
                })
        });
    }
}
