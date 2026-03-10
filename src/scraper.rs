use reqwest::Client;
use scraper::{Html, Selector};
use serde::Deserialize;
use std::collections::HashMap;

use crate::models::*;
use crate::{news, profile, sources::fundamentus, sources::nasdaq};

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

// ─── News date sorting ────────────────────────────────────────────────────────

/// Convert a `published_at` string to a comparable "YYYY-MM-DD" key so that
/// items from different sources (RFC 2822 from Yahoo, DD/MM/YYYY from
/// Fundamentus) sort correctly.  Unknown formats return an empty string
/// (those items sink to the bottom).
fn date_sort_key(published_at: &str) -> String {
    let s = published_at.trim();

    // Fundamentus: "DD/MM/YYYY"
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() == 3 && parts[2].len() == 4 {
        return format!("{}-{}-{}", parts[2], parts[1], parts[0]);
    }

    // RFC 2822: "Mon, 09 Mar 2026 10:00:00 +0000"
    //   or without day-of-week: "09 Mar 2026 10:00:00 +0000"
    let s = if let Some(after_comma) = s.find(',') {
        s[after_comma + 1..].trim()
    } else {
        s
    };
    let tokens: Vec<&str> = s.split_whitespace().collect();
    if tokens.len() >= 3 {
        let day = tokens[0];
        let month = match tokens[1].to_lowercase().as_str() {
            "jan" | "january" => "01",
            "feb" | "february" => "02",
            "mar" | "march" => "03",
            "apr" | "april" => "04",
            "may" => "05",
            "jun" | "june" => "06",
            "jul" | "july" => "07",
            "aug" | "august" => "08",
            "sep" | "september" => "09",
            "oct" | "october" => "10",
            "nov" | "november" => "11",
            "dec" | "december" => "12",
            _ => return String::new(),
        };
        let year = tokens[2];
        return format!("{}-{}-{:0>2}", year, month, day);
    }

    String::new()
}

/// Merge two optional news lists, sort most-recent-first, limit to 50 items.
fn merge_and_sort_news(
    a: Option<Vec<NewsItem>>,
    b: Option<Vec<NewsItem>>,
) -> Option<Vec<NewsItem>> {
    let mut combined: Vec<NewsItem> = Vec::new();
    if let Some(v) = a {
        combined.extend(v);
    }
    if let Some(v) = b {
        combined.extend(v);
    }
    if combined.is_empty() {
        return None;
    }
    combined.sort_by(|x, y| {
        let kx = x
            .published_at
            .as_deref()
            .map(date_sort_key)
            .unwrap_or_default();
        let ky = y
            .published_at
            .as_deref()
            .map(date_sort_key)
            .unwrap_or_default();
        ky.cmp(&kx) // descending
    });
    combined.truncate(50);
    Some(combined)
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
    let price_client = client.clone();
    let fatos_client = client.clone();
    let yahoo_client = client.clone();
    let profile_client = client.clone();
    let reports_client = client.clone();
    let nasdaq_client = client.clone();
    let is_brazil = matches!(market, Market::Brazil);
    let is_usa = matches!(market, Market::Usa);
    let (
        price_history,
        fatos_news,
        yahoo_news,
        profile,
        quarterly_reports,
        nasdaq_quarterly,
        nasdaq_annual,
    ) = tokio::join!(
        fetch_price_period(&price_client, &price_base, ticker, &page_url, 4),
        async {
            if is_brazil {
                fundamentus::fetch_fatos_relevantes(&fatos_client, ticker).await
            } else {
                None
            }
        },
        news::fetch_yahoo_news_for_ticker(&yahoo_client, ticker),
        profile::fetch_profile_for_ticker(&profile_client, ticker),
        async {
            if is_brazil {
                fundamentus::fetch_quarterly_reports(&reports_client, ticker).await
            } else {
                None
            }
        },
        async {
            if is_usa {
                nasdaq::fetch_financials(&nasdaq_client, ticker, nasdaq::NasdaqFrequency::Quarterly)
                    .await
            } else {
                None
            }
        },
        async {
            if is_usa {
                nasdaq::fetch_financials(&nasdaq_client, ticker, nasdaq::NasdaqFrequency::Annual)
                    .await
            } else {
                None
            }
        },
    );
    let news = merge_and_sort_news(fatos_news, yahoo_news);

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
        news,
        profile,
        quarterly_reports,
        nasdaq_financials_quarterly: nasdaq_quarterly,
        nasdaq_financials_annual: nasdaq_annual,
    })
}

pub async fn fetch_current_price(
    ticker: &str,
) -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
    let market = Market::detect(ticker);
    let client = Client::builder()
        .user_agent(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
             AppleWebKit/537.36 (KHTML, like Gecko) \
             Chrome/121.0.0.0 Safari/537.36",
        )
        .build()?;

    let page_url = format!("https://statusinvest.com.br/{}", market.page_path(ticker));
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

    let html = Html::parse_document(&html_text);
    let price_text = select_text(&html, "div.info.special strong.value")
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "Price not found"))?;
    let current_price = parse_br_number(&price_text);
    if current_price <= 0.0 {
        return Err(std::io::Error::new(std::io::ErrorKind::Other, "Price invalid").into());
    }

    Ok(current_price)
}
