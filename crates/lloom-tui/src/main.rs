//! LLooM TUI — terminal user interface.
//!
//! Unified cursor navigation: a global cell cursor moves freely with arrow
//! keys across a per-page grid of focusable cells. Enter activates the cell.
//! Fully async — all data flows through the REST API via tokio tasks.
//!
//! Keys: ←→↑↓ move cursor · Enter activate · Tab next page · Ctrl+C quit

mod rest;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::{Frame, Terminal};
use serde_json::Value;
use std::io;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Overview,
    Usage,
    Chat,
    Models,
    Settings,
}

const TABS: [&str; 5] = ["总览", "用量", "对话", "模型", "设置"];

const ENV_SCHEMA: [(&str, &[(&str, &str)]); 4] = [
    ("DashScope", &[
        ("DASHSCOPE_API_KEY", "API Key"),
        ("DASHSCOPE_API_BASE", "API Base"),
    ]),
    ("OpenAI", &[
        ("OPENAI_API_KEY", "API Key"),
        ("OPENAI_BASE_URL", "Base URL"),
    ]),
    ("Anthropic", &[("ANTHROPIC_API_KEY", "API Key")]),
    ("核心配置", &[
        ("OLLAMA_API_BASE", "Ollama 地址"),
        ("LLOOM_WEB_PORT", "Web 端口"),
        ("LLOOM_DATA_DIR", "数据目录"),
    ]),
];

// ── Cursor / cell model ──

/// A cell in the per-page focus grid.
struct Cell {
    text: String,
    kind: CellKind,
}

enum CellKind {
    Data,      // data row cell (navigable)
    Action,    // action button (navigable)
    Input,     // text input (Enter to edit, typing goes here)
    Header,    // non-focusable label
}

/// Global cursor position + current input text.
#[derive(Default)]
struct Cursor {
    row: usize,
    col: usize,
    input: String,
}

/// Per-page grid of focusable cells.
struct Grid {
    cells: Vec<Vec<Cell>>,
    rows: usize,
    cols: usize,
}

impl Grid {
    fn new() -> Self {
        Self { cells: vec![], rows: 0, cols: 0 }
    }

    fn set(&mut self, data: Vec<Vec<Cell>>) {
        self.rows = data.len();
        self.cols = data.first().map(|r| r.len()).unwrap_or(0);
        self.cells = data;
    }

    fn cell(&self, row: usize, col: usize) -> Option<&Cell> {
        self.cells.get(row).and_then(|r| r.get(col))
    }

    fn is_focusable(&self, row: usize, col: usize) -> bool {
        match self.cell(row, col) {
            Some(c) => matches!(c.kind, CellKind::Data | CellKind::Action | CellKind::Input),
            None => false,
        }
    }

    /// Clamp cursor to bounds, skipping non-focusable cells.
    fn clamp(&self, row: usize, col: usize) -> (usize, usize) {
        let r = row.min(self.rows.saturating_sub(1));
        let c = col.min(self.cols.saturating_sub(1));
        if self.is_focusable(r, c) {
            (r, c)
        } else {
            // Find nearest focusable in the row
            for cc in 0..self.cols {
                if self.is_focusable(r, cc) {
                    return (r, cc);
                }
            }
            // Fall back to any focusable anywhere
            for rr in 0..self.rows {
                for cc in 0..self.cols {
                    if self.is_focusable(rr, cc) {
                        return (rr, cc);
                    }
                }
            }
            (0, 0)
        }
    }
}

// ── Events ──

enum UiEvent {
    Key(KeyCode),
    Overview(Result<(Vec<ServiceInfo>, Vec<(String, u64)>, f64), String>),
    Usage(Result<(Vec<UsageRow>, f64, Vec<(String, String, f64, f64)>, usize), String>),
    Conversations(Result<Vec<(String, String, usize)>, String>),
    Models(Result<Vec<Value>, String>),
    Env(Result<Vec<(String, String, String)>, String>),
    ChatReply(Result<(String, String), String>),
    ConvLoaded(Result<Vec<ChatMsg>, String>),
    ConvSaved(Result<String, String>),
    ServiceOp(Result<Value, String>),
}

#[derive(Clone)]
struct ChatMsg {
    role: String,
    content: String,
    detail: String,
}

struct ServiceInfo {
    name: String,
    status: String,
    healthy: bool,
}

