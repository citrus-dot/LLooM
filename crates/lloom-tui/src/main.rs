//! LLooM TUI — terminal user interface.
//!
//! Five tabs mirroring the WebUI layout:
//!   - 总览 Overview:  service status cards + service list + routing stats
//!   - 用量 Usage:     spend/request stats + budgets + pricing
//!   - 对话 Chat:      conversation list + orchestrated chat
//!   - 模型 Models:    model table + add/remove
//!   - 设置 Settings:  env check + API key editing
//!
//! Keys: Tab/←/→ switch tab · Enter/typing in Chat · Ctrl+C quit

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use lloom_core::db;
use lloom_core::models::Model;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Table, TableState};
use ratatui::{Frame, Terminal};
use std::io;

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Overview,
    Usage,
    Chat,
    Models,
    Settings,
}

const TABS: [&str; 5] = ["总览", "用量", "对话", "模型", "设置"];

const WEB_PORT: u16 = 7861;

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

struct ChatMsg {
    role: String, // user | assistant | decomp | summary
    content: String,
    detail: String,
}

struct App {
    tab: Tab,
    status_line: String,

    // Overview
    services: Vec<(String, String, bool)>, // name, status, healthy
    routing_stats: Vec<(String, u64)>,

    // Usage
    usage_stats: Vec<UsageRow>,
    total_spend: f64,
    budgets: Vec<(String, String, f64, f64)>, // scope, id, spent, max
    models_cache: Vec<Model>,

    // Chat
    conversations: Vec<(String, String, usize)>, // id, title, msg_count
    chat_msgs: Vec<ChatMsg>,
    conv_input: String,
    conv_active: Option<String>,
    conv_loading: bool,

    // Models
    model_list: Vec<Model>,

    // Settings
    env_values: std::collections::HashMap<String, String>,
    env_input: String,
    env_cursor_key: Option<String>,
    env_msg: String,

    // UI state
    scroll: usize,
    table_state: TableState,
    selected_conv: usize,
}

#[derive(Default)]
struct UsageRow {
    model: String,
    input: i64,
    output: i64,
    cost: f64,
    requests: i64,
}

impl App {
    fn new() -> Self {
        Self {
            tab: Tab::Overview,
            status_line: "Tab/←/→ 切换 · 对话页输入消息 Enter 发送 · Ctrl+C 退出".into(),
            services: vec![],
            routing_stats: vec![],
            usage_stats: vec![],
            total_spend: 0.0,
            budgets: vec![],
            models_cache: vec![],
            conversations: vec![],
            chat_msgs: vec![],
            conv_input: String::new(),
            conv_active: None,
            conv_loading: false,
            model_list: vec![],
            env_values: Default::default(),
            env_input: String::new(),
            env_cursor_key: None,
            env_msg: String::new(),
            scroll: 0,
            table_state: TableState::default(),
            selected_conv: 0,
        }
    }

    fn refresh_all(&mut self) {
        self.refresh_overview();
        self.refresh_usage();
        self.refresh_conversations();
        self.refresh_models();
        self.refresh_env();
    }

    fn refresh_overview(&mut self) {
        // Services
        self.services.clear();
        let rt = tokio::runtime::Runtime::new().expect("tokio");
        let ai = rt.block_on(lloom_core::processes::check_ai_health());
        self.services.push(("核心服务".into(), "Up (本进程)".into(), true));
        let ollama = rt.block_on(lloom_core::processes::check_ollama_health());
        self.services.push(("Ollama".into(), if ollama { "Up".into() } else { "Down".into() }, ollama));
        let ai_ok = ai.status == "ok";
        let ai_ready = ai.ready;
        self.services.push((
            "AI 服务".into(),
            if ai_ok {
                if ai_ready { "Up (ready)".into() } else { "Up (未配置模型)".into() }
            } else {
                "Down".into()
            },
            ai_ok && ai_ready,
        ));

        // Routing stats
        self.routing_stats.clear();
        if let Ok(s) = db::get_usage_stats(None, None, None) {
            for r in &s {
                self.routing_stats.push((r.model_name.clone(), r.request_count as u64));
            }
        }
    }

