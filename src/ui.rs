use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{
        Axis, Block, Borders, Cell, Chart, Clear, Dataset, GraphType, List, ListItem, ListState,
        Paragraph, Row, Table, TableState, Tabs, Wrap,
    },
};

use crate::app::{
    AiState, App, ChatMessage, ChatRole, ChatState, HistoryForm, HistoryMode, MenuMode, PERIODS,
    Screen, StockState, TABS,
};
use crate::financials::{self, FinancialSelection};
use crate::models::{IndicatorData, StockIndicators};

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ─── Colour palette ───────────────────────────────────────────────────────────
const C_TITLE: Color = Color::Cyan;
const C_LABEL: Color = Color::White;
const C_VALUE: Color = Color::Yellow;
const C_POS: Color = Color::Green;
const C_NEG: Color = Color::Red;
const C_DIM: Color = Color::DarkGray;
const C_TAB: Color = Color::Cyan;

// ─── Entry point ─────────────────────────────────────────────────────────────

pub fn render(f: &mut Frame, app: &App) {
    let area = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title bar
            Constraint::Min(0),    // content
            Constraint::Length(1), // status bar
        ])
        .split(area);

    render_title_bar(f, chunks[0], app);
    render_content(f, chunks[1], app);
    render_status_bar(f, chunks[2], app);
}

// ─── Title bar ────────────────────────────────────────────────────────────────

fn render_title_bar(f: &mut Frame, area: Rect, app: &App) {
    let suffix = match app.screen {
        Screen::Menu => " · Menu".to_string(),
        Screen::History => " · History".to_string(),
        Screen::Stock => match &app.stock.state {
            StockState::Loaded(data) => format!(" · {}", data.ticker),
            StockState::Loading(t) => format!(" · {t}"),
            StockState::Error { ticker, .. } => format!(" · {ticker}"),
            StockState::Input => " · Stock".to_string(),
        },
    };

    let title = Line::from(vec![
        Span::styled(
            " Graham",
            Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(suffix, Style::default().fg(C_DIM)),
    ]);

    f.render_widget(Paragraph::new(title), area);
}

// ─── Status bar ───────────────────────────────────────────────────────────────

fn render_status_bar(f: &mut Frame, area: Rect, app: &App) {
    let news_tab = TABS
        .iter()
        .position(|&tab| tab == "News")
        .unwrap_or_else(|| TABS.len().saturating_sub(1));
    let ai_tab = TABS.iter().position(|&tab| tab == "AI").unwrap_or(2);
    let financials_tab = TABS
        .iter()
        .position(|&tab| tab == "Financials")
        .unwrap_or(1);
    let hint = match app.screen {
        Screen::Menu => match app.menu_mode {
            MenuMode::Idle => " h history  |  s stock  |  q quit  |  Ctrl+C quit",
            MenuMode::StockInput => " Type ticker  |  Enter search  |  Esc cancel  |  Ctrl+C quit",
        },
        Screen::History => match &app.history.mode {
            HistoryMode::View => {
                " j/k or ↑↓ move  |  Enter open  |  a add  |  e edit  |  d delete  |  r refresh  |  Esc menu  |  Ctrl+C quit"
            }
            HistoryMode::Add(_) | HistoryMode::Edit(_) => {
                " Enter next/save  |  Tab next  |  Shift+Tab prev  |  Esc cancel  |  Ctrl+C quit"
            }
            HistoryMode::DeleteConfirm { .. } => {
                " y confirm delete  |  n / Esc cancel  |  Ctrl+C quit"
            }
        },
        Screen::Stock => {
            let is_news = app.stock.active_tab == news_tab;
            let is_financials = app.stock.active_tab == financials_tab;
            let is_ai = app.stock.active_tab == ai_tab;
            match (&app.stock.state, app.stock.active_tab) {
                (StockState::Input, _) => " Enter ticker and press ↵  |  Esc menu  |  Ctrl+C quit",
                (StockState::Loading(_), _) => " Loading…  |  Ctrl+C quit",
                (StockState::Loaded(_), _) if is_news => {
                    " ↑ ↓  select  |  Enter open  |  r refresh  |  o f a n  jump tab  |  q / Esc back  |  Ctrl+C quit"
                }
                (StockState::Loaded(_), _) if is_financials => {
                    if app.stock.financials_modal.is_some() {
                        " Enter / Esc close  |  o f a n  jump tab  |  q / Esc back  |  Ctrl+C quit"
                    } else {
                        " h j k l  move  |  Enter open  |  o f a n  jump tab  |  q / Esc back  |  Ctrl+C quit"
                    }
                }
                (StockState::Loaded(_), _) if is_ai => {
                    " Type message  |  Enter send  |  ↑ ↓ / PgUp PgDn  scroll  |  ← → / Tab  cycle  |  q / Esc back  |  Ctrl+C quit"
                }
                (StockState::Loaded(_), 0) => {
                    " o f a n  jump tab  |  ← →  cycle  |  1-4 / , .  period  |  q / Esc back  |  Ctrl+C quit"
                }
                (StockState::Loaded(_), _) => {
                    " o f a n  jump tab  |  ← →  cycle  |  q / Esc back  |  Ctrl+C quit"
                }
                (StockState::Error { .. }, _) => " q / Esc back to search  |  Ctrl+C quit",
            }
        }
    };

    let (text, style) = match &app.status_message {
        Some(msg) => (msg.as_str(), Style::default().fg(C_NEG)),
        None => (hint, Style::default().fg(C_DIM)),
    };

    f.render_widget(Paragraph::new(text).style(style), area);
}

// ─── Content router ──────────────────────────────────────────────────────────

fn render_content(f: &mut Frame, area: Rect, app: &App) {
    match app.screen {
        Screen::Menu => render_menu(f, area, app),
        Screen::History => render_history(f, area, app),
        Screen::Stock => match &app.stock.state {
            StockState::Input => render_stock_input(f, area, app),
            StockState::Loading(t) => render_loading(f, area, t, app.tick),
            StockState::Loaded(data) => render_loaded(
                f,
                area,
                data,
                app.stock.active_tab,
                app.stock.active_period,
                &app.stock.ai_state,
                &app.stock.chat_state,
                &app.stock.chat_messages,
                app.stock.chat_input.as_str(),
                app.stock.chat_scroll,
                app.openrouter_key.is_some(),
                app.tick,
                app.stock.news_selected,
                app.stock.financials_selected,
                app.stock.financials_modal,
            ),
            StockState::Error { ticker, message } => render_error(f, area, ticker, message),
        },
    }
}

const LOGO: &[&str] = &[
    r"  ____  ____    _    _   _    _    __  __ ",
    r" / ___||  _ \  / \  | | | |  / \  |  \/  |",
    r"| |  _ | |_) |/ _ \ | |_| | / _ \ | |\/| |",
    r"| |_| ||  _ </ ___ \|  _  |/ ___ \| |  | |",
    r" \____||_| \_/_/   \_\_| |_/_/   \_\_|  |_|",
];

const TAGLINE: &[&str] = &[
    "AI-powered stock analysis for B3 and US markets",
    "10-year fundamental history · valuation · debt · efficiency",
];

// ─── Menu screen ────────────────────────────────────────────────────────────

fn render_menu(f: &mut Frame, area: Rect, app: &App) {
    // logo(5) + gap(1) + menu(4) = 10 rows
    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),    // top padding
            Constraint::Length(5), // logo
            Constraint::Length(1), // gap
            Constraint::Length(4), // menu
            Constraint::Min(0),    // bottom padding
        ])
        .split(area);

    // Logo — each line coloured cyan, bold
    let logo_lines: Vec<Line> = LOGO
        .iter()
        .map(|l| {
            Line::from(Span::styled(
                *l,
                Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
            ))
        })
        .collect();
    f.render_widget(
        Paragraph::new(logo_lines).alignment(Alignment::Center),
        vchunks[1],
    );

    // Menu — horizontally centred
    let hchunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(35),
            Constraint::Percentage(30),
            Constraint::Percentage(35),
        ])
        .split(vchunks[3]);

    let menu_area = hchunks[1];
    let label_width = 18usize;
    let rows = vec![("[H] History", "h"), ("[S] Stocks", "s")];

    let lines: Vec<Line> = rows
        .into_iter()
        .map(|(label, key)| {
            Line::from(vec![
                Span::styled(
                    format!("{label:<label_width$}"),
                    Style::default().fg(C_LABEL),
                ),
                Span::styled(key, Style::default().fg(C_TAB).add_modifier(Modifier::BOLD)),
            ])
        })
        .collect();

    f.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        menu_area,
    );

    if matches!(app.menu_mode, MenuMode::StockInput) {
        render_menu_stock_input(f, area, app);
    }
}

