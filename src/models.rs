use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricePoint {
    pub date: String,
    pub price: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsItem {
    pub title: String,
    pub link: String,
    pub publisher: Option<String>,
    pub published_at: Option<String>,
    /// Short description / summary (RSS description, or extra columns from
    /// Fundamentus fatos relevantes).
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuarterlyReport {
    /// e.g. "3T2024" or "12/2023"
    pub period: String,
    /// Publication date as shown by Fundamentus (e.g. "14/11/2024")
    pub published: String,
    /// Direct URL to the rad.cvm.gov.br document (resolved from the
    /// Fundamentus redirect at fetch time).
    pub link: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanyProfile {
    pub description: String,
    pub sector: Option<String>,
    pub industry: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct YearlyValue {
    pub year: i32,
    pub value: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IndicatorData {
    pub current: f64,
    /// Historical yearly values sorted oldest → newest (up to 10 years).
    pub history: Vec<YearlyValue>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Valuation {
    pub dy: IndicatorData,
    pub p_e: IndicatorData,
    pub p_b: IndicatorData,
    pub p_ebitda: IndicatorData,
    pub p_ebit: IndicatorData,
    pub p_s: IndicatorData,
    pub p_assets: IndicatorData,
    pub p_working_capital: IndicatorData,
    pub p_net_current_assets: IndicatorData,
    pub ev_ebitda: IndicatorData,
    pub ev_ebit: IndicatorData,
    pub eps: IndicatorData,
    pub bvps: IndicatorData,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Debt {
    pub net_debt_equity: IndicatorData,
    pub net_debt_ebitda: IndicatorData,
    pub net_debt_ebit: IndicatorData,
    pub equity_to_assets: IndicatorData,
    pub liabilities_to_assets: IndicatorData,
    pub current_ratio: IndicatorData,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Efficiency {
    pub gross_margin: IndicatorData,
    pub ebitda_margin: IndicatorData,
    pub ebit_margin: IndicatorData,
    pub net_margin: IndicatorData,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Profitability {
    pub roe: IndicatorData,
    pub roa: IndicatorData,
    pub roic: IndicatorData,
    pub asset_turnover: IndicatorData,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Growth {
    pub revenue_cagr5: IndicatorData,
    pub earnings_cagr5: IndicatorData,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StockIndicators {
    pub ticker: String,
    pub current_price: f64,
    pub min_52w: f64,
    pub max_52w: f64,
    pub min_month: f64,
    pub max_month: f64,
    /// Dividend yield (%) based on last 12 months.
    pub dividend_yield: f64,
    /// Price appreciation (%) over the last 12 months. Negative = decay.
    pub growth_12m: f64,
    /// Price appreciation (%) in the current month. Negative = decay.
    pub growth_month: f64,
    pub valuation: Valuation,
    pub debt: Debt,
    pub efficiency: Efficiency,
    pub profitability: Profitability,
    pub growth: Growth,
    /// Full 5-year daily price history, oldest → newest. Shorter periods are sliced from the tail.
    #[serde(skip_serializing)]
    pub price_history: Vec<PricePoint>,
    /// Latest news headlines (if available).
    #[serde(skip_serializing)]
    pub news: Option<Vec<NewsItem>>,
    /// Company profile data from Yahoo Finance (if available).
    #[serde(skip_serializing)]
    pub profile: Option<CompanyProfile>,
    /// Quarterly / annual ITR-DFP filings from CVM via Fundamentus.
    /// Only populated for Brazilian (B3) tickers.
    #[serde(skip_serializing)]
    pub quarterly_reports: Option<Vec<QuarterlyReport>>,
}
