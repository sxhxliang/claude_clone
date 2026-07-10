use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName, Root, Sizable as _, TitleBar, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::{DialogFooter, DialogHeader, DialogTitle},
    dock::{
        DockArea, DockAreaState, DockEvent, DockItem, DockPlacement, Panel, PanelInfo, PanelStyle,
        PanelView, TabPanel, register_panel,
    },
    h_flex,
    input::{Input, InputEvent, InputState},
    notification::Notification,
    scroll::ScrollableElement as _,
    v_flex,
};
use gpui_component_assets::Assets;
use gpui_updater::{EngineConfig, GitHubSource, Version};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

rust_i18n::i18n!("locales", fallback = "en");

#[macro_export]
macro_rules! tr {
    ($($tokens:tt)*) => {
        gpui::SharedString::from(rust_i18n::t!($($tokens)*).into_owned())
    };
}

mod app_updater;
mod chat_view;
mod conversation_panel;
mod dialogs;
mod document_parser;
mod export;
mod genai_backend;
mod i18n;
mod mcp_backend;
mod menus;
mod mock_backend;
mod models;
mod panel_data;
mod provider_settings;
mod search_dialog;
mod settings_window;
mod side_panel;
mod sidebar;
mod store;
mod system_file;
mod theme;
mod titlebar;
use app_updater::{UpdateStatus, Updater};
use chat_view::ArtifactHighlightTarget;
use conversation_panel::ConversationPanel;
use dialogs::static_row;
use genai_backend::ChatRoute;
use models::{
    AppSettings, BranchOrigin, ChatMessage, ChatMode, ChatRole, Conversation,
    ConversationPanelLayout, CurrentModel, PersistedAppSettings, PersistedState, Project, Provider,
    ProviderKind, ProviderModel, current_time_ms,
};
use search_dialog::SearchDialog;
use settings_window::SettingsWindow;
use side_panel::SidePanel;
use sidebar::Sidebar;
use theme::{bg_color, border_color, hover_bg, hover_surface, text_2, text_3, text_color};
use titlebar::TopBar;

const DOCK_LAYOUT_VERSION: usize = 1;
const SAVE_LAYOUT_DEBOUNCE: Duration = Duration::from_millis(750);
const DEFAULT_UPDATE_REPO_OWNER: &str = "sxhxliang";
const DEFAULT_UPDATE_REPO_NAME: &str = "claude_clone";

#[derive(Clone, Copy)]
pub(crate) enum ConversationTabCloseScope {
    Others,
    Left,
    Right,
}

pub struct ClaudeApp {
    title_input: Entity<InputState>,
    dock_area: Entity<DockArea>,
    top_bar: Entity<TopBar>,
    sidebar: Entity<Sidebar>,
    updater: Entity<Updater>,
    pub(crate) sidebar_collapsed: bool,
    pub(crate) settings: AppSettings,
    pub(crate) providers: Vec<Provider>,
    next_provider_id: usize,
    pub(crate) current_model_ref: Option<CurrentModel>,
    /// Persistent title for the current chat — initialised from the first user
    /// message and renamable via the topbar dropdown / double-click.
    pub chat_title: SharedString,
    /// When true, the topbar title turns into an inline editable input.
    pub editing_title: bool,
    /// Whether the current chat is pinned (toggleable via the title dropdown).
    pub chat_pinned: bool,
    pub(crate) active_conversation_id: Option<usize>,
    next_conversation_id: usize,
    pub(crate) conversations: Vec<Conversation>,
    pub(crate) projects: Vec<Project>,
    next_project_id: usize,
    conversation_panels: HashMap<usize, Entity<ConversationPanel>>,
    open_conversation_ids: HashSet<usize>,
    active_tab_panel: Option<WeakEntity<TabPanel>>,
    last_layout_state: DockAreaState,
    last_save_error: Option<SharedString>,
    _save_layout_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

struct DockLoadState {
    conversations: Vec<Conversation>,
    conversation_panels: HashMap<usize, Entity<ConversationPanel>>,
}

impl DockLoadState {
    fn new(conversations: Vec<Conversation>) -> Self {
        Self {
            conversations,
            conversation_panels: HashMap::new(),
        }
    }

    fn next_conversation_id(&self) -> usize {
        self.conversations
            .iter()
            .map(|conversation| conversation.id + 1)
            .max()
            .unwrap_or(0)
    }

    fn conversation_for_layout(&mut self, layout: ConversationPanelLayout) -> Conversation {
        if let Some(conversation) = self
            .conversations
            .iter()
            .find(|conversation| conversation.id == layout.conversation_id)
        {
            return conversation.clone();
        }

        let title = if layout.title.trim().is_empty() {
            crate::tr!("conversation.fallback_title", id = layout.conversation_id).to_string()
        } else {
            layout.title
        };
        let conversation = Conversation::empty(layout.conversation_id, title);
        self.conversations.push(conversation.clone());
        conversation
    }
}

struct ProjectPickerDialog {
    app: WeakEntity<ClaudeApp>,
    input: Entity<InputState>,
    selected_project_id: Option<usize>,
    _subscriptions: Vec<Subscription>,
}

impl ProjectPickerDialog {
    fn new(app: WeakEntity<ClaudeApp>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(crate::tr!("project.new_name_placeholder"))
                .auto_grow(1, 1)
        });
        let subscriptions = vec![cx.subscribe_in(&input, window, {
            move |this: &mut ProjectPickerDialog, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.save(window, cx);
                }
            }
        })];

        Self {
            app,
            input,
            selected_project_id: None,
            _subscriptions: subscriptions,
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed_name = self.input.read(cx).value().to_string();
        let typed_name = typed_name.trim();
        let Some(app) = self.app.upgrade() else {
            return;
        };

        let assigned = app.update(cx, |app, cx| {
            let project_id = if typed_name.is_empty() {
                self.selected_project_id
            } else {
                app.create_project(typed_name, cx)
            };

            project_id
                .is_some_and(|project_id| app.assign_active_conversation_to_project(project_id, cx))
        });

        if assigned {
            window.push_notification(Notification::success(crate::tr!("project.added")), cx);
            window.close_dialog(cx);
        } else {
            window.push_notification(
                Notification::info(crate::tr!("project.choose_or_create")),
                cx,
            );
        }
    }
}

impl Render for ProjectPickerDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (projects, active_project_id) = self
            .app
            .upgrade()
            .map(|app| {
                let app = app.read(cx);
                let active_project_id = app
                    .active_conversation_id
                    .and_then(|id| app.conversations.iter().find(|c| c.id == id))
                    .and_then(|conversation| conversation.project_id);
                (app.projects.clone(), active_project_id)
            })
            .unwrap_or_default();
        let selected_project_id = self.selected_project_id.or(active_project_id);

        v_flex()
            .bg(bg_color())
            .child(
                DialogHeader::new()
                    .px_5()
                    .py_3p5()
                    .border_b_1()
                    .border_color(border_color())
                    .child(DialogTitle::new().child(crate::tr!("project.picker_title"))),
            )
            .child(
                v_flex()
                    .p_5()
                    .gap_3()
                    .child(
                        h_flex()
                            .h(px(40.))
                            .items_center()
                            .gap_2()
                            .px_3()
                            .rounded_md()
                            .border_1()
                            .border_color(border_color())
                            .bg(hover_surface())
                            .child(Icon::new(IconName::Folder).small().text_color(text_3()))
                            .child(
                                div().flex_1().child(
                                    Input::new(&self.input)
                                        .appearance(false)
                                        .bordered(false)
                                        .focus_bordered(false)
                                        .cleanable(true),
                                ),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(11.5))
                            .text_color(text_3())
                            .child(crate::tr!("project.title"))
                            .child(crate::tr!("project.total", count = projects.len())),
                    )
                    .child(div().max_h(px(260.)).overflow_y_scrollbar().map(|this| {
                        if projects.is_empty() {
                            this.child(
                                v_flex()
                                    .h(px(150.))
                                    .items_center()
                                    .justify_center()
                                    .gap_2()
                                    .text_center()
                                    .text_color(text_3())
                                    .child(Icon::new(IconName::Folder).size_6())
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .child(crate::tr!("project.create_to_save")),
                                    ),
                            )
                        } else {
                            this.child(v_flex().gap_1().children(projects.into_iter().map(
                                |project| {
                                    let selected = selected_project_id == Some(project.id);
                                    let id = project.id;
                                    h_flex()
                                        .id(("project-picker-row", id))
                                        .items_center()
                                        .gap_2()
                                        .px_2p5()
                                        .py_2()
                                        .rounded_md()
                                        .cursor_pointer()
                                        .text_color(if selected { text_color() } else { text_2() })
                                        .when(selected, |this| {
                                            this.bg(hover_surface())
                                                .border_1()
                                                .border_color(border_color())
                                        })
                                        .when(!selected, |this| {
                                            this.hover(|this| this.bg(hover_bg()))
                                        })
                                        .child(Icon::new(IconName::Folder).small())
                                        .child(
                                            div()
                                                .flex_1()
                                                .truncate()
                                                .text_size(px(13.))
                                                .child(project.name),
                                        )
                                        .when(selected, |this| {
                                            this.child(Icon::new(IconName::Check).small())
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.selected_project_id = Some(id);
                                            cx.notify();
                                        }))
                                },
                            )))
                        }
                    })),
            )
            .child(
                DialogFooter::new()
                    .px_5()
                    .py_3p5()
                    .border_t_1()
                    .border_color(border_color())
                    .child(
                        Button::new("project-picker-cancel")
                            .label(crate::tr!("common.cancel"))
                            .on_click(|_, window, cx| {
                                window.close_dialog(cx);
                            }),
                    )
                    .child(
                        Button::new("project-picker-save")
                            .label(crate::tr!("common.save"))
                            .primary()
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            )
    }
}