    fn refresh_usage(&mut self) {
        self.usage_stats.clear();
        if let Ok(s) = db::get_usage_stats(None, None, None) {
            for r in &s {
                self.usage_stats.push(UsageRow {
                    model: r.model_name.clone(),
                    input: r.total_input_tokens,
                    output: r.total_output_tokens,
                    cost: r.total_cost,
                    requests: r.request_count,
                });
            }
        }
        if let Ok(t) = db::get_total_spend(None, None, None) {
            self.total_spend = t;
        }
        self.budgets.clear();
        if let Ok(b) = db::list_budgets() {
            for x in &b {
                let spent = db::get_total_spend(
                    if x.scope == "user" { Some(&x.scope_id) } else { None },
                    if x.scope == "model" { Some(&x.scope_id) } else { None },
                    None,
                )
                .unwrap_or(0.0);
                self.budgets.push((x.scope.clone(), x.scope_id.clone(), spent, x.max_budget));
            }
        }
        self.models_cache = db::list_models(true).unwrap_or_default();
    }

    fn refresh_conversations(&mut self) {
        self.conversations = lloom_core::conversations::list()
            .unwrap_or_default()
            .into_iter()
            .map(|c| (c.id, c.title, c.message_count))
            .collect();
        if self.conversations.is_empty() {
            self.selected_conv = 0;
        } else if self.selected_conv >= self.conversations.len() {
            self.selected_conv = 0;
        }
    }

    fn refresh_models(&mut self) {
        self.model_list = db::list_models(true).unwrap_or_default();
    }

    fn refresh_env(&mut self) {
        self.env_values = lloom_core::config::read_env();
    }

    fn send_chat(&mut self) {
        let q = std::mem::take(&mut self.conv_input);
        if q.trim().is_empty() {
            return;
        }
        self.chat_msgs.push(ChatMsg { role: "user".into(), content: q.clone(), detail: String::new() });
        self.status_line = "思考中...".into();
        self.conv_loading = true;
        let reply = self.do_orchestrate(&q);
        self.conv_loading = false;
        match reply {
            Ok((text, detail)) => {
                self.chat_msgs.push(ChatMsg { role: "assistant".into(), content: text, detail });
                self.status_line = "✓ 已回复".into();
            }
            Err(e) => {
                self.chat_msgs.push(ChatMsg { role: "assistant".into(), content: format!("请求失败: {e}"), detail: String::new() });
                self.status_line = format!("✗ 失败: {e}");
            }
        }
    }

    fn do_orchestrate(&self, query: &str) -> Result<(String, String), String> {
        let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        rt.block_on(async {
            let models = db::list_models(true).map_err(|e| e.to_string())?;
            let specs: Vec<lloom_core::ai_client::ModelSpec> = models.iter().map(|m| m.into()).collect();
            let events = lloom_core::ai_client::orchestrate_stream(query, &[], "", &specs, "")
                .await
                .map_err(|e| e.to_string())?;
            let mut response = String::new();
            let mut models_used: Vec<String> = Vec::new();
            for ev in &events {
                if ev.event == "task_start" {
                    if let Some(m) = ev.data.get("model").and_then(|x| x.as_str()) {
                        models_used.push(m.to_string());
                    }
                } else if ev.event == "result" {
                    if let Some(r) = ev.data.get("response").and_then(|x| x.as_str()) {
                        response = r.to_string();
                    }
                }
            }
            if response.is_empty() {
                response = "(无返回结果)".into();
            }
            let mut uniq: Vec<String> = Vec::new();
            for m in models_used {
                if !uniq.contains(&m) {
                    uniq.push(m);
                }
            }
            let detail = if uniq.is_empty() { String::new() } else { format!("调用模型: {}", uniq.join(" | ")) };
            Ok((response, detail))
        })
    }

    fn new_conversation(&mut self) {
        self.conv_active = None;
        self.chat_msgs.clear();
        self.status_line = "新对话".into();
    }