fn render_menu_stock_input(f: &mut Frame, area: Rect, app: &App) {
    let modal_area = centered_rect(50, 20, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_TAB))
        .title(Span::styled(
            " Stock Ticker ",
            Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(modal_area);
    f.render_widget(Clear, modal_area);
    f.render_widget(block, modal_area);

    let line = Line::from(vec![
        Span::styled("Ticker: ", Style::default().fg(C_LABEL)),
        Span::styled(
            app.stock.input.clone(),
            Style::default().fg(C_VALUE).add_modifier(Modifier::BOLD),
        ),
        Span::styled("█", Style::default().fg(C_VALUE)),
    ]);
    f.render_widget(Paragraph::new(line).alignment(Alignment::Center), inner);
}

// ─── Stock input screen ──────────────────────────────────────────────────────

fn render_stock_input(f: &mut Frame, area: Rect, app: &App) {
    // logo(5) + gap(1) + tagline(2) + gap(1) + search box(3) = 12 rows
    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),    // top padding
            Constraint::Length(5), // logo
            Constraint::Length(1), // gap
            Constraint::Length(2), // tagline
            Constraint::Length(1), // gap
            Constraint::Length(3), // search box
            Constraint::Min(0),    // bottom padding
        ])
        .split(area);

    // Logo — each line coloured cyan, bold
    let logo_lines: Vec<Line> = LOGO
        .iter()
        .map(|l| {
            Line::from(Span::styled(
                *l,
                Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
            ))
        })
        .collect();
    f.render_widget(
        Paragraph::new(logo_lines).alignment(Alignment::Center),
        vchunks[1],
    );

    // Tagline
    let tagline_lines: Vec<Line> = TAGLINE
        .iter()
        .map(|l| Line::from(Span::styled(*l, Style::default().fg(C_DIM))))
        .collect();
    f.render_widget(
        Paragraph::new(tagline_lines).alignment(Alignment::Center),
        vchunks[3],
    );

    // Search box — horizontally centred
    let hchunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
        ])
        .split(vchunks[5]);

    let box_area = hchunks[1];
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_TAB))
        .title(Span::styled(
            " Search ",
            Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(box_area);

    f.render_widget(Clear, box_area);
    f.render_widget(block, box_area);

    let prompt = Line::from(vec![
        Span::styled("Ticker: ", Style::default().fg(C_LABEL)),
        Span::styled(
            app.stock.input.clone(),
            Style::default().fg(C_VALUE).add_modifier(Modifier::BOLD),
        ),
        Span::styled("█", Style::default().fg(C_VALUE)),
    ]);
    f.render_widget(Paragraph::new(prompt).alignment(Alignment::Center), inner);
}

// ─── Loading screen ──────────────────────────────────────────────────────────

fn render_loading(f: &mut Frame, area: Rect, ticker: &str, tick: u64) {
    let spinner = SPINNER[(tick as usize / 2) % SPINNER.len()];

    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(45),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);

    let text = Line::from(vec![
        Span::styled(format!("{spinner} "), Style::default().fg(C_TAB)),
        Span::styled(format!("Fetching {ticker}…"), Style::default().fg(C_LABEL)),
    ]);

    f.render_widget(
        Paragraph::new(text).alignment(Alignment::Center),
        vchunks[1],
    );
}

