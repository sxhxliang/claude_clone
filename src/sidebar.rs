//! The left sidebar as its own view: mode tabs, navigation, the recents list,
//! and the user footer. It holds a `WeakEntity<ClaudeApp>`, reads conversation
//! state from the app, and dispatches actions back into it.
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    Icon, IconName, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    popover::{Popover, PopoverState},
    scroll::ScrollableElement as _,
    tab::{Tab, TabBar},
    v_flex,
};
use std::collections::{HashMap, HashSet};

use crate::ClaudeApp;
use crate::menus::{menu_item, user_menu_content};
use crate::models::{ChatMode, Conversation};
use crate::theme::{
    accent, border_color, hover_bg, recent_active_bg, sidebar_bg, text_2, text_3, text_color,
    white_color,
};

/// One conversation row's render data, pulled out of the app before building UI.
#[derive(Clone)]
struct RecentRow {
    id: usize,
    title: SharedString,
    active: bool,
    pinned: bool,
    pending: bool,
    depth: usize,
    has_children: bool,
    collapsed: bool,
}

#[derive(Clone, Copy)]
struct NavItemStyle {
    badge: Option<&'static str>,
    muted: bool,
    selected: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConversationListMode {
    Recent,
    Tree,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SidebarNav {
    Chats,
    Projects,
    Artifacts,
}

pub(crate) struct Sidebar {
    app: WeakEntity<ClaudeApp>,
    list_mode: ConversationListMode,
    active_nav: SidebarNav,
    collapsed_conversation_ids: HashSet<usize>,
}

impl Sidebar {
    pub(crate) fn new(app: WeakEntity<ClaudeApp>) -> Self {
        Self {
            app,
            list_mode: ConversationListMode::Recent,
            active_nav: SidebarNav::Chats,
            collapsed_conversation_ids: HashSet::new(),
        }
    }

    fn nav_item(
        id: &'static str,
        icon: IconName,
        label: &'static str,
        style: NavItemStyle,
        on_click: impl Fn(&mut Sidebar, &gpui::ClickEvent, &mut Window, &mut Context<Sidebar>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        h_flex()
            .id(id)
            .items_center()
            .gap_2p5()
            .px_2p5()
            .py_0p5()
            .rounded_md()
            .cursor_pointer()
            .text_color(if style.selected {
                text_color()
            } else if style.muted {
                text_3()
            } else {
                text_2()
            })
            .text_size(px(13.5))
            .when(style.selected, |this| this.bg(recent_active_bg()))
            .hover(|this| this.bg(hover_bg()).text_color(text_color()))
            .child(Icon::new(icon).size_4())
            .child(div().flex_1().child(label))
            .when_some(style.badge, |this, badge| {
                this.child(
                    div()
                        .px_1p5()
                        .py_0p5()
                        .text_size(px(10.))
                        .rounded_full()
                        .border_1()
                        .border_color(border_color())
                        .text_color(text_3())
                        .child(badge),
                )
            })
            .on_click(cx.listener(on_click))
    }

    fn conversation_title(conversation: &Conversation) -> SharedString {
        if conversation.title.is_empty() {
            SharedString::from("Untitled conversation")
        } else {
            conversation.title.clone()
        }
    }

    fn row_for_conversation(
        conversation: &Conversation,
        active_id: Option<usize>,
        depth: usize,
        has_children: bool,
        collapsed: bool,
    ) -> RecentRow {
        RecentRow {
            id: conversation.id,
            title: Self::conversation_title(conversation),
            active: active_id == Some(conversation.id),
            pinned: conversation.pinned,
            pending: conversation.pending,
            depth,
            has_children,
            collapsed,
        }
    }

    fn recent_rows(conversations: &[Conversation], active_id: Option<usize>) -> Vec<RecentRow> {
        conversations
            .iter()
            .map(|conversation| {
                Self::row_for_conversation(conversation, active_id, 0, false, false)
            })
            .collect()
    }

    fn tree_rows(
        conversations: &[Conversation],
        active_id: Option<usize>,
        collapsed_conversation_ids: &HashSet<usize>,
    ) -> Vec<RecentRow> {
        struct TreeRows<'a> {
            by_id: &'a HashMap<usize, &'a Conversation>,
            children_by_parent: &'a HashMap<usize, Vec<usize>>,
            active_id: Option<usize>,
            collapsed_conversation_ids: &'a HashSet<usize>,
            visited: HashSet<usize>,
            rows: Vec<RecentRow>,
        }

        impl TreeRows<'_> {
            fn push(&mut self, id: usize, depth: usize) {
                if !self.visited.insert(id) {
                    return;
                }

                let Some(conversation) = self.by_id.get(&id).copied() else {
                    return;
                };
                let child_ids = self
                    .children_by_parent
                    .get(&id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let has_children = !child_ids.is_empty();
                let collapsed = has_children && self.collapsed_conversation_ids.contains(&id);
                self.rows.push(Sidebar::row_for_conversation(
                    conversation,
                    self.active_id,
                    depth,
                    has_children,
                    collapsed,
                ));

                if collapsed {
                    return;
                }

                for child_id in child_ids {
                    self.push(*child_id, depth + 1);
                }
            }
        }

        let by_id = conversations
            .iter()
            .map(|conversation| (conversation.id, conversation))
            .collect::<HashMap<_, _>>();
        let conversation_ids = by_id.keys().copied().collect::<HashSet<_>>();
        let mut root_ids = Vec::new();
        let mut children_by_parent: HashMap<usize, Vec<usize>> = HashMap::new();

        for conversation in conversations {
            if let Some(origin) = &conversation.branch_origin {
                let parent_id = origin.source_conversation_id;
                if parent_id != conversation.id && conversation_ids.contains(&parent_id) {
                    children_by_parent
                        .entry(parent_id)
                        .or_default()
                        .push(conversation.id);
                    continue;
                }
            }

            root_ids.push(conversation.id);
        }

        let mut tree = TreeRows {
            by_id: &by_id,
            children_by_parent: &children_by_parent,
            active_id,
            collapsed_conversation_ids,
            visited: HashSet::new(),
            rows: Vec::new(),
        };
        for id in root_ids {
            tree.push(id, 0);
        }

        for conversation in conversations {
            if !tree.visited.contains(&conversation.id) {
                tree.rows.push(Self::row_for_conversation(
                    conversation,
                    active_id,
                    0,
                    false,
                    false,
                ));
            }
        }

        tree.rows
    }

    fn render_mode_tabs(&self, active_ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        fn tab_content(mode: ChatMode) -> AnyElement {
            h_flex()
                .gap_1()
                .items_center()
                .justify_center()
                .child(Icon::new(mode.icon()).size_3p5())
                .child(mode.label())
                .into_any_element()
        }

        div().mx_2().mt_1().mb_2().flex_shrink_0().child(
            TabBar::new("mode-tabs")
                .segmented()
                .w_full()
                .selected_index(active_ix)
                .on_click(cx.listener(|this, ix: &usize, window, cx| {
                    let new_mode = ChatMode::from_index(*ix);
                    if let Some(app) = this.app.upgrade() {
                        app.update(cx, |app, cx| {
                            if app.settings.mode != new_mode {
                                app.settings.mode = new_mode;
                                ClaudeApp::toast(window, cx, format!("Mode: {}", new_mode.label()));
                                cx.notify();
                            }
                        });
                    }
                }))
                .child(Tab::new().flex_1().child(tab_content(ChatMode::Chat)))
                .child(Tab::new().flex_1().child(tab_content(ChatMode::Cowork)))
                .child(Tab::new().flex_1().child(tab_content(ChatMode::Code))),
        )
    }

    fn render_nav(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .px_2()
            .gap(px(1.))
            .flex_shrink_0()
            .child(Self::nav_item(
                "nav-new",
                IconName::Plus,
                "New chat",
                NavItemStyle {
                    badge: None,
                    muted: false,
                    selected: false,
                },
                |this, _, window, cx| {
                    if let Some(app) = this.app.upgrade() {
                        app.update(cx, |app, cx| app.new_chat(window, cx));
                    }
                },
                cx,
            ))
            .child(Self::nav_item(
                "nav-search",
                IconName::Search,
                "Search",
                NavItemStyle {
                    badge: None,
                    muted: false,
                    selected: false,
                },
                |this, _, window, cx| {
                    if let Some(app) = this.app.upgrade() {
                        app.update(cx, |app, cx| app.open_search(window, cx));
                    }
                },
                cx,
            ))
            .child(Self::nav_item(
                "nav-chats",
                IconName::Inbox,
                "Chats",
                NavItemStyle {
                    badge: None,
                    muted: false,
                    selected: self.active_nav == SidebarNav::Chats,
                },
                |this, _, _, cx| {
                    this.active_nav = SidebarNav::Chats;
                    this.list_mode = ConversationListMode::Recent;
                    cx.notify();
                },
                cx,
            ))
            .child(Self::nav_item(
                "nav-projects",
                IconName::Folder,
                "Projects",
                NavItemStyle {
                    badge: None,
                    muted: false,
                    selected: self.active_nav == SidebarNav::Projects,
                },
                |this, _, window, cx| {
                    this.active_nav = SidebarNav::Projects;
                    if let Some(app) = this.app.upgrade() {
                        app.update(cx, |app, cx| app.open_projects_panel(window, cx));
                    }
                    cx.notify();
                },
                cx,
            ))
            .child(Self::nav_item(
                "nav-artifacts",
                IconName::File,
                "Artifacts",
                NavItemStyle {
                    badge: None,
                    muted: false,
                    selected: self.active_nav == SidebarNav::Artifacts,
                },
                |this, _, window, cx| {
                    this.active_nav = SidebarNav::Artifacts;
                    if let Some(app) = this.app.upgrade() {
                        app.update(cx, |app, cx| app.open_artifacts_panel(window, cx));
                    }
                    cx.notify();
                },
                cx,
            ))
            .child(Self::nav_item(
                "nav-customize",
                IconName::Settings,
                "Customize",
                NavItemStyle {
                    badge: None,
                    muted: false,
                    selected: false,
                },
                |this, _, window, cx| {
                    if let Some(app) = this.app.upgrade() {
                        app.update(cx, |app, cx| app.open_customize(window, cx));
                    }
                },
                cx,
            ))
    }

    fn render_recents_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .flex_shrink_0()
            .px(px(18.))
            .pt_3()
            .pb_1()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(text_3())
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("RECENTS"),
            )
            .child(
                h_flex()
                    .gap_0p5()
                    .child(
                        Button::new("recents-flat-mode")
                            .ghost()
                            .small()
                            .selected(self.list_mode == ConversationListMode::Recent)
                            .icon(IconName::Inbox)
                            .tooltip("Recent order")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.list_mode = ConversationListMode::Recent;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("recents-tree-mode")
                            .ghost()
                            .small()
                            .selected(self.list_mode == ConversationListMode::Tree)
                            .icon(IconName::Network)
                            .tooltip("Tree order")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.list_mode = ConversationListMode::Tree;
                                cx.notify();
                            })),
                    ),
            )
    }

    fn conversation_menu_content(
        row: RecentRow,
        app: WeakEntity<ClaudeApp>,
        cx: &mut Context<PopoverState>,
    ) -> Stateful<Div> {
        let id = row.id;
        let weak_open = app.clone();
        let weak_pin = app.clone();
        let weak_rename = app.clone();
        let weak_project = app.clone();
        let weak_delete = app.clone();

        v_flex()
            .id("recent-conversation-menu")
            .w(px(210.))
            .py_1()
            .child(menu_item(
                "recent-open",
                IconName::Inbox,
                "Open",
                move |window, cx| {
                    if let Some(app) = weak_open.upgrade() {
                        app.update(cx, |app, cx| app.select_conversation(id, window, cx));
                    }
                },
                cx,
            ))
            .child(menu_item(
                "recent-rename",
                IconName::Replace,
                "Rename",
                move |window, cx| {
                    if let Some(app) = weak_rename.upgrade() {
                        app.update(cx, |app, cx| app.begin_rename_conversation(id, window, cx));
                    }
                },
                cx,
            ))
            .child(menu_item(
                "recent-pin",
                if row.pinned {
                    IconName::StarFill
                } else {
                    IconName::Star
                },
                if row.pinned { "Unpin" } else { "Pin" },
                move |window, cx| {
                    if let Some(app) = weak_pin.upgrade() {
                        app.update(cx, |app, cx| {
                            app.set_conversation_pinned(id, !row.pinned, window, cx);
                        });
                    }
                },
                cx,
            ))
            .child(menu_item(
                "recent-project",
                IconName::Folder,
                "Add to project",
                move |window, cx| {
                    if let Some(app) = weak_project.upgrade() {
                        app.update(cx, |app, cx| {
                            app.open_project_picker_for_conversation(id, window, cx);
                        });
                    }
                },
                cx,
            ))
            .child(div().h(px(1.)).bg(border_color()).my_1())
            .child(menu_item(
                "recent-delete",
                IconName::Delete,
                "Delete",
                move |window, cx| {
                    if let Some(app) = weak_delete.upgrade() {
                        app.update(cx, |app, cx| app.delete_conversation(id, window, cx));
                    }
                },
                cx,
            ))
    }

    fn render_recents(&self, rows: Vec<RecentRow>, cx: &mut Context<Self>) -> impl IntoElement {
        let app = self.app.clone();
        div().flex_1().min_h_0().child(
            div()
                .id("recents-list")
                .size_full()
                .overflow_y_scrollbar()
                .child(v_flex().px_2().children(rows.into_iter().map(|row| {
                    let id = row.id;
                    let depth = row.depth;
                    let indent = px(8. + depth as f32 * 14.);
                    let toggle_id = id;
                    let menu_row = row.clone();
                    let menu_app = app.clone();
                    h_flex()
                        .id(("recent", id))
                        .min_h(px(28.))
                        .flex_shrink_0()
                        .pl(indent)
                        .pr_2p5()
                        .py_1p5()
                        .gap_1p5()
                        .items_center()
                        .rounded_md()
                        .cursor_pointer()
                        .text_size(px(13.))
                        .text_color(if row.active { text_color() } else { text_2() })
                        .overflow_hidden()
                        .when(row.active, |this| this.bg(recent_active_bg()))
                        .when(self.list_mode == ConversationListMode::Tree, |this| {
                            this.child(
                                div()
                                    .id(("recent-toggle", toggle_id))
                                    .size_4()
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_sm()
                                    .text_color(text_3())
                                    .when(row.has_children, |this| {
                                        this.cursor_pointer()
                                            .hover(|this| {
                                                this.bg(hover_bg()).text_color(text_color())
                                            })
                                            .child(
                                                Icon::new(if row.collapsed {
                                                    IconName::ChevronRight
                                                } else {
                                                    IconName::ChevronDown
                                                })
                                                .size_3(),
                                            )
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                if this
                                                    .collapsed_conversation_ids
                                                    .contains(&toggle_id)
                                                {
                                                    this.collapsed_conversation_ids
                                                        .remove(&toggle_id);
                                                } else {
                                                    this.collapsed_conversation_ids
                                                        .insert(toggle_id);
                                                }
                                                cx.notify();
                                            }))
                                    }),
                            )
                        })
                        .when(row.pinned, |this| {
                            this.child(
                                div()
                                    .flex_shrink_0()
                                    .child(Icon::new(IconName::StarFill).size_3()),
                            )
                        })
                        .child(div().flex_1().truncate().child(row.title))
                        .when(row.pending, |this| {
                            this.child(div().size_2().rounded_full().bg(accent()).flex_shrink_0())
                        })
                        .child(
                            Popover::new(format!("recent-menu-{id}"))
                                .anchor(Anchor::BottomRight)
                                .p_0()
                                .trigger(
                                    Button::new(format!("recent-menu-btn-{id}"))
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::Ellipsis)
                                        .tooltip("Conversation actions"),
                                )
                                .content(move |_, _, cx| {
                                    Self::conversation_menu_content(
                                        menu_row.clone(),
                                        menu_app.clone(),
                                        cx,
                                    )
                                    .into_any_element()
                                }),
                        )
                        .hover(|this| this.bg(hover_bg()).text_color(text_color()))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(app) = this.app.upgrade() {
                                app.update(cx, |app, cx| app.select_conversation(id, window, cx));
                            }
                        }))
                }))),
        )
    }

    fn render_footer(&self) -> impl IntoElement {
        let weak = self.app.clone();
        v_flex()
            .px_2()
            .pt_2()
            .pb_3()
            .flex_shrink_0()
            .border_t_1()
            .border_color(border_color())
            .child(
                h_flex()
                    .id("user-row")
                    .px_2p5()
                    .py_2()
                    .gap_2p5()
                    .rounded_md()
                    .items_center()
                    .cursor_pointer()
                    .hover(|this| this.bg(hover_bg()))
                    .child(
                        div()
                            .size_8()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(text_color())
                            .text_color(white_color())
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("SX"),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(text_color())
                                    .child("sxhxliang"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(text_3())
                                    .child("Free plan"),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new("dl-btn")
                                    .ghost()
                                    .small()
                                    .icon(IconName::ArrowDown)
                                    .tooltip("Downloads")
                                    .on_click(|_, window, cx| {
                                        ClaudeApp::toast(window, cx, "Downloads panel coming soon");
                                    }),
                            )
                            .child(
                                Popover::new("user-menu")
                                    .anchor(Anchor::BottomRight)
                                    .p_0()
                                    .trigger(
                                        Button::new("user-menu-btn")
                                            .ghost()
                                            .small()
                                            .icon(IconName::ChevronDown),
                                    )
                                    .content(move |_, _, cx| {
                                        user_menu_content(cx, weak.clone()).into_any_element()
                                    }),
                            ),
                    ),
            )
    }
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(app) = self.app.upgrade() else {
            return div();
        };
        let (collapsed, active_ix, rows) = {
            let app = app.read(cx);
            let active_id = app.active_conversation_id;
            let rows = match self.list_mode {
                ConversationListMode::Recent => Self::recent_rows(&app.conversations, active_id),
                ConversationListMode::Tree => Self::tree_rows(
                    &app.conversations,
                    active_id,
                    &self.collapsed_conversation_ids,
                ),
            };
            (app.sidebar_collapsed, app.settings.mode.index(), rows)
        };

        v_flex()
            .h_full()
            .min_h_0()
            .bg(sidebar_bg())
            .border_r_1()
            .border_color(border_color())
            .overflow_hidden()
            .when(collapsed, |this| this.w(px(0.)))
            .when(!collapsed, |this| this.w(px(272.)))
            .child(
                h_flex()
                    .flex_shrink_0()
                    .px_2p5()
                    .pt_2p5()
                    .pb_1p5()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .id("sb-logo")
                            .px_2()
                            .py_1()
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Claude")
                            .rounded_md()
                            .hover(|this| this.bg(hover_bg()))
                            .cursor_pointer(),
                    )
                    .child(
                        Button::new("collapse-sb")
                            .ghost()
                            .small()
                            .icon(IconName::PanelLeft)
                            .tooltip("Collapse sidebar")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(app) = this.app.upgrade() {
                                    app.update(cx, |app, cx| {
                                        app.sidebar_collapsed = true;
                                        cx.notify();
                                    });
                                }
                            })),
                    ),
            )
            .child(self.render_mode_tabs(active_ix, cx))
            .child(self.render_nav(cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_recents_header(cx))
                    .child(self.render_recents(rows, cx)),
            )
            .child(self.render_footer())
    }
}
