//! LLooM TUI — terminal user interface.
//!
//! Links `lloom-core` directly. Three tabs:
//!   - Overview: service status + usage summary
//!   - Chat:     interactive chat via the AI service
//!   - Models:   registered models table
//!
//! Keys: Tab/←→ switch tab, Ctrl+C quit.

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use lloom_core::db;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Tabs};
use ratatui::{Frame, Terminal};
use std::io;

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Overview,
    Chat,
    Models,
}

const TABS: [&str; 3] = ["概览", "聊天", "模型"];

struct App {
    tab: Tab,
    messages: Vec<(String, String)>, // (role, content)
    input: String,
    status_line: String,
}

impl App {
    fn new() -> Self {
        Self {
            tab: Tab::Overview,
            messages: vec![],
            input: String::new(),
            status_line: "Tab 切换 · Ctrl+C 退出".to_string(),
        }
    }
}

fn main() -> io::Result<()> {
    // Initialize DB + install dir so core modules work offline.
    let _ = lloom_core::config::resolve_install_dir();
    let _ = db::init_db();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let res = run_app(&mut terminal, &mut app);

    disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    res
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('c') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                    return Ok(());
                }
                KeyCode::Tab => {
                    app.tab = match app.tab {
                        Tab::Overview => Tab::Chat,
                        Tab::Chat => Tab::Models,
                        Tab::Models => Tab::Overview,
                    };
                }
                KeyCode::Left => {
                    app.tab = match app.tab {
                        Tab::Overview => Tab::Models,
                        Tab::Chat => Tab::Overview,
                        Tab::Models => Tab::Chat,
                    };
                }
                KeyCode::Right => {
                    app.tab = match app.tab {
                        Tab::Overview => Tab::Chat,
                        Tab::Chat => Tab::Models,
                        Tab::Models => Tab::Overview,
                    };
                }
                KeyCode::Enter if app.tab == Tab::Chat => {
                    if !app.input.is_empty() {
                        let q = std::mem::take(&mut app.input);
                        app.messages.push(("user".into(), q.clone()));
                        app.status_line = "思考中...".into();
                        // Send to AI service (blocking on the TUI thread is fine).
                        match send_chat(&q) {
                            Ok(reply) => {
                                app.messages.push(("assistant".into(), reply));
                                app.status_line = "✓ 已回复".into();
                            }
                            Err(e) => {
                                app.status_line = format!("✗ 失败: {e}");
                            }
                        }
                    }
                }
                KeyCode::Char(c) if app.tab == Tab::Chat => {
                    app.input.push(c);
                }
                KeyCode::Backspace if app.tab == Tab::Chat => {
                    app.input.pop();
                }
                _ => {}
            }
        }
    }
}

fn send_chat(query: &str) -> Result<String, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    rt.block_on(async {
        let models = db::list_models(true).map_err(|e| e.to_string())?;
        let spec = match models.first() {
            Some(m) => lloom_core::ai_client::ModelSpec::from(m),
            None => lloom_core::ai_client::ModelSpec {
                name: "qwen2.5-local".into(),
                litellm_model: "ollama/qwen2.5:latest".into(),
                api_base: "http://localhost:11434".into(),
                api_key: String::new(),
                input_cost_per_token: 0.0,
                output_cost_per_token: 0.0,
            },
        };
        let messages = vec![serde_json::json!({ "role": "user", "content": query })];
        let res = lloom_core::ai_client::chat(&spec, &messages, 500, 0.3)
            .await
            .map_err(|e| e.to_string())?;
        Ok(res.content)
    })
}

fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)])
        .split(f.area());

    let tabs = Tabs::new(TABS.to_vec())
        .select(match app.tab {
            Tab::Overview => 0,
            Tab::Chat => 1,
            Tab::Models => 2,
        })
        .block(Block::default().title(" LLooM ").borders(Borders::ALL));
    f.render_widget(tabs, chunks[0]);

    match app.tab {
        Tab::Overview => render_overview(f, chunks[1]),
        Tab::Chat => render_chat(f, chunks[1], app),
        Tab::Models => render_models(f, chunks[1]),
    }

    let status = Paragraph::new(app.status_line.clone())
        .style(Style::default().fg(Color::Cyan));
    f.render_widget(status, chunks[2]);
}

fn render_overview(f: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(vec![Span::styled("服务状态", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))]),
        Line::from("  Core Server : Up (this process)"),
        Line::from(format!("  AI Service  : {}", ai_status())),
        Line::from(format!("  Ollama      : {}", ollama_status())),
        Line::from(""),
        Line::from(vec![Span::styled("用量概览", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))]),
    ];
    let mut items = lines;
    if let Ok(stats) = db::get_usage_stats(None, None, None) {
        if stats.is_empty() {
            items.push(Line::from("  (暂无用量记录)"));
        } else {
            for s in &stats {
                items.push(Line::from(format!(
                    "  {:<16} 花费=${:.6}  请求={}",
                    s.model_name, s.total_cost, s.request_count
                )));
            }
        }
    }
    if let Ok(total) = db::get_total_spend(None, None, None) {
        items.push(Line::from(format!("  累计花费: ${:.6}", total)));
    }
    let block = Block::default().title(" 概览 ").borders(Borders::ALL);
    f.render_widget(Paragraph::new(items).block(block), area);
}

fn render_chat(f: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    let mut items: Vec<ListItem> = app
        .messages
        .iter()
        .map(|(role, content)| {
            let prefix = if role == "user" { "你: " } else { "AI: " };
            ListItem::new(format!("{prefix}{content}"))
        })
        .collect();
    if items.is_empty() {
        items.push(ListItem::new("输入消息开始聊天（Enter 发送）"));
    }
    let chat = List::new(items).block(Block::default().title(" 聊天 ").borders(Borders::ALL));
    f.render_widget(chat, chunks[0]);

    let input = Paragraph::new(app.input.as_str())
        .style(Style::default().fg(Color::Green))
        .block(Block::default().title(" 输入 ").borders(Borders::ALL));
    f.render_widget(input, chunks[1]);
    f.set_cursor_position((chunks[1].x + 1 + app.input.len() as u16, chunks[1].y + 1));
}

fn render_models(f: &mut Frame, area: Rect) {
    let mut items = vec![ListItem::new(format!(
        "{:<16} {:<12} {:<32} cost/tok",
        "名称", "提供商", "模型"
    )).style(Style::default().add_modifier(Modifier::BOLD))];

    match db::list_models(true) {
        Ok(models) => {
            if models.is_empty() {
                items.push(ListItem::new("  (无模型 — 用 lloom-cli models add 注册)"));
            } else {
                for m in &models {
                    items.push(ListItem::new(format!(
                        "{:<16} {:<12} {:<32} in=${:.6}/tok",
                        m.name, m.provider, m.litellm_model, m.input_cost_per_token
                    )));
                }
            }
        }
        Err(e) => items.push(ListItem::new(format!("  读取失败: {e}"))),
    }
    let list = List::new(items).block(Block::default().title(" 模型 ").borders(Borders::ALL));
    f.render_widget(list, area);
}

fn ai_status() -> String {
    let rt = tokio::runtime::Runtime::new().expect("tokio");
    let h = rt.block_on(lloom_core::processes::check_ai_health());
    if h.status == "ok" {
        if h.ready { "Up (ready)".into() } else { "Up (not ready)".into() }
    } else {
        "Down".into()
    }
}

fn ollama_status() -> String {
    let rt = tokio::runtime::Runtime::new().expect("tokio");
    if rt.block_on(lloom_core::processes::check_ollama_health()) {
        "Up".into()
    } else {
        "Down".into()
    }
}