// ─── Error screen ────────────────────────────────────────────────────────────

fn render_error(f: &mut Frame, area: Rect, ticker: &str, message: &str) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_NEG))
        .title(Span::styled(
            format!(" Error — {ticker} "),
            Style::default().fg(C_NEG).add_modifier(Modifier::BOLD),
        ));

    let text = Paragraph::new(message)
        .block(block)
        .wrap(Wrap { trim: true })
        .style(Style::default().fg(C_LABEL));

    f.render_widget(text, area);
}

// ─── History screen ───────────────────────────────────────────────────────────

fn render_history(f: &mut Frame, area: Rect, app: &App) {
    let rows_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    let trades_block = Block::default().borders(Borders::ALL).title(Span::styled(
        " Trade History ",
        Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
    ));

    if app.history.trades.is_empty() {
        f.render_widget(
            Paragraph::new("No trades yet. Press 'a' to add.")
                .block(trades_block)
                .style(Style::default().fg(C_DIM))
                .alignment(Alignment::Center),
            rows_area[0],
        );
    } else {
        let header = Row::new(vec![
            Cell::from("Date").style(Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD)),
            Cell::from("Ticker").style(Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD)),
            Cell::from("Side").style(Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD)),
            Cell::from("Shares").style(Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD)),
            Cell::from("Price").style(Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD)),
        ]);

        let table_rows: Vec<Row> = app
            .history
            .trades
            .iter()
            .map(|row| {
                let side_color = if row.side == "BUY" { C_POS } else { C_NEG };

                Row::new(vec![
                    Cell::from(fmt_date(row.date.as_str())).style(Style::default().fg(C_DIM)),
                    Cell::from(row.ticker.as_str()).style(Style::default().fg(C_LABEL)),
                    Cell::from(row.side.as_str()).style(Style::default().fg(side_color)),
                    Cell::from(fmt_qty(row.shares)).style(Style::default().fg(C_VALUE)),
                    Cell::from(fmt_price(row.price)).style(Style::default().fg(C_VALUE)),
                ])
            })
            .collect();

        let widths = [
            Constraint::Length(12),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(12),
            Constraint::Length(10),
        ];

        let mut state = TableState::default();
        state.select(Some(
            app.history
                .selected
                .min(app.history.trades.len().saturating_sub(1)),
        ));

        let table = Table::new(table_rows, widths)
            .header(header)
            .block(trades_block)
            .row_highlight_style(Style::default().fg(C_TAB).add_modifier(Modifier::BOLD))
            .highlight_symbol("▶ ");

        f.render_stateful_widget(table, rows_area[0], &mut state);
    }

    render_history_summary(f, rows_area[1], app);

    match &app.history.mode {
        HistoryMode::Add(form) => render_history_form(f, area, "Add Trade", form),
        HistoryMode::Edit(form) => render_history_form(f, area, "Edit Trade", form),
        HistoryMode::DeleteConfirm { ticker, .. } => render_delete_confirm(f, area, ticker),
        HistoryMode::View => {}
    }
}

