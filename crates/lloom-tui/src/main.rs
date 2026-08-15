//! LLooM TUI — terminal user interface.
//!
//! Fully async: a single tokio runtime drives both keyboard input and all REST
//! requests to lloom-server (:7861). No blocking calls, no block_on.
//!
//! Five tabs mirror the WebUI: 总览 / 用量 / 对话 / 模型 / 设置.

mod rest;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Table, TableState};
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
        ("DASHSCOPE_API_KEY", "API Key (主要供应商)"),
        ("DASHSCOPE_API_BASE", "API Base"),
    ]),
    ("OpenAI", &[
        ("OPENAI_API_KEY", "API Key (sk-...)"),
        ("OPENAI_BASE_URL", "Base URL"),
    ]),
    ("Anthropic", &[("ANTHROPIC_API_KEY", "API Key (sk-ant-...)")]),
    ("核心配置", &[
        ("OLLAMA_API_BASE", "Ollama 地址"),
        ("LLOOM_WEB_PORT", "Web 端口"),
        ("LLOOM_DATA_DIR", "数据目录"),
    ]),
];

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

// ── Events flowing into the main loop ──

enum UiEvent {
    // Keyboard
    Key(KeyCode),
    // Async fetch results
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

// ── App state ──

struct App {
    tab: Tab,
    status_line: String,

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
    conv_input: String,
    conv_active: Option<String>,
    conv_loading: bool,
    selected_conv: usize,

    // Models
    model_list: Vec<Value>,

    // Settings
    env_values: Vec<(String, String, String)>,
    env_input: String,
    env_cursor: usize,
    env_msg: String,

    // UI
    scroll: usize,
    table_state: TableState,
}

impl App {
    fn new() -> Self {
        Self {
            tab: Tab::Overview,
            status_line: "Tab/←/→ 切换 · 对话页输入消息 Enter 发送 · Ctrl+C 退出".into(),
            services: vec![],
            routing_stats: vec![],
            total_spend: 0.0,
            usage_stats: vec![],
            budgets: vec![],
            model_count: 0,
            conversations: vec![],
            chat_msgs: vec![],
            conv_input: String::new(),
            conv_active: None,
            conv_loading: false,
            selected_conv: 0,
            model_list: vec![],
            env_values: vec![],
            env_input: String::new(),
            env_cursor: 0,
            env_msg: String::new(),
            scroll: 0,
            table_state: TableState::default(),
        }
    }