struct UsageRow {
    model: String,
    input: i64,
    output: i64,
    cost: f64,
    requests: i64,
}

// ── App ──

struct App {
    tab: Tab,
    status_line: String,
    cursor: Cursor,
    grid: Grid,

    // Overview
    services: Vec<ServiceInfo>,
    routing_stats: Vec<(String, u64)>,
    total_spend: f64,

    // Usage
    usage_stats: Vec<UsageRow>,
    budgets: Vec<(String, String, f64, f64)>,
    model_count: usize,

    // Chat
    conversations: Vec<(String, String, usize)>,
    chat_msgs: Vec<ChatMsg>,
    conv_active: Option<String>,
    conv_loading: bool,

    // Models
    model_list: Vec<Value>,

    // Settings
    env_values: Vec<(String, String, String)>,
}

impl App {
    fn new() -> Self {
        Self {
            tab: Tab::Overview,
            status_line: "←→↑↓ 移动光标 · Enter 激活 · Tab 下一页 · Ctrl+C 退出".into(),
            cursor: Cursor::default(),
            grid: Grid::new(),
            services: vec![],
            routing_stats: vec![],
            total_spend: 0.0,
            usage_stats: vec![],
            budgets: vec![],
            model_count: 0,
            conversations: vec![],
            chat_msgs: vec![],
            conv_active: None,
            conv_loading: false,
            model_list: vec![],
            env_values: vec![],
        }
    }

    fn apply(&mut self, ev: UiEvent) {
        match ev {
            UiEvent::Overview(Ok((services, routing, spend))) => {
                self.services = services;
                self.routing_stats = routing;
                self.total_spend = spend;
            }
            UiEvent::Overview(Err(e)) => self.status_line = format!("✗ 总览失败: {e}"),
            UiEvent::Usage(Ok((usage, spend, budgets, count))) => {
                self.usage_stats = usage;
                self.total_spend = spend;
                self.budgets = budgets;
                self.model_count = count;
            }
            UiEvent::Usage(Err(e)) => self.status_line = format!("✗ 用量失败: {e}"),
            UiEvent::Conversations(Ok(c)) => self.conversations = c,
            UiEvent::Conversations(Err(_)) => {}
            UiEvent::Models(Ok(m)) => self.model_list = m,
            UiEvent::Models(Err(_)) => {}
            UiEvent::Env(Ok(e)) => self.env_values = e,
            UiEvent::Env(Err(_)) => {}
            UiEvent::ChatReply(Ok((text, detail))) => {
                self.conv_loading = false;
                self.chat_msgs.push(ChatMsg { role: "assistant".into(), content: text, detail });
                self.status_line = "✓ 已回复".into();
            }
            UiEvent::ChatReply(Err(e)) => {
                self.conv_loading = false;
                self.chat_msgs.push(ChatMsg { role: "assistant".into(), content: format!("请求失败: {e}"), detail: String::new() });
                self.status_line = format!("✗ 失败: {e}");
            }
            UiEvent::ConvLoaded(Ok(msgs)) => self.chat_msgs = msgs,
            UiEvent::ConvLoaded(Err(_)) => {}
            UiEvent::ConvSaved(Ok(id)) => self.conv_active = Some(id),
            UiEvent::ConvSaved(Err(e)) => self.status_line = format!("✗ 保存失败: {e}"),
            UiEvent::ServiceOp(Ok(_)) => {}
            UiEvent::ServiceOp(Err(e)) => self.status_line = format!("✗ 操作失败: {e}"),
            UiEvent::Key(_) => {}
        }
    }

    /// Rebuild the focus grid for the current tab based on current data.
    fn rebuild_grid(&mut self) {
        match self.tab {
            Tab::Overview => self.build_overview_grid(),
            Tab::Usage => self.build_usage_grid(),
            Tab::Chat => self.build_chat_grid(),
            Tab::Models => self.build_models_grid(),
            Tab::Settings => self.build_settings_grid(),
        }
        // Clamp cursor to new grid
        let (r, c) = self.grid.clamp(self.cursor.row, self.cursor.col);
        self.cursor.row = r;
        self.cursor.col = c;
    }

