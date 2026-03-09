use crate::models::StockIndicators;

pub const TABS: &[&str] = &[
    "Overview",
    "Financials",
    "News",
];

pub const PERIODS: &[&str] = &["30d", "6m", "1y", "5y"];

pub enum Screen {
    Menu,
    Stock,
    Positions,
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

pub struct StockScreen {
    pub state: StockState,
    pub input: String,
    pub active_tab: usize,
    pub active_period: usize, // 0=30d 1=6m 2=1y 3=5y
    pub ai_state: AiState,
    pub news_selected: usize,
}

#[derive(Clone)]
pub struct PositionRow {
    pub id: i64,
    pub ticker: String,
    pub shares: f64,
    pub avg_cost: f64,
    pub current_price: Option<f64>,
}

pub struct PositionForm {
    pub id: Option<i64>,
    pub ticker: String,
    pub shares: String,
    pub avg_cost: String,
    pub active_field: usize,
}

pub enum PositionsMode {
    View,
    Add(PositionForm),
    Edit(PositionForm),
    DeleteConfirm { id: i64, ticker: String },
}

pub struct PositionsScreen {
    pub rows: Vec<PositionRow>,
    pub selected: usize,
    pub mode: PositionsMode,
}

pub struct App {
    pub screen: Screen,
    pub menu_mode: MenuMode,
    pub stock: StockScreen,
    pub positions: PositionsScreen,
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
            positions: PositionsScreen::new(),
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
            news_selected: 0,
        }
    }

    pub fn next_tab(&mut self) {
        self.active_tab = (self.active_tab + 1) % TABS.len();
    }

    pub fn prev_tab(&mut self) {
        self.active_tab = (self.active_tab + TABS.len() - 1) % TABS.len();
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
        self.news_selected = 0;
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

impl PositionsScreen {
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            selected: 0,
            mode: PositionsMode::View,
        }
    }

    pub fn clamp_selection(&mut self) {
        if self.rows.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.rows.len() {
            self.selected = self.rows.len() - 1;
        }
    }

    pub fn next(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        self.selected = (self.selected + 1).min(self.rows.len() - 1);
    }

    pub fn prev(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn selected_row(&self) -> Option<&PositionRow> {
        self.rows.get(self.selected)
    }
}

impl PositionForm {
    pub fn new_add() -> Self {
        Self {
            id: None,
            ticker: String::new(),
            shares: String::new(),
            avg_cost: String::new(),
            active_field: 0,
        }
    }

    pub fn new_edit(row: &PositionRow) -> Self {
        Self {
            id: Some(row.id),
            ticker: row.ticker.clone(),
            shares: format!("{:.4}", row.shares),
            avg_cost: format!("{:.4}", row.avg_cost),
            active_field: 0,
        }
    }
}