    fn apply(&mut self, ev: UiEvent) {
        match ev {
            UiEvent::Overview(Ok((services, routing, spend))) => {
                self.services = services;
                self.routing_stats = routing;
                self.total_spend = spend;
            }
            UiEvent::Overview(Err(e)) => self.status_line = format!("✗ 总览加载失败: {e}"),
            UiEvent::Usage(Ok((usage, spend, budgets, count))) => {
                self.usage_stats = usage;
                self.total_spend = spend;
                self.budgets = budgets;
                self.model_count = count;
            }
            UiEvent::Usage(Err(e)) => self.status_line = format!("✗ 用量加载失败: {e}"),
            UiEvent::Conversations(Ok(c)) => {
                self.conversations = c;
                if self.selected_conv >= self.conversations.len() {
                    self.selected_conv = 0;
                }
            }
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
            UiEvent::ConvLoaded(Ok(msgs)) => {
                self.chat_msgs = msgs;
            }
            UiEvent::ConvLoaded(Err(_)) => {}
            UiEvent::ConvSaved(Ok(id)) => {
                self.conv_active = Some(id);
                self.status_line = "✓ 对话已保存".into();
            }
            UiEvent::ConvSaved(Err(e)) => self.status_line = format!("✗ 保存失败: {e}"),
            UiEvent::ServiceOp(Ok(_)) => {
                self.status_line = "✓ 服务操作成功".into();
            }
            UiEvent::ServiceOp(Err(e)) => self.status_line = format!("✗ 服务操作失败: {e}"),
            UiEvent::Key(_) => {}
        }
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

// ── Spawn all background fetches ──

fn spawn_refresh(tx: mpsc::Sender<UiEvent>) {
    let t = tx.clone();
    tokio::spawn(async move {
        let _ = t.send(UiEvent::Overview(fetch_overview().await)).await;
    });
    let t = tx.clone();
    tokio::spawn(async move {
        let _ = t.send(UiEvent::Usage(fetch_usage().await)).await;
    });
    let t = tx.clone();
    tokio::spawn(async move {
        let _ = t.send(UiEvent::Conversations(fetch_conversations().await)).await;
    });
    let t = tx.clone();
    tokio::spawn(async move {
        let _ = t.send(UiEvent::Models(fetch_models().await)).await;
    });
    let t = tx.clone();
    tokio::spawn(async move {
        let _ = t.send(UiEvent::Env(fetch_env().await)).await;
    });
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

    // Keyboard task (crossterm read is blocking; run on a dedicated task)
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
                // Periodic refresh
                if last_refresh.elapsed().as_secs() >= 15 {
                    spawn_refresh(tx.clone());
                    *last_refresh = std::time::Instant::now();
                }
            }
        }
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

/// Handle a key press. Returns true if the app should quit.
fn handle_key(app: &mut App, code: KeyCode, tx: mpsc::Sender<UiEvent>) -> bool {
    match code {
        KeyCode::Char('c') => return true,
        KeyCode::Tab | KeyCode::Right => app.tab = next_tab(app.tab, 1),
        KeyCode::Left => app.tab = next_tab(app.tab, -1),
        KeyCode::Char('q') if app.tab != Tab::Chat => return true,
        _ => handle_tab_key(app, code, tx),
    }
    false
}

fn handle_tab_key(app: &mut App, code: KeyCode, tx: mpsc::Sender<UiEvent>) {
    match app.tab {
        Tab::Chat => match code {
            KeyCode::Enter => {
                if !app.conv_loading {
                    let q = std::mem::take(&mut app.conv_input);
                    if q.trim().is_empty() {
                        return;
                    }
                    app.chat_msgs.push(ChatMsg { role: "user".into(), content: q.clone(), detail: String::new() });
                    app.conv_loading = true;
                    app.status_line = "思考中...".into();
                    // Save current state async
                    let msgs = app.chat_msgs.clone();
                    let id = app.conv_active.clone();
                    let t = tx.clone();
                    tokio::spawn(async move {
                        let _ = t.send(UiEvent::ConvSaved(save_conv(id.as_deref(), &msgs).await)).await;
                    });
                    // Send chat async
                    let t = tx.clone();
                    tokio::spawn(async move {
                        let _ = t.send(UiEvent::ChatReply(do_chat(&q).await)).await;
                    });
                }
            }
            KeyCode::Backspace => {
                app.conv_input.pop();
            }
            KeyCode::Char('n') if app.conv_input.is_empty() => {
                app.conv_active = None;
                app.chat_msgs.clear();
                app.status_line = "新对话".into();
            }
            KeyCode::Char(c) => app.conv_input.push(c),
            KeyCode::Down => {
                if app.selected_conv + 1 < app.conversations.len() {
                    select_conv(app, app.selected_conv + 1, tx.clone());
                }
            }
            KeyCode::Up => {
                if app.selected_conv > 0 {
                    select_conv(app, app.selected_conv - 1, tx.clone());
                }
            }
            _ => {}
        },
        Tab::Models => match code {
            KeyCode::Down => {
                if !app.model_list.is_empty() {
                    let i = (app.table_state.selected().unwrap_or(0) + 1).min(app.model_list.len() - 1);
                    app.table_state.select(Some(i));
                }
            }
            KeyCode::Up => {
                let i = app.table_state.selected().unwrap_or(0).saturating_sub(1);
                app.table_state.select(Some(i));
            }
            KeyCode::Char('d') => {
                if let Some(i) = app.table_state.selected() {
                    if i < app.model_list.len() {
                        let name = app.model_list[i]["name"].as_str().unwrap_or("").to_string();
                        let t = tx.clone();
                        let name2 = name.clone();
                        tokio::spawn(async move {
                            let _ = t.send(UiEvent::ServiceOp(delete_model(&name2).await)).await;
                            let _ = t.send(UiEvent::Models(fetch_models().await)).await;
                        });
                        app.status_line = format!("删除模型 {name}...");
                    }
                }
            }
            KeyCode::Char('e') => {
                if let Some(i) = app.table_state.selected() {
                    if i < app.model_list.len() {
                        let name = app.model_list[i]["name"].as_str().unwrap_or("").to_string();
                        let task_types = ["", "general", "coding", "math_logic", "simple_qa", "complex_reasoning"];
                        let cur = app.model_list[i]["task_type"].as_str().unwrap_or("");
                        let next = task_types
                            .iter()
                            .position(|t| *t == cur)
                            .map(|p| task_types[(p + 1) % task_types.len()])
                            .unwrap_or("")
                            .to_string();
                        let t = tx.clone();
                        let name2 = name.clone();
                        let next2 = next.clone();
                        tokio::spawn(async move {
                            let _ = t.send(UiEvent::ServiceOp(update_model(&name2, &next2).await)).await;
                            let _ = t.send(UiEvent::Models(fetch_models().await)).await;
                        });
                        app.status_line = format!("模型 {name} 任务类型 → {next}");
                    }
                }
            }
            _ => {}
        },
        Tab::Settings => match code {
            KeyCode::Down => {
                app.env_cursor = (app.env_cursor + 1).min(app.env_values.len().saturating_sub(1));
                app.env_input = app.env_values.get(app.env_cursor).map(|(_, v, _)| v.clone()).unwrap_or_default();
            }
            KeyCode::Up => {
                app.env_cursor = app.env_cursor.saturating_sub(1);
                app.env_input = app.env_values.get(app.env_cursor).map(|(_, v, _)| v.clone()).unwrap_or_default();
            }
            KeyCode::Enter => {
                if let Some((key, _, _)) = app.env_values.get(app.env_cursor).cloned() {
                    let v = std::mem::take(&mut app.env_input);
                    let t = tx.clone();
                    let key2 = key.clone();
                    tokio::spawn(async move {
                        let _ = t.send(UiEvent::ServiceOp(write_env(&key, &v).await)).await;
                        let _ = t.send(UiEvent::Env(fetch_env().await)).await;
                    });
                    app.status_line = format!("✓ 已保存 {key2}");
                }
            }
            KeyCode::Backspace => {
                app.env_input.pop();
            }
            KeyCode::Char(c) => {
                app.env_input.push(c);
            }
            _ => {}
        },
        _ => {
            // Overview / Usage
            match code {
                KeyCode::Down => app.scroll += 1,
                KeyCode::Up => app.scroll = app.scroll.saturating_sub(1),
                KeyCode::Char('s') => {
                    let t = tx.clone();
                    tokio::spawn(async move {
                        let _ = t.send(UiEvent::ServiceOp(service_op("start", "ai").await)).await;
                        let _ = t.send(UiEvent::ServiceOp(service_op("start", "ollama").await)).await;
                        let _ = t.send(UiEvent::Overview(fetch_overview().await)).await;
                    });
                    app.status_line = "启动 AI 服务 + Ollama...".into();
                }
                KeyCode::Char('x') => {
                    let t = tx.clone();
                    tokio::spawn(async move {
                        let _ = t.send(UiEvent::ServiceOp(service_op("stop", "ai").await)).await;
                        let _ = t.send(UiEvent::ServiceOp(service_op("stop", "ollama").await)).await;
                        let _ = t.send(UiEvent::Overview(fetch_overview().await)).await;
                    });
                    app.status_line = "停止 AI 服务 + Ollama...".into();
                }
                _ => {}
            }
        }
    }
}

fn select_conv(app: &mut App, idx: usize, tx: mpsc::Sender<UiEvent>) {
    if idx >= app.conversations.len() {
        return;
    }
    let (id, _, _) = &app.conversations[idx];
    let id = id.clone();
    app.selected_conv = idx;
    app.conv_active = Some(id.clone());
    app.chat_msgs.clear();
    let t = tx.clone();
    tokio::spawn(async move {
        let _ = t.send(UiEvent::ConvLoaded(fetch_conv(&id).await)).await;
    });
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

    let status = Paragraph::new(app.status_line.clone()).style(Style::default().fg(Color::Cyan));
    f.render_widget(status, outer[1]);
}

fn card_block(title: &str) -> Block<'static> {
    Block::default().title(title.to_string()).borders(Borders::ALL)
}

fn render_overview(f: &mut Frame, area: Rect, app: &mut App) {
    let cols = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(0), Constraint::Length(3), Constraint::Length(3)])
        .split(area);

    let n_healthy = app.services.iter().filter(|s| s.healthy).count();
    let cards = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, 4); 4])
        .split(cols[0]);
    let stats: Vec<(String, String, Color)> = vec![
        ("服务健康".into(), format!("{}/{}", n_healthy, app.services.len()), Color::Blue),
        (
            "核心服务".into(),
            if app.services.iter().any(|s| s.name == "核心服务" && s.healthy) { "运行中".into() } else { "异常".into() },
            Color::Green,
        ),
        (
            "Ollama".into(),
            if app.services.iter().any(|s| s.name == "Ollama" && s.healthy) { "运行中".into() } else { "未运行".into() },
            Color::Green,
        ),
        ("累计花费".into(), format!("${:.6}", app.total_spend), Color::Yellow),
    ];
    for (i, (label, val, color)) in stats.iter().enumerate() {
        let lines = vec![
            Line::from(Span::styled(val.clone(), Style::default().fg(*color).add_modifier(Modifier::BOLD))),
            Line::from(Span::styled(label.clone(), Style::default().fg(Color::Gray))),
        ];
        f.render_widget(Paragraph::new(lines).block(Block::default().borders(Borders::ALL)), cards[i]);
    }

    let header = ["服务名", "状态", "健康"];
    let widths = [Constraint::Length(16), Constraint::Length(26), Constraint::Length(6)];
    let rows: Vec<Vec<String>> = app
        .services
        .iter()
        .map(|s| vec![s.name.clone(), s.status.clone(), if s.healthy { "✓ 健康".into() } else { "✗ 异常".into() }])
        .collect();
    let table = build_table(&header, &widths, &rows, 20);
    f.render_widget(table.block(card_block("服务列表")), cols[1]);

    let mut rlines = Vec::new();
    if app.routing_stats.is_empty() {
        rlines.push(Line::from("  (暂无路由数据)"));
    } else {
        for (m, c) in &app.routing_stats {
            rlines.push(Line::from(format!("  {m:<16} {c} 次")));
        }
    }
    f.render_widget(Paragraph::new(rlines).block(card_block("智能路由统计")), cols[2]);

    f.render_widget(
        Paragraph::new(" [s] 启动服务   [x] 停止服务   [Tab] 切换页面").style(Style::default().fg(Color::DarkGray)),
        cols[3],
    );
}