fn render_history_summary(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title(Span::styled(
        " Holdings (Average Cost) ",
        Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
    ));

    if app.history.holdings.is_empty() {
        f.render_widget(
            Paragraph::new("No open holdings yet.")
                .block(block)
                .style(Style::default().fg(C_DIM))
                .alignment(Alignment::Center),
            area,
        );
        return;
    }

    let header = Row::new(vec![
        Cell::from("Ticker").style(Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD)),
        Cell::from("Shares").style(Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD)),
        Cell::from("Avg Cost").style(Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD)),
        Cell::from("Current").style(Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD)),
        Cell::from("P/L $").style(Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD)),
        Cell::from("P/L %").style(Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD)),
    ]);

    let rows: Vec<Row> = app
        .history
        .holdings
        .iter()
        .map(|row| {
            let current_text = row
                .current_price
                .map(fmt_price)
                .unwrap_or_else(|| "—".to_string());

            let (pl_text, pl_pct_text, pl_color) = match row.current_price {
                Some(current) => {
                    let pl = (current - row.avg_cost) * row.shares;
                    let pl_pct = if row.avg_cost.abs() > f64::EPSILON {
                        (current - row.avg_cost) / row.avg_cost * 100.0
                    } else {
                        0.0
                    };
                    let color = if pl >= 0.0 { C_POS } else { C_NEG };
                    (fmt_money(pl), fmt_pct(pl_pct), color)
                }
                None => ("—".to_string(), "—".to_string(), C_DIM),
            };

            Row::new(vec![
                Cell::from(row.ticker.as_str()).style(Style::default().fg(C_LABEL)),
                Cell::from(fmt_qty(row.shares)).style(Style::default().fg(C_VALUE)),
                Cell::from(fmt_price(row.avg_cost)).style(Style::default().fg(C_VALUE)),
                Cell::from(current_text).style(Style::default().fg(C_VALUE)),
                Cell::from(pl_text).style(Style::default().fg(pl_color)),
                Cell::from(pl_pct_text).style(Style::default().fg(pl_color)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths).header(header).block(block);

    f.render_widget(table, area);
}

fn render_history_form(f: &mut Frame, area: Rect, title: &str, form: &HistoryForm) {
    let modal_area = centered_rect(60, 40, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_TAB))
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(modal_area);
    f.render_widget(Clear, modal_area);
    f.render_widget(block, modal_area);

    let fields = [
        ("Ticker", form.ticker.as_str()),
        ("Side", form.side.as_str()),
        ("Shares", form.shares.as_str()),
        ("Price", form.price.as_str()),
        ("Date", form.date.as_str()),
    ];

    let lines: Vec<Line> = fields
        .iter()
        .enumerate()
        .map(|(idx, (label, value))| {
            let is_active = idx == form.active_field;
            let value_display = if is_active {
                format!("{value}█")
            } else {
                value.to_string()
            };
            let label_style = if is_active {
                Style::default().fg(C_TAB).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(C_DIM)
            };
            let value_style = if is_active {
                Style::default().fg(C_VALUE).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(C_LABEL)
            };
            Line::from(vec![
                Span::styled(format!("{label:<10}"), label_style),
                Span::styled(value_display, value_style),
            ])
        })
        .collect();

    f.render_widget(Paragraph::new(lines).alignment(Alignment::Left), inner);
}

fn render_delete_confirm(f: &mut Frame, area: Rect, ticker: &str) {
    let modal_area = centered_rect(50, 20, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_NEG))
        .title(Span::styled(
            " Delete Position ",
            Style::default().fg(C_NEG).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(modal_area);
    f.render_widget(Clear, modal_area);
    f.render_widget(block, modal_area);

    let text = format!("Delete position for {ticker}? (y/n)");
    f.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(C_LABEL)),
        inner,
    );
}

// ─── Loaded screen ───────────────────────────────────────────────────────────

fn render_loaded(
    f: &mut Frame,
    area: Rect,
    data: &StockIndicators,
    active_tab: usize,
    active_period: usize,
    ai_state: &AiState,
    chat_state: &ChatState,
    chat_messages: &[ChatMessage],
    chat_input: &str,
    chat_scroll: usize,
    ai_enabled: bool,
    tick: u64,
    news_selected: usize,
    financials_selected: FinancialSelection,
    financials_modal: Option<FinancialSelection>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    // Tab bar
    let tab_titles: Vec<Line> = TABS
        .iter()
        .map(|t| Line::from(Span::styled(*t, Style::default().fg(C_LABEL))))
        .collect();

    let tabs = Tabs::new(tab_titles)
        .select(active_tab)
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .fg(C_TAB)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        );

    f.render_widget(tabs, chunks[0]);

    // Tab content
    let financials_tab = TABS.iter().position(|&tab| tab == "Financials").unwrap_or(1);
    let ai_tab = TABS.iter().position(|&tab| tab == "AI").unwrap_or(2);
    let news_tab = TABS
        .iter()
        .position(|&tab| tab == "News")
        .unwrap_or_else(|| TABS.len().saturating_sub(1));

    match active_tab {
        0 => render_overview(f, chunks[1], data, active_period, ai_state, tick),
        tab if tab == financials_tab => render_financials(f, chunks[1], data, financials_selected),
        tab if tab == ai_tab => render_ai_tab(
            f,
            chunks[1],
            data,
            chat_state,
            chat_messages,
            chat_input,
            chat_scroll,
            ai_enabled,
            tick,
        ),
        tab if tab == news_tab => render_news(f, chunks[1], data, news_selected),
        _ => {}
    }

    if active_tab == financials_tab {
        if let Some(selection) = financials_modal {
            let sections = financials::sections(data);
            let selection = financials::clamp_selection(selection, &sections);
            if let Some((name, indicator)) =
                financials::indicator_from_selection(&sections, selection)
            {
                render_indicator_modal(f, area, name, indicator);
            }
        }
    }
}

// ─── Overview tab ────────────────────────────────────────────────────────────