    fn select_conversation(&mut self, idx: usize) {
        if idx >= self.conversations.len() {
            return;
        }
        let (id, _, _) = &self.conversations[idx];
        let id = id.clone();
        self.conv_active = Some(id.clone());
        self.chat_msgs.clear();
        if let Ok(v) = lloom_core::conversations::load(&id) {
            if let Some(arr) = v.get("messages").and_then(|x| x.as_array()) {
                for m in arr {
                    let role = m.get("role").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let content = m.get("content").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    self.chat_msgs.push(ChatMsg { role, content, detail: String::new() });
                }
            }
        }
        self.selected_conv = idx;
    }

    fn save_current_conv(&mut self) {
        if self.chat_msgs.is_empty() {
            return;
        }
        let messages: Vec<serde_json::Value> = self
            .chat_msgs
            .iter()
            .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
            .collect();
        let title = self
            .chat_msgs
            .first()
            .filter(|m| m.role == "user")
            .map(|m| m.content.chars().take(20).collect::<String>())
            .unwrap_or_else(|| "新对话".to_string());
        let id = self.conv_active.clone().unwrap_or_default();
        match lloom_core::conversations::save_or_create(&id, &title, &messages) {
            Ok(new_id) => {
                self.conv_active = Some(new_id);
                self.refresh_conversations();
            }
            Err(_) => {}
        }
    }
}

fn main() -> io::Result<()> {
    let _ = lloom_core::config::resolve_install_dir();
    let _ = db::init_db();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    app.refresh_all();
    let res = run_app(&mut terminal, &mut app);

    disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    res
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.save_current_conv();
                    return Ok(());
                }
                KeyCode::Tab | KeyCode::Right => {
                    app.tab = next_tab(app.tab, 1);
                }
                KeyCode::Left => {
                    app.tab = next_tab(app.tab, -1);
                }
                KeyCode::Char('q') if app.tab != Tab::Chat => {
                    app.save_current_conv();
                    return Ok(());
                }
                _ => handle_key(app, key.code),
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