    fn build_overview_grid(&mut self) {
        let mut grid: Vec<Vec<Cell>> = vec![];
        // Header row
        grid.push(vec![
            Cell { text: "服务名".into(), kind: CellKind::Header },
            Cell { text: "状态".into(), kind: CellKind::Header },
            Cell { text: "操作".into(), kind: CellKind::Header },
        ]);
        // Service rows
        for s in &self.services {
            grid.push(vec![
                Cell { text: format!("{}", if s.healthy { "✓" } else { "✗" }), kind: CellKind::Data },
                Cell { text: s.status.clone(), kind: CellKind::Data },
                Cell {
                    text: if s.name == "核心服务" { "[—]".into() } else { "[重启]".into() },
                    kind: if s.name == "核心服务" { CellKind::Header } else { CellKind::Action },
                },
            ]);
        }
        // Action row: start / stop
        grid.push(vec![
            Cell { text: "[启动服务]".into(), kind: CellKind::Action },
            Cell { text: "[停止服务]".into(), kind: CellKind::Action },
            Cell { text: format!("花费 ${:.6}", self.total_spend), kind: CellKind::Header },
        ]);
        self.grid.set(grid);
    }

    fn build_usage_grid(&mut self) {
        let mut grid: Vec<Vec<Cell>> = vec![];
        grid.push(vec![
            Cell { text: "模型".into(), kind: CellKind::Header },
            Cell { text: "输入".into(), kind: CellKind::Header },
            Cell { text: "输出".into(), kind: CellKind::Header },
            Cell { text: "请求".into(), kind: CellKind::Header },
            Cell { text: "花费".into(), kind: CellKind::Header },
        ]);
        for u in &self.usage_stats {
            grid.push(vec![
                Cell { text: u.model.clone(), kind: CellKind::Data },
                Cell { text: u.input.to_string(), kind: CellKind::Data },
                Cell { text: u.output.to_string(), kind: CellKind::Data },
                Cell { text: u.requests.to_string(), kind: CellKind::Data },
                Cell { text: format!("${:.4}", u.cost), kind: CellKind::Data },
            ]);
        }
        if self.usage_stats.is_empty() {
            grid.push(vec![Cell { text: "(暂无用量)".into(), kind: CellKind::Header }]);
        }
        // Budget rows
        for (scope, id, _spent, max) in &self.budgets {
            grid.push(vec![
                Cell { text: "预算".into(), kind: CellKind::Header },
                Cell { text: format!("{scope}/{id}"), kind: CellKind::Data },
                Cell { text: format!("${max:.2}"), kind: CellKind::Data },
                Cell { text: "".into(), kind: CellKind::Header },
                Cell { text: "".into(), kind: CellKind::Header },
            ]);
        }
        self.grid.set(grid);
    }

    fn build_chat_grid(&mut self) {
        let mut grid: Vec<Vec<Cell>> = vec![];
        // Left: conversation list (col 0)
        grid.push(vec![
            Cell { text: "会话".into(), kind: CellKind::Header },
            Cell { text: "消息区".into(), kind: CellKind::Header },
        ]);
        for (_, title, n) in &self.conversations {
            grid.push(vec![
                Cell { text: format!("{title} ({n})"), kind: CellKind::Data },
                Cell { text: String::new(), kind: CellKind::Header },
            ]);
        }
        if self.conversations.is_empty() {
            grid.push(vec![
                Cell { text: "(无对话)".into(), kind: CellKind::Header },
                Cell { text: String::new(), kind: CellKind::Header },
            ]);
        }
        // Input row at the bottom (col 1 is the input cell)
        grid.push(vec![
            Cell { text: "[新建]".into(), kind: CellKind::Action },
            Cell { text: format!("> {}", self.cursor.input), kind: CellKind::Input },
        ]);
        self.grid.set(grid);
    }

    fn build_models_grid(&mut self) {
        let mut grid: Vec<Vec<Cell>> = vec![];
        grid.push(vec![
            Cell { text: "名称".into(), kind: CellKind::Header },
            Cell { text: "提供商".into(), kind: CellKind::Header },
            Cell { text: "LiteLLM 模型".into(), kind: CellKind::Header },
            Cell { text: "操作".into(), kind: CellKind::Header },
        ]);
        for m in &self.model_list {
            let name = m["name"].as_str().unwrap_or("").to_string();
            grid.push(vec![
                Cell { text: name.clone(), kind: CellKind::Data },
                Cell { text: m["provider"].as_str().unwrap_or("").to_string(), kind: CellKind::Data },
                Cell { text: m["litellm_model"].as_str().unwrap_or("").to_string(), kind: CellKind::Data },
                Cell { text: "[删除]".into(), kind: CellKind::Action },
            ]);
        }
        if self.model_list.is_empty() {
            grid.push(vec![Cell { text: "(无模型)".into(), kind: CellKind::Header }]);
        }
        self.grid.set(grid);
    }