fn render_overview(
    f: &mut Frame,
    area: Rect,
    data: &StockIndicators,
    active_period: usize,
    ai_state: &AiState,
    tick: u64,
) {
    // Three rows: chart | metrics/ratios | AI analysis
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(45),
            Constraint::Percentage(30),
            Constraint::Percentage(25),
        ])
        .split(area);

    render_price_chart(f, rows[0], data, active_period);

    // ── Middle: price metrics + key ratios ──
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);

    let price_lines = vec![
        kv("Current Price", fmt_price(data.current_price)),
        kv("52w Low", fmt_price(data.min_52w)),
        kv("52w High", fmt_price(data.max_52w)),
        kv("Month Low", fmt_price(data.min_month)),
        kv("Month High", fmt_price(data.max_month)),
        kv_colored(
            "Growth 12m",
            fmt_pct(data.growth_12m),
            sign_color(data.growth_12m),
        ),
        kv_colored(
            "Growth Month",
            fmt_pct(data.growth_month),
            sign_color(data.growth_month),
        ),
        kv("Dividend Yield", fmt_pct(data.dividend_yield)),
    ];

    f.render_widget(
        Paragraph::new(price_lines)
            .block(Block::default().borders(Borders::ALL).title(Span::styled(
                " Price ",
                Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
            )))
            .style(Style::default().fg(C_LABEL)),
        columns[0],
    );

    let v = &data.valuation;
    let ratio_lines = vec![
        kv("P/E", fmt_val(v.p_e.current)),
        kv("P/B", fmt_val(v.p_b.current)),
        kv("P/S", fmt_val(v.p_s.current)),
        kv("EV/EBITDA", fmt_val(v.ev_ebitda.current)),
        kv("EV/EBIT", fmt_val(v.ev_ebit.current)),
        kv("ROE", fmt_pct(data.profitability.roe.current)),
        kv("ROA", fmt_pct(data.profitability.roa.current)),
        kv("ROIC", fmt_pct(data.profitability.roic.current)),
        kv("Net Margin", fmt_pct(data.efficiency.net_margin.current)),
        kv(
            "Gross Margin",
            fmt_pct(data.efficiency.gross_margin.current),
        ),
    ];

    f.render_widget(
        Paragraph::new(ratio_lines)
            .block(Block::default().borders(Borders::ALL).title(Span::styled(
                " Key Ratios ",
                Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
            )))
            .style(Style::default().fg(C_LABEL)),
        columns[1],
    );

    // ── Bottom: AI analysis panel ──
    render_ai_analysis(f, rows[2], ai_state, tick);
}

// ─── AI tab ───────────────────────────────────────────────────────────────────

fn render_ai_tab(
    f: &mut Frame,
    area: Rect,
    data: &StockIndicators,
    chat_state: &ChatState,
    chat_messages: &[ChatMessage],
    chat_input: &str,
    chat_scroll: usize,
    ai_enabled: bool,
    tick: u64,
) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(75), Constraint::Percentage(25)])
        .split(area);

    render_ai_chat_log(
        f,
        rows[0],
        chat_messages,
        chat_state,
        chat_scroll,
        ai_enabled,
        tick,
    );
    render_ai_chat_input(f, rows[1], chat_input, ai_enabled, chat_state, &data.ticker);
}

fn render_ai_chat_log(
    f: &mut Frame,
    area: Rect,
    chat_messages: &[ChatMessage],
    chat_state: &ChatState,
    chat_scroll: usize,
    ai_enabled: bool,
    tick: u64,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_TAB))
        .title(Span::styled(
            " AI Chat ",
            Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
        ));

    if !ai_enabled {
        f.render_widget(
            Paragraph::new(
                "AI chat is unavailable. Define the OPENROUTER_API_KEY environment variable.",
            )
            .block(block)
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(C_DIM)),
            area,
        );
        return;
    }

    let mut lines = if chat_messages.is_empty() {
        vec![Line::from(Span::styled(
            "Ask a question about this stock to start the conversation.",
            Style::default().fg(C_DIM),
        ))]
    } else {
        format_chat_lines(chat_messages)
    };

    if matches!(chat_state, ChatState::Loading) {
        let spinner = SPINNER[(tick as usize / 2) % SPINNER.len()];
        lines.push(Line::from(vec![
            Span::styled(format!("{spinner} "), Style::default().fg(C_TAB)),
            Span::styled("AI is responding…", Style::default().fg(C_DIM)),
        ]));
    }

    if let ChatState::Failed(msg) = chat_state {
        lines.push(Line::from(Span::styled(
            format!("Error: {msg}"),
            Style::default().fg(C_NEG),
        )));
    }

    let max_scroll = lines.len().saturating_sub(area.height as usize);
    let scroll = chat_scroll.min(max_scroll).min(u16::MAX as usize) as u16;

    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: true })
            .scroll((scroll, 0))
            .style(Style::default().fg(C_LABEL)),
        area,
    );
}

fn render_ai_chat_input(
    f: &mut Frame,
    area: Rect,
    chat_input: &str,
    ai_enabled: bool,
    chat_state: &ChatState,
    ticker: &str,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_TAB))
        .title(Span::styled(
            " Ask AI ",
            Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
        ));

    if !ai_enabled {
        f.render_widget(
            Paragraph::new("OPENROUTER_API_KEY is not set.")
                .block(block)
                .style(Style::default().fg(C_DIM)),
            area,
        );
        return;
    }

    if matches!(chat_state, ChatState::Loading) && chat_input.is_empty() {
        f.render_widget(
            Paragraph::new("Waiting for the AI response…")
                .block(block)
                .style(Style::default().fg(C_DIM)),
            area,
        );
        return;
    }

    let placeholder = format!("Ask about {ticker} and press Enter.");
    let (text, style) = if chat_input.is_empty() {
        (placeholder, Style::default().fg(C_DIM))
    } else {
        (chat_input.to_string(), Style::default().fg(C_LABEL))
    };

    f.render_widget(Paragraph::new(text).block(block).style(style), area);
}

