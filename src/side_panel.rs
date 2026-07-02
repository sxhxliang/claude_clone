//! Side-dock panels. The Projects panel is static; Artifacts renders a live
//! rollup of images and file-like outputs from every conversation.
use gpui::prelude::FluentBuilder as _;
use gpui::{InteractiveElement as _, StatefulInteractiveElement as _, *};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _, WindowExt as _,
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
use crate::chat_view::{self, ImageAttachment, MessageBlock};
use crate::menus::menu_item;
use crate::models::{ChatMode, Conversation, Project};
use crate::panel_data::{
    ArtifactFacts, ArtifactFilter, ArtifactKind as ArtifactFactKind, ConversationFacts,
    PanelChatMode, ProjectConversationRow, ProjectFacts, ProjectTreeNode, filter_artifacts,
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
    artifact_filter: ArtifactFilter,
    collapsed_project_ids: HashSet<usize>,
}

enum SidePanelKind {
    Projects { app: WeakEntity<ClaudeApp> },
    Artifacts { app: WeakEntity<ClaudeApp> },
}

#[derive(Clone)]
struct ArtifactItem {
    conversation_id: usize,
    title: SharedString,
    detail: SharedString,
    conversation: SharedString,
    kind: ArtifactPayload,
}

#[derive(Clone)]
enum ArtifactPayload {
    Image(ImageAttachment),
    File,
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
            artifact_filter: ArtifactFilter::All,
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
            artifact_filter: ArtifactFilter::All,
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
        let artifacts = Self::filter_artifacts(&all_artifacts, self.artifact_filter);
        let count = artifacts.len();

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
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(text_3())
                            .child(format!("{count} item{}", if count == 1 { "" } else { "s" })),
                    ),
            )
            .child(self.render_artifact_filter(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .px_3()
                    .pb_3()
                    .map(|this| {
                        if artifacts.is_empty() {
                            this.child(Self::render_empty_artifacts())
                        } else {
                            this.child(v_flex().gap_2().children(
                                artifacts.into_iter().enumerate().map(|(ix, artifact)| {
                                    Self::render_artifact_item(ix, artifact, weak.clone())
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

    fn filter_artifacts(items: &[ArtifactItem], filter: ArtifactFilter) -> Vec<ArtifactItem> {
        let facts = items
            .iter()
            .map(|item| ArtifactFacts {
                conversation_id: item.conversation_id,
                kind: match &item.kind {
                    ArtifactPayload::Image(_) => ArtifactFactKind::Image,
                    ArtifactPayload::File => ArtifactFactKind::File,
                },
            })
            .collect::<Vec<_>>();
        let visible_ids = filter_artifacts(&facts, filter)
            .into_iter()
            .map(|fact| (fact.conversation_id, fact.kind))
            .collect::<Vec<_>>();

        items
            .iter()
            .filter(|item| {
                let kind = match &item.kind {
                    ArtifactPayload::Image(_) => ArtifactFactKind::Image,
                    ArtifactPayload::File => ArtifactFactKind::File,
                };
                visible_ids.contains(&(item.conversation_id, kind))
            })
            .cloned()
            .collect()
    }

    fn collect_artifacts(conversations: &[Conversation]) -> Vec<ArtifactItem> {
        let mut items = Vec::new();
        for conversation in conversations {
            let conversation_title = conversation_title(conversation);

            for message in &conversation.messages {
                for attachment in &message.attachments {
                    if attachment.is_document() {
                        continue;
                    }
                    items.push(ArtifactItem {
                        conversation_id: conversation.id,
                        title: attachment.title.clone(),
                        detail: format!("Uploaded image · {}", attachment.detail).into(),
                        conversation: conversation_title.clone(),
                        kind: ArtifactPayload::Image(attachment.clone()),
                    });
                }

                if let Some(blocks) = &message.blocks {
                    for block in blocks {
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
                                    title: image.title.clone(),
                                    detail: format!("Generated image · {}", image.size).into(),
                                    conversation: conversation_title.clone(),
                                    kind: ArtifactPayload::Image(attachment),
                                });
                            }
                            MessageBlock::Tool(tool) => {
                                for step in &tool.steps {
                                    if let Some(file_chip) = &step.file_chip {
                                        items.push(ArtifactItem {
                                            conversation_id: conversation.id,
                                            title: file_chip.clone(),
                                            detail: format!("{} · {}", tool.title, step.title)
                                                .into(),
                                            conversation: conversation_title.clone(),
                                            kind: ArtifactPayload::File,
                                        });
                                    }
                                }
                            }
                            MessageBlock::Thinking(_) | MessageBlock::Markdown(_) => {}
                        }
                    }
                }
            }
        }
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

    fn render_artifact_filter(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .px_3()
            .pb_2()
            .gap_1()
            .children(ArtifactFilter::ALL.into_iter().map(|filter| {
                Button::new(format!("artifact-filter-{}", filter.label()))
                    .ghost()
                    .small()
                    .selected(self.artifact_filter == filter)
                    .label(filter.label())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.artifact_filter = filter;
                        cx.notify();
                    }))
            }))
    }

    fn render_empty_artifacts() -> impl IntoElement {
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
                    .child("No artifacts yet"),
            )
            .child(
                div()
                    .max_w(px(210.))
                    .text_size(px(12.))
                    .line_height(relative(1.45))
                    .text_color(text_3())
                    .child("Images and files from all conversations appear here."),
            )
    }

    fn render_artifact_item(
        ix: usize,
        artifact: ArtifactItem,
        app: WeakEntity<ClaudeApp>,
    ) -> impl IntoElement {
        let conversation_id = artifact.conversation_id;
        let title = artifact.title.clone();
        let title_for_copy = artifact.title.clone();
        let detail = artifact.detail.clone();
        let conversation = artifact.conversation.clone();
        let preview = match artifact.kind.clone() {
            ArtifactPayload::Image(image) => Some(image),
            ArtifactPayload::File => None,
        };
        let open_app = app.clone();

        h_flex()
            .id(("artifact-item", ix))
            .gap_2p5()
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
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(text_color())
                            .truncate()
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(text_2())
                            .truncate()
                            .child(detail),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(text_3())
                            .truncate()
                            .child(conversation),
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
                            .tooltip("Open source conversation")
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                if let Some(app) = open_app.upgrade() {
                                    app.update(cx, |app, cx| {
                                        app.select_conversation(conversation_id, window, cx)
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
                        app.select_conversation(conversation_id, window, cx)
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
                    .size(px(56.))
                    .rounded_lg()
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
                .size(px(56.))
                .rounded_lg()
                .border_1()
                .border_color(border_color())
                .bg(file_chip_bg())
                .flex()
                .items_center()
                .justify_center()
                .flex_shrink_0()
                .text_color(text_2())
                .child(Icon::new(IconName::File).size_5())
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
