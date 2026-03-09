use crate::financials::FinancialSelection;
use crate::models::StockIndicators;

pub const TABS: &[&str] = &[
    "Overview",
    "Financials",
    "AI",
    "News",
];

pub const PERIODS: &[&str] = &["30d", "6m", "1y", "5y"];

pub enum Screen {
    Menu,
    Stock,
    History,
}

pub enum MenuMode {
    Idle,
    StockInput,
}

pub enum StockState {
    Input,
    Loading(String),
    Loaded(Box<StockIndicators>),
    Error { ticker: String, message: String },
}

pub enum AiState {
    Unavailable,
    Loading,
    Done(String),
    Failed(String),
}

#[derive(Clone)]
pub enum ChatRole {
    User,
    Assistant,
}

#[derive(Clone)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

pub enum ChatState {
    Idle,
    Loading,
    Failed(String),
}

pub struct StockScreen {
    pub state: StockState,
    pub input: String,
    pub active_tab: usize,
    pub active_period: usize, // 0=30d 1=6m 2=1y 3=5y
    pub ai_state: AiState,
    pub chat_state: ChatState,
    pub chat_messages: Vec<ChatMessage>,
    pub chat_input: String,
    pub chat_scroll: usize,
    pub news_selected: usize,
    pub financials_selected: FinancialSelection,
    pub financials_modal: Option<FinancialSelection>,
}

#[derive(Clone)]
pub struct TradeRow {
    pub id: i64,
    pub ticker: String,
    pub side: String,
    pub shares: f64,
    pub price: f64,
    pub date: String,
}

#[derive(Clone)]
pub struct HoldingRow {
    pub ticker: String,
    pub shares: f64,
    pub avg_cost: f64,
    pub current_price: Option<f64>,
}

pub struct HistoryForm {
    pub id: Option<i64>,
    pub ticker: String,
    pub side: String,
    pub shares: String,
    pub price: String,
    pub date: String,
    pub active_field: usize,
}

pub enum HistoryMode {
    View,
    Add(HistoryForm),
    Edit(HistoryForm),
    DeleteConfirm { id: i64, ticker: String },
}

pub struct HistoryScreen {
    pub trades: Vec<TradeRow>,
    pub holdings: Vec<HoldingRow>,
    pub selected: usize,
    pub mode: HistoryMode,
}

pub struct App {
    pub screen: Screen,
    pub menu_mode: MenuMode,
    pub stock: StockScreen,
    pub history: HistoryScreen,
    pub should_quit: bool,
    pub tick: u64,
    pub openrouter_key: Option<String>,
    pub status_message: Option<String>,
    pub status_expires_at: Option<u64>,
}

impl App {
    pub fn new() -> Self {
        let openrouter_key = std::env::var("OPENROUTER_API_KEY").ok();
        Self {
            screen: Screen::Menu,
            menu_mode: MenuMode::Idle,
            stock: StockScreen::new(),
            history: HistoryScreen::new(),
            should_quit: false,
            tick: 0,
            openrouter_key,
            status_message: None,
            status_expires_at: None,
        }
    }

    pub fn on_tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        if let Some(expires_at) = self.status_expires_at {
            if self.tick >= expires_at {
                self.status_message = None;
                self.status_expires_at = None;
            }
        }
    }

    pub fn set_status(&mut self, message: impl Into<String>, ttl_ticks: u64) {
        self.status_message = Some(message.into());
        self.status_expires_at = Some(self.tick.saturating_add(ttl_ticks));
    }
}

impl StockScreen {
    pub fn new() -> Self {
        Self {
            state: StockState::Input,
            input: String::new(),
            active_tab: 0,
            active_period: 2, // default 1y
            ai_state: AiState::Unavailable,
            chat_state: ChatState::Idle,
            chat_messages: Vec::new(),
            chat_input: String::new(),
            chat_scroll: 0,
            news_selected: 0,
            financials_selected: FinancialSelection::new(0, 0),
            financials_modal: None,
        }
    }

    pub fn next_tab(&mut self) {
        self.active_tab = (self.active_tab + 1) % TABS.len();
        self.financials_modal = None;
    }

    pub fn prev_tab(&mut self) {
        self.active_tab = (self.active_tab + TABS.len() - 1) % TABS.len();
        self.financials_modal = None;
    }

    pub fn next_period(&mut self) {
        self.active_period = (self.active_period + 1) % PERIODS.len();
    }

    pub fn prev_period(&mut self) {
        self.active_period = (self.active_period + PERIODS.len() - 1) % PERIODS.len();
    }

    pub fn go_to_input(&mut self) {
        self.state = StockState::Input;
        self.input.clear();
        self.active_tab = 0;
        self.ai_state = AiState::Unavailable;
        self.chat_state = ChatState::Idle;
        self.chat_messages.clear();
        self.chat_input.clear();
        self.chat_scroll = 0;
        self.news_selected = 0;
        self.financials_selected = FinancialSelection::new(0, 0);
        self.financials_modal = None;
    }

    pub fn clamp_news_selection(&mut self, len: usize) {
        if len == 0 {
            self.news_selected = 0;
        } else if self.news_selected >= len {
            self.news_selected = len - 1;
        }
    }

    pub fn next_news(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        self.news_selected = (self.news_selected + 1).min(len - 1);
    }

    pub fn prev_news(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        self.news_selected = self.news_selected.saturating_sub(1);
    }
}

impl HistoryScreen {
    pub fn new() -> Self {
        Self {
            trades: Vec::new(),
            holdings: Vec::new(),
            selected: 0,
            mode: HistoryMode::View,
        }
    }

    pub fn clamp_selection(&mut self) {
        if self.trades.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.trades.len() {
            self.selected = self.trades.len() - 1;
        }
    }

    pub fn next(&mut self) {
        if self.trades.is_empty() {
            return;
        }
        self.selected = (self.selected + 1).min(self.trades.len() - 1);
    }

    pub fn prev(&mut self) {
        if self.trades.is_empty() {
            return;
        }
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn selected_trade(&self) -> Option<&TradeRow> {
        self.trades.get(self.selected)
    }
}

impl HistoryForm {
    pub fn new_add() -> Self {
        Self {
            id: None,
            ticker: String::new(),
            side: "BUY".to_string(),
            shares: String::new(),
            price: String::new(),
            date: String::new(),
            active_field: 0,
        }
    }

    pub fn new_edit(row: &TradeRow) -> Self {
        Self {
            id: Some(row.id),
            ticker: row.ticker.clone(),
            side: row.side.clone(),
            shares: format!("{:.4}", row.shares),
            price: format!("{:.4}", row.price),
            date: format_date_display(&row.date),
            active_field: 0,
        }
    }
}

fn format_date_display(date: &str) -> String {
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