    fn build_settings_grid(&mut self) {
        let mut grid: Vec<Vec<Cell>> = vec![];
        grid.push(vec![
            Cell { text: "键".into(), kind: CellKind::Header },
            Cell { text: "值".into(), kind: CellKind::Header },
            Cell { text: "分组".into(), kind: CellKind::Header },
        ]);
        for (key, val, section) in &self.env_values {
            let shown = if val.trim().is_empty() { "(空)".into() } else { "***".into() };
            grid.push(vec![
                Cell { text: key.clone(), kind: CellKind::Data },
                Cell { text: shown, kind: CellKind::Input },
                Cell { text: section.clone(), kind: CellKind::Header },
            ]);
        }
        self.grid.set(grid);
    }

    /// Move the cursor by (dr, dc). Returns true if moved.
    fn move_cursor(&mut self, dr: i64, dc: i64) -> bool {
        let nr = (self.cursor.row as i64 + dr).clamp(0, self.grid.rows as i64 - 1) as usize;
        let nc = (self.cursor.col as i64 + dc).clamp(0, self.grid.cols as i64 - 1) as usize;
        if self.grid.is_focusable(nr, nc) {
            self.cursor.row = nr;
            self.cursor.col = nc;
            true
        } else {
            // Try to land on a focusable cell in the target row/col
            let (r, c) = self.grid.clamp(nr, nc);
            if r != self.cursor.row || c != self.cursor.col {
                self.cursor.row = r;
                self.cursor.col = c;
                true
            } else {
                false
            }
        }
    }

    /// Activate the current cell. Returns true if quit.
    fn activate(&mut self, tx: mpsc::Sender<UiEvent>) -> bool {
        match self.tab {
            Tab::Overview => {
                let row = self.cursor.row;
                let col = self.cursor.col;
                // Action row = last row
                if row + 1 == self.grid.rows {
                    if col == 0 {
                        self.spawn_service_op(&tx, "start", "ai");
                        self.spawn_service_op(&tx, "start", "ollama");
                        self.status_line = "启动 AI + Ollama...".into();
                    } else if col == 1 {
                        self.spawn_service_op(&tx, "stop", "ai");
                        self.spawn_service_op(&tx, "stop", "ollama");
                        self.status_line = "停止 AI + Ollama...".into();
                    }
                } else if col == 2 && row >= 1 && row - 1 < self.services.len() {
                    let name = self.services[row - 1].name.clone();
                    if name != "核心服务" {
                        self.spawn_service_op(&tx, "restart", &name);
                        self.status_line = format!("重启 {name}...");
                    }
                }
            }
            Tab::Chat => {
                let row = self.cursor.row;
                let col = self.cursor.col;
                if col == 0 && row + 1 == self.grid.rows {
                    // New conversation
                    self.conv_active = None;
                    self.chat_msgs.clear();
                    self.status_line = "新对话".into();
                } else if col == 1 && row + 1 == self.grid.rows {
                    // Send the input
                    self.send_chat(&tx);
                } else if col == 0 && row >= 1 && row - 1 < self.conversations.len() {
                    let (id, _, _) = &self.conversations[row - 1];
                    let id = id.clone();
                    self.conv_active = Some(id.clone());
                    self.chat_msgs.clear();
                    let t = tx.clone();
                    tokio::spawn(async move {
                        let _ = t.send(UiEvent::ConvLoaded(fetch_conv(&id).await)).await;
                    });
                }
            }
            Tab::Models => {
                let row = self.cursor.row;
                let col = self.cursor.col;
                if col == 3 && row >= 1 && row - 1 < self.model_list.len() {
                    let name = self.model_list[row - 1]["name"].as_str().unwrap_or("").to_string();
                    let t = tx.clone();
                    let n2 = name.clone();
                    tokio::spawn(async move {
                        let _ = t.send(UiEvent::ServiceOp(delete_model(&n2).await)).await;
                        let _ = t.send(UiEvent::Models(fetch_models().await)).await;
                    });
                    self.status_line = format!("删除模型 {name}...");
                } else if col == 0 && row >= 1 && row - 1 < self.model_list.len() {
                    let name = self.model_list[row - 1]["name"].as_str().unwrap_or("").to_string();
                    let task_types = ["", "general", "coding", "math_logic", "simple_qa", "complex_reasoning"];
                    let cur = self.model_list[row - 1]["task_type"].as_str().unwrap_or("");
                    let next = task_types.iter().position(|t| *t == cur).map(|p| task_types[(p + 1) % task_types.len()]).unwrap_or("").to_string();
                    let t = tx.clone();
                    let n2 = name.clone();
                    let x2 = next.clone();
                    tokio::spawn(async move {
                        let _ = t.send(UiEvent::ServiceOp(update_model(&n2, &x2).await)).await;
                        let _ = t.send(UiEvent::Models(fetch_models().await)).await;
                    });
                    self.status_line = format!("模型 {name} 任务 → {next}");
                }
            }
            Tab::Settings => {
                let row = self.cursor.row;
                let col = self.cursor.col;
                if col == 1 && row >= 1 && row - 1 < self.env_values.len() {
                    let (key, _, _) = self.env_values[row - 1].clone();
                    let v = std::mem::take(&mut self.cursor.input);
                    let t = tx.clone();
                    let k2 = key.clone();
                    tokio::spawn(async move {
                        let _ = t.send(UiEvent::ServiceOp(write_env(&key, &v).await)).await;
                        let _ = t.send(UiEvent::Env(fetch_env().await)).await;
                    });
                    self.status_line = format!("✓ 已保存 {k2}");
                }
            }
            Tab::Usage => {}
        }
        false
    }

