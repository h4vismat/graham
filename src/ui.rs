use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{
        Axis, Block, Borders, Cell, Chart, Clear, Dataset, GraphType, List, ListItem, ListState,
        Paragraph, Row, Table, Tabs, Wrap,
    },
};

use crate::app::{AiState, App, PERIODS, State, TABS};
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
    let ticker_label = match &app.state {
        State::Loaded(data) => format!(" · {}", data.ticker),
        State::Loading(t) => format!(" · {t}"),
        State::Error { ticker, .. } => format!(" · {ticker}"),
        State::Input => String::new(),
    };

    let title = Line::from(vec![
        Span::styled(
            " Graham",
            Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(ticker_label, Style::default().fg(C_DIM)),
    ]);

    f.render_widget(Paragraph::new(title), area);
}

// ─── Status bar ───────────────────────────────────────────────────────────────

fn render_status_bar(f: &mut Frame, area: Rect, app: &App) {
    let is_news = app.active_tab + 1 == TABS.len();
    let hint = match (&app.state, app.active_tab) {
        (State::Input, _) => " Enter ticker and press ↵  |  Ctrl+C quit",
        (State::Loading(_), _) => " Loading…  |  Ctrl+C quit",
        (State::Loaded(_), _) if is_news => {
            " ↑ ↓  select  |  Enter open  |  r refresh  |  o v d e p g n  jump tab  |  q / Esc back  |  Ctrl+C quit"
        }
        (State::Loaded(_), 0) => {
            " o v d e p g n  jump tab  |  ← →  cycle  |  1-4 / , .  period  |  q / Esc back  |  Ctrl+C quit"
        }
        (State::Loaded(_), _) => {
            " o v d e p g n  jump tab  |  ← →  cycle  |  q / Esc back  |  Ctrl+C quit"
        }
        (State::Error { .. }, _) => " q / Esc back to search  |  Ctrl+C quit",
    };

    let (text, style) = match &app.status_message {
        Some(msg) => (msg.as_str(), Style::default().fg(C_NEG)),
        None => (hint, Style::default().fg(C_DIM)),
    };

    f.render_widget(Paragraph::new(text).style(style), area);
}

// ─── Content router ──────────────────────────────────────────────────────────

fn render_content(f: &mut Frame, area: Rect, app: &App) {
    match &app.state {
        State::Input => render_input(f, area, app),
        State::Loading(t) => render_loading(f, area, t, app.tick),
        State::Loaded(data) => {
            render_loaded(
                f,
                area,
                data,
                app.active_tab,
                app.active_period,
                &app.ai_state,
                app.tick,
                app.news_selected,
            )
        }
        State::Error { ticker, message } => render_error(f, area, ticker, message),
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

// ─── Input screen ────────────────────────────────────────────────────────────

fn render_input(f: &mut Frame, area: Rect, app: &App) {
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
            app.input.clone(),
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

// ─── Loaded screen ───────────────────────────────────────────────────────────

fn render_loaded(
    f: &mut Frame,
    area: Rect,
    data: &StockIndicators,
    active_tab: usize,
    active_period: usize,
    ai_state: &AiState,
    tick: u64,
    news_selected: usize,
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
    match active_tab {
        0 => render_overview(f, chunks[1], data, active_period, ai_state, tick),
        1 => render_indicator_table(f, chunks[1], "Valuation", &valuation_rows(data)),
        2 => render_indicator_table(f, chunks[1], "Debt", &debt_rows(data)),
        3 => render_indicator_table(f, chunks[1], "Efficiency", &efficiency_rows(data)),
        4 => render_indicator_table(f, chunks[1], "Profitability", &profitability_rows(data)),
        5 => render_indicator_table(f, chunks[1], "Growth", &growth_rows(data)),
        6 => render_news(f, chunks[1], data, news_selected),
        _ => {}
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
                Paragraph::new(text).block(block).style(Style::default().fg(C_LABEL)),
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
    let (first, middle, last) = match (
        prices.first(),
        prices.get(prices.len() / 2),
        prices.last(),
    ) {
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

fn render_indicator_table(f: &mut Frame, area: Rect, title: &str, rows: &[(&str, &IndicatorData)]) {
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

    let table = Table::new(table_rows, widths)
        .header(Row::new(header).height(1).bottom_margin(0))
        .block(Block::default().borders(Borders::ALL).title(Span::styled(
            format!(" {title} "),
            Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
        )));

    f.render_widget(table, area);
}

// ─── Row builders ────────────────────────────────────────────────────────────

fn valuation_rows(d: &StockIndicators) -> Vec<(&'static str, &IndicatorData)> {
    vec![
        ("DY", &d.valuation.dy),
        ("P/E", &d.valuation.p_e),
        ("P/B", &d.valuation.p_b),
        ("P/EBITDA", &d.valuation.p_ebitda),
        ("P/EBIT", &d.valuation.p_ebit),
        ("P/S", &d.valuation.p_s),
        ("P/Assets", &d.valuation.p_assets),
        ("P/Working Capital", &d.valuation.p_working_capital),
        ("P/Net Current Assets", &d.valuation.p_net_current_assets),
        ("EV/EBITDA", &d.valuation.ev_ebitda),
        ("EV/EBIT", &d.valuation.ev_ebit),
        ("EPS", &d.valuation.eps),
        ("BVPS", &d.valuation.bvps),
    ]
}

fn debt_rows(d: &StockIndicators) -> Vec<(&'static str, &IndicatorData)> {
    vec![
        ("Net Debt / Equity", &d.debt.net_debt_equity),
        ("Net Debt / EBITDA", &d.debt.net_debt_ebitda),
        ("Net Debt / EBIT", &d.debt.net_debt_ebit),
        ("Equity / Assets", &d.debt.equity_to_assets),
        ("Liabilities / Assets", &d.debt.liabilities_to_assets),
        ("Current Ratio", &d.debt.current_ratio),
    ]
}

fn efficiency_rows(d: &StockIndicators) -> Vec<(&'static str, &IndicatorData)> {
    vec![
        ("Gross Margin", &d.efficiency.gross_margin),
        ("EBITDA Margin", &d.efficiency.ebitda_margin),
        ("EBIT Margin", &d.efficiency.ebit_margin),
        ("Net Margin", &d.efficiency.net_margin),
    ]
}

fn profitability_rows(d: &StockIndicators) -> Vec<(&'static str, &IndicatorData)> {
    vec![
        ("ROE", &d.profitability.roe),
        ("ROA", &d.profitability.roa),
        ("ROIC", &d.profitability.roic),
        ("Asset Turnover", &d.profitability.asset_turnover),
    ]
}

fn growth_rows(d: &StockIndicators) -> Vec<(&'static str, &IndicatorData)> {
    vec![
        ("Revenue CAGR 5Y", &d.growth.revenue_cagr5),
        ("Earnings CAGR 5Y", &d.growth.earnings_cagr5),
    ]
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
