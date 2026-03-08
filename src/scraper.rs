use reqwest::Client;
use scraper::{Html, Selector};
use serde::Deserialize;
use std::collections::HashMap;

use crate::models::*;

// ─── Internal JSON types ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ApiResponse {
    #[allow(dead_code)]
    success: bool,
    data: HashMap<String, Vec<RawIndicator>>,
}

#[derive(Deserialize)]
struct RawIndicator {
    key: String,
    actual: Option<f64>,
    ranks: Vec<RawRank>,
}

#[derive(Deserialize)]
struct RawRank {
    #[serde(rename = "timeType")]
    time_type: i32,
    rank: i32,
    value: Option<f64>,
}

#[derive(Deserialize)]
struct PriceCurrency {
    prices: Vec<RawPricePoint>,
}

#[derive(Deserialize)]
struct RawPricePoint {
    price: f64,
    date: String,
}

// ─── Internal market detection ────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Market {
    Brazil,
    Usa,
}

impl Market {
    /// B3 tickers always end with digits (VALE3, PETR4, BBAS3, HGLG11).
    /// US tickers are purely alphabetical (PLTR, AAPL, MSFT).
    fn detect(ticker: &str) -> Self {
        if ticker.chars().last().map_or(false, |c| c.is_ascii_digit()) {
            Market::Brazil
        } else {
            Market::Usa
        }
    }

    fn page_path(&self, ticker: &str) -> String {
        match self {
            Market::Brazil => format!("acoes/{}", ticker.to_lowercase()),
            Market::Usa => format!("acoes/eua/{}", ticker.to_lowercase()),
        }
    }

    fn api_prefix(&self) -> &'static str {
        match self {
            Market::Brazil => "acao",
            Market::Usa => "stock",
        }
    }
}

// ─── Price history fetch helper ───────────────────────────────────────────────