    fn spawn_service_op(&self, tx: &mpsc::Sender<UiEvent>, action: &str, name: &str) {
        let t = tx.clone();
        let action = action.to_string();
        let name = name.to_string();
        tokio::spawn(async move {
            let _ = t.send(UiEvent::ServiceOp(service_op(&action, &name).await)).await;
            let _ = t.send(UiEvent::Overview(fetch_overview().await)).await;
        });
    }

    fn send_chat(&mut self, tx: &mpsc::Sender<UiEvent>) {
        if self.conv_loading {
            return;
        }
        let q = std::mem::take(&mut self.cursor.input);
        if q.trim().is_empty() {
            return;
        }
        self.chat_msgs.push(ChatMsg { role: "user".into(), content: q.clone(), detail: String::new() });
        self.conv_loading = true;
        self.status_line = "思考中...".into();
        let msgs = self.chat_msgs.clone();
        let id = self.conv_active.clone();
        let t = tx.clone();
        tokio::spawn(async move {
            let _ = t.send(UiEvent::ConvSaved(save_conv(id.as_deref(), &msgs).await)).await;
        });
        let t = tx.clone();
        tokio::spawn(async move {
            let _ = t.send(UiEvent::ChatReply(do_chat(&q).await)).await;
        });
    }
}

// ── Async REST fetchers ──

async fn fetch_overview() -> Result<(Vec<ServiceInfo>, Vec<(String, u64)>, f64), String> {
    let status = rest::get("/api/services/status").await?;
    let stats = rest::get("/api/stats").await;
    let services = status["services"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|s| ServiceInfo {
                    name: s["name"].as_str().unwrap_or("").into(),
                    status: s["status"].as_str().unwrap_or("").into(),
                    healthy: s["healthy"].as_bool().unwrap_or(false),
                })
                .collect()
        })
        .unwrap_or_default();
    let mut routing = Vec::new();
    if let Ok(s) = &stats {
        if let Some(rs) = s["routing_stats"].as_object() {
            for (k, v) in rs {
                routing.push((k.clone(), v.as_u64().unwrap_or(0)));
            }
        }
    }
    let spend = stats.as_ref().map(|s| s["total_spend"].as_f64().unwrap_or(0.0)).unwrap_or(0.0);
    Ok((services, routing, spend))
}