fn handle_key(app: &mut App, key: KeyCode) {
    match app.tab {
        Tab::Chat => match key {
            KeyCode::Enter => {
                if !app.conv_loading {
                    app.send_chat();
                    app.save_current_conv();
                }
            }
            KeyCode::Backspace => {
                app.conv_input.pop();
            }
            // 'n' alone (empty input) starts a new conversation
            KeyCode::Char('n') if app.conv_input.is_empty() => app.new_conversation(),
            KeyCode::Char(c) => app.conv_input.push(c),
            KeyCode::Down => {
                if app.selected_conv + 1 < app.conversations.len() {
                    app.select_conversation(app.selected_conv + 1);
                }
            }
            KeyCode::Up => {
                if app.selected_conv > 0 {
                    app.select_conversation(app.selected_conv - 1);
                }
            }
            _ => {}
        },
        Tab::Models => match key {
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
                        let name = app.model_list[i].name.clone();
                        let _ = db::delete_model(&name);
                        app.refresh_models();
                        app.status_line = format!("✓ 模型已删除: {name}");
                    }
                }
            }
            KeyCode::Char('e') => {
                if let Some(i) = app.table_state.selected() {
                    if i < app.model_list.len() {
                        let name = app.model_list[i].name.clone();
                        let task_types = ["", "general", "coding", "math_logic", "simple_qa", "complex_reasoning"];
                        let cur = app.model_list[i].task_type.as_str();
                        let next = task_types
                            .iter()
                            .position(|t| *t == cur)
                            .map(|p| task_types[(p + 1) % task_types.len()])
                            .unwrap_or("");
                        let mut updates = serde_json::Map::new();
                        updates.insert("task_type".into(), serde_json::json!(next));
                        let _ = db::update_model(&name, &updates);
                        app.refresh_models();
                        app.status_line = format!("✓ 模型 {name} 任务类型 → {}", if next.is_empty() { "不分配" } else { next });
                    }
                }
            }
            _ => {}
        },
        Tab::Settings => match key {
            KeyCode::Enter => {
                if let Some(k) = app.env_cursor_key.clone() {
                    let v = std::mem::take(&mut app.env_input);
                    let mut updates = std::collections::HashMap::new();
                    updates.insert(k.clone(), v);
                    write_env(&updates);
                    app.refresh_env();
                    app.env_msg = format!("✓ 已保存 {k}");
                    app.env_cursor_key = None;
                }
            }
            KeyCode::Down => {
                // cycle through env keys
                let keys: Vec<String> = ENV_SCHEMA.iter().flat_map(|(_, items)| items.iter().map(|(k, _)| k.to_string())).collect();
                let idx = keys.iter().position(|k| Some(k) == app.env_cursor_key.as_ref()).unwrap_or(0);
                let next = (idx + 1) % keys.len();
                app.env_cursor_key = Some(keys[next].clone());
                app.env_input = app.env_values.get(&keys[next]).cloned().unwrap_or_default();
                app.env_msg = String::new();
            }
            KeyCode::Up => {
                let keys: Vec<String> = ENV_SCHEMA.iter().flat_map(|(_, items)| items.iter().map(|(k, _)| k.to_string())).collect();
                let idx = keys.iter().position(|k| Some(k) == app.env_cursor_key.as_ref()).unwrap_or(0);
                let next = (idx + keys.len() - 1) % keys.len();
                app.env_cursor_key = Some(keys[next].clone());
                app.env_input = app.env_values.get(&keys[next]).cloned().unwrap_or_default();
                app.env_msg = String::new();
            }
            KeyCode::Backspace => {
                if app.env_cursor_key.is_some() {
                    app.env_input.pop();
                }
            }
            KeyCode::Char(c) => {
                if app.env_cursor_key.is_some() {
                    app.env_input.push(c);
                }
            }
            _ => {}
        },
        Tab::Overview => match key {
            KeyCode::Down => app.scroll += 1,
            KeyCode::Up => app.scroll = app.scroll.saturating_sub(1),
            KeyCode::Char('s') => {
                app.status_line = "启动 AI 服务 + Ollama...".into();
                let rt = tokio::runtime::Runtime::new().expect("tokio");
                rt.block_on(async {
                    let _ = lloom_core::processes::start_ai().await;
                    let _ = lloom_core::processes::start_ollama().await;
                });
                app.refresh_overview();
                app.status_line = "✓ 服务已启动".into();
            }
            KeyCode::Char('x') => {
                app.status_line = "停止 AI 服务 + Ollama...".into();
                // Stop via REST (needs server running; child handles live there)
                let rt = tokio::runtime::Runtime::new().expect("tokio");
                rt.block_on(async {
                    let c = reqwest::Client::new();
                    let _ = c.post(format!("http://localhost:{WEB_PORT}/api/services/ai/stop")).send().await;
                    let _ = c.post(format!("http://localhost:{WEB_PORT}/api/services/ollama/stop")).send().await;
                });
                app.refresh_overview();
                app.status_line = "✓ 服务已停止".into();
            }
            _ => {}
        },
        _ => {
            // Usage: scroll
            match key {
                KeyCode::Down => app.scroll += 1,
                KeyCode::Up => app.scroll = app.scroll.saturating_sub(1),
                _ => {}
            }
        }
    }
}

fn write_env(updates: &std::collections::HashMap<String, String>) {
    let path = lloom_core::config::env_file_path();
    let mut env = lloom_core::config::read_env();
    for (k, v) in updates {
        env.insert(k.clone(), v.clone());
    }
    let mut keys: Vec<&String> = env.keys().collect();
    keys.sort();
    let mut out = String::new();
    for k in keys {
        out.push_str(&format!("{k}={}\n", env.get(k).unwrap()));
    }
    let _ = std::fs::write(path, out);
}

fn ui(f: &mut Frame, app: &mut App) {
    // Ant Design style: left sidebar nav + right content area + bottom status.
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(f.area());

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(22), Constraint::Min(0)])
        .split(outer[0]);

    // ── Sidebar nav ──
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

    // ── Content area ──
    let content = main[1];
    match app.tab {
        Tab::Overview => render_overview(f, content, app),
        Tab::Usage => render_usage(f, content, app),
        Tab::Chat => render_chat(f, content, app),
        Tab::Models => render_models(f, content, app),
        Tab::Settings => render_settings(f, content, app),
    }

    // ── Bottom status bar ──
    let status = Paragraph::new(app.status_line.clone())
        .style(Style::default().fg(Color::Cyan));
    f.render_widget(status, outer[1]);
}