fn format_chat_lines(chat_messages: &[ChatMessage]) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    for message in chat_messages {
        let (label, color) = match message.role {
            ChatRole::User => ("You", C_VALUE),
            ChatRole::Assistant => ("AI", C_TAB),
        };

        let mut content_lines = message.content.lines();
        let first = content_lines.next().unwrap_or_default();
        lines.push(Line::from(vec![
            Span::styled(
                format!("{label}: "),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(first.to_string(), Style::default().fg(C_LABEL)),
        ]));

        for line in content_lines {
            lines.push(Line::from(vec![
                Span::raw("     "),
                Span::styled(line.to_string(), Style::default().fg(C_LABEL)),
            ]));
        }

        lines.push(Line::from(""));
    }

    if !lines.is_empty() {
        lines.pop();
    }

    lines
}

// ─── Financials tab ──────────────────────────────────────────────────────────

fn render_financials(
    f: &mut Frame,
    area: Rect,
    data: &StockIndicators,
    selection: FinancialSelection,
) {
    let sections = financials::sections(data);
    let selection = financials::clamp_selection(selection, &sections);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(area);

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[0]);
    render_indicator_table(
        f,
        top[0],
        sections.get(0).map(|s| s.title).unwrap_or("Valuation"),
        sections.get(0).map(|s| s.rows.as_slice()).unwrap_or(&[]),
        if selection.section == 0 {
            Some(selection.row)
        } else {
            None
        },
    );
    render_indicator_table(
        f,
        top[1],
        sections.get(1).map(|s| s.title).unwrap_or("Debt"),
        sections.get(1).map(|s| s.rows.as_slice()).unwrap_or(&[]),
        if selection.section == 1 {
            Some(selection.row)
        } else {
            None
        },
    );

    let middle = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);
    render_indicator_table(
        f,
        middle[0],
        sections.get(2).map(|s| s.title).unwrap_or("Efficiency"),
        sections.get(2).map(|s| s.rows.as_slice()).unwrap_or(&[]),
        if selection.section == 2 {
            Some(selection.row)
        } else {
            None
        },
    );
    render_indicator_table(
        f,
        middle[1],
        sections.get(3).map(|s| s.title).unwrap_or("Profitability"),
        sections.get(3).map(|s| s.rows.as_slice()).unwrap_or(&[]),
        if selection.section == 3 {
            Some(selection.row)
        } else {
            None
        },
    );

    render_indicator_table(
        f,
        rows[2],
        sections.get(4).map(|s| s.title).unwrap_or("Growth"),
        sections.get(4).map(|s| s.rows.as_slice()).unwrap_or(&[]),
        if selection.section == 4 {
            Some(selection.row)
        } else {
            None
        },
    );
}

// ─── News tab ────────────────────────────────────────────────────────────────

fn render_news(f: &mut Frame, area: Rect, data: &StockIndicators, selected: usize) {
    let block = Block::default().borders(Borders::ALL).title(Span::styled(
        " News ",
        Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
    ));

    let Some(items) = data.news.as_ref() else {
        f.render_widget(
            Paragraph::new("No news available for this ticker.")
                .block(block)
                .style(Style::default().fg(C_DIM)),
            area,
        );
        return;
    };

    if items.is_empty() {
        f.render_widget(
            Paragraph::new("No news available for this ticker.")
                .block(block)
                .style(Style::default().fg(C_DIM)),
            area,
        );
        return;
    }

    let list_items: Vec<ListItem> = items
        .iter()
        .map(|item| {
            let source = item.publisher.as_deref().unwrap_or("Yahoo Finance");
            let meta = match item.published_at.as_deref() {
                Some(date) if !date.is_empty() => format!("{source} • {date}"),
                _ => source.to_string(),
            };
            ListItem::new(vec![
                Line::from(Span::styled(
                    item.title.as_str(),
                    Style::default().fg(C_LABEL),
                )),
                Line::from(Span::styled(meta, Style::default().fg(C_DIM))),
            ])
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(selected.min(items.len().saturating_sub(1))));

    let list = List::new(list_items)
        .block(block)
        .highlight_symbol("▶ ")
        .highlight_style(Style::default().fg(C_TAB).add_modifier(Modifier::BOLD));

    f.render_stateful_widget(list, area, &mut state);
}

fn render_ai_analysis(f: &mut Frame, area: Rect, ai_state: &AiState, tick: u64) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_TAB))
        .title(Span::styled(
            " AI Analysis ",
            Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
        ));

    match ai_state {
        AiState::Loading => {
            let spinner = SPINNER[(tick as usize / 2) % SPINNER.len()];
            let text = Line::from(vec![
                Span::styled(format!("{spinner} "), Style::default().fg(C_TAB)),
                Span::styled("Analysing…", Style::default().fg(C_DIM)),
            ]);
            f.render_widget(
                Paragraph::new(text)
                    .block(block)
                    .style(Style::default().fg(C_LABEL)),
                area,
            );
        }
        AiState::Done(text) => {
            f.render_widget(
                Paragraph::new(text.as_str())
                    .block(block)
                    .wrap(Wrap { trim: true })
                    .style(Style::default().fg(C_LABEL)),
                area,
            );
        }
        AiState::Failed(msg) => {
            f.render_widget(
                Paragraph::new(format!("Error: {msg}"))
                    .block(block)
                    .wrap(Wrap { trim: true })
                    .style(Style::default().fg(C_NEG)),
                area,
            );
        }
        AiState::Unavailable => {
            f.render_widget(
                Paragraph::new(
                    "AI capabilities are not online. \
                    Define the OPENROUTER_API_KEY environment variable to enable AI analysis.",
                )
                .block(block)
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(C_DIM)),
                area,
            );
        }
    }
}