async fn fetch_usage() -> Result<(Vec<UsageRow>, f64, Vec<(String, String, f64, f64)>, usize), String> {
    let usage = rest::get("/api/usage").await?;
    let stats = rest::get("/api/stats").await;
    let budgets = rest::get("/api/budgets").await;
    let models = rest::get("/api/models").await;

    let rows = usage["usage"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|u| UsageRow {
                    model: u["model_name"].as_str().unwrap_or("").into(),
                    input: u["total_input_tokens"].as_i64().unwrap_or(0),
                    output: u["total_output_tokens"].as_i64().unwrap_or(0),
                    cost: u["total_cost"].as_f64().unwrap_or(0.0),
                    requests: u["request_count"].as_i64().unwrap_or(0),
                })
                .collect()
        })
        .unwrap_or_default();
    let spend = stats.as_ref().map(|s| s["total_spend"].as_f64().unwrap_or(0.0)).unwrap_or(0.0);
    let mut budget_list = Vec::new();
    if let Ok(b) = &budgets {
        if let Some(arr) = b["budgets"].as_array() {
            for x in arr {
                budget_list.push((
                    x["scope"].as_str().unwrap_or("").into(),
                    x["scope_id"].as_str().unwrap_or("").into(),
                    0.0,
                    x["max_budget"].as_f64().unwrap_or(0.0),
                ));
            }
        }
    }
    let count = models.as_ref().map(|m| m["models"].as_array().map(|a| a.len()).unwrap_or(0)).unwrap_or(0);
    Ok((rows, spend, budget_list, count))
}

async fn fetch_conversations() -> Result<Vec<(String, String, usize)>, String> {
    let v = rest::get("/api/conversations").await?;
    Ok(v["conversations"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|c| {
                    (
                        c["id"].as_str().unwrap_or("").into(),
                        c["title"].as_str().unwrap_or("").into(),
                        c["message_count"].as_u64().unwrap_or(0) as usize,
                    )
                })
                .collect()
        })
        .unwrap_or_default())
}

async fn fetch_conv(id: &str) -> Result<Vec<ChatMsg>, String> {
    let v = rest::get(&format!("/api/conversations/{}", rest::urlencode(id))).await?;
    Ok(v["messages"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|m| ChatMsg {
                    role: m["role"].as_str().unwrap_or("").into(),
                    content: m["content"].as_str().unwrap_or("").into(),
                    detail: String::new(),
                })
                .collect()
        })
        .unwrap_or_default())
}

async fn fetch_models() -> Result<Vec<Value>, String> {
    let v = rest::get("/api/models").await?;
    Ok(v["models"].as_array().cloned().unwrap_or_default())
}

async fn fetch_env() -> Result<Vec<(String, String, String)>, String> {
    let v = rest::get("/api/config").await?;
    let mut out = Vec::new();
    for (section, items) in ENV_SCHEMA {
        for (key, _desc) in items {
            let val = v.get(*key).and_then(|x| x.as_str()).unwrap_or("").to_string();
            out.push(((*key).to_string(), val, (*section).to_string()));
        }
    }
    Ok(out)
}

async fn do_chat(query: &str) -> Result<(String, String), String> {
    let body = serde_json::json!({ "messages": [{"role": "user", "content": query}] });
    let text = rest::sse_text("/api/chat/stream", body).await?;
    let mut response = String::new();
    for line in text.lines() {
        if let Some(data) = line.strip_prefix("data: ") {
            if let Ok(v) = serde_json::from_str::<Value>(data) {
                if let Some(c) = v["content"].as_str() {
                    response.push_str(c);
                }
            }
        }
    }
    Ok((response, String::new()))
}

async fn save_conv(id: Option<&str>, msgs: &[ChatMsg]) -> Result<String, String> {
    let messages: Vec<Value> = msgs
        .iter()
        .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
        .collect();
    let title = msgs
        .iter()
        .find(|m| m.role == "user")
        .map(|m| m.content.chars().take(20).collect::<String>())
        .unwrap_or_else(|| "新对话".into());
    let body = serde_json::json!({ "id": id.unwrap_or(""), "title": title, "messages": messages });
    let r = rest::post("/api/conversations", body).await?;
    Ok(r["id"].as_str().unwrap_or("").to_string())
}

async fn service_op(action: &str, name: &str) -> Result<Value, String> {
    let id = if name.to_lowercase().contains("ollama") { "ollama" } else { "ai" };
    rest::post(&format!("/api/services/{id}/{action}"), Value::Null).await
}

async fn delete_model(name: &str) -> Result<Value, String> {
    rest::delete(&format!("/api/models/{}", rest::urlencode(name))).await
}

