mod ai;
mod app;
mod financials;
mod history;
mod models;
mod news;
mod profile;
mod scraper;
mod ui;
mod yahoo;

use std::io;
use std::process::Command;
use std::time::Duration;

use app::{
    AiState, App, ChatMessage, ChatRole, ChatState, HistoryForm, HistoryMode, HoldingRow, MenuMode,
    Screen, StockState, TABS, TradeRow,
};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode, size},
};
use financials::{NavDir, clamp_selection, move_selection, sections as financial_sections};
use ratatui::{Terminal, backend::CrosstermBackend};
use sqlx::SqlitePool;
use tokio::sync::mpsc;

struct PriceMessage {
    ticker: String,
    price: f64,
}

enum HistoryMessage {
    List(Result<Vec<history::Trade>, String>),
}

enum FormAction {
    None,
    Cancel,
    Submit {
        ticker: String,
        side: String,
        shares: String,
        price: String,
        date: String,
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
    let (chat_tx, mut chat_rx) = mpsc::channel::<Result<String, String>>(1);
    let (news_tx, mut news_rx) = mpsc::channel::<Result<Vec<models::NewsItem>, String>>(1);
    let (history_tx, mut history_rx) = mpsc::channel::<HistoryMessage>(8);
    let (price_tx, mut price_rx) = mpsc::channel::<PriceMessage>(32);
    let db = history::init_db().await?;
    let mut app = App::new();
    let news_tab = TABS
        .iter()
        .position(|&tab| tab == "News")
        .unwrap_or_else(|| TABS.len().saturating_sub(1));
    let ai_tab = TABS.iter().position(|&tab| tab == "AI").unwrap_or(2);
    let financials_tab = TABS
        .iter()
        .position(|&tab| tab == "Financials")
        .unwrap_or(1);

    loop {
        app.on_tick();
        let news_len = match &app.stock.state {
            StockState::Loaded(data) => data.news.as_ref().map(|n| n.len()).unwrap_or(0),
            _ => 0,
        };
        app.stock.clamp_news_selection(news_len);
        let chat_max = chat_max_scroll(
            &app.stock.chat_messages,
            &app.stock.chat_state,
            app.openrouter_key.is_some(),
        );
        if app.stock.chat_scroll > chat_max {
            app.stock.chat_scroll = chat_max;
        }
        terminal.draw(|f| ui::render(f, &app))?;

        // Poll terminal events (50 ms timeout keeps the spinner animating)
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                // ── Universal quit ──────────────────────────────────────
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    app.should_quit = true;
                    continue;
                }

                match app.screen {
                    Screen::Menu => match app.menu_mode {
                        MenuMode::Idle => match key.code {
                            KeyCode::Char('h') | KeyCode::Char('H') => {
                                app.screen = Screen::History;
                                app.history.mode = HistoryMode::View;
                                app.set_status("Loading history…", 60);
                                spawn_history_list(db.clone(), history_tx.clone());
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

                        (StockState::Loaded(_), KeyCode::Esc, _)
                            if app.stock.active_tab == financials_tab
                                && app.stock.financials_modal.is_some() =>
                        {
                            app.stock.financials_modal = None;
                        }
                        (StockState::Loaded(_), KeyCode::Enter, _)
                            if app.stock.active_tab == financials_tab
                                && app.stock.financials_modal.is_some() =>
                        {
                            app.stock.financials_modal = None;
                        }
                        (
                            StockState::Loaded(_) | StockState::Error { .. },
                            KeyCode::Char('q'),
                            _,
                        )
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
                        (StockState::Loaded(data), KeyCode::Enter, _)
                            if app.stock.active_tab == ai_tab =>
                        {
                            if matches!(app.stock.chat_state, ChatState::Loading) {
                                continue;
                            }
                            let input = app.stock.chat_input.trim().to_string();
                            if input.is_empty() {
                                continue;
                            }
                            let Some(key) = app.openrouter_key.clone() else {
                                app.set_status(
                                    "Define OPENROUTER_API_KEY to enable AI chat.",
                                    180,
                                );
                                continue;
                            };
                            let json = serde_json::to_string(&**data).unwrap_or_default();
                            app.stock.chat_messages.push(ChatMessage {
                                role: ChatRole::User,
                                content: input.clone(),
                            });
                            app.stock.chat_input.clear();
                            app.stock.chat_state = ChatState::Loading;
                            app.stock.chat_scroll = chat_max_scroll(
                                &app.stock.chat_messages,
                                &app.stock.chat_state,
                                app.openrouter_key.is_some(),
                            );
                            let history = app.stock.chat_messages.clone();
                            let chat_tx = chat_tx.clone();
                            tokio::spawn(async move {
                                let res = ai::chat_about_stock(&json, &history, &key).await;
                                let _ = chat_tx.send(res).await;
                            });
                        }
                        (StockState::Loaded(_), KeyCode::Backspace, _)
                            if app.stock.active_tab == ai_tab =>
                        {
                            app.stock.chat_input.pop();
                        }
                        (StockState::Loaded(_), KeyCode::Char(c), modifiers)
                            if app.stock.active_tab == ai_tab
                                && !modifiers.contains(KeyModifiers::CONTROL)
                                && !modifiers.contains(KeyModifiers::ALT) =>
                        {
                            app.stock.chat_input.push(c);
                        }
                        (StockState::Loaded(_), KeyCode::Up, _)
                            if app.stock.active_tab == ai_tab =>
                        {
                            app.stock.chat_scroll = app.stock.chat_scroll.saturating_sub(1);
                        }
                        (StockState::Loaded(_), KeyCode::Down, _)
                            if app.stock.active_tab == ai_tab =>
                        {
                            let max = chat_max_scroll(
                                &app.stock.chat_messages,
                                &app.stock.chat_state,
                                app.openrouter_key.is_some(),
                            );
                            app.stock.chat_scroll = (app.stock.chat_scroll + 1).min(max);
                        }
                        (StockState::Loaded(_), KeyCode::PageUp, _)
                            if app.stock.active_tab == ai_tab =>
                        {
                            let step = chat_log_height().max(1);
                            app.stock.chat_scroll = app.stock.chat_scroll.saturating_sub(step);
                        }
                        (StockState::Loaded(_), KeyCode::PageDown, _)
                            if app.stock.active_tab == ai_tab =>
                        {
                            let step = chat_log_height().max(1);
                            let max = chat_max_scroll(
                                &app.stock.chat_messages,
                                &app.stock.chat_state,
                                app.openrouter_key.is_some(),
                            );
                            app.stock.chat_scroll = (app.stock.chat_scroll + step).min(max);
                        }
                        (StockState::Loaded(_), KeyCode::Char('n'), _)
                            if app.stock.active_tab != ai_tab =>
                        {
                            app.stock.active_tab = news_tab;
                            app.stock.financials_modal = None;
                        }
                        (StockState::Loaded(_), KeyCode::Char('a'), _)
                            if app.stock.active_tab != ai_tab =>
                        {
                            app.stock.active_tab = ai_tab;
                            app.stock.financials_modal = None;
                        }
                        (StockState::Loaded(data), KeyCode::Char('h'), _)
                            if app.stock.active_tab == financials_tab
                                && app.stock.financials_modal.is_none() =>
                        {
                            let sections = financial_sections(data);
                            app.stock.financials_selected = move_selection(
                                app.stock.financials_selected,
                                NavDir::Left,
                                &sections,
                            );
                        }
                        (StockState::Loaded(data), KeyCode::Char('j'), _)
                            if app.stock.active_tab == financials_tab
                                && app.stock.financials_modal.is_none() =>
                        {
                            let sections = financial_sections(data);
                            app.stock.financials_selected = move_selection(
                                app.stock.financials_selected,
                                NavDir::Down,
                                &sections,
                            );
                        }
                        (StockState::Loaded(data), KeyCode::Char('k'), _)
                            if app.stock.active_tab == financials_tab
                                && app.stock.financials_modal.is_none() =>
                        {
                            let sections = financial_sections(data);
                            app.stock.financials_selected = move_selection(
                                app.stock.financials_selected,
                                NavDir::Up,
                                &sections,
                            );
                        }
                        (StockState::Loaded(data), KeyCode::Char('l'), _)
                            if app.stock.active_tab == financials_tab
                                && app.stock.financials_modal.is_none() =>
                        {
                            let sections = financial_sections(data);
                            app.stock.financials_selected = move_selection(
                                app.stock.financials_selected,
                                NavDir::Right,
                                &sections,
                            );
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
                        (StockState::Loaded(_), KeyCode::Enter, _)
                            if app.stock.active_tab == financials_tab
                                && app.stock.financials_modal.is_none() =>
                        {
                            app.stock.financials_modal = Some(app.stock.financials_selected);
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
                        (StockState::Loaded(_), KeyCode::Char('o'), _)
                            if app.stock.active_tab != ai_tab =>
                        {
                            app.stock.active_tab = 0;
                            app.stock.financials_modal = None;
                        }
                        (StockState::Loaded(_), KeyCode::Char('f'), _)
                            if app.stock.active_tab != ai_tab =>
                        {
                            app.stock.active_tab = 1;
                            app.stock.financials_modal = None;
                        }
                        (StockState::Loaded(_), KeyCode::Char('v' | 'd' | 'e' | 'p' | 'g'), _)
                            if app.stock.active_tab != ai_tab =>
                        {
                            app.stock.active_tab = 1;
                            app.stock.financials_modal = None;
                        }
                        _ => {}
                    },
                    Screen::History => match &mut app.history.mode {
                        HistoryMode::View => match key.code {
                            KeyCode::Char('j') | KeyCode::Down => app.history.next(),
                            KeyCode::Char('k') | KeyCode::Up => app.history.prev(),
                            KeyCode::Char('a') => {
                                app.history.mode = HistoryMode::Add(HistoryForm::new_add());
                            }
                            KeyCode::Char('e') => {
                                if let Some(row) = app.history.selected_trade() {
                                    app.history.mode =
                                        HistoryMode::Edit(HistoryForm::new_edit(row));
                                }
                            }
                            KeyCode::Char('d') => {
                                if let Some(row) = app.history.selected_trade() {
                                    app.history.mode = HistoryMode::DeleteConfirm {
                                        id: row.id,
                                        ticker: row.ticker.clone(),
                                    };
                                }
                            }
                            KeyCode::Char('r') => {
                                app.set_status("Refreshing prices…", 60);
                                spawn_price_refresh(&app.history.holdings, price_tx.clone());
                            }
                            KeyCode::Enter => {
                                if let Some(row) = app.history.selected_trade() {
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
                        HistoryMode::Add(form) => match handle_form_key(form, &key) {
                            FormAction::Cancel => {
                                app.history.mode = HistoryMode::View;
                            }
                            FormAction::Submit {
                                ticker,
                                side,
                                shares,
                                price,
                                date,
                            } => match parse_trade_fields(&ticker, &side, &shares, &price, &date) {
                                Ok((ticker, side, shares, price, date)) => {
                                    app.history.mode = HistoryMode::View;
                                    app.set_status("Saving trade…", 60);
                                    let db = db.clone();
                                    let history_tx = history_tx.clone();
                                    tokio::spawn(async move {
                                        let res = match history::insert_trade(
                                            &db, &ticker, &side, shares, price, &date,
                                        )
                                        .await
                                        {
                                            Ok(()) => history::list_trades(&db)
                                                .await
                                                .map_err(|e| e.to_string()),
                                            Err(e) => Err(e.to_string()),
                                        };
                                        let _ = history_tx.send(HistoryMessage::List(res)).await;
                                    });
                                }
                                Err(msg) => {
                                    app.set_status(msg, 120);
                                }
                            },
                            FormAction::None => {}
                        },
                        HistoryMode::Edit(form) => match handle_form_key(form, &key) {
                            FormAction::Cancel => {
                                app.history.mode = HistoryMode::View;
                            }
                            FormAction::Submit {
                                ticker,
                                side,
                                shares,
                                price,
                                date,
                            } => match parse_trade_fields(&ticker, &side, &shares, &price, &date) {
                                Ok((ticker, side, shares, price, date)) => {
                                    if let Some(id) = form.id {
                                        app.history.mode = HistoryMode::View;
                                        app.set_status("Updating trade…", 60);
                                        let db = db.clone();
                                        let history_tx = history_tx.clone();
                                        tokio::spawn(async move {
                                            let res = match history::update_trade(
                                                &db, id, &ticker, &side, shares, price, &date,
                                            )
                                            .await
                                            {
                                                Ok(()) => history::list_trades(&db)
                                                    .await
                                                    .map_err(|e| e.to_string()),
                                                Err(e) => Err(e.to_string()),
                                            };
                                            let _ =
                                                history_tx.send(HistoryMessage::List(res)).await;
                                        });
                                    }
                                }
                                Err(msg) => {
                                    app.set_status(msg, 120);
                                }
                            },
                            FormAction::None => {}
                        },
                        HistoryMode::DeleteConfirm { id, .. } => match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                let delete_id = *id;
                                app.history.mode = HistoryMode::View;
                                app.set_status("Deleting trade…", 60);
                                let db = db.clone();
                                let history_tx = history_tx.clone();
                                tokio::spawn(async move {
                                    let res = match history::delete_trade(&db, delete_id).await {
                                        Ok(()) => history::list_trades(&db)
                                            .await
                                            .map_err(|e| e.to_string()),
                                        Err(e) => Err(e.to_string()),
                                    };
                                    let _ = history_tx.send(HistoryMessage::List(res)).await;
                                });
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                app.history.mode = HistoryMode::View;
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
                        app.stock.chat_state = ChatState::Idle;
                        app.stock.chat_messages.clear();
                        app.stock.chat_input.clear();
                        app.stock.chat_scroll = 0;
                        let sections = financial_sections(&data);
                        app.stock.financials_selected =
                            clamp_selection(app.stock.financials_selected, &sections);
                        app.stock.financials_modal = None;
                        app.stock.state = StockState::Loaded(Box::new(data));
                        app.stock.news_selected = 0;
                    }
                    Err(msg) => {
                        let ticker = app.stock.input.clone();
                        app.stock.state = StockState::Error {
                            ticker,
                            message: msg,
                        };
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

        // Receive AI chat result
        if let Ok(result) = chat_rx.try_recv() {
            match result {
                Ok(text) => {
                    app.stock.chat_messages.push(ChatMessage {
                        role: ChatRole::Assistant,
                        content: text,
                    });
                    app.stock.chat_state = ChatState::Idle;
                    app.stock.chat_scroll = chat_max_scroll(
                        &app.stock.chat_messages,
                        &app.stock.chat_state,
                        app.openrouter_key.is_some(),
                    );
                }
                Err(msg) => {
                    app.stock.chat_state = ChatState::Failed(msg);
                    app.stock.chat_scroll = chat_max_scroll(
                        &app.stock.chat_messages,
                        &app.stock.chat_state,
                        app.openrouter_key.is_some(),
                    );
                }
            }
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

        while let Ok(msg) = history_rx.try_recv() {
            match msg {
                HistoryMessage::List(result) => match result {
                    Ok(list) => {
                        app.history.trades = list
                            .into_iter()
                            .map(|t| TradeRow {
                                id: t.id,
                                ticker: t.ticker,
                                side: t.side,
                                shares: t.shares,
                                price: t.price,
                                date: t.date,
                            })
                            .collect();
                        app.history.clamp_selection();
                        app.history.holdings = compute_holdings(&app.history.trades);
                        spawn_price_refresh(&app.history.holdings, price_tx.clone());
                    }
                    Err(msg) => {
                        app.set_status(msg, 120);
                    }
                },
            }
        }

        while let Ok(msg) = price_rx.try_recv() {
            if let Some(row) = app
                .history
                .holdings
                .iter_mut()
                .find(|row| row.ticker == msg.ticker)
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

fn handle_form_key(form: &mut HistoryForm, key: &event::KeyEvent) -> FormAction {
    match key.code {
        KeyCode::Esc => return FormAction::Cancel,
        KeyCode::Enter => {
            if form.active_field < 4 {
                form.active_field += 1;
                return FormAction::None;
            }
            return FormAction::Submit {
                ticker: form.ticker.clone(),
                side: form.side.clone(),
                shares: form.shares.clone(),
                price: form.price.clone(),
                date: form.date.clone(),
            };
        }
        KeyCode::Tab => {
            if form.active_field < 4 {
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
                1 => &mut form.side,
                2 => &mut form.shares,
                3 => &mut form.price,
                _ => &mut form.date,
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
                1 => {
                    if c == 'b' || c == 'B' {
                        form.side = "BUY".to_string();
                    } else if c == 's' || c == 'S' {
                        form.side = "SELL".to_string();
                    } else if c.is_ascii_alphabetic() {
                        form.side.push(c.to_ascii_uppercase());
                    }
                }
                2 | 3 => {
                    let field = if form.active_field == 2 {
                        &mut form.shares
                    } else {
                        &mut form.price
                    };
                    if c.is_ascii_digit() {
                        field.push(c);
                    } else if c == '.' && !field.contains('.') {
                        field.push(c);
                    }
                }
                4 => {
                    if c.is_ascii_digit() || c == '-' {
                        form.date.push(c);
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

fn parse_trade_fields(
    ticker: &str,
    side: &str,
    shares: &str,
    price: &str,
    date: &str,
) -> Result<(String, String, f64, f64, String), String> {
    let ticker = ticker.trim().to_uppercase();
    if ticker.is_empty() {
        return Err("Ticker is required.".to_string());
    }
    let side = side.trim().to_uppercase();
    if side != "BUY" && side != "SELL" {
        return Err("Side must be BUY or SELL.".to_string());
    }
    let shares: f64 = shares
        .trim()
        .parse()
        .map_err(|_| "Shares must be a number.".to_string())?;
    if shares <= 0.0 {
        return Err("Shares must be greater than 0.".to_string());
    }
    let price: f64 = price
        .trim()
        .parse()
        .map_err(|_| "Price must be a number.".to_string())?;
    if price <= 0.0 {
        return Err("Price must be greater than 0.".to_string());
    }
    let date = normalize_date_input(date)?;
    Ok((ticker, side, shares, price, date))
}

fn spawn_history_list(db: SqlitePool, tx: mpsc::Sender<HistoryMessage>) {
    tokio::spawn(async move {
        let res = history::list_trades(&db).await.map_err(|e| e.to_string());
        let _ = tx.send(HistoryMessage::List(res)).await;
    });
}

fn spawn_price_refresh(rows: &[HoldingRow], tx: mpsc::Sender<PriceMessage>) {
    for row in rows {
        let tx = tx.clone();
        let ticker = row.ticker.clone();
        tokio::spawn(async move {
            if let Ok(price) = scraper::fetch_current_price(&ticker).await {
                let _ = tx.send(PriceMessage { ticker, price }).await;
            }
        });
    }
}

fn compute_holdings(trades: &[TradeRow]) -> Vec<HoldingRow> {
    let mut grouped: std::collections::HashMap<String, Vec<&TradeRow>> =
        std::collections::HashMap::new();
    for trade in trades {
        grouped.entry(trade.ticker.clone()).or_default().push(trade);
    }

    let mut holdings: Vec<HoldingRow> = grouped
        .into_iter()
        .map(|(ticker, mut t)| {
            t.sort_by(|a, b| {
                let date_cmp = a.date.cmp(&b.date);
                if date_cmp == std::cmp::Ordering::Equal {
                    a.id.cmp(&b.id)
                } else {
                    date_cmp
                }
            });

            let mut shares = 0.0;
            let mut avg_cost = 0.0;
            for trade in t {
                let qty = trade.shares;
                if trade.side == "BUY" {
                    let total_cost = avg_cost * shares + trade.price * qty;
                    shares += qty;
                    if shares > 0.0 {
                        avg_cost = total_cost / shares;
                    }
                } else if trade.side == "SELL" {
                    shares -= qty;
                    if shares <= 0.0 {
                        shares = 0.0;
                        avg_cost = 0.0;
                    }
                }
            }

            HoldingRow {
                ticker,
                shares,
                avg_cost,
                current_price: None,
            }
        })
        .filter(|row| row.shares > 0.0)
        .collect();

    holdings.sort_by(|a, b| a.ticker.cmp(&b.ticker));
    holdings
}

fn normalize_date_input(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Date is required (DD/MM/YYYY).".to_string());
    }

    if let Some((d, m, y)) = parse_ddmmyyyy(trimmed) {
        return Ok(format!("{:04}-{:02}-{:02}", y, m, d));
    }

    if let Some((y, m, d)) = parse_yyyymmdd(trimmed) {
        return Ok(format!("{:04}-{:02}-{:02}", y, m, d));
    }

    Err("Date must be DD/MM/YYYY or YYYY-MM-DD.".to_string())
}

fn parse_ddmmyyyy(s: &str) -> Option<(i32, i32, i32)> {
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() != 3 {
        return None;
    }
    let d = parts[0].parse::<i32>().ok()?;
    let m = parts[1].parse::<i32>().ok()?;
    let y = parts[2].parse::<i32>().ok()?;
    if valid_date(d, m, y) {
        Some((d, m, y))
    } else {
        None
    }
}

fn parse_yyyymmdd(s: &str) -> Option<(i32, i32, i32)> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let y = parts[0].parse::<i32>().ok()?;
    let m = parts[1].parse::<i32>().ok()?;
    let d = parts[2].parse::<i32>().ok()?;
    if valid_date(d, m, y) {
        Some((y, m, d))
    } else {
        None
    }
}

fn valid_date(d: i32, m: i32, y: i32) -> bool {
    if y < 1900 || y > 2100 {
        return false;
    }
    if m < 1 || m > 12 {
        return false;
    }
    if d < 1 || d > 31 {
        return false;
    }
    true
}

fn chat_log_height() -> usize {
    let Ok((_, rows)) = size() else {
        return 0;
    };
    let total = rows as usize;
    let content = total.saturating_sub(2); // title + status bars
    let inner = content.saturating_sub(3); // tab bar
    inner.saturating_mul(75) / 100
}

fn chat_line_count(
    chat_messages: &[ChatMessage],
    chat_state: &ChatState,
    ai_enabled: bool,
) -> usize {
    if !ai_enabled {
        return 1;
    }

    let mut count: usize = if chat_messages.is_empty() { 1 } else { 0 };

    for message in chat_messages {
        let lines = message.content.lines().count().max(1);
        count = count.saturating_add(lines + 1);
    }

    if !chat_messages.is_empty() {
        count = count.saturating_sub(1);
    }

    if matches!(chat_state, ChatState::Loading) {
        count = count.saturating_add(1);
    }

    if matches!(chat_state, ChatState::Failed(_)) {
        count = count.saturating_add(1);
    }

    count
}

fn chat_max_scroll(
    chat_messages: &[ChatMessage],
    chat_state: &ChatState,
    ai_enabled: bool,
) -> usize {
    let height = chat_log_height();
    if height == 0 {
        return 0;
    }
    let lines = chat_line_count(chat_messages, chat_state, ai_enabled);
    lines.saturating_sub(height)
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