fn card_block(title: &str) -> Block<'static> {
    Block::default().title(title.to_string()).borders(Borders::ALL)
}

fn render_overview(f: &mut Frame, area: Rect, app: &mut App) {
    // Ant Design style: 4 stat cards on top, then service table + routing.
    let cols = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(0),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(area);

    // ── Stat cards row (4 columns) ──
    let n_healthy = app.services.iter().filter(|(_, _, h)| *h).count();
    let cards = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, 4); 4])
        .split(cols[0]);

    let stats: Vec<(String, String, Color)> = vec![
        ("服务健康".into(), format!("{}/{}", n_healthy, app.services.len()), Color::Blue),
        (
            "核心服务".into(),
            if app.services.iter().any(|(n, _, h)| n == "核心服务" && *h) { "运行中".into() } else { "异常".into() },
            Color::Green,
        ),
        (
            "Ollama".into(),
            if app.services.iter().any(|(n, _, h)| n == "Ollama" && *h) { "运行中".into() } else { "未运行".into() },
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

    // ── Service table ──
    let header = ["服务名", "状态", "健康"];
    let widths = [Constraint::Length(16), Constraint::Length(24), Constraint::Length(6)];
    let rows: Vec<Vec<String>> = app
        .services
        .iter()
        .map(|(name, status, healthy)| vec![name.clone(), status.clone(), if *healthy { "✓ 健康".into() } else { "✗ 异常".into() }])
        .collect();
    let table = build_table(&header, &widths, &rows, 20);
    f.render_widget(table.block(card_block("服务列表")), cols[1]);

    // ── Routing stats ──
    let mut rlines = Vec::new();
    if app.routing_stats.is_empty() {
        rlines.push(Line::from("  (暂无路由数据)"));
    } else {
        for (m, c) in &app.routing_stats {
            rlines.push(Line::from(format!("  {:<16} {} 次", m, c)));
        }
    }
    f.render_widget(Paragraph::new(rlines).block(card_block("智能路由统计")), cols[2]);

    // ── Action hints ──
    f.render_widget(
        Paragraph::new(" [s] 启动服务   [x] 停止服务   [Tab] 切换页面")
            .style(Style::default().fg(Color::DarkGray)),
        cols[3],
    );
}

fn render_usage(f: &mut Frame, area: Rect, app: &mut App) {
    let cols = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(0),
            Constraint::Length(4 + app.budgets.len() as u16),
        ])
        .split(area);

    // ── Stat cards row ──
    let cards = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, 3); 3])
        .split(cols[0]);
    let stats: Vec<(String, String, Color)> = vec![
        ("核心服务".into(), "正常".into(), Color::Green),
        ("可用模型".into(), app.models_cache.len().to_string(), Color::Blue),
        ("累计花费".into(), format!("${:.6}", app.total_spend), Color::Yellow),
    ];
    for (i, (label, val, color)) in stats.iter().enumerate() {
        let lines = vec![
            Line::from(Span::styled(val.clone(), Style::default().fg(*color).add_modifier(Modifier::BOLD))),
            Line::from(Span::styled(label.clone(), Style::default().fg(Color::Gray))),
        ];
        f.render_widget(Paragraph::new(lines).block(Block::default().borders(Borders::ALL)), cards[i]);
    }

    // ── Usage table ──
    let header = ["模型", "输入", "输出", "请求", "花费"];
    let widths = [Constraint::Length(18), Constraint::Length(12), Constraint::Length(12), Constraint::Length(8), Constraint::Length(12)];
    let rows: Vec<Vec<String>> = app
        .usage_stats
        .iter()
        .map(|r| vec![
            r.model.clone(),
            r.input.to_string(),
            r.output.to_string(),
            r.requests.to_string(),
            format!("${:.4}", r.cost),
        ])
        .collect();
    let table = build_table(&header, &widths, &rows, 20);
    f.render_widget(table.block(card_block("用量明细")), cols[1]);

    // ── Budget progress bars ──
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

    // Left: conversation list
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
            ListItem::new(format!("  {} ({})", title, n)).style(s)
        })
        .collect();
    if items.is_empty() {
        items.push(ListItem::new("  (无对话 — 输入消息自动新建)"));
    }
    items.push(ListItem::new(""));
    items.push(ListItem::new("  [n] 新建"));
    let list = List::new(items).block(card_block("会话"));
    f.render_widget(list, cols[0]);

    // Right: messages + input
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

    let hint = "↓↑ 选择 · d 删除 · e 切换任务类型";
    f.render_widget(Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)), cols[0]);

    let header = ["名称", "提供商", "LiteLLM 模型", "输入 $/1K", "输出 $/1K"];
    let widths = [Constraint::Length(20), Constraint::Length(12), Constraint::Length(30), Constraint::Length(12), Constraint::Length(12)];
    let rows: Vec<Vec<String>> = app
        .model_list
        .iter()
        .map(|m| vec![
            m.name.clone(),
            m.provider.clone(),
            m.litellm_model.clone(),
            format!("{:.6}", m.input_cost_per_token * 1000.0),
            format!("{:.6}", m.output_cost_per_token * 1000.0),
        ])
        .collect();

    let header_cells: Vec<ratatui::widgets::Cell> = header
        .iter()
        .map(|h| ratatui::widgets::Cell::from(Span::styled(*h, Style::default().add_modifier(Modifier::BOLD))))
        .collect();
    let header_row = ratatui::widgets::Row::new(header_cells);
    let t = Table::new(
        rows.into_iter().map(|r| ratatui::widgets::Row::new(r)),
        widths,
    )
    .header(header_row)
    .block(card_block("模型管理"))
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
    .highlight_symbol("> ");
    let mut state = app.table_state.clone();
    if !app.model_list.is_empty() && state.selected().is_none() {
        state.select(Some(0));
    }
    f.render_stateful_widget(t, cols[1], &mut state);
    app.table_state = state;

    let footer = format!("共 {} 个模型", app.model_list.len());
    f.render_widget(Paragraph::new(footer).style(Style::default().fg(Color::DarkGray)), cols[2]);
}