async fn update_model(name: &str, task_type: &str) -> Result<Value, String> {
    rest::put(
        &format!("/api/models/{}", rest::urlencode(name)),
        serde_json::json!({ "task_type": task_type }),
    )
    .await
}

async fn write_env(key: &str, value: &str) -> Result<Value, String> {
    rest::post("/api/config", serde_json::json!({ "updates": { key: value } })).await
}

// ── Spawn background fetches ──

fn spawn_refresh(tx: mpsc::Sender<UiEvent>) {
    let t = tx.clone();
    tokio::spawn(async move { let _ = t.send(UiEvent::Overview(fetch_overview().await)).await; });
    let t = tx.clone();
    tokio::spawn(async move { let _ = t.send(UiEvent::Usage(fetch_usage().await)).await; });
    let t = tx.clone();
    tokio::spawn(async move { let _ = t.send(UiEvent::Conversations(fetch_conversations().await)).await; });
    let t = tx.clone();
    tokio::spawn(async move { let _ = t.send(UiEvent::Models(fetch_models().await)).await; });
    let t = tx.clone();
    tokio::spawn(async move { let _ = t.send(UiEvent::Env(fetch_env().await)).await; });
}

// ── Main ──

#[tokio::main]
async fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let (tx, mut rx) = mpsc::channel::<UiEvent>(128);
    spawn_refresh(tx.clone());

    let kbd_tx = tx.clone();
    let kbd = tokio::spawn(async move {
        loop {
            if let Ok(Event::Key(k)) = event::read() {
                if k.kind == KeyEventKind::Press {
                    if kbd_tx.send(UiEvent::Key(k.code)).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    let mut last_refresh = std::time::Instant::now();
    let result = event_loop(&mut terminal, &mut app, &mut rx, tx.clone(), &mut last_refresh).await;

    let _ = kbd.abort();
    disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    rx: &mut mpsc::Receiver<UiEvent>,
    tx: mpsc::Sender<UiEvent>,
    last_refresh: &mut std::time::Instant,
) -> io::Result<()> {
    loop {
        app.rebuild_grid();
        terminal.draw(|f| ui(f, app))?;

        let deadline = tokio::time::sleep(Duration::from_millis(100));
        tokio::pin!(deadline);

        tokio::select! {
            ev = rx.recv() => {
                match ev {
                    Some(UiEvent::Key(code)) => {
                        if handle_key(app, code, tx.clone()) {
                            return Ok(());
                        }
                    }
                    Some(e) => app.apply(e),
                    None => return Ok(()),
                }
            }
            _ = &mut deadline => {
                if last_refresh.elapsed().as_secs() >= 15 {
                    spawn_refresh(tx.clone());
                    *last_refresh = std::time::Instant::now();
                }
            }
        }
    }
}

/// Handle a key press. Returns true if the app should quit.
fn handle_key(app: &mut App, code: KeyCode, tx: mpsc::Sender<UiEvent>) -> bool {
    match code {
        KeyCode::Char('c') => true,
        KeyCode::Tab => {
            app.tab = next_tab(app.tab, 1);
            app.cursor = Cursor::default();
            app.rebuild_grid();
            false
        }
        KeyCode::Up => {
            app.move_cursor(-1, 0);
            false
        }
        KeyCode::Down => {
            app.move_cursor(1, 0);
            false
        }
        KeyCode::Left => {
            app.move_cursor(0, -1);
            false
        }
        KeyCode::Right => {
            app.move_cursor(0, 1);
            false
        }
        KeyCode::Enter => app.activate(tx),
        KeyCode::Backspace => {
            app.cursor.input.pop();
            false
        }
        KeyCode::Char(c) => {
            // Typing always goes into the cursor input buffer
            app.cursor.input.push(c);
            false
        }
        _ => false,
    }
}

fn next_tab(t: Tab, dir: i8) -> Tab {
    let idx = (t as i8 + dir).rem_euclid(5);
    match idx {
        0 => Tab::Overview,
        1 => Tab::Usage,
        2 => Tab::Chat,
        3 => Tab::Models,
        _ => Tab::Settings,
    }
}

// ═══════════════ UI RENDERING ═══════════════

fn ui(f: &mut Frame, app: &mut App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(f.area());

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(22), Constraint::Min(0)])
        .split(outer[0]);

    // Sidebar nav
    let items: Vec<ListItem> = TABS
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let active = i as i8 == app.tab as i8;
            let prefix = if active { "▶ " } else { "   " };
            let style = if active {
                Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            ListItem::new(format!("{prefix}{label}")).style(style)
        })
        .collect();
    let nav = List::new(items)
        .block(Block::default().title(" LLooM ").borders(Borders::ALL))
        .highlight_style(Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD));
    f.render_widget(nav, main[0]);

    let content = main[1];
    match app.tab {
        Tab::Overview => render_overview(f, content, app),
        Tab::Usage => render_usage(f, content, app),
        Tab::Chat => render_chat(f, content, app),
        Tab::Models => render_models(f, content, app),
        Tab::Settings => render_settings(f, content, app),
    }

    let status = format!(
        "{}  ·  光标({},{})  {}",
        app.status_line,
        app.cursor.row,
        app.cursor.col,
        if app.cursor.input.is_empty() { "" } else { &app.cursor.input }
    );
    let status = Paragraph::new(status).style(Style::default().fg(Color::Cyan));
    f.render_widget(status, outer[1]);
}