async fn fetch_price_period(
    client: &Client,
    base_url: &str,
    ticker: &str,
    page_url: &str,
    period_type: i32,
) -> Vec<PricePoint> {
    let url = format!(
        "{}/tickerprice?ticker={}&type={}&currences%5B%5D=1",
        base_url, ticker, period_type
    );
    let Ok(resp) = client
        .get(&url)
        .header("Accept", "application/json, text/javascript, */*; q=0.01")
        .header("X-Requested-With", "XMLHttpRequest")
        .header("Referer", page_url)
        .send()
        .await
    else {
        return vec![];
    };
    let Ok(text) = resp.text().await else {
        return vec![];
    };

    serde_json::from_str::<Vec<PriceCurrency>>(&text)
        .ok()
        .and_then(|mut v| v.pop())
        .map(|c| {
            c.prices
                .into_iter()
                .map(|p| PricePoint {
                    date: p.date,
                    price: p.price,
                })
                .collect()
        })
        .unwrap_or_default()
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Parse a Brazilian-formatted number like "R$ 78,86", "57,72%" or "-10,55%".
fn parse_br_number(s: &str) -> f64 {
    s.trim()
        .replace("R$", "")
        .replace('%', "")
        .replace('.', "")
        .replace(',', ".")
        .trim()
        .parse::<f64>()
        .unwrap_or(0.0)
}

fn select_text(html: &Html, selector_str: &str) -> Option<String> {
    let sel = Selector::parse(selector_str).ok()?;
    html.select(&sel)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
}

fn build_indicator(indicators: &[RawIndicator], key: &str) -> IndicatorData {
    match indicators.iter().find(|r| r.key == key) {
        None => IndicatorData {
            current: 0.0,
            history: vec![],
        },
        Some(r) => {
            let mut history: Vec<YearlyValue> = r
                .ranks
                .iter()
                .filter(|rank| rank.time_type == 0)
                .filter_map(|rank| {
                    rank.value.map(|v| YearlyValue {
                        year: rank.rank,
                        value: v,
                    })
                })
                .collect();
            history.sort_by_key(|y| y.year);
            IndicatorData {
                current: r.actual.unwrap_or(0.0),
                history,
            }
        }
    }
}

// ─── Main scraping function ───────────────────────────────────────────────────

pub async fn scrape_stock(ticker: &str) -> Result<StockIndicators, Box<dyn std::error::Error>> {
    let market = Market::detect(ticker);

    let client = Client::builder()
        .user_agent(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
             AppleWebKit/537.36 (KHTML, like Gecko) \
             Chrome/121.0.0.0 Safari/537.36",
        )
        .build()?;

    let page_url = format!("https://statusinvest.com.br/{}", market.page_path(ticker));
    let api_url = format!(
        "https://statusinvest.com.br/{}/indicatorhistoricallist",
        market.api_prefix()
    );

    // ── Fetch and parse the main page ────────────────────────────────────────
    let html_text = client
        .get(&page_url)
        .header(
            "Accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .header("Accept-Language", "pt-BR,pt;q=0.9")
        .send()
        .await?
        .text()
        .await?;

    // Parse all values from the HTML page in a dedicated block so that `Html`
    // (which is not `Send`) is guaranteed to be dropped before the next `.await`.
    let (
        current_price,
        min_52w,
        max_52w,
        min_month,
        max_month,
        dividend_yield,
        growth_12m,
        growth_month,
    ) = {
        let html = Html::parse_document(&html_text);

        let current_price = select_text(&html, "div.info.special strong.value")
            .map(|s| parse_br_number(&s))
            .unwrap_or(0.0);

        let min_52w = select_text(
            &html,
            r#"div[title="Valor mínimo das últimas 52 semanas"] strong.value"#,
        )
        .map(|s| parse_br_number(&s))
        .unwrap_or(0.0);

        let max_52w = select_text(
            &html,
            r#"div[title="Valor máximo das últimas 52 semanas"] strong.value"#,
        )
        .map(|s| parse_br_number(&s))
        .unwrap_or(0.0);

        let min_month = select_text(
            &html,
            r#"div[title="Valor mínimo do mês atual"] span.sub-value"#,
        )
        .map(|s| parse_br_number(&s))
        .unwrap_or(0.0);

        let max_month = select_text(
            &html,
            r#"div[title="Valor máximo do mês atual"] span.sub-value"#,
        )
        .map(|s| parse_br_number(&s))
        .unwrap_or(0.0);

        let dividend_yield = select_text(
            &html,
            r#"div[title="Dividend Yield com base nos últimos 12 meses"] strong.value"#,
        )
        .map(|s| parse_br_number(&s))
        .unwrap_or(0.0);

        let growth_12m = {
            let raw = select_text(
                &html,
                r#"div[title="Valorização no preço do ativo com base nos últimos 12 meses"] strong.value"#,
            )
            .unwrap_or_default();
            let value = parse_br_number(&raw);
            if raw.contains('-') {
                value
            } else {
                let is_down = Selector::parse(
                    r#"div[title="Valorização no preço do ativo com base nos últimos 12 meses"] i.material-icons"#,
                )
                .ok()
                .and_then(|icon_sel| {
                    html.select(&icon_sel)
                        .next()
                        .and_then(|el| el.value().attr("class"))
                })
                .map(|c| c.contains("value-down-color"))
                .unwrap_or(false);
                if is_down { -value } else { value }
            }
        };

        let growth_month = select_text(
            &html,
            r#"div[title="Valorização no preço do ativo com base no mês atual"] b"#,
        )
        .map(|s| parse_br_number(&s))
        .unwrap_or(0.0);

        // `html` drops here — before the next `.await`
        (
            current_price,
            min_52w,
            max_52w,
            min_month,
            max_month,
            dividend_yield,
            growth_12m,
            growth_month,
        )
    };

    // ── Fetch indicator history from the JSON API ─────────────────────────────
    let api_body = format!("codes[]={}&time=7&byQuarter=false&futureData=false", ticker);

    let api_text = client
        .post(&api_url)
        .header(
            "Content-Type",
            "application/x-www-form-urlencoded; charset=UTF-8",
        )
        .header("X-Requested-With", "XMLHttpRequest")
        .header("Referer", &page_url)
        .body(api_body)
        .send()
        .await?
        .text()
        .await?;

    let api_response: ApiResponse = serde_json::from_str(&api_text)?;

    let indicators = api_response
        .data
        .into_values()
        .next()
        .ok_or("API returned empty data")?;

    // ── Fetch 5-year price history (shorter periods are sliced from the tail) ─
    let price_base = format!("https://statusinvest.com.br/{}", market.api_prefix());
    let price_history = fetch_price_period(&client, &price_base, ticker, &page_url, 4).await;

    Ok(StockIndicators {
        ticker: ticker.to_uppercase(),
        current_price,
        min_52w,
        max_52w,
        min_month,
        max_month,
        dividend_yield,
        growth_12m,
        growth_month,
        valuation: Valuation {
            dy: build_indicator(&indicators, "dy"),
            p_e: build_indicator(&indicators, "p_l"),
            p_b: build_indicator(&indicators, "p_vp"),
            p_ebitda: build_indicator(&indicators, "p_ebita"),
            p_ebit: build_indicator(&indicators, "p_ebit"),
            p_s: build_indicator(&indicators, "p_sr"),
            p_assets: build_indicator(&indicators, "p_ativo"),
            p_working_capital: build_indicator(&indicators, "p_capitlgiro"),
            p_net_current_assets: build_indicator(&indicators, "p_ativocirculante"),
            ev_ebitda: build_indicator(&indicators, "ev_ebitda"),
            ev_ebit: build_indicator(&indicators, "ev_ebit"),
            eps: build_indicator(&indicators, "lpa"),
            bvps: build_indicator(&indicators, "vpa"),
        },
        debt: Debt {
            net_debt_equity: build_indicator(&indicators, "dividaliquida_patrimonioliquido"),
            net_debt_ebitda: build_indicator(&indicators, "dividaliquida_ebitda"),
            net_debt_ebit: build_indicator(&indicators, "dividaliquida_ebit"),
            equity_to_assets: build_indicator(&indicators, "patrimonio_ativo"),
            liabilities_to_assets: build_indicator(&indicators, "passivo_ativo"),
            current_ratio: build_indicator(&indicators, "liquidezcorrente"),
        },
        efficiency: Efficiency {
            gross_margin: build_indicator(&indicators, "margembruta"),
            ebitda_margin: build_indicator(&indicators, "margemebitda"),
            ebit_margin: build_indicator(&indicators, "margemebit"),
            net_margin: build_indicator(&indicators, "margemliquida"),
        },
        profitability: Profitability {
            roe: build_indicator(&indicators, "roe"),
            roa: build_indicator(&indicators, "roa"),
            roic: build_indicator(&indicators, "roic"),
            asset_turnover: build_indicator(&indicators, "giro_ativos"),
        },
        growth: Growth {
            revenue_cagr5: build_indicator(&indicators, "receitas_cagr5"),
            earnings_cagr5: build_indicator(&indicators, "lucros_cagr5"),
        },
        price_history,
    })
}
