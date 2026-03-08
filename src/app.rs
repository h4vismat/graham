use crate::models::StockIndicators;

pub const TABS: &[&str] = &[
    "Overview",
    "Valuation",
    "Debt",
    "Efficiency",
    "Profitability",
    "Growth",
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
        }
    }

    pub fn on_tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
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
    }
}