fn card_block(title: &str) -> Block<'static> {
    Block::default().title(title.to_string()).borders(Borders::ALL)
}

/// Style a grid cell — highlight the focused cell.
fn cell_style(app: &App, row: usize, col: usize, is_header: bool) -> Style {
    if row == app.cursor.row && col == app.cursor.col {
        return Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
    }
    if is_header {
        return Style::default().add_modifier(Modifier::BOLD);
    }
    Style::default()
}

/// Render a grid as a simple table-like paragraph with cursor highlight.
fn render_grid(f: &mut Frame, area: Rect, app: &App, title: &str) {
    let mut lines: Vec<Line> = Vec::new();
    for (row, cells) in app.grid.cells.iter().enumerate() {
        let mut spans: Vec<Span> = Vec::new();
        for (col, cell) in cells.iter().enumerate() {
            let is_header = matches!(cell.kind, CellKind::Header);
            let style = cell_style(app, row, col, is_header);
            spans.push(Span::styled(format!(" {:<18} ", cell.text), style));
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines).block(card_block(title)), area);
}

fn render_overview(f: &mut Frame, area: Rect, app: &mut App) {
    render_grid(f, area, app, "总览");
}

fn render_usage(f: &mut Frame, area: Rect, app: &mut App) {
    render_grid(f, area, app, "用量");
}

fn render_chat(f: &mut Frame, area: Rect, app: &mut App) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    // Left: grid (conversation list + input)
    render_grid(f, cols[0], app, "会话");

    // Right: messages
    let mut mlines: Vec<Line> = Vec::new();
    for m in &app.chat_msgs {
        match m.role.as_str() {
            "user" => {
                mlines.push(Line::from(Span::styled("你: ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))));
                mlines.push(Line::from(m.content.clone()));
            }
            "assistant" => {
                mlines.push(Line::from(Span::styled("AI: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
                for l in m.content.split('\n') {
                    mlines.push(Line::from(l.to_string()));
                }
                if !m.detail.is_empty() {
                    mlines.push(Line::from(Span::styled(m.detail.clone(), Style::default().fg(Color::DarkGray))));
                }
            }
            _ => {}
        }
        mlines.push(Line::from(""));
    }
    if app.conv_loading {
        mlines.push(Line::from(Span::styled("  思考中...", Style::default().fg(Color::Yellow))));
    }
    let msgs = Paragraph::new(mlines).block(card_block("对话"));
    f.render_widget(msgs, cols[1]);
}

fn render_models(f: &mut Frame, area: Rect, app: &mut App) {
    render_grid(f, area, app, "模型管理");
}

fn render_settings(f: &mut Frame, area: Rect, app: &mut App) {
    let cols = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled("环境检查", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
    for s in &app.services {
        let color = if s.healthy { Color::Green } else { Color::Red };
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<12}", s.name), Style::default().fg(Color::White)),
            Span::styled(s.status.clone(), Style::default().fg(color)),
        ]));
    }
    lines.push(Line::from(""));
    f.render_widget(Paragraph::new(lines).block(card_block("设置")), cols[0]);

    // Env grid in the upper portion
    render_grid(f, cols[0], app, "API 密钥");
    f.render_widget(
        Paragraph::new("←→↑↓ 移动 · Enter 激活 · 输入会进入光标处的输入框").style(Style::default().fg(Color::DarkGray)),
        cols[1],
    );
}
