mod ai;
mod app;
mod models;
mod news;
mod positions;
mod profile;
mod scraper;
mod yahoo;
mod ui;

use std::io;
use std::process::Command;
use std::time::Duration;

use app::{
    AiState, App, MenuMode, PositionForm, PositionRow, PositionsMode, Screen, StockState, TABS,
};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use sqlx::SqlitePool;
use tokio::sync::mpsc;

struct PriceMessage {
    id: i64,
    price: f64,
}

enum PositionsMessage {
    List(Result<Vec<positions::Position>, String>),
}

enum FormAction {
    None,
    Cancel,
    Submit {
        ticker: String,
        shares: String,
        avg_cost: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal).await;

    // Always restore terminal before propagating errors
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (tx, mut rx) = mpsc::channel::<Result<models::StockIndicators, String>>(1);
    let (ai_tx, mut ai_rx) = mpsc::channel::<Result<String, String>>(1);
    let (news_tx, mut news_rx) = mpsc::channel::<Result<Vec<models::NewsItem>, String>>(1);
    let (pos_tx, mut pos_rx) = mpsc::channel::<PositionsMessage>(8);
    let (price_tx, mut price_rx) = mpsc::channel::<PriceMessage>(32);
    let db = positions::init_db().await?;
    let mut app = App::new();
    let news_tab = TABS.len().saturating_sub(1);

    loop {
        app.on_tick();
        let news_len = match &app.stock.state {
            StockState::Loaded(data) => data.news.as_ref().map(|n| n.len()).unwrap_or(0),
            _ => 0,
        };
        app.stock.clamp_news_selection(news_len);
        terminal.draw(|f| ui::render(f, &app))?;

        // Poll terminal events (50 ms timeout keeps the spinner animating)
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                // ── Universal quit ──────────────────────────────────────
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    app.should_quit = true;
                    continue;
                }