impl ClaudeApp {
    fn new_updater(cx: &mut Context<Self>) -> Entity<Updater> {
        let source = GitHubSource::new(
            option_env!("CLAUDE_CLONE_UPDATE_REPO_OWNER").unwrap_or(DEFAULT_UPDATE_REPO_OWNER),
            option_env!("CLAUDE_CLONE_UPDATE_REPO_NAME").unwrap_or(DEFAULT_UPDATE_REPO_NAME),
        )
        .asset_contains(std::env::consts::ARCH)
        .with_checksums("SHA256SUMS");
        let version =
            Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 0, 0));
        cx.new(|cx| Updater::new(source, EngineConfig::new(version), cx))
    }

    fn path_label(path: Option<PathBuf>) -> SharedString {
        path.map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default()
            .into()
    }

    fn plain_message(
        role: ChatRole,
        content: impl Into<SharedString>,
        mode: ChatMode,
    ) -> ChatMessage {
        ChatMessage {
            role,
            content: content.into(),
            thinking: SharedString::default(),
            model: "Sonnet 4.6".into(),
            mode,
            created_at_ms: Some(current_time_ms()),
            token_stats: None,
            attachments: Vec::new(),
            blocks: None,
        }
    }

    fn seed_conversations() -> Vec<Conversation> {
        vec![
            Conversation::new(
                0,
                "Single-file chatbot frontend HTML",
                vec![
                    Self::plain_message(
                        ChatRole::User,
                        "Create a single-file HTML chatbot frontend with a polished message list.",
                        ChatMode::Code,
                    ),
                    Self::plain_message(
                        ChatRole::Ai,
                        "Use one HTML file with inline CSS and a tiny script. Keep state in an array of messages, render the transcript after each send, and pin the composer at the bottom so the flow feels like a real chat surface.",
                        ChatMode::Code,
                    ),
                ],
            ),
            Conversation::new(
                1,
                "Ollama chatbot playground interfa…",
                vec![
                    Self::plain_message(
                        ChatRole::User,
                        "Sketch an Ollama chatbot playground interface.",
                        ChatMode::Chat,
                    ),
                    Self::plain_message(
                        ChatRole::Ai,
                        "Start with a two-pane layout: model controls and generation settings on the left, the active conversation on the right. Add a compact model selector, temperature and context sliders, plus a request log for quick debugging.",
                        ChatMode::Chat,
                    ),
                ],
            ),
            Conversation::new(
                2,
                "Your first chat with Claude",
                vec![
                    Self::plain_message(
                        ChatRole::User,
                        "What can you help me with?",
                        ChatMode::Chat,
                    ),
                    Self::plain_message(
                        ChatRole::Ai,
                        "I can help draft, explain, plan, debug, and reason through tradeoffs. Pick a task and I will keep the answer practical.",
                        ChatMode::Chat,
                    ),
                ],
            ),
        ]
    }

    pub(crate) fn title_from_text(text: &str) -> SharedString {
        let mut snippet: String = text.chars().take(28).collect();
        if text.chars().count() > 28 {
            snippet.push('…');
        }
        snippet.into()
    }

    fn branch_title(source_title: &SharedString, messages: &[ChatMessage]) -> SharedString {
        let base = if source_title.as_ref().trim().is_empty() {
            messages
                .iter()
                .find(|message| message.role == ChatRole::User)
                .map(|message| Self::title_from_text(message.content.as_ref()))
                .unwrap_or_else(|| crate::tr!("conversation.untitled"))
                .to_string()
        } else {
            source_title.to_string()
        };

        if base.starts_with("Branch: ") {
            base.into()
        } else {
            crate::tr!("conversation.branch_prefix", title = base)
        }
    }

    fn conversation_index(&self, id: usize) -> Option<usize> {
        self.conversations.iter().position(|c| c.id == id)
    }

    fn apply_active_conversation_fields(&mut self, conversation: &Conversation) {
        self.chat_title = conversation.title.clone();
        self.editing_title = false;
        self.chat_pinned = conversation.pinned;
    }

    pub(crate) fn upsert_conversation_snapshot(
        &mut self,
        conversation: Conversation,
        cx: &mut Context<Self>,
    ) {
        if let Some(ix) = self.conversation_index(conversation.id) {
            self.conversations[ix] = conversation.clone();
        } else {
            self.conversations.insert(0, conversation.clone());
        }

        if self.active_conversation_id == Some(conversation.id) {
            self.apply_active_conversation_fields(&conversation);
        }
        self.save_state_with_last_layout();
        cx.notify();
    }

    fn activate_conversation_snapshot(
        &mut self,
        conversation: &Conversation,
        cx: &mut Context<Self>,
    ) {
        self.active_conversation_id = Some(conversation.id);
        self.upsert_conversation_snapshot(conversation.clone(), cx);
        self.apply_active_conversation_fields(conversation);
        cx.notify();
    }

    pub(crate) fn activate_conversation_panel(
        &mut self,
        conversation: &Conversation,
        tab_panel: Option<WeakEntity<TabPanel>>,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab_panel) = tab_panel {
            self.active_tab_panel = Some(tab_panel);
        }
        self.activate_conversation_snapshot(conversation, cx);
    }

    pub(crate) fn mark_conversation_panel_closed(&mut self, id: usize, cx: &mut Context<Self>) {
        self.open_conversation_ids.remove(&id);
        if self.active_conversation_id == Some(id) {
            self.active_tab_panel = None;
        }
        cx.notify();
    }

    fn tab_panel_contains_focus(tab_panel: &Entity<TabPanel>, window: &Window, cx: &App) -> bool {
        let focus_handle = tab_panel.read(cx).focus_handle(cx);
        focus_handle.is_focused(window) || focus_handle.contains_focused(window, cx)
    }

    /// The live, in-tree tab panels that currently host open conversations.
    ///
    /// We derive targets from the open panels' own `tab_panel` refs rather than
    /// walking the static center `DockItem` tree, which goes stale once tab
    /// panels self-remove on close.
    fn open_tab_panels(&self, cx: &App) -> Vec<Entity<TabPanel>> {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for id in &self.open_conversation_ids {
            if let Some(tab_panel) = self
                .conversation_panels
                .get(id)
                .and_then(|panel| panel.read(cx).tab_panel.clone())
                .and_then(|weak| weak.upgrade())
            {
                if seen.insert(tab_panel.entity_id()) {
                    result.push(tab_panel);
                }
            }
        }
        result
    }

    fn focused_tab_panel(&self, window: &Window, cx: &App) -> Option<Entity<TabPanel>> {
        self.open_tab_panels(cx)
            .into_iter()
            .find(|tab_panel| Self::tab_panel_contains_focus(tab_panel, window, cx))
    }

    fn first_tab_panel(&self, cx: &App) -> Option<Entity<TabPanel>> {
        self.open_tab_panels(cx).into_iter().next()
    }

    fn contains_tab_panel(&self, target: &Entity<TabPanel>, cx: &App) -> bool {
        self.open_tab_panels(cx)
            .iter()
            .any(|tab_panel| tab_panel == target)
    }

    fn target_tab_panel(&self, window: &Window, cx: &App) -> Option<Entity<TabPanel>> {
        if let Some(tab_panel) = self.focused_tab_panel(window, cx) {
            return Some(tab_panel);
        }

        if let Some(tab_panel) = self
            .active_tab_panel
            .as_ref()
            .and_then(|tab_panel| tab_panel.upgrade())
        {
            if self.contains_tab_panel(&tab_panel, cx) {
                return Some(tab_panel);
            }
        }

        self.first_tab_panel(cx)
    }

    fn ensure_conversation_panel(
        &mut self,
        id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<ConversationPanel>> {
        if let Some(panel) = self.conversation_panels.get(&id) {
            return Some(panel.clone());
        }

        let conversation = self
            .conversation_index(id)
            .map(|ix| self.conversations[ix].clone())?;
        let app = cx.entity().downgrade();
        let panel = cx.new(move |cx| ConversationPanel::new(conversation, app, window, cx));
        self.conversation_panels.insert(id, panel.clone());
        Some(panel)
    }

    fn add_panel_to_center_dock(
        &mut self,
        panel: Entity<ConversationPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = panel.read(cx).id;
        if self.open_conversation_ids.contains(&id) {
            return;
        }

        let rebuild_center = self.open_conversation_ids.is_empty();
        let target_tab_panel = if rebuild_center {
            None
        } else {
            self.target_tab_panel(window, cx)
        };
        let panel_entity = panel.clone();
        let panel_view: Arc<dyn PanelView> = Arc::new(panel);
        let weak_dock_area = self.dock_area.downgrade();

        self.dock_area.update(cx, |dock_area, cx| {
            if rebuild_center {
                dock_area.set_center(
                    DockItem::tabs(vec![panel_view.clone()], &weak_dock_area, window, cx),
                    window,
                    cx,
                );
            } else if let Some(tab_panel) = target_tab_panel.clone() {
                tab_panel.update(cx, |tab_panel, cx| {
                    tab_panel.add_panel(panel_view.clone(), window, cx);
                });
            } else {
                dock_area.set_center(
                    DockItem::tabs(vec![panel_view.clone()], &weak_dock_area, window, cx),
                    window,
                    cx,
                );
            }
        });
        self.active_tab_panel = target_tab_panel
            .map(|tab_panel| tab_panel.downgrade())
            .or_else(|| panel_entity.read(cx).tab_panel.clone());
        self.open_conversation_ids.insert(id);
    }

    pub(crate) fn conversation_tab_close_targets(
        &self,
        id: usize,
        scope: ConversationTabCloseScope,
        cx: &App,
    ) -> Vec<usize> {
        let Some(tab_panel) = self
            .conversation_panels
            .get(&id)
            .and_then(|panel| panel.read(cx).tab_panel.clone())
            .and_then(|tab_panel| tab_panel.upgrade())
        else {
            return Vec::new();
        };

        let state = tab_panel.read(cx).dump(cx);
        let Some(current_ix) = state.children.iter().position(|child| {
            Self::conversation_panel_layout(&child.info)
                .is_some_and(|layout| layout.conversation_id == id)
        }) else {
            return Vec::new();
        };

        let left = state.children[..current_ix]
            .iter()
            .filter_map(|child| Self::conversation_panel_layout(&child.info))
            .map(|layout| layout.conversation_id);
        let right = state.children[current_ix + 1..]
            .iter()
            .filter_map(|child| Self::conversation_panel_layout(&child.info))
            .map(|layout| layout.conversation_id);

        match scope {
            ConversationTabCloseScope::Others => right.chain(left.rev()).collect(),
            ConversationTabCloseScope::Left => left.rev().collect(),
            ConversationTabCloseScope::Right => right.collect(),
        }
    }

    pub(crate) fn close_conversation_tabs(
        &mut self,
        id: usize,
        scope: ConversationTabCloseScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_active_conversation();

        let target_ids = self.conversation_tab_close_targets(id, scope, cx);
        if target_ids.is_empty() {
            return;
        }

        let panel_views = target_ids
            .iter()
            .filter_map(|target_id| {
                self.open_conversation_ids.remove(target_id);
                self.conversation_panels
                    .get(target_id)
                    .cloned()
                    .map(|panel| Arc::new(panel) as Arc<dyn PanelView>)
            })
            .collect::<Vec<_>>();

        if panel_views.is_empty() {
            return;
        }

        self.dock_area.update(cx, |dock_area, cx| {
            for panel_view in panel_views {
                dock_area.remove_panel_from_all_docks(panel_view, window, cx);
            }
        });
        self.save_state(cx);
        cx.notify();
    }

    fn open_conversation_panel(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.ensure_conversation_panel(id, window, cx) else {
            window.push_notification(Notification::info(crate::tr!("conversation.not_found")), cx);
            return;
        };

        let (snapshot, tab_panel) = {
            let panel = panel.read(cx);
            (panel.snapshot(), panel.tab_panel.clone())
        };
        self.activate_conversation_panel(&snapshot, tab_panel, cx);

        if self.open_conversation_ids.contains(&id) {
            // Upstream gpui-component does not expose a public API for activating
            // an existing tab by panel view. Keep the conversation state in sync;
            // newly opened conversations are still added and activated below.
            if let Some(tab_panel) = panel.read(cx).tab_panel.clone().and_then(|tp| tp.upgrade()) {
                self.active_tab_panel = Some(tab_panel.downgrade());
            }
        } else {
            self.add_panel_to_center_dock(panel, window, cx);
        }
    }

    pub fn sync_active_conversation(&mut self) {
        if let Some(id) = self.active_conversation_id {
            if let Some(ix) = self.conversation_index(id) {
                let conversation = &mut self.conversations[ix];
                conversation.title = self.chat_title.clone();
                conversation.pinned = self.chat_pinned;
            }
        }
    }

    pub(crate) fn select_conversation(
        &mut self,
        id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_conversation_panel(id, window, cx);
    }

    pub(crate) fn select_conversation_artifact(
        &mut self,
        id: usize,
        target: ArtifactHighlightTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_conversation_panel(id, window, cx);
        if let Some(panel) = self.conversation_panels.get(&id) {
            panel.update(cx, |panel, cx| {
                panel.reveal_artifact(target, window, cx);
            });
        }
    }

    pub(crate) fn set_conversation_pinned(
        &mut self,
        id: usize,
        pinned: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_active_conversation();
        let Some(ix) = self.conversation_index(id) else {
            return;
        };

        self.conversations[ix].pinned = pinned;
        if self.active_conversation_id == Some(id) {
            self.chat_pinned = pinned;
        }
        self.save_state(cx);
        window.push_notification(
            Notification::info(if pinned {
                crate::tr!("menu.pinned")
            } else {
                crate::tr!("menu.unpinned")
            }),
            cx,
        );
        cx.notify();
    }

    pub(crate) fn begin_rename_conversation(
        &mut self,
        id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_conversation(id, window, cx);
        self.begin_rename(window, cx);
    }

    pub(crate) fn open_project_picker_for_conversation(
        &mut self,
        id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_conversation(id, window, cx);
        self.open_project_picker(window, cx);
    }

    pub(crate) fn remove_conversation_from_project(
        &mut self,
        id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_active_conversation();
        let Some(ix) = self.conversation_index(id) else {
            return;
        };

        self.conversations[ix].project_id = None;
        if let Some(panel) = self.conversation_panels.get(&id) {
            panel.update(cx, |panel, cx| panel.set_project_id(None, cx));
        }
        self.save_state(cx);
        window.push_notification(Notification::info(crate::tr!("project.removed")), cx);
        cx.notify();
    }

    fn clear_current_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.active_conversation_id = None;
        self.active_tab_panel = None;
        self.chat_title = SharedString::default();
        self.editing_title = false;
        self.chat_pinned = false;
        self.title_input
            .update(cx, |s, cx| s.set_value("", window, cx));
    }

    fn new_conversation_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_active_conversation();

        let id = self.next_conversation_id;
        self.next_conversation_id += 1;
        let conversation = Conversation::empty(id, SharedString::default());
        self.conversations.insert(0, conversation.clone());

        let app = cx.entity().downgrade();
        let panel_conversation = conversation.clone();
        let panel = cx
            .new(|cx| ConversationPanel::new(panel_conversation.clone(), app.clone(), window, cx));
        self.conversation_panels.insert(id, panel.clone());
        self.activate_conversation_snapshot(&conversation, cx);
        self.add_panel_to_center_dock(panel, window, cx);
        self.save_state(cx);
    }

    pub(crate) fn branch_conversation(
        &mut self,
        mut conversation: Conversation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if conversation.messages.is_empty() {
            return;
        }

        let source_id = conversation.id;
        self.sync_active_conversation();
        let source_title = self
            .conversation_index(source_id)
            .map(|ix| self.conversations[ix].title.clone())
            .filter(|title| !title.as_ref().trim().is_empty())
            .unwrap_or_else(|| conversation.title.clone());
        let source_title = if source_title.as_ref().trim().is_empty() {
            conversation
                .messages
                .iter()
                .find(|message| message.role == ChatRole::User)
                .map(|message| Self::title_from_text(message.content.as_ref()))
                .unwrap_or_else(|| crate::tr!("conversation.untitled"))
        } else {
            source_title
        };

        let id = self.next_conversation_id;
        self.next_conversation_id += 1;
        let branch_message_count = conversation.messages.len();
        conversation.id = id;
        conversation.title = Self::branch_title(&source_title, &conversation.messages);
        conversation.pinned = false;
        conversation.pending = false;
        conversation.branch_origin = Some(BranchOrigin {
            source_conversation_id: source_id,
            source_title,
            message_count: branch_message_count,
        });
        conversation
            .cowork_user_expanded
            .resize(conversation.messages.len(), false);
        conversation
            .tool_expanded
            .retain(|(msg_ix, _), _| *msg_ix < conversation.messages.len());

        self.conversations.insert(0, conversation.clone());

        let app = cx.entity().downgrade();
        let panel_conversation = conversation.clone();
        let panel = cx
            .new(|cx| ConversationPanel::new(panel_conversation.clone(), app.clone(), window, cx));
        self.conversation_panels.insert(id, panel.clone());
        self.activate_conversation_snapshot(&conversation, cx);
        self.add_panel_to_center_dock(panel, window, cx);
        self.save_state(cx);
        window.push_notification(Notification::info(crate::tr!("conversation.branched")), cx);
    }

    fn close_conversation_tab(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_active_conversation();

        let Some(ix) = self.conversation_index(id) else {
            return;
        };
        let was_active = self.active_conversation_id == Some(id);
        self.conversations.remove(ix);
        self.open_conversation_ids.remove(&id);
        if let Some(panel) = self.conversation_panels.remove(&id) {
            let panel_view: Arc<dyn PanelView> = Arc::new(panel);
            self.dock_area.update(cx, |dock_area, cx| {
                dock_area.remove_panel_from_all_docks(panel_view, window, cx);
            });
        }

        if was_active {
            let next_id = self
                .conversations
                .get(ix)
                .or_else(|| self.conversations.last())
                .map(|conversation| conversation.id);

            if let Some(next_id) = next_id {
                self.select_conversation(next_id, window, cx);
            } else {
                self.clear_current_chat(window, cx);
                cx.notify();
            }
        } else {
            cx.notify();
        }
        self.save_state(cx);
    }

    pub(crate) fn delete_active_conversation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.active_conversation_id else {
            return;
        };
        self.delete_conversation(id, window, cx);
    }

    pub(crate) fn delete_conversation(
        &mut self,
        id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = self
            .conversations
            .iter()
            .find(|conversation| conversation.id == id)
            .map(|conversation| {
                if conversation.title.is_empty() {
                    crate::tr!("conversation.untitled").to_string()
                } else {
                    conversation.title.to_string()
                }
            })
            .unwrap_or_else(|| crate::tr!("conversation.this_conversation").to_string());
        let app = cx.entity().downgrade();

        window.open_dialog(cx, move |dialog, _, _| {
            let app = app.clone();
            let title = title.clone();
            dialog.w(px(440.)).p_0().content(move |content, _, cx| {
                content
                    .child(
                        DialogHeader::new()
                            .px_5()
                            .py_4()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                DialogTitle::new().child(crate::tr!("conversation.delete_title")),
                            ),
                    )
                    .child(
                        v_flex()
                            .px_5()
                            .py_4()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(text_color())
                                    .child(crate::tr!("conversation.delete_body")),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(text_3())
                                    .truncate()
                                    .child(title.clone()),
                            ),
                    )
                    .child(
                        DialogFooter::new()
                            .px_5()
                            .py_3p5()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .child(
                                Button::new("cancel-delete-conversation")
                                    .label(crate::tr!("common.cancel"))
                                    .on_click(|_, window, cx| {
                                        window.close_dialog(cx);
                                    }),
                            )
                            .child(
                                Button::new("confirm-delete-conversation")
                                    .primary()
                                    .label(crate::tr!("common.delete"))
                                    .on_click({
                                        let app = app.clone();
                                        move |_, window, cx| {
                                            window.close_dialog(cx);
                                            if let Some(app) = app.upgrade() {
                                                app.update(cx, |app, cx| {
                                                    app.delete_conversation_now(id, window, cx);
                                                });
                                            }
                                        }
                                    }),
                            ),
                    )
            })
        });
    }

    fn delete_conversation_now(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.close_conversation_tab(id, window, cx);
        window.push_notification(Notification::info(crate::tr!("conversation.deleted")), cx);
    }

    fn conversation_panel_layout(info: &PanelInfo) -> Option<ConversationPanelLayout> {
        let PanelInfo::Panel(value) = info else {
            return None;
        };

        serde_json::from_value(value.clone()).ok()
    }

    fn register_dock_panels(
        app: WeakEntity<Self>,
        load_state: Rc<RefCell<DockLoadState>>,
        cx: &mut App,
    ) {
        let conversation_app = app.clone();
        let conversation_load_state = load_state.clone();
        register_panel(
            cx,
            conversation_panel::PANEL_NAME,
            move |_, _, info, window, cx| {
                let layout = Self::conversation_panel_layout(info).unwrap_or_else(|| {
                    let conversation_id = conversation_load_state.borrow().next_conversation_id();
                    ConversationPanelLayout {
                        conversation_id,
                        title: String::new(),
                    }
                });
                let id = layout.conversation_id;
                let conversation = conversation_load_state
                    .borrow_mut()
                    .conversation_for_layout(layout);

                let panel = cx.new(|cx| {
                    ConversationPanel::new(
                        conversation.clone(),
                        conversation_app.clone(),
                        window,
                        cx,
                    )
                });
                conversation_load_state
                    .borrow_mut()
                    .conversation_panels
                    .insert(id, panel.clone());
                Box::new(panel)
            },
        );

        register_panel(cx, side_panel::PROJECTS_PANEL_NAME, {
            let projects_app = app.clone();
            move |_, _, _, window, cx| {
                Box::new(cx.new(|cx| SidePanel::projects(projects_app.clone(), window, cx)))
            }
        });

        let artifacts_app = app;
        register_panel(
            cx,
            side_panel::ARTIFACTS_PANEL_NAME,
            move |_, _, _, window, cx| {
                Box::new(cx.new(|cx| SidePanel::artifacts(artifacts_app.clone(), window, cx)))
            },
        );
    }

    fn set_dock_collapsible(
        dock_area: &Entity<DockArea>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        dock_area.update(cx, |dock_area, cx| {
            dock_area.set_dock_collapsible(
                Edges {
                    left: true,
                    bottom: false,
                    right: true,
                    ..Default::default()
                },
                window,
                cx,
            );
        });
    }

    fn install_default_dock_layout(
        dock_area: &Entity<DockArea>,
        conversations: &[Conversation],
        conversation_panels: &mut HashMap<usize, Entity<ConversationPanel>>,
        app: WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for conversation in conversations.iter().cloned() {
            let id = conversation.id;
            if conversation_panels.contains_key(&id) {
                continue;
            }

            let panel_app = app.clone();
            let panel =
                cx.new(|cx| ConversationPanel::new(conversation.clone(), panel_app, window, cx));
            conversation_panels.insert(id, panel);
        }

        let weak_dock_area = dock_area.downgrade();
        let mut panel_items = conversations
            .iter()
            .take(3)
            .filter_map(|conversation| conversation_panels.get(&conversation.id))
            .map(|panel| {
                DockItem::tabs(
                    vec![Arc::new(panel.clone()) as Arc<dyn PanelView>],
                    &weak_dock_area,
                    window,
                    cx,
                )
            })
            .collect::<Vec<_>>();
        let center = match panel_items.len() {
            0 => DockItem::h_split(Vec::new(), &weak_dock_area, window, cx),
            1 => panel_items.remove(0),
            2 => DockItem::h_split(panel_items, &weak_dock_area, window, cx),
            _ => {
                let first = panel_items.remove(0);
                DockItem::h_split(
                    vec![
                        first,
                        DockItem::v_split(panel_items, &weak_dock_area, window, cx).size(px(420.)),
                    ],
                    &weak_dock_area,
                    window,
                    cx,
                )
            }
        };
        let left_dock = DockItem::tab(
            cx.new(|cx| SidePanel::projects(app.clone(), window, cx)),
            &weak_dock_area,
            window,
            cx,
        );
        let right_dock = DockItem::tab(
            cx.new(|cx| SidePanel::artifacts(app.clone(), window, cx)),
            &weak_dock_area,
            window,
            cx,
        );
        dock_area.update(cx, |dock_area, cx| {
            dock_area.set_version(DOCK_LAYOUT_VERSION, window, cx);
            dock_area.set_center(center, window, cx);
            dock_area.set_left_dock(left_dock, Some(px(240.)), false, window, cx);
            dock_area.set_right_dock(right_dock, Some(px(300.)), false, window, cx);
            dock_area.set_dock_collapsible(
                Edges {
                    left: true,
                    bottom: false,
                    right: true,
                    ..Default::default()
                },
                window,
                cx,
            );
        });
    }

    fn load_dock_layout(
        dock_area: &Entity<DockArea>,
        state: DockAreaState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if state.version != Some(DOCK_LAYOUT_VERSION) {
            return false;
        }

        let loaded = dock_area.update(cx, |dock_area, cx| {
            dock_area.load(state, window, cx).is_ok()
        });
        if loaded {
            Self::set_dock_collapsible(dock_area, window, cx);
        }
        loaded
    }

    fn active_conversation_from_layout(
        conversation_panels: &HashMap<usize, Entity<ConversationPanel>>,
        cx: &App,
    ) -> Option<(Conversation, Option<WeakEntity<TabPanel>>)> {
        for panel in conversation_panels.values() {
            let Some(tab_panel) = panel
                .read(cx)
                .tab_panel
                .clone()
                .and_then(|tab_panel| tab_panel.upgrade())
            else {
                continue;
            };

            let is_active = tab_panel
                .read(cx)
                .active_panel(cx)
                .is_some_and(|active| active.panel_id(cx) == panel.entity_id());
            if is_active {
                return Some((panel.read(cx).snapshot(), Some(tab_panel.downgrade())));
            }
        }

        conversation_panels.values().next().map(|panel| {
            let panel = panel.read(cx);
            (panel.snapshot(), panel.tab_panel.clone())
        })
    }

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let PersistedState {
            providers,
            next_provider_id: saved_next_provider_id,
            current,
            settings: saved_settings,
            conversations: saved_conversations,
            projects: saved_projects,
            dock_layout,
        } = store::load();
        let locale = crate::i18n::set_locale(&saved_settings.locale);
        let title_input = cx
            .new(|cx| InputState::new(window, cx).placeholder(crate::tr!("conversation.untitled")));

        let mut subs: Vec<Subscription> = vec![cx.subscribe_in(&title_input, window, {
            move |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { .. } => {
                    let value = this.title_input.read(cx).value().to_string();
                    this.save_title(value, window, cx);
                }
                InputEvent::Blur => {
                    let value = this.title_input.read(cx).value().to_string();
                    this.save_title(value, window, cx);
                }
                _ => {}
            }
        })];
        let persist_conversations = saved_settings.persist_conversations;
        let document_parsing_enabled = saved_settings.document_parsing_enabled;
        let document_ocr_enabled = saved_settings.document_ocr_enabled;
        let mcp_enabled = saved_settings.mcp_enabled;
        let mcp_server_enabled = saved_settings.mcp_server_enabled.clone();
        let config_dir = Self::path_label(store::config_dir());
        let storage_dir = if saved_settings.storage_dir.trim().is_empty() {
            Self::path_label(store::default_storage_dir())
        } else {
            saved_settings.storage_dir.clone().into()
        };
        let initial_conversations = if !persist_conversations || saved_conversations.is_empty() {
            Self::seed_conversations()
        } else {
            saved_conversations
        };
        let projects = if persist_conversations {
            saved_projects
        } else {
            Vec::new()
        };
        let next_provider_id =
            saved_next_provider_id.max(providers.iter().map(|p| p.id + 1).max().unwrap_or(0));
        let current_model_ref = current.filter(|c| {
            providers.iter().any(|p| {
                p.id == c.provider_id && p.models.iter().any(|m| m.id == c.model_id && m.selected)
            })
        });
        let current_model: SharedString = current_model_ref
            .as_ref()
            .map(|c| SharedString::from(c.model_id.clone()))
            .unwrap_or_default();

        let dock_area = cx.new(|cx| {
            DockArea::new(
                "claude-conversation-dock",
                Some(DOCK_LAYOUT_VERSION),
                window,
                cx,
            )
            .panel_style(PanelStyle::TabBar)
        });
        let app = cx.entity().downgrade();

        let load_state = Rc::new(RefCell::new(DockLoadState::new(
            initial_conversations.clone(),
        )));
        Self::register_dock_panels(app.clone(), load_state.clone(), cx);

        let sidebar = cx.new(|_| Sidebar::new(app.clone()));
        let top_bar = cx.new(|_| TopBar::new(app.clone()));
        let updater = Self::new_updater(cx);

        let layout_loaded = if persist_conversations {
            dock_layout
                .map(|layout| Self::load_dock_layout(&dock_area, layout, window, cx))
                .unwrap_or(false)
        } else {
            false
        };
        let (mut conversations, mut conversation_panels) = {
            let mut load_state = load_state.borrow_mut();
            (
                std::mem::take(&mut load_state.conversations),
                std::mem::take(&mut load_state.conversation_panels),
            )
        };

        if !layout_loaded {
            conversations = initial_conversations;
            conversation_panels.clear();
            Self::install_default_dock_layout(
                &dock_area,
                &conversations,
                &mut conversation_panels,
                app.clone(),
                window,
                cx,
            );
        }

        let open_conversation_ids = if layout_loaded {
            conversation_panels.keys().copied().collect::<HashSet<_>>()
        } else {
            conversations
                .iter()
                .take(3)
                .map(|conversation| conversation.id)
                .collect::<HashSet<_>>()
        };

        let active = if layout_loaded {
            if open_conversation_ids.is_empty() {
                None
            } else {
                Self::active_conversation_from_layout(&conversation_panels, cx)
            }
        } else {
            conversations.first().cloned().map(|conversation| {
                let tab_panel = conversation_panels
                    .get(&conversation.id)
                    .and_then(|panel| panel.read(cx).tab_panel.clone());
                (conversation, tab_panel)
            })
        };
        let (active_conversation, active_tab_panel) = match active {
            Some((conversation, tab_panel)) => (Some(conversation), tab_panel),
            None => (None, None),
        };
        let next_conversation_id = conversations
            .iter()
            .map(|conversation| conversation.id + 1)
            .max()
            .unwrap_or(0);
        let next_project_id = projects
            .iter()
            .map(|project| project.id + 1)
            .max()
            .unwrap_or(0);
        let last_layout_state = dock_area.read(cx).dump(cx);

        subs.push(cx.subscribe_in(
            &dock_area,
            window,
            |this, dock_area, event: &DockEvent, window, cx| {
                if matches!(event, DockEvent::LayoutChanged) {
                    this.save_dock_layout(dock_area, window, cx);
                }
            },
        ));
        subs.push(cx.observe(&updater, |this, _, cx| {
            cx.notify();
            this.sidebar.update(cx, |_, cx| cx.notify());
        }));

        cx.on_app_quit({
            let app = app.clone();
            move |_, cx| {
                let Some(app) = app.upgrade() else {
                    return cx.background_executor().spawn(async {});
                };
                let state = app.read(cx).persisted_snapshot(cx);
                cx.background_executor().spawn(async move {
                    let _ = store::save(&state);
                })
            }
        })
        .detach();

        Self {
            title_input,
            dock_area,
            top_bar,
            sidebar,
            updater,
            sidebar_collapsed: false,
            settings: AppSettings {
                current_model,
                locale,
                persist_conversations,
                document_parsing_enabled,
                document_ocr_enabled,
                mcp_enabled,
                mcp_server_enabled,
                storage_dir,
                config_dir,
                ..AppSettings::default()
            },
            providers,
            next_provider_id,
            current_model_ref,
            chat_title: active_conversation
                .as_ref()
                .map(|conversation| conversation.title.clone())
                .unwrap_or_default(),
            editing_title: false,
            chat_pinned: active_conversation
                .as_ref()
                .map(|conversation| conversation.pinned)
                .unwrap_or(false),
            active_conversation_id: active_conversation
                .as_ref()
                .map(|conversation| conversation.id),
            next_conversation_id,
            conversations,
            projects,
            next_project_id,
            conversation_panels,
            open_conversation_ids,
            active_tab_panel,
            last_layout_state,
            last_save_error: None,
            _save_layout_task: None,
            _subscriptions: subs,
        }
    }

    fn project_id_by_name(&self, name: &str) -> Option<usize> {
        let normalized = name.trim().to_ascii_lowercase();
        self.projects
            .iter()
            .find(|project| project.name.trim().to_ascii_lowercase() == normalized)
            .map(|project| project.id)
    }

    pub(crate) fn create_project(&mut self, name: &str, cx: &mut Context<Self>) -> Option<usize> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        if let Some(id) = self.project_id_by_name(name) {
            return Some(id);
        }

        let id = self.next_project_id;
        self.next_project_id += 1;
        self.projects.push(Project::new(id, name));
        self.save_state(cx);
        cx.notify();
        Some(id)
    }

    pub(crate) fn assign_active_conversation_to_project(
        &mut self,
        project_id: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.projects.iter().any(|project| project.id == project_id) {
            return false;
        }
        let Some(conversation_id) = self.active_conversation_id else {
            return false;
        };

        self.sync_active_conversation();
        if let Some(ix) = self.conversation_index(conversation_id) {
            self.conversations[ix].project_id = Some(project_id);
        }
        if let Some(panel) = self.conversation_panels.get(&conversation_id) {
            panel.update(cx, |panel, cx| panel.set_project_id(Some(project_id), cx));
        }
        self.save_state(cx);
        self.dock_area.update(cx, |_, cx| cx.notify());
        cx.notify();
        true
    }

    /// An unused conversation is an empty, non-pending one already open in the
    /// dock — clicking "New chat" should reuse it instead of stacking duplicates.
    fn find_unused_open_conversation(&self) -> Option<usize> {
        self.conversations
            .iter()
            .find(|conversation| {
                conversation.messages.is_empty()
                    && !conversation.pending
                    && self.open_conversation_ids.contains(&conversation.id)
            })
            .map(|conversation| conversation.id)
    }

    pub(crate) fn new_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.find_unused_open_conversation() {
            self.open_conversation_panel(id, window, cx);
            return;
        }
        self.new_conversation_tab(window, cx);
        window.push_notification(
            Notification::info(crate::tr!("conversation.new_started")),
            cx,
        );
    }

    /// Enter inline-rename mode for the chat title.
    pub(crate) fn begin_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.chat_title.clone();
        self.title_input
            .update(cx, |s, cx| s.set_value(current, window, cx));
        self.editing_title = true;
        let input = self.title_input.clone();
        cx.spawn_in(window, async move |_, cx| {
            _ = input.update_in(cx, |s, window, cx| {
                s.focus(window, cx);
            });
        })
        .detach();
        cx.notify();
    }

    /// Persist the edited title and exit edit mode.
    fn save_title(&mut self, text: String, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.editing_title {
            return;
        }
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            self.chat_title = trimmed.to_string().into();
            self.sync_active_conversation();
            self.save_state(cx);
        }
        self.editing_title = false;
        cx.notify();
    }

    pub(crate) fn toast(window: &mut Window, cx: &mut App, msg: impl Into<SharedString>) {
        window.push_notification(Notification::info(msg.into()), cx);
    }

    fn persisted_snapshot_with_layout(&self, dock_layout: DockAreaState) -> PersistedState {
        let persist_conversations = self.settings.persist_conversations;
        PersistedState {
            providers: self.providers.clone(),
            next_provider_id: self.next_provider_id,
            current: self.current_model_ref.clone(),
            settings: PersistedAppSettings {
                locale: self.settings.locale.to_string(),
                persist_conversations,
                document_parsing_enabled: self.settings.document_parsing_enabled,
                document_ocr_enabled: self.settings.document_ocr_enabled,
                mcp_enabled: self.settings.mcp_enabled,
                mcp_server_enabled: self.settings.mcp_server_enabled.clone(),
                storage_dir: self.settings.storage_dir.to_string(),
            },
            conversations: if persist_conversations {
                self.conversations.clone()
            } else {
                Vec::new()
            },
            projects: if persist_conversations {
                self.projects.clone()
            } else {
                Vec::new()
            },
            dock_layout: persist_conversations.then_some(dock_layout),
        }
    }

    fn persisted_snapshot(&self, cx: &App) -> PersistedState {
        self.persisted_snapshot_with_layout(self.dock_area.read(cx).dump(cx))
    }

    pub(crate) fn save_state(&mut self, cx: &App) {
        let dock_layout = self.dock_area.read(cx).dump(cx);
        let snapshot = self.persisted_snapshot_with_layout(dock_layout.clone());
        match store::save(&snapshot) {
            Ok(()) => {
                self.last_save_error = None;
                self.last_layout_state = dock_layout;
            }
            Err(err) => {
                self.last_save_error = Some(err.into());
            }
        }
    }

    fn save_state_with_last_layout(&mut self) {
        let snapshot = self.persisted_snapshot_with_layout(self.last_layout_state.clone());
        match store::save(&snapshot) {
            Ok(()) => self.last_save_error = None,
            Err(err) => self.last_save_error = Some(err.into()),
        }
    }

    pub(crate) fn last_save_error(&self) -> Option<SharedString> {
        self.last_save_error.clone()
    }

    pub(crate) fn set_persist_conversations(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.settings.persist_conversations = enabled;
        self.save_state(cx);
        cx.notify();
    }

    pub(crate) fn set_document_parsing_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.settings.document_parsing_enabled = enabled;
        self.save_state(cx);
        cx.notify();
    }

    pub(crate) fn set_document_ocr_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.settings.document_ocr_enabled = enabled;
        self.save_state(cx);
        cx.notify();
    }

    pub(crate) fn set_locale(&mut self, locale: &str, cx: &mut Context<Self>) {
        let locale = crate::i18n::set_locale(locale);
        if self.settings.locale == locale {
            return;
        }
        self.settings.locale = locale;
        self.save_state(cx);
        self.sidebar.update(cx, |_, cx| cx.notify());
        self.top_bar.update(cx, |_, cx| cx.notify());
        for panel in self.conversation_panels.values() {
            panel.update(cx, |_, cx| cx.notify());
        }
        cx.notify();
    }

    pub(crate) fn set_mcp_server_enabled(
        &mut self,
        server_name: String,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        self.settings
            .mcp_server_enabled
            .insert(server_name, enabled);
        self.save_state(cx);
        cx.notify();
    }

    pub(crate) fn set_storage_dir(
        &mut self,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        store::ensure_writable_dir(&path)?;
        self.settings.storage_dir = path.to_string_lossy().into_owned().into();
        self.save_state(cx);
        if let Some(err) = self.last_save_error.clone() {
            return Err(err.to_string());
        }
        cx.notify();
        Ok(())
    }

    pub(crate) fn reset_storage_dir(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let path = store::default_storage_dir()
            .ok_or_else(|| crate::tr!("errors.default_storage_dir_unavailable").to_string())?;
        store::ensure_writable_dir(&path)?;
        self.settings.storage_dir = Self::path_label(Some(path));
        self.save_state(cx);
        if let Some(err) = self.last_save_error.clone() {
            return Err(err.to_string());
        }
        cx.notify();
        Ok(())
    }

    pub(crate) fn set_config_dir(
        &mut self,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        store::set_config_dir(path.clone())?;
        self.settings.config_dir = path.to_string_lossy().into_owned().into();
        self.save_state(cx);
        if let Some(err) = self.last_save_error.clone() {
            return Err(err.to_string());
        }
        cx.notify();
        Ok(())
    }

    pub(crate) fn reset_config_dir(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        store::reset_config_dir()?;
        self.settings.config_dir = Self::path_label(store::config_dir());
        self.save_state(cx);
        if let Some(err) = self.last_save_error.clone() {
            return Err(err.to_string());
        }
        cx.notify();
        Ok(())
    }

    pub(crate) fn clear_saved_conversations(&mut self) -> Result<(), String> {
        store::clear_saved_conversations(&self.settings.storage_dir)?;
        let snapshot = PersistedState {
            providers: self.providers.clone(),
            next_provider_id: self.next_provider_id,
            current: self.current_model_ref.clone(),
            settings: PersistedAppSettings {
                locale: self.settings.locale.to_string(),
                persist_conversations: self.settings.persist_conversations,
                document_parsing_enabled: self.settings.document_parsing_enabled,
                document_ocr_enabled: self.settings.document_ocr_enabled,
                mcp_enabled: self.settings.mcp_enabled,
                mcp_server_enabled: self.settings.mcp_server_enabled.clone(),
                storage_dir: self.settings.storage_dir.to_string(),
            },
            conversations: Vec::new(),
            projects: Vec::new(),
            dock_layout: None,
        };
        store::save(&snapshot)?;
        self.last_save_error = None;
        Ok(())
    }

    fn save_dock_layout(
        &mut self,
        dock_area: &Entity<DockArea>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dock_area = dock_area.clone();
        self._save_layout_task = Some(cx.spawn_in(window, async move |app, window| {
            window
                .background_executor()
                .timer(SAVE_LAYOUT_DEBOUNCE)
                .await;

            _ = app.update_in(window, move |this, _, cx| {
                let state = dock_area.read(cx).dump(cx);
                if this.last_layout_state == state {
                    return;
                }

                let snapshot = this.persisted_snapshot_with_layout(state.clone());
                match store::save(&snapshot) {
                    Ok(()) => {
                        this.last_save_error = None;
                        this.last_layout_state = state;
                    }
                    Err(err) => {
                        this.last_save_error = Some(err.into());
                    }
                }
            });
        }));
    }

    pub(crate) fn add_provider(
        &mut self,
        kind: ProviderKind,
        api_key: String,
        base_url: String,
        cx: &mut Context<Self>,
    ) -> usize {
        let id = self.next_provider_id;
        self.next_provider_id += 1;
        self.providers.push(Provider {
            id,
            kind,
            enabled: true,
            api_key,
            base_url,
            models: Vec::new(),
        });
        self.save_state(cx);
        cx.notify();
        id
    }

    /// Replace a provider's fetched model list, preserving prior `selected` flags.
    pub(crate) fn set_provider_models(
        &mut self,
        provider_id: usize,
        ids: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(provider) = self.providers.iter_mut().find(|p| p.id == provider_id) {
            let prev: HashMap<String, bool> = provider
                .models
                .iter()
                .map(|model| (model.id.clone(), model.selected))
                .collect();
            provider.models = ids
                .into_iter()
                .map(|id| {
                    let selected = prev.get(&id).copied().unwrap_or(false);
                    ProviderModel { id, selected }
                })
                .collect();
        }
        self.save_state(cx);
        cx.notify();
    }

    pub(crate) fn update_provider(
        &mut self,
        provider_id: usize,
        kind: ProviderKind,
        api_key: String,
        base_url: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(provider) = self.providers.iter_mut().find(|p| p.id == provider_id) {
            provider.kind = kind;
            provider.api_key = api_key;
            provider.base_url = base_url;
        }
        self.save_state(cx);
        cx.notify();
    }

    pub(crate) fn set_provider_enabled(
        &mut self,
        provider_id: usize,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(provider) = self.providers.iter_mut().find(|p| p.id == provider_id) {
            provider.enabled = enabled;
            if !enabled
                && self
                    .current_model_ref
                    .as_ref()
                    .is_some_and(|current| current.provider_id == provider_id)
            {
                self.current_model_ref = None;
                self.settings.current_model = "".into();
            }
        }
        self.save_state(cx);
        cx.notify();
    }

    pub(crate) fn toggle_model_selected(
        &mut self,
        provider_id: usize,
        model_id: &str,
        cx: &mut Context<Self>,
    ) {
        if let Some(provider) = self.providers.iter_mut().find(|p| p.id == provider_id) {
            if let Some(model) = provider.models.iter_mut().find(|m| m.id == model_id) {
                model.selected = !model.selected;
                if !model.selected
                    && self
                        .current_model_ref
                        .as_ref()
                        .is_some_and(|c| c.provider_id == provider_id && c.model_id == model_id)
                {
                    self.current_model_ref = None;
                    self.settings.current_model = "".into();
                }
            }
        }
        self.save_state(cx);
        cx.notify();
    }

    pub(crate) fn set_provider_models_selected(
        &mut self,
        provider_id: usize,
        selected: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(provider) = self.providers.iter_mut().find(|p| p.id == provider_id) {
            for model in &mut provider.models {
                model.selected = selected;
            }
        }
        if !selected
            && self
                .current_model_ref
                .as_ref()
                .is_some_and(|current| current.provider_id == provider_id)
        {
            self.current_model_ref = None;
            self.settings.current_model = "".into();
        }
        self.save_state(cx);
        cx.notify();
    }

    pub(crate) fn delete_provider(&mut self, provider_id: usize, cx: &mut Context<Self>) {
        self.providers.retain(|p| p.id != provider_id);
        if self
            .current_model_ref
            .as_ref()
            .is_some_and(|c| c.provider_id == provider_id)
        {
            self.current_model_ref = None;
            self.settings.current_model = "".into();
        }
        self.save_state(cx);
        cx.notify();
    }

    pub(crate) fn select_model(
        &mut self,
        provider_id: usize,
        model_id: String,
        cx: &mut Context<Self>,
    ) {
        self.settings.current_model = model_id.clone().into();
        self.current_model_ref = Some(CurrentModel {
            provider_id,
            model_id,
        });
        self.save_state(cx);
        cx.notify();
    }

    /// Build the genai route for the currently selected model, if any.
    pub(crate) fn route_for_current(&self) -> Option<ChatRoute> {
        let current = self.current_model_ref.as_ref()?;
        let provider = self
            .providers
            .iter()
            .find(|p| p.id == current.provider_id)?;
        Some(ChatRoute {
            kind: provider.kind,
            model_id: current.model_id.clone(),
            base_url: provider.effective_base_url(),
            api_key: provider.api_key.clone(),
        })
    }

    pub(crate) fn update_menu_label(&self, cx: &App) -> SharedString {
        match self.updater.read(cx).status() {
            UpdateStatus::Idle => crate::tr!("updates.check"),
            UpdateStatus::Checking => crate::tr!("updates.checking"),
            UpdateStatus::UpToDate => crate::tr!("updates.up_to_date"),
            UpdateStatus::Available(version) => {
                crate::tr!("updates.install", version = version.to_string())
            }
            UpdateStatus::Downloading { downloaded, total } => match total {
                Some(total) if *total > 0 => {
                    let pct = (*downloaded as f64 / *total as f64 * 100.0).clamp(0.0, 100.0);
                    crate::tr!("updates.downloading_pct", percent = format!("{pct:.0}"))
                }
                _ => crate::tr!("updates.downloading"),
            },
            UpdateStatus::Installing => crate::tr!("updates.installing"),
            UpdateStatus::Staged(version) => {
                crate::tr!("updates.staged", version = version.to_string())
            }
            UpdateStatus::Errored(_) => crate::tr!("updates.retry"),
        }
    }

    pub(crate) fn handle_update_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let status = self.updater.read(cx).status().clone();
        match status {
            UpdateStatus::Idle | UpdateStatus::UpToDate | UpdateStatus::Errored(_) => {
                self.updater.update(cx, |updater, cx| updater.check(cx));
                window.push_notification(
                    Notification::info(crate::tr!("updates.checking_notice")),
                    cx,
                );
            }
            UpdateStatus::Available(_) => {
                self.updater
                    .update(cx, |updater, cx| updater.download_and_install(cx));
                window.push_notification(
                    Notification::info(crate::tr!("updates.downloading_notice")),
                    cx,
                );
            }
            UpdateStatus::Staged(_) => {
                self.updater.update(cx, |updater, cx| updater.restart(cx));
            }
            UpdateStatus::Checking
            | UpdateStatus::Downloading { .. }
            | UpdateStatus::Installing => {
                window.push_notification(Notification::info(crate::tr!("updates.in_progress")), cx);
            }
        }
    }

    /// Title chip in the topbar: shows the chat title with chevron dropdown,
    /// supports double-click to enter inline rename, and renders an editable
    /// input when `editing_title` is true.
    pub(crate) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.entity().downgrade();
        // Defer: this runs inside a ClaudeApp update, and opening the window
        // renders `SettingsWindow` synchronously, which reads ClaudeApp — that
        // would panic while ClaudeApp is still being updated.
        window.defer(cx, move |_, cx| {
            let opts = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1240.), px(820.)), cx)),
                titlebar: Some(TitleBar::title_bar_options()),
                ..Default::default()
            };
            let _ = cx.open_window(opts, move |window, cx| {
                let title = crate::tr!("settings.title");
                window.set_window_title(title.as_ref());
                let view = cx.new(|cx| SettingsWindow::new(app.clone(), window, cx));
                cx.new(|cx| Root::new(view, window, cx).bg(bg_color()))
            });
        });
    }

    pub(crate) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.entity().downgrade();
        window.defer(cx, move |window, cx| {
            let search = cx.new(|cx| SearchDialog::new(app.clone(), window, cx));
            window.open_dialog(cx, move |dialog, _, _| {
                let search = search.clone();
                dialog
                    .w(px(600.))
                    .bg(bg_color())
                    .p_0()
                    .content(move |content, _, _| content.bg(bg_color()).child(search.clone()))
            });
        });
    }

    pub(crate) fn open_project_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.entity().downgrade();
        window.defer(cx, move |window, cx| {
            let picker = cx.new(|cx| ProjectPickerDialog::new(app.clone(), window, cx));
            window.open_dialog(cx, move |dialog, _, _| {
                let picker = picker.clone();
                dialog
                    .w(px(520.))
                    .bg(bg_color())
                    .p_0()
                    .content(move |content, _, _| content.bg(bg_color()).child(picker.clone()))
            });
        });
    }

    pub(crate) fn open_projects_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.entity().downgrade();
        self.dock_area.update(cx, |dock_area, cx| {
            if dock_area.has_dock(DockPlacement::Left) {
                if !dock_area.is_dock_open(DockPlacement::Left, cx) {
                    dock_area.toggle_dock(DockPlacement::Left, window, cx);
                }
                return;
            }

            let weak_dock_area = cx.entity().downgrade();
            let panel = cx.new(|cx| SidePanel::projects(app.clone(), window, cx));
            let dock = DockItem::tab(panel, &weak_dock_area, window, cx);
            dock_area.set_left_dock(dock, Some(px(280.)), true, window, cx);
            dock_area.set_dock_collapsible(
                Edges {
                    left: true,
                    bottom: false,
                    right: true,
                    ..Default::default()
                },
                window,
                cx,
            );
        });
        cx.notify();
    }

    pub(crate) fn open_artifacts_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.entity().downgrade();
        self.dock_area.update(cx, |dock_area, cx| {
            if dock_area.has_dock(DockPlacement::Right) {
                if !dock_area.is_dock_open(DockPlacement::Right, cx) {
                    dock_area.toggle_dock(DockPlacement::Right, window, cx);
                }
                return;
            }

            let weak_dock_area = cx.entity().downgrade();
            let panel = cx.new(|cx| SidePanel::artifacts(app.clone(), window, cx));
            let dock = DockItem::tab(panel, &weak_dock_area, window, cx);
            dock_area.set_right_dock(dock, Some(px(320.)), true, window, cx);
            dock_area.set_dock_collapsible(
                Edges {
                    left: true,
                    bottom: false,
                    right: true,
                    ..Default::default()
                },
                window,
                cx,
            );
        });
        cx.notify();
    }

    pub(crate) fn open_customize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.open_dialog(cx, move |dialog, _, _| {
            dialog.w(px(520.)).p_0().content(move |content, _, cx| {
                content
                    .child(
                        DialogHeader::new()
                            .px_5()
                            .py_3p5()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(DialogTitle::new().child(crate::tr!("customize.title"))),
                    )
                    .child(v_flex().px_5().py_2().child(static_row(
                        crate::tr!("customize.your_name"),
                        crate::tr!("customize.name_hint"),
                        "Jack Henry",
                    )))
                    .child(
                        DialogFooter::new()
                            .px_5()
                            .py_3p5()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .child(
                                Button::new("close-cust")
                                    .label(crate::tr!("common.done"))
                                    .primary()
                                    .on_click(|_, window, cx| {
                                        window.close_dialog(cx);
                                    }),
                            ),
                    )
            })
        });
    }
}

impl Render for ClaudeApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sheet_layer = Root::render_sheet_layer(window, cx);
        let dialog_layer = Root::render_dialog_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);

        v_flex()
            .size_full()
            .relative()
            .bg(bg_color())
            .text_color(text_color())
            .child(self.top_bar.clone())
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.sidebar.clone())
                    .child(
                        div()
                            .id("conversation-dock")
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .h_full()
                            .child(self.dock_area.clone()),
                    ),
            )
            .children(sheet_layer)
            .children(dialog_layer)
            .children(notification_layer)
    }
}

fn main() {
    let app = gpui_platform::application().with_assets(Assets);

    app.run(move |cx| {
        gpui_component::init(cx);
        search_dialog::init(cx);

        let opts = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1180.), px(820.)), cx)),
            titlebar: Some(TitleBar::title_bar_options()),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(opts, |window, cx| {
                window.set_window_title("Claude Clone");
                let view = cx.new(|cx| ClaudeApp::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx).bg(bg_color()))
            })
            .expect("Failed to open window");
        })
        .detach();
    });
}