/// Approximate trading-day counts for each period label.
const PERIOD_DAYS: [usize; 4] = [22, 130, 252, usize::MAX];

fn render_price_chart(f: &mut Frame, area: Rect, data: &StockIndicators, active_period: usize) {
    let all = &data.price_history;
    let count = PERIOD_DAYS[active_period];
    let prices = if count >= all.len() {
        all.as_slice()
    } else {
        &all[all.len() - count..]
    };

    // Period selector label in title
    let period_spans: Vec<Span> = PERIODS
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            if i == active_period {
                Span::styled(
                    format!(" [{p}] "),
                    Style::default().fg(C_TAB).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(format!(" {p} "), Style::default().fg(C_DIM))
            }
        })
        .collect();

    let block = Block::default().borders(Borders::ALL).title(Line::from(
        std::iter::once(Span::styled(
            " Price Chart ",
            Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
        ))
        .chain(period_spans)
        .collect::<Vec<_>>(),
    ));

    let chart_data: Vec<(f64, f64)> = prices
        .iter()
        .enumerate()
        .map(|(i, p)| (i as f64, p.price))
        .collect();

    let min_price = prices.iter().map(|p| p.price).fold(f64::INFINITY, f64::min);
    let max_price = prices
        .iter()
        .map(|p| p.price)
        .fold(f64::NEG_INFINITY, f64::max);
    let price_range = (max_price - min_price).max(0.01);
    let y_min = min_price - price_range * 0.05;
    let y_max = max_price + price_range * 0.05;
    let (first, middle, last) = match (prices.first(), prices.get(prices.len() / 2), prices.last())
    {
        (Some(first), Some(middle), Some(last)) => (first, middle, last),
        _ => {
            f.render_widget(
                Paragraph::new("No data available for this period.")
                    .block(block)
                    .style(Style::default().fg(C_DIM)),
                area,
            );
            return;
        }
    };
    let n = (prices.len() - 1) as f64;

    // X labels: first, middle, last date (strip time)
    let date_label = |date: &str| date.split(' ').next().unwrap_or(date).to_string();
    let x_labels = vec![
        Span::styled(date_label(&first.date), Style::default().fg(C_DIM)),
        Span::styled(date_label(&middle.date), Style::default().fg(C_DIM)),
        Span::styled(date_label(&last.date), Style::default().fg(C_DIM)),
    ];

    let line_color = if last.price >= first.price {
        C_POS
    } else {
        C_NEG
    };

    let dataset = Dataset::default()
        .graph_type(GraphType::Line)
        .marker(symbols::Marker::Braille)
        .style(Style::default().fg(line_color))
        .data(&chart_data);

    let chart = Chart::new(vec![dataset])
        .block(block)
        .x_axis(
            Axis::default()
                .bounds([0.0, n])
                .labels(x_labels)
                .style(Style::default().fg(C_DIM)),
        )
        .y_axis(
            Axis::default()
                .bounds([y_min, y_max])
                .labels(vec![
                    Span::styled(fmt_price(y_min), Style::default().fg(C_DIM)),
                    Span::styled(fmt_price((y_min + y_max) / 2.0), Style::default().fg(C_DIM)),
                    Span::styled(fmt_price(y_max), Style::default().fg(C_DIM)),
                ])
                .style(Style::default().fg(C_DIM)),
        );

    f.render_widget(chart, area);
}

// ─── Indicator history table ──────────────────────────────────────────────────

fn render_indicator_table(
    f: &mut Frame,
    area: Rect,
    title: &str,
    rows: &[(&str, &IndicatorData)],
    selected_row: Option<usize>,
) {
    if rows.is_empty() {
        let block = Block::default().borders(Borders::ALL).title(Span::styled(
            format!(" {title} "),
            Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
        ));
        f.render_widget(
            Paragraph::new("No indicators available.")
                .block(block)
                .style(Style::default().fg(C_DIM)),
            area,
        );
        return;
    }
    // Collect all years present across all indicators in this tab
    let mut years: Vec<i32> = rows
        .iter()
        .flat_map(|(_, d)| d.history.iter().map(|y| y.year))
        .collect();
    years.sort_unstable_by(|a, b| b.cmp(a));
    years.dedup();

    // Header
    let header: Vec<Cell> = std::iter::once(
        Cell::from("Indicator").style(Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD)),
    )
    .chain(std::iter::once(Cell::from("Current").style(
        Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
    )))
    .chain(years.iter().map(|y| {
        Cell::from(y.to_string()).style(Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD))
    }))
    .collect();

    // Rows
    let table_rows: Vec<Row> = rows
        .iter()
        .map(|(name, data)| {
            let cur_style = Style::default()
                .fg(sign_color(data.current))
                .add_modifier(Modifier::BOLD);

            let mut cells = vec![
                Cell::from(*name).style(Style::default().fg(C_LABEL)),
                Cell::from(fmt_val(data.current)).style(cur_style),
            ];

            for year in &years {
                let cell = data
                    .history
                    .iter()
                    .find(|y| y.year == *year)
                    .map(|y| {
                        Cell::from(fmt_val(y.value)).style(Style::default().fg(sign_color(y.value)))
                    })
                    .unwrap_or_else(|| Cell::from("—").style(Style::default().fg(C_DIM)));
                cells.push(cell);
            }

            Row::new(cells).height(1)
        })
        .collect();

    // Column widths: name=22, current=10, years=8 each
    let widths: Vec<Constraint> = std::iter::once(Constraint::Length(22))
        .chain(std::iter::once(Constraint::Length(10)))
        .chain(years.iter().map(|_| Constraint::Length(8)))
        .collect();

    let mut state = TableState::default();
    if let Some(selected) = selected_row {
        state.select(Some(selected.min(table_rows.len().saturating_sub(1))));
    }

    let table = Table::new(table_rows, widths)
        .header(Row::new(header).height(1).bottom_margin(0))
        .block(Block::default().borders(Borders::ALL).title(Span::styled(
            format!(" {title} "),
            Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
        )))
        .row_highlight_style(Style::default().fg(C_TAB).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");

    f.render_stateful_widget(table, area, &mut state);
}