                match app.screen {
                    Screen::Menu => match app.menu_mode {
                        MenuMode::Idle => match key.code {
                            KeyCode::Char('p') | KeyCode::Char('P') => {
                                app.screen = Screen::Positions;
                                app.positions.mode = PositionsMode::View;
                                app.set_status("Loading positions…", 60);
                                spawn_positions_list(db.clone(), pos_tx.clone());
                            }
                            KeyCode::Char('s') | KeyCode::Char('S') => {
                                app.menu_mode = MenuMode::StockInput;
                                app.stock.input.clear();
                            }
                            KeyCode::Char('q') | KeyCode::Char('Q') => {
                                app.should_quit = true;
                            }
                            _ => {}
                        },
                        MenuMode::StockInput => match key.code {
                            KeyCode::Esc => {
                                app.menu_mode = MenuMode::Idle;
                                app.stock.input.clear();
                            }
                            KeyCode::Backspace => {
                                app.stock.input.pop();
                            }
                            KeyCode::Enter => {
                                if !app.stock.input.is_empty() {
                                    let ticker = app.stock.input.clone();
                                    app.menu_mode = MenuMode::Idle;
                                    app.screen = Screen::Stock;
                                    app.stock.state = StockState::Loading(ticker.clone());
                                    let tx = tx.clone();
                                    tokio::spawn(async move {
                                        let res = scraper::scrape_stock(&ticker)
                                            .await
                                            .map_err(|e| e.to_string());
                                        let _ = tx.send(res).await;
                                    });
                                }
                            }
                            KeyCode::Char(c) => {
                                app.stock.input.push(c.to_ascii_uppercase());
                            }
                            _ => {}
                        },
                    },
                    Screen::Stock => match (&app.stock.state, key.code, key.modifiers) {
                        (StockState::Input, KeyCode::Esc, _) => {
                            if app.stock.input.is_empty() {
                                app.screen = Screen::Menu;
                                app.menu_mode = MenuMode::Idle;
                            } else {
                                app.stock.input.clear();
                            }
                        }
                        (StockState::Input, KeyCode::Char(c), _) => {
                            app.stock.input.push(c.to_ascii_uppercase());
                        }
                        (StockState::Input, KeyCode::Backspace, _) => {
                            app.stock.input.pop();
                        }
                        (StockState::Input, KeyCode::Enter, _) => {
                            if !app.stock.input.is_empty() {
                                let ticker = app.stock.input.clone();
                                app.stock.state = StockState::Loading(ticker.clone());
                                let tx = tx.clone();
                                tokio::spawn(async move {
                                    let res = scraper::scrape_stock(&ticker)
                                        .await
                                        .map_err(|e| e.to_string());
                                    let _ = tx.send(res).await;
                                });
                            }
                        }

                        (StockState::Loaded(_) | StockState::Error { .. }, KeyCode::Char('q'), _)
                        | (StockState::Loaded(_) | StockState::Error { .. }, KeyCode::Esc, _) => {
                            app.stock.go_to_input();
                        }
                        (StockState::Loaded(_), KeyCode::Right, _)
                        | (StockState::Loaded(_), KeyCode::Tab, _) => {
                            app.stock.next_tab();
                        }
                        (StockState::Loaded(_), KeyCode::Left, _)
                        | (StockState::Loaded(_), KeyCode::BackTab, _) => {
                            app.stock.prev_tab();
                        }
                        (StockState::Loaded(_), KeyCode::Char('n'), _) => {
                            app.stock.active_tab = news_tab;
                        }
                        (StockState::Loaded(_), KeyCode::Down, _)
                            if app.stock.active_tab == news_tab =>
                        {
                            let len = match &app.stock.state {
                                StockState::Loaded(data) => {
                                    data.news.as_ref().map(|n| n.len()).unwrap_or(0)
                                }
                                _ => 0,
                            };
                            app.stock.next_news(len);
                        }
                        (StockState::Loaded(_), KeyCode::Up, _)
                            if app.stock.active_tab == news_tab =>
                        {
                            let len = match &app.stock.state {
                                StockState::Loaded(data) => {
                                    data.news.as_ref().map(|n| n.len()).unwrap_or(0)
                                }
                                _ => 0,
                            };
                            app.stock.prev_news(len);
                        }
                        (StockState::Loaded(_), KeyCode::Enter, _)
                            if app.stock.active_tab == news_tab =>
                        {
                            let link = match &app.stock.state {
                                StockState::Loaded(data) => data
                                    .news
                                    .as_ref()
                                    .and_then(|n| n.get(app.stock.news_selected))
                                    .map(|item| item.link.clone()),
                                _ => None,
                            };
                            match link {
                                Some(link) => {
                                    if let Err(err) = open_in_browser(&link) {
                                        app.set_status(
                                            format!("Failed to open browser: {err}"),
                                            120,
                                        );
                                    }
                                }
                                None => {
                                    app.set_status("No news item selected.", 120);
                                }
                            }
                        }
                        (StockState::Loaded(data), KeyCode::Char('r'), _)
                            if app.stock.active_tab == news_tab =>
                        {
                            let ticker = data.ticker.clone();
                            app.set_status("Refreshing news…", 120);
                            let news_tx = news_tx.clone();
                            tokio::spawn(async move {
                                let client = reqwest::Client::new();
                                let news =
                                    news::fetch_yahoo_news_for_ticker(&client, &ticker).await;
                                let res = match news {
                                    Some(items) => Ok(items),
                                    None => Err("Failed to refresh news.".to_string()),
                                };
                                let _ = news_tx.send(res).await;
                            });
                        }
                        (StockState::Loaded(_), KeyCode::Char(','), _)
                            if app.stock.active_tab == 0 =>
                        {
                            app.stock.prev_period();
                        }
                        (StockState::Loaded(_), KeyCode::Char('.'), _)
                            if app.stock.active_tab == 0 =>
                        {
                            app.stock.next_period();
                        }
                        (StockState::Loaded(_), KeyCode::Char(c @ '1'..='4'), _)
                            if app.stock.active_tab == 0 =>
                        {
                            app.stock.active_period = (c as usize) - ('1' as usize);
                        }
                        (StockState::Loaded(_), KeyCode::Char('o'), _) => {
                            app.stock.active_tab = 0;
                        }
                        (StockState::Loaded(_), KeyCode::Char('f'), _) => {
                            app.stock.active_tab = 1;
                        }
                        (StockState::Loaded(_), KeyCode::Char('v' | 'd' | 'e' | 'p' | 'g'), _) => {
                            app.stock.active_tab = 1;
                        }
                        _ => {}
                    },
                    Screen::Positions => match &mut app.positions.mode {
                        PositionsMode::View => match key.code {
                            KeyCode::Char('j') | KeyCode::Down => app.positions.next(),
                            KeyCode::Char('k') | KeyCode::Up => app.positions.prev(),
                            KeyCode::Char('a') => {
                                app.positions.mode = PositionsMode::Add(PositionForm::new_add());
                            }
                            KeyCode::Char('e') => {
                                if let Some(row) = app.positions.selected_row() {
                                    app.positions.mode =
                                        PositionsMode::Edit(PositionForm::new_edit(row));
                                }
                            }
                            KeyCode::Char('d') => {
                                if let Some(row) = app.positions.selected_row() {
                                    app.positions.mode = PositionsMode::DeleteConfirm {
                                        id: row.id,
                                        ticker: row.ticker.clone(),
                                    };
                                }
                            }
                            KeyCode::Char('r') => {
                                app.set_status("Refreshing prices…", 60);
                                spawn_price_refresh(&app.positions.rows, price_tx.clone());
                            }
                            KeyCode::Enter => {
                                if let Some(row) = app.positions.selected_row() {
                                    let ticker = row.ticker.clone();
                                    app.screen = Screen::Stock;
                                    app.stock.input = ticker.clone();
                                    app.stock.state = StockState::Loading(ticker.clone());
                                    let tx = tx.clone();
                                    tokio::spawn(async move {
                                        let res = scraper::scrape_stock(&ticker)
                                            .await
                                            .map_err(|e| e.to_string());
                                        let _ = tx.send(res).await;
                                    });
                                }
                            }
                            KeyCode::Esc => {
                                app.screen = Screen::Menu;
                                app.menu_mode = MenuMode::Idle;
                            }
                            _ => {}
                        },
                        PositionsMode::Add(form) => {
                            match handle_form_key(form, &key) {
                                FormAction::Cancel => {
                                    app.positions.mode = PositionsMode::View;
                                }
                                FormAction::Submit {
                                    ticker,
                                    shares,
                                    avg_cost,
                                } => match parse_position_fields(&ticker, &shares, &avg_cost) {
                                    Ok((ticker, shares, avg_cost)) => {
                                        app.positions.mode = PositionsMode::View;
                                        app.set_status("Saving position…", 60);
                                        let db = db.clone();
                                        let pos_tx = pos_tx.clone();
                                        tokio::spawn(async move {
                                            let res = match positions::insert_position(
                                                &db, &ticker, shares, avg_cost,
                                            )
                                            .await
                                            {
                                                Ok(()) => positions::list_positions(&db)
                                                    .await
                                                    .map_err(|e| e.to_string()),
                                                Err(e) => Err(e.to_string()),
                                            };
                                            let _ = pos_tx.send(PositionsMessage::List(res)).await;
                                        });
                                    }
                                    Err(msg) => {
                                        app.set_status(msg, 120);
                                    }
                                },
                                FormAction::None => {}
                            }
                        }
                        PositionsMode::Edit(form) => {
                            match handle_form_key(form, &key) {
                                FormAction::Cancel => {
                                    app.positions.mode = PositionsMode::View;
                                }
                                FormAction::Submit {
                                    ticker,
                                    shares,
                                    avg_cost,
                                } => match parse_position_fields(&ticker, &shares, &avg_cost) {
                                    Ok((ticker, shares, avg_cost)) => {
                                        if let Some(id) = form.id {
                                            app.positions.mode = PositionsMode::View;
                                            app.set_status("Updating position…", 60);
                                            let db = db.clone();
                                            let pos_tx = pos_tx.clone();
                                            tokio::spawn(async move {
                                                let res = match positions::update_position(
                                                    &db, id, &ticker, shares, avg_cost,
                                                )
                                                .await
                                                {
                                                    Ok(()) => positions::list_positions(&db)
                                                        .await
                                                        .map_err(|e| e.to_string()),
                                                    Err(e) => Err(e.to_string()),
                                                };
                                                let _ =
                                                    pos_tx.send(PositionsMessage::List(res)).await;
                                            });
                                        }
                                    }
                                    Err(msg) => {
                                        app.set_status(msg, 120);
                                    }
                                },
                                FormAction::None => {}
                            }
                        }
                        PositionsMode::DeleteConfirm { id, .. } => match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                let delete_id = *id;
                                app.positions.mode = PositionsMode::View;
                                app.set_status("Deleting position…", 60);
                                let db = db.clone();
                                let pos_tx = pos_tx.clone();
                                tokio::spawn(async move {
                                    let res = match positions::delete_position(&db, delete_id).await {
                                        Ok(()) => positions::list_positions(&db)
                                            .await
                                            .map_err(|e| e.to_string()),
                                        Err(e) => Err(e.to_string()),
                                    };
                                    let _ = pos_tx.send(PositionsMessage::List(res)).await;
                                });
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                app.positions.mode = PositionsMode::View;
                            }
                            _ => {}
                        },
                    },
                }
            }
        }

        // Receive scraping result
        if let Ok(result) = rx.try_recv() {
            if matches!(app.stock.state, StockState::Loading(_)) {
                match result {
                    Ok(data) => {
                        // Kick off AI analysis on first load (if key present)
                        if let Some(key) = &app.openrouter_key {
                            app.stock.ai_state = AiState::Loading;
                            let key = key.clone();
                            let json = serde_json::to_string(&data).unwrap_or_default();
                            let ai_tx = ai_tx.clone();
                            tokio::spawn(async move {
                                let res = ai::analyze_stock(&json, &key).await;
                                let _ = ai_tx.send(res).await;
                            });
                        }
                        app.stock.state = StockState::Loaded(Box::new(data));
                        app.stock.news_selected = 0;
                    }
                    Err(msg) => {
                        let ticker = app.stock.input.clone();
                        app.stock.state = StockState::Error { ticker, message: msg };
                    }
                }
                app.stock.active_tab = 0;
            }
        }

        // Receive AI analysis result
        if let Ok(result) = ai_rx.try_recv() {
            app.stock.ai_state = match result {
                Ok(text) => AiState::Done(text),
                Err(msg) => AiState::Failed(msg),
            };
        }

        // Receive news refresh result
        if let Ok(result) = news_rx.try_recv() {
            match result {
                Ok(items) => {
                    if let StockState::Loaded(data) = &mut app.stock.state {
                        data.news = Some(items);
                        app.stock.news_selected = 0;
                        app.set_status("News refreshed.", 120);
                    }
                }
                Err(msg) => {
                    app.set_status(msg, 120);
                }
            }
        }

        while let Ok(msg) = pos_rx.try_recv() {
            match msg {
                PositionsMessage::List(result) => match result {
                    Ok(list) => {
                        let mut price_map = std::collections::HashMap::new();
                        for row in &app.positions.rows {
                            price_map.insert(row.id, row.current_price);
                        }
                        app.positions.rows = list
                            .into_iter()
                            .map(|p| PositionRow {
                                id: p.id,
                                ticker: p.ticker,
                                shares: p.shares,
                                avg_cost: p.avg_cost,
                                current_price: price_map.get(&p.id).copied().flatten(),
                            })
                            .collect();
                        app.positions.clamp_selection();
                        spawn_price_refresh(&app.positions.rows, price_tx.clone());
                    }
                    Err(msg) => {
                        app.set_status(msg, 120);
                    }
                },
            }
        }

        while let Ok(msg) = price_rx.try_recv() {
            if let Some(row) = app
                .positions
                .rows
                .iter_mut()
                .find(|row| row.id == msg.id)
            {
                row.current_price = Some(msg.price);
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

fn handle_form_key(form: &mut PositionForm, key: &event::KeyEvent) -> FormAction {
    match key.code {
        KeyCode::Esc => return FormAction::Cancel,
        KeyCode::Enter => {
            if form.active_field < 2 {
                form.active_field += 1;
                return FormAction::None;
            }
            return FormAction::Submit {
                ticker: form.ticker.clone(),
                shares: form.shares.clone(),
                avg_cost: form.avg_cost.clone(),
            };
        }
        KeyCode::Tab => {
            if form.active_field < 2 {
                form.active_field += 1;
            }
            return FormAction::None;
        }
        KeyCode::BackTab => {
            if form.active_field > 0 {
                form.active_field -= 1;
            }
            return FormAction::None;
        }
        KeyCode::Backspace => {
            let field = match form.active_field {
                0 => &mut form.ticker,
                1 => &mut form.shares,
                _ => &mut form.avg_cost,
            };
            field.pop();
            return FormAction::None;
        }
        KeyCode::Char(c) => {
            match form.active_field {
                0 => {
                    if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                        form.ticker.push(c.to_ascii_uppercase());
                    }
                }
                1 | 2 => {
                    let field = if form.active_field == 1 {
                        &mut form.shares
                    } else {
                        &mut form.avg_cost
                    };
                    if c.is_ascii_digit() {
                        field.push(c);
                    } else if c == '.' && !field.contains('.') {
                        field.push(c);
                    }
                }
                _ => {}
            }
            return FormAction::None;
        }
        _ => {}
    }
    FormAction::None
}

fn parse_position_fields(
    ticker: &str,
    shares: &str,
    avg_cost: &str,
) -> Result<(String, f64, f64), String> {
    let ticker = ticker.trim().to_uppercase();
    if ticker.is_empty() {
        return Err("Ticker is required.".to_string());
    }
    let shares: f64 = shares
        .trim()
        .parse()
        .map_err(|_| "Shares must be a number.".to_string())?;
    if shares <= 0.0 {
        return Err("Shares must be greater than 0.".to_string());
    }
    let avg_cost: f64 = avg_cost
        .trim()
        .parse()
        .map_err(|_| "Avg cost must be a number.".to_string())?;
    if avg_cost <= 0.0 {
        return Err("Avg cost must be greater than 0.".to_string());
    }
    Ok((ticker, shares, avg_cost))
}

fn spawn_positions_list(db: SqlitePool, tx: mpsc::Sender<PositionsMessage>) {
    tokio::spawn(async move {
        let res = positions::list_positions(&db)
            .await
            .map_err(|e| e.to_string());
        let _ = tx.send(PositionsMessage::List(res)).await;
    });
}

fn spawn_price_refresh(rows: &[PositionRow], tx: mpsc::Sender<PriceMessage>) {
    for row in rows {
        let tx = tx.clone();
        let ticker = row.ticker.clone();
        let id = row.id;
        tokio::spawn(async move {
            if let Ok(price) = scraper::fetch_current_price(&ticker).await {
                let _ = tx.send(PriceMessage { id, price }).await;
            }
        });
    }
}

fn open_in_browser(url: &str) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(url).spawn()?;
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("cmd").args(["/C", "start", "", url]).spawn()?;
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open").arg(url).spawn()?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Unsupported platform",
    ))
}