fn render_usage(f: &mut Frame, area: Rect, app: &mut App) {
    let cols = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(0), Constraint::Length(4 + app.budgets.len() as u16)])
        .split(area);

    let cards = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, 3); 3])
        .split(cols[0]);
    let stats: Vec<(String, String, Color)> = vec![
        ("核心服务".into(), "正常".into(), Color::Green),
        ("可用模型".into(), app.model_count.to_string(), Color::Blue),
        ("累计花费".into(), format!("${:.6}", app.total_spend), Color::Yellow),
    ];
    for (i, (label, val, color)) in stats.iter().enumerate() {
        let lines = vec![
            Line::from(Span::styled(val.clone(), Style::default().fg(*color).add_modifier(Modifier::BOLD))),
            Line::from(Span::styled(label.clone(), Style::default().fg(Color::Gray))),
        ];
        f.render_widget(Paragraph::new(lines).block(Block::default().borders(Borders::ALL)), cards[i]);
    }

    let header = ["模型", "输入", "输出", "请求", "花费"];
    let widths = [Constraint::Length(18), Constraint::Length(12), Constraint::Length(12), Constraint::Length(8), Constraint::Length(12)];
    let rows: Vec<Vec<String>> = app
        .usage_stats
        .iter()
        .map(|r| vec![r.model.clone(), r.input.to_string(), r.output.to_string(), r.requests.to_string(), format!("${:.4}", r.cost)])
        .collect();
    let table = build_table(&header, &widths, &rows, 20);
    f.render_widget(table.block(card_block("用量明细")), cols[1]);

    let mut blines = Vec::new();
    if app.budgets.is_empty() {
        blines.push(Line::from("  (未设置预算 — 用 lloom-cli budgets set)"));
    } else {
        for (scope, id, spent, max) in &app.budgets {
            let pct = if *max > 0.0 { (spent / max * 100.0) as u32 } else { 0 };
            let color = if pct >= 100 { Color::Red } else { Color::Green };
            let filled = (pct / 10).min(10) as usize;
            let bar = format!("[{}{}]", "█".repeat(filled), "░".repeat(10 - filled));
            blines.push(Line::from(vec![
                Span::raw(format!("  {scope}/{id}  ${spent:.2}/${max:.2}  ")),
                Span::styled(bar, Style::default().fg(color)),
                Span::styled(format!(" {pct}%"), Style::default().fg(color)),
            ]));
        }
    }
    f.render_widget(Paragraph::new(blines).block(card_block("配额管理")), cols[2]);
}