fn render_indicator_modal(f: &mut Frame, area: Rect, title: &str, data: &IndicatorData) {
    let modal_area = centered_rect(70, 60, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_TAB))
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(modal_area);
    f.render_widget(Clear, modal_area);
    f.render_widget(block, modal_area);

    let mut history: Vec<_> = data.history.iter().collect();
    history.sort_by_key(|y| y.year);

    let mut labels: Vec<String> = Vec::new();
    let mut values: Vec<f64> = Vec::new();
    for item in history {
        labels.push(item.year.to_string());
        values.push(item.value);
    }
    labels.push("Now".to_string());
    values.push(data.current);

    if values.is_empty() {
        f.render_widget(
            Paragraph::new("No historical data available.")
                .style(Style::default().fg(C_DIM))
                .alignment(Alignment::Center),
            inner,
        );
        return;
    }

    let points: Vec<(f64, f64)> = values
        .iter()
        .enumerate()
        .map(|(i, v)| (i as f64, *v))
        .collect();

    let min_val = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_val = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    if !min_val.is_finite() || !max_val.is_finite() {
        f.render_widget(
            Paragraph::new("No historical data available.")
                .style(Style::default().fg(C_DIM))
                .alignment(Alignment::Center),
            inner,
        );
        return;
    }

    let span = (max_val - min_val).max(0.01);
    let y_min = min_val - span * 0.05;
    let y_max = max_val + span * 0.05;

    let last_idx = values.len().saturating_sub(1);
    let mid_idx = values.len() / 2;
    let x_labels = vec![
        Span::styled(
            labels.first().cloned().unwrap_or_default(),
            Style::default().fg(C_DIM),
        ),
        Span::styled(
            labels.get(mid_idx).cloned().unwrap_or_default(),
            Style::default().fg(C_DIM),
        ),
        Span::styled(
            labels.get(last_idx).cloned().unwrap_or_default(),
            Style::default().fg(C_DIM),
        ),
    ];

    let first = values.first().copied().unwrap_or(0.0);
    let last = values.last().copied().unwrap_or(0.0);
    let line_color = if last >= first { C_POS } else { C_NEG };

    let dataset = Dataset::default()
        .graph_type(GraphType::Line)
        .marker(symbols::Marker::Braille)
        .style(Style::default().fg(line_color))
        .data(&points);

    let mut x_max = last_idx as f64;
    if x_max <= 0.0 {
        x_max = 1.0;
    }

    let chart = Chart::new(vec![dataset])
        .x_axis(
            Axis::default()
                .bounds([0.0, x_max])
                .labels(x_labels)
                .style(Style::default().fg(C_DIM)),
        )
        .y_axis(
            Axis::default()
                .bounds([y_min, y_max])
                .labels(vec![
                    Span::styled(fmt_val(y_min), Style::default().fg(C_DIM)),
                    Span::styled(fmt_val((y_min + y_max) / 2.0), Style::default().fg(C_DIM)),
                    Span::styled(fmt_val(y_max), Style::default().fg(C_DIM)),
                ])
                .style(Style::default().fg(C_DIM)),
        );

    f.render_widget(chart, inner);
}

// ─── Formatting helpers ───────────────────────────────────────────────────────

fn fmt_val(v: f64) -> String {
    format!("{v:.2}")
}
fn fmt_pct(v: f64) -> String {
    format!("{v:.2}%")
}
fn fmt_price(v: f64) -> String {
    format!("{v:.2}")
}
fn fmt_money(v: f64) -> String {
    format!("{v:.2}")
}
fn fmt_qty(v: f64) -> String {
    format!("{v:.4}")
}
fn fmt_date(date: &str) -> String {
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() == 3 {
        if let (Ok(y), Ok(m), Ok(d)) = (
            parts[0].parse::<i32>(),
            parts[1].parse::<i32>(),
            parts[2].parse::<i32>(),
        ) {
            return format!("{:02}/{:02}/{:04}", d, m, y);
        }
    }
    date.to_string()
}

fn sign_color(v: f64) -> Color {
    if v < 0.0 { C_NEG } else { C_VALUE }
}

fn kv(label: &'static str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {label:<18}"), Style::default().fg(C_DIM)),
        Span::styled(
            value,
            Style::default().fg(C_VALUE).add_modifier(Modifier::BOLD),
        ),
    ])
}

fn kv_colored(label: &'static str, value: String, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {label:<18}"), Style::default().fg(C_DIM)),
        Span::styled(
            value,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ])
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1]);

    horizontal[1]
}
