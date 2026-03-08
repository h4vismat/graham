mod ai;
mod app;
mod models;
mod scraper;
mod ui;

use std::io;
use std::time::Duration;

use app::{AiState, App, State};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use tokio::sync::mpsc;

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
    let mut app = App::new();

    loop {
        app.on_tick();
        terminal.draw(|f| ui::render(f, &app))?;

        // Poll terminal events (50 ms timeout keeps the spinner animating)
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match (&app.state, key.code, key.modifiers) {
                    // ── Universal quit ──────────────────────────────────────
                    (_, KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
                        app.should_quit = true;
                    }

                    // ── Input state ─────────────────────────────────────────
                    (State::Input, KeyCode::Esc, _) => {
                        if app.input.is_empty() {
                            app.should_quit = true;
                        } else {
                            app.input.clear();
                        }
                    }
                    (State::Input, KeyCode::Char(c), _) => {
                        app.input.push(c.to_ascii_uppercase());
                    }
                    (State::Input, KeyCode::Backspace, _) => {
                        app.input.pop();
                    }
                    (State::Input, KeyCode::Enter, _) => {
                        if !app.input.is_empty() {
                            let ticker = app.input.clone();
                            app.state = State::Loading(ticker.clone());
                            let tx = tx.clone();
                            tokio::spawn(async move {
                                let res = scraper::scrape_stock(&ticker)
                                    .await
                                    .map_err(|e| e.to_string());
                                let _ = tx.send(res).await;
                            });
                        }
                    }

                    // ── Loaded / Error state ─────────────────────────────────
                    (State::Loaded(_) | State::Error { .. }, KeyCode::Char('q'), _)
                    | (State::Loaded(_) | State::Error { .. }, KeyCode::Esc, _) => {
                        app.go_to_input();
                    }
                    (State::Loaded(_), KeyCode::Right, _) | (State::Loaded(_), KeyCode::Tab, _) => {
                        app.next_tab();
                    }
                    (State::Loaded(_), KeyCode::Left, _)
                    | (State::Loaded(_), KeyCode::BackTab, _) => {
                        app.prev_tab();
                    }
                    // Period switching on Overview tab (keys 1-4 or , / .)
                    (State::Loaded(_), KeyCode::Char(','), _) if app.active_tab == 0 => {
                        app.prev_period();
                    }
                    (State::Loaded(_), KeyCode::Char('.'), _) if app.active_tab == 0 => {
                        app.next_period();
                    }
                    (State::Loaded(_), KeyCode::Char(c @ '1'..='4'), _) if app.active_tab == 0 => {
                        app.active_period = (c as usize) - ('1' as usize);
                    }
                    // Tab shortcuts
                    (State::Loaded(_), KeyCode::Char('o'), _) => {
                        app.active_tab = 0;
                    }
                    (State::Loaded(_), KeyCode::Char('v'), _) => {
                        app.active_tab = 1;
                    }
                    (State::Loaded(_), KeyCode::Char('d'), _) => {
                        app.active_tab = 2;
                    }
                    (State::Loaded(_), KeyCode::Char('e'), _) => {
                        app.active_tab = 3;
                    }
                    (State::Loaded(_), KeyCode::Char('p'), _) => {
                        app.active_tab = 4;
                    }
                    (State::Loaded(_), KeyCode::Char('g'), _) => {
                        app.active_tab = 5;
                    }

                    _ => {}
                }
            }
        }

        // Receive scraping result
        if let Ok(result) = rx.try_recv() {
            if matches!(app.state, State::Loading(_)) {
                match result {
                    Ok(data) => {
                        // Kick off AI analysis on first load (if key present)
                        if let Some(key) = &app.openrouter_key {
                            app.ai_state = AiState::Loading;
                            let key = key.clone();
                            let json = serde_json::to_string(&data).unwrap_or_default();
                            let ai_tx = ai_tx.clone();
                            tokio::spawn(async move {
                                let res = ai::analyze_stock(&json, &key).await;
                                let _ = ai_tx.send(res).await;
                            });
                        }
                        app.state = State::Loaded(Box::new(data));
                    }
                    Err(msg) => {
                        let ticker = app.input.clone();
                        app.state = State::Error { ticker, message: msg };
                    }
                }
                app.active_tab = 0;
            }
        }

        // Receive AI analysis result
        if let Ok(result) = ai_rx.try_recv() {
            app.ai_state = match result {
                Ok(text) => AiState::Done(text),
                Err(msg) => AiState::Failed(msg),
            };
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