fn render_chat(f: &mut Frame, area: Rect, app: &mut App) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);

    let mut items: Vec<ListItem> = app
        .conversations
        .iter()
        .enumerate()
        .map(|(i, (_, title, n))| {
            let s = if i == app.selected_conv {
                Style::default().add_modifier(Modifier::BOLD).bg(Color::DarkGray)
            } else {
                Style::default()
            };
            ListItem::new(format!("  {title} ({n})")).style(s)
        })
        .collect();
    if items.is_empty() {
        items.push(ListItem::new("  (无对话 — 输入消息自动新建)"));
    }
    items.push(ListItem::new(""));
    items.push(ListItem::new("  [n] 新建"));
    let list = List::new(items).block(card_block("会话"));
    f.render_widget(list, cols[0]);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(cols[1]);

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
    f.render_widget(msgs, right[0]);

    let prompt = if app.conv_loading { "处理中..." } else { "输入消息 (Enter 发送, n 新建) > " };
    let input = Paragraph::new(format!("{prompt}{}", app.conv_input))
        .style(Style::default().fg(Color::Green))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(input, right[1]);
}

fn render_models(f: &mut Frame, area: Rect, app: &mut App) {
    let cols = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0), Constraint::Length(2)])
        .split(area);

    f.render_widget(
        Paragraph::new("↓↑ 选择 · d 删除 · e 切换任务类型").style(Style::default().fg(Color::DarkGray)),
        cols[0],
    );

    let header = ["名称", "提供商", "LiteLLM 模型", "输入 $/1K", "输出 $/1K"];
    let widths = [Constraint::Length(20), Constraint::Length(12), Constraint::Length(30), Constraint::Length(12), Constraint::Length(12)];
    let rows: Vec<Vec<String>> = app
        .model_list
        .iter()
        .map(|m| {
            vec![
                m["name"].as_str().unwrap_or("").to_string(),
                m["provider"].as_str().unwrap_or("").to_string(),
                m["litellm_model"].as_str().unwrap_or("").to_string(),
                format!("{:.6}", m["input_cost_per_token"].as_f64().unwrap_or(0.0) * 1000.0),
                format!("{:.6}", m["output_cost_per_token"].as_f64().unwrap_or(0.0) * 1000.0),
            ]
        })
        .collect();

    let header_cells: Vec<ratatui::widgets::Cell> = header
        .iter()
        .map(|h| ratatui::widgets::Cell::from(Span::styled(h.to_string(), Style::default().add_modifier(Modifier::BOLD))))
        .collect();
    let header_row = ratatui::widgets::Row::new(header_cells);
    let table = Table::new(rows.into_iter().map(ratatui::widgets::Row::new), widths)
        .header(header_row)
        .block(card_block("模型管理"))
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");
    let mut state = app.table_state.clone();
    if !app.model_list.is_empty() && state.selected().is_none() {
        state.select(Some(0));
    }
    f.render_stateful_widget(table, cols[1], &mut state);
    app.table_state = state;

    f.render_widget(
        Paragraph::new(format!("共 {} 个模型", app.model_list.len())).style(Style::default().fg(Color::DarkGray)),
        cols[2],
    );
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
    lines.push(Line::from(Span::styled("API 密钥 (↑↓ 选择 · Enter 保存)", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
    for (i, (key, val, section)) in app.env_values.iter().enumerate() {
        let marker = if i == app.env_cursor { "▶" } else { " " };
        let is_set = !val.trim().is_empty();
        let dot = if is_set { "✓" } else { "○" };
        let dot_color = if is_set { Color::Green } else { Color::Gray };
        let shown = if i == app.env_cursor { app.env_input.clone() } else if is_set { "***配置***".to_string() } else { String::new() };
        lines.push(Line::from(vec![
            Span::styled(format!("{marker} "), Style::default().fg(Color::Yellow)),
            Span::styled(format!("{dot} "), Style::default().fg(dot_color)),
            Span::styled(format!("{key:<20}"), Style::default().fg(Color::White)),
            Span::styled(format!("[{section}]"), Style::default().fg(Color::Gray)),
            Span::styled(format!("  {shown}"), Style::default().fg(Color::Green)),
        ]));
    }
    if !app.env_msg.is_empty() {
        lines.push(Line::from(Span::styled(format!("  {}", app.env_msg), Style::default().fg(Color::Green))));
    }
    f.render_widget(Paragraph::new(lines).block(card_block("设置")), cols[0]);

    f.render_widget(
        Paragraph::new("↑↓ 选择密钥 · 输入新值 · Enter 保存").style(Style::default().fg(Color::DarkGray)),
        cols[1],
    );
}

fn build_table<'a>(header: &'a [&'a str], widths: &[Constraint], rows: &[Vec<String>], _max_rows: usize) -> Table<'a> {
    let header_cells: Vec<ratatui::widgets::Cell> = header
        .iter()
        .map(|h| ratatui::widgets::Cell::from(Span::styled(h.to_string(), Style::default().add_modifier(Modifier::BOLD))))
        .collect();
    let header_row = ratatui::widgets::Row::new(header_cells);
    Table::new(rows.iter().map(|r| ratatui::widgets::Row::new(r.clone())), widths.to_vec())
        .header(header_row)
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
}
