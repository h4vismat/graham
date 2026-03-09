use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

use crate::models::CompanyProfile;
use crate::yahoo::yahoo_symbols_for_ticker;

const YAHOO_PROFILE_PAGE_TEMPLATE: &str = "https://finance.yahoo.com/quote/{symbol}/profile/";

const YAHOO_PROFILE_TEMPLATES: [&str; 2] = [
    "https://query1.finance.yahoo.com/v10/finance/quoteSummary/{symbol}?modules=assetProfile",
    "https://query2.finance.yahoo.com/v10/finance/quoteSummary/{symbol}?modules=assetProfile",
];

#[derive(Deserialize)]
struct QuoteSummaryResponse {
    #[serde(rename = "quoteSummary")]
    quote_summary: QuoteSummary,
}

#[derive(Deserialize)]
struct QuoteSummary {
    result: Option<Vec<QuoteSummaryResult>>,
}

#[derive(Deserialize)]
struct QuoteSummaryResult {
    #[serde(rename = "assetProfile")]
    asset_profile: Option<AssetProfile>,
}

#[derive(Deserialize)]
struct AssetProfile {
    #[serde(rename = "longBusinessSummary")]
    long_business_summary: Option<String>,
    sector: Option<String>,
    industry: Option<String>,
}

pub async fn fetch_profile_for_ticker(
    client: &Client,
    ticker: &str,
) -> Option<CompanyProfile> {
    let symbols = yahoo_symbols_for_ticker(ticker);
    for symbol in symbols {
        if let Some(profile) = fetch_profile_for_symbol(client, &symbol).await {
            return Some(profile);
        }
    }
    None
}

async fn fetch_profile_for_symbol(
    client: &Client,
    symbol: &str,
) -> Option<CompanyProfile> {
    if let Some(profile) = fetch_profile_from_html(client, symbol).await {
        return Some(profile);
    }

    let referer = format!("https://finance.yahoo.com/quote/{symbol}/profile/");

    for template in YAHOO_PROFILE_TEMPLATES {
        let url = template.replace("{symbol}", symbol);
        let resp = client
            .get(url)
            .header("Accept", "application/json, text/plain;q=0.9, */*;q=0.8")
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("Referer", &referer)
            .send()
            .await
            .ok()?;

        if !resp.status().is_success() {
            continue;
        }

        let text = resp.text().await.ok()?;
        if text.trim_start().starts_with('<') {
            continue;
        }

        if let Some(profile) = parse_profile(&text) {
            return Some(profile);
        }
    }

    None
}

fn parse_profile(text: &str) -> Option<CompanyProfile> {
    let response: QuoteSummaryResponse = serde_json::from_str(text).ok()?;
    let profile = response
        .quote_summary
        .result?
        .into_iter()
        .find_map(|r| r.asset_profile)?;

    profile_to_company(profile)
}

async fn fetch_profile_from_html(
    client: &Client,
    symbol: &str,
) -> Option<CompanyProfile> {
    let url = YAHOO_PROFILE_PAGE_TEMPLATE.replace("{symbol}", symbol);
    let resp = client
        .get(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
             AppleWebKit/537.36 (KHTML, like Gecko) \
             Chrome/121.0.0.0 Safari/537.36",
        )
        .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let html = resp.text().await.ok()?;
    parse_profile_from_html(&html)
}

fn parse_profile_from_html(html: &str) -> Option<CompanyProfile> {
    if let Some(json) = extract_root_app_main(html) {
        if let Ok(value) = serde_json::from_str::<Value>(&json) {
            if let Some(profile) = find_profile_in_value(&value) {
                return Some(profile);
            }
        }
    }

    if let Some(json) = extract_next_data(html) {
        if let Ok(value) = serde_json::from_str::<Value>(&json) {
            if let Some(profile) = find_profile_in_value(&value) {
                return Some(profile);
            }
        }
    }

    None
}

fn extract_root_app_main(html: &str) -> Option<String> {
    let marker = "root.App.main";
    let start = html.find(marker)?;
    let after = &html[start + marker.len()..];
    let assign = after.find('=')?;
    let after_assign = &after[assign + 1..];
    let first_brace = after_assign.find('{')?;
    let json = extract_balanced_json(&after_assign[first_brace..])?;
    Some(json)
}

fn extract_next_data(html: &str) -> Option<String> {
    let marker = "id=\"__NEXT_DATA__\"";
    let start = html.find(marker)?;
    let after = &html[start..];
    let tag_start = after.find('>')?;
    let content = &after[tag_start + 1..];
    let end = content.find("</script>")?;
    Some(content[..end].trim().to_string())
}

fn extract_balanced_json(s: &str) -> Option<String> {
    let mut depth = 0;
    let mut in_string = false;
    let mut escape = false;
    for (i, ch) in s.char_indices() {
        match ch {
            '"' if !escape => in_string = !in_string,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(s[..=i].to_string());
                }
            }
            _ => {}
        }
        escape = !escape && ch == '\\';
    }
    None
}

fn find_profile_in_value(value: &Value) -> Option<CompanyProfile> {
    match value {
        Value::Object(map) => {
            if let Some(profile) = map.get("summaryProfile").and_then(|v| v.as_object()) {
                if let Some(company) = profile_from_object(profile) {
                    return Some(company);
                }
            }

            if let Some(company) = profile_from_object(map) {
                return Some(company);
            }

            for val in map.values() {
                if let Some(company) = find_profile_in_value(val) {
                    return Some(company);
                }
            }
        }
        Value::Array(arr) => {
            for val in arr {
                if let Some(company) = find_profile_in_value(val) {
                    return Some(company);
                }
            }
        }
        _ => {}
    }
    None
}

fn profile_from_object(map: &serde_json::Map<String, Value>) -> Option<CompanyProfile> {
    let description = map
        .get("longBusinessSummary")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string());

    let description = match description {
        Some(text) if !text.is_empty() => text,
        _ => return None,
    };

    let sector = map
        .get("sector")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string());
    let industry = map
        .get("industry")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string());

    Some(CompanyProfile {
        description,
        sector,
        industry,
    })
}

fn profile_to_company(profile: AssetProfile) -> Option<CompanyProfile> {
    let description = profile.long_business_summary?.trim().to_string();
    if description.is_empty() {
        return None;
    }

    Some(CompanyProfile {
        description,
        sector: profile.sector.map(|s| s.trim().to_string()),
        industry: profile.industry.map(|s| s.trim().to_string()),
    })
}