fn render_settings(f: &mut Frame, area: Rect, app: &mut App) {
    let cols = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled("环境检查", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
    for (name, status, healthy) in &app.services {
        let color = if *healthy { Color::Green } else { Color::Red };
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<12}", name), Style::default().fg(Color::White)),
            Span::styled(status.clone(), Style::default().fg(color)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("API 密钥 (↑↓ 选择 · Enter 保存)", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
    for (section, items) in ENV_SCHEMA {
        lines.push(Line::from(Span::styled(format!("  {section}"), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
        for (key, desc) in items {
            let val = app.env_values.get(*key).cloned().unwrap_or_default();
            let is_set = !val.trim().is_empty();
            let marker = if app.env_cursor_key.as_deref() == Some(*key) { "▶" } else { " " };
            let dot = if is_set { "✓" } else { "○" };
            let dot_color = if is_set { Color::Green } else { Color::Gray };
            let shown = if is_set { "***配置***".to_string() } else { String::new() };
            lines.push(Line::from(vec![
                Span::styled(format!("{marker} "), Style::default().fg(Color::Yellow)),
                Span::styled(format!("{dot} "), Style::default().fg(dot_color)),
                Span::styled(format!("{key:<20}"), Style::default().fg(Color::White)),
                Span::styled(format!("{:<16}", desc), Style::default().fg(Color::Gray)),
                Span::styled(if app.env_cursor_key.as_deref() == Some(*key) { app.env_input.clone() } else { shown }, Style::default().fg(Color::Green)),
            ]));
        }
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
    Table::new(
        rows.iter().map(|r| ratatui::widgets::Row::new(r.clone())),
        widths.to_vec(),
    )
    .header(header_row)
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
}
