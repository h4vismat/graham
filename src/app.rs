use crate::models::StockIndicators;

pub const TABS: &[&str] = &[
    "Overview",
    "Valuation",
    "Debt",
    "Efficiency",
    "Profitability",
    "Growth",
    "News",
];

pub const PERIODS: &[&str] = &["30d", "6m", "1y", "5y"];

pub enum State {
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

pub struct App {
    pub state: State,
    pub input: String,
    pub active_tab: usize,
    pub active_period: usize, // 0=30d 1=6m 2=1y 3=5y
    pub should_quit: bool,
    pub tick: u64,
    pub ai_state: AiState,
    pub openrouter_key: Option<String>,
    pub news_selected: usize,
    pub status_message: Option<String>,
    pub status_expires_at: Option<u64>,
}

impl App {
    pub fn new() -> Self {
        let openrouter_key = std::env::var("OPENROUTER_API_KEY").ok();
        Self {
            state: State::Input,
            input: String::new(),
            active_tab: 0,
            active_period: 2, // default 1y
            should_quit: false,
            tick: 0,
            ai_state: AiState::Unavailable,
            openrouter_key,
            news_selected: 0,
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
        self.state = State::Input;
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

    pub fn set_status(&mut self, message: impl Into<String>, ttl_ticks: u64) {
        self.status_message = Some(message.into());
        self.status_expires_at = Some(self.tick.saturating_add(ttl_ticks));
    }
}
