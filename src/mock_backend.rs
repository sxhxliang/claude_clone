//! Simulated backend for the demo: per-mode canned replies and the streaming
//! "cowork" mock that pushes Markdown chunks into `TextViewState`s. Kept apart
//! from the data models so the simulation is easy to swap for a real backend.
use std::time::Duration;

use gpui::{AppContext as _, AsyncApp, Context, SharedString, WeakEntity};
use gpui_component::{IconName, text::TextViewState};

use crate::chat_view::{MessageBlock, ToolCall, ToolStep};
use crate::models::ChatMode;

/// Per-mode simulated backend: returns (think delay, reply text).
pub(crate) fn backend_response(mode: ChatMode, user_text: &str) -> (Duration, SharedString) {
    match mode {
        ChatMode::Chat => (
            Duration::from_millis(500),
            format!(
                "Sure — here's a quick reply to: \"{}\".\n\nChat mode runs a short, \
                 conversational backend task and streams a single response.",
                user_text
            )
            .into(),
        ),
        ChatMode::Cowork => (
            Duration::from_millis(1400),
            format!(
                "✓ Cowork task accepted: \"{}\".\n\nI'll work on this in the background \
                 using the long-running Cowork agent and check in when I have an update.",
                user_text
            )
            .into(),
        ),
        ChatMode::Code => (
            Duration::from_millis(900),
            format!(
                "Here's a code-mode response for: \"{}\".\n\n```rust\nfn main() {{\n    \
                 println!(\"hello from Code mode\");\n}}\n```\n\nCode mode routes through a \
                 code-aware backend with project context.",
                user_text
            )
            .into(),
        ),
    }
}

/// Markdown chunks for the cowork demo body.
const COWORK_MD_1: &str = "Now I'll create a comprehensive technical proposal document in Markdown format with \
     military-grade system language.";

const COWORK_MD_2: &str = r#"技术方案文档已生成，共覆盖以下 12 个核心章节：

**文档结构概览：**

1. **项目概述** — 项目背景、定位与量化建设目标（含 6 项 KPI 指标表）
2. **需求分析与能力目标** — 作战场景牵引、能力缺口分析、五类使用场景矩阵
3. **总体技术路线** — "感知-理解-生成-校验-部署-优化" 六步闭环，五大技术攻关方向
4. **业务架构** — 多用户角色业务域划分、主业务流程、权限矩阵
5. **功能架构** — F1～F6 六大功能模块，含关键技术与性能指标明细
6. **技术架构** — 六层技术栈（接入→网关→应用→AI→数据→基础设施），技术选型表，安全架构
7. **数据架构** — 数据全链路处理视图、核心实体模型、六类数据分级管理策略
8. **系统架构** — 物理部署视图（研发中心节点 + 三类边缘节点），16 个微服务拆分方案，高可用设计
9. **关键技术攻关** — 8 项技术攻关清单，含 T-01 大模型适配和 T-06 多智能体协同的详细方案
10. **实施路径与里程碑** — 24 个月四阶段规划，8 个关键里程碑验收标准
11. **技术风险与应对** — 7 类风险识别与应对措施
12. **预期成果与效益** — 技术成果量化清单，军事/经济/战略三维效益评估

文档采用军工体系规范语言撰写，所有架构图均以文本 ASCII 格式呈现，便于 Word/PDF 进一步排版使用。
"#;

/// Build the mocked Cowork reply that streams Markdown into `TextViewState`s.
///
/// Creates two empty markdown states and spawns a background task that pushes
/// chunks (~8 chars / 35 ms) into each one in sequence — matching the
/// `crates/story/examples/stream_markdown.rs` pattern.
pub(crate) fn build_cowork_mock_reply<T: 'static>(cx: &mut Context<T>) -> Vec<MessageBlock> {
    let md1 = cx.new(|cx| TextViewState::markdown("", cx));
    let md2 = cx.new(|cx| TextViewState::markdown("", cx));

    let md1_weak = md1.downgrade();
    let md2_weak = md2.downgrade();
    cx.spawn(async move |_app, cx| {
        stream_into(md1_weak, COWORK_MD_1, cx).await;
        cx.background_executor()
            .timer(Duration::from_millis(400))
            .await;
        stream_into(md2_weak, COWORK_MD_2, cx).await;
    })
    .detach();

    vec![
        MessageBlock::Tool(ToolCall {
            title: "Reading the docx skill".into(),
            steps: vec![
                ToolStep {
                    icon: IconName::BookOpen,
                    title: "Reading the docx skill".into(),
                    detail: None,
                    file_chip: None,
                    done: false,
                },
                ToolStep {
                    icon: IconName::CircleCheck,
                    title: "Done".into(),
                    detail: None,
                    file_chip: None,
                    done: true,
                },
            ],
        }),
        MessageBlock::Markdown(md1),
        MessageBlock::Tool(ToolCall {
            title: "Created a file, read a file".into(),
            steps: vec![
                ToolStep {
                    icon: IconName::File,
                    title: "Creating the comprehensive technical proposal markdown document".into(),
                    detail: None,
                    file_chip: Some("智能网络信息系统技术方案.md".into()),
                    done: false,
                },
                ToolStep {
                    icon: IconName::File,
                    title: "Presented file".into(),
                    detail: None,
                    file_chip: None,
                    done: false,
                },
                ToolStep {
                    icon: IconName::CircleCheck,
                    title: "Done".into(),
                    detail: None,
                    file_chip: None,
                    done: true,
                },
            ],
        }),
        MessageBlock::Markdown(md2),
    ]
}

/// Push `text` into `state` in fixed chunks, with a short delay between
/// chunks so callers see a streaming render.
async fn stream_into(state: WeakEntity<TextViewState>, text: &str, cx: &mut AsyncApp) {
    let chars: Vec<char> = text.chars().collect();
    let mut pos = 0;
    while pos < chars.len() {
        let chunk_size = 8usize.min(chars.len() - pos);
        let chunk: String = chars[pos..pos + chunk_size].iter().collect();
        if state.update(cx, |s, cx| s.push_str(&chunk, cx)).is_err() {
            return; // entity dropped
        }
        pos += chunk_size;
        cx.background_executor()
            .timer(Duration::from_millis(35))
            .await;
    }
}
