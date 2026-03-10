use reqwest::Client;
use serde_json::Value;

use crate::models::{NasdaqFinancials, NasdaqStatementRow, NasdaqStatementTable};

const BASE_URL: &str = "https://api.nasdaq.com";
const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
                  AppleWebKit/537.36 (KHTML, like Gecko) \
                  Chrome/121.0.0.0 Safari/537.36";

#[derive(Clone, Copy, Debug)]
pub enum NasdaqFrequency {
    Quarterly,
    Annual,
}

impl NasdaqFrequency {
    fn param(self) -> u8 {
        match self {
            NasdaqFrequency::Quarterly => 2,
            NasdaqFrequency::Annual => 1,
        }
    }
}

struct ParsedTable {
    periods: Vec<String>,
    rows: Vec<NasdaqStatementRow>,
}

pub async fn fetch_financials(
    client: &Client,
    ticker: &str,
    frequency: NasdaqFrequency,
) -> Option<NasdaqFinancials> {
    let url = format!(
        "{}/api/company/{}/financials?frequency={}",
        BASE_URL,
        ticker.to_uppercase(),
        frequency.param()
    );

    let resp = client
        .get(url)
        .header("User-Agent", UA)
        .header("Accept", "application/json, text/plain, */*")
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Origin", "https://www.nasdaq.com")
        .header("Referer", "https://www.nasdaq.com/")
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let value: Value = resp.json().await.ok()?;
    parse_financials(&value)
}

fn parse_financials(value: &Value) -> Option<NasdaqFinancials> {
    let data = value.get("data")?;

    let income = parse_table(data.get("incomeStatementTable"));
    let balance = parse_table(data.get("balanceSheetTable"));
    let cash = parse_table(data.get("cashFlowTable"));

    let periods = income
        .as_ref()
        .filter(|t| !t.periods.is_empty())
        .map(|t| t.periods.clone())
        .or_else(|| {
            balance
                .as_ref()
                .filter(|t| !t.periods.is_empty())
                .map(|t| t.periods.clone())
        })
        .or_else(|| {
            cash.as_ref()
                .filter(|t| !t.periods.is_empty())
                .map(|t| t.periods.clone())
        })?;

    let income = normalize_table(income, &periods);
    let balance = normalize_table(balance, &periods);
    let cash = normalize_table(cash, &periods);

    Some(NasdaqFinancials {
        periods,
        income_statement: income,
        balance_sheet: balance,
        cash_flow: cash,
    })
}

fn parse_table(table: Option<&Value>) -> Option<ParsedTable> {
    let table = table?;
    let rows_value = table.get("rows")?.as_array()?;
    let (label_key, data_keys, periods) = parse_headers(table, rows_value);

    if data_keys.is_empty() {
        return None;
    }

    let mut rows = Vec::new();
    for row in rows_value {
        let name = row_label(row, label_key.as_deref());
        if name.is_empty() {
            continue;
        }

        let values = data_keys
            .iter()
            .map(|key| json_to_string(row.get(key)))
            .collect();
        rows.push(NasdaqStatementRow {
            label: name,
            values,
        });
    }

    if rows.is_empty() {
        return None;
    }

    Some(ParsedTable { periods, rows })
}

fn parse_headers(table: &Value, rows: &[Value]) -> (Option<String>, Vec<String>, Vec<String>) {
    let mut label_key = None;
    let mut data_keys = Vec::new();
    let mut periods = Vec::new();

    if let Some(headers) = table.get("headers").and_then(Value::as_array) {
        let mut entries: Vec<(String, String)> = Vec::new();
        for header in headers {
            let key = header_key(header);
            if let Some(key) = key {
                let label = header
                    .get("label")
                    .and_then(Value::as_str)
                    .unwrap_or(key)
                    .trim()
                    .to_string();
                entries.push((key.to_string(), label));
            }
        }
        if !entries.is_empty() {
            entries.sort_by_key(|(k, _)| key_suffix(k));
            if entries.first().map(|(k, _)| k == "value1").unwrap_or(false) {
                label_key = Some("value1".to_string());
                data_keys = entries.iter().skip(1).map(|(k, _)| k.clone()).collect();
                periods = entries.iter().skip(1).map(|(_, l)| l.clone()).collect();
            } else {
                data_keys = entries.iter().map(|(k, _)| k.clone()).collect();
                periods = entries.iter().map(|(_, l)| l.clone()).collect();
            }
        }
    } else if let Some(headers) = table.get("headers").and_then(Value::as_object) {
        let mut entries: Vec<(String, String)> = headers
            .iter()
            .filter(|(k, _)| k.starts_with("value"))
            .map(|(k, v)| (k.clone(), json_to_string(Some(v))))
            .collect();
        if !entries.is_empty() {
            entries.sort_by_key(|(k, _)| key_suffix(k));
            label_key = Some(entries[0].0.clone());
            data_keys = entries.iter().skip(1).map(|(k, _)| k.clone()).collect();
            periods = entries.iter().skip(1).map(|(_, l)| l.clone()).collect();
        }
    }

    if data_keys.is_empty() {
        if let Some(first) = rows.first().and_then(Value::as_object) {
            let mut keys: Vec<String> = first
                .keys()
                .filter(|k| k.starts_with("value"))
                .cloned()
                .collect();
            keys.sort_by_key(|k| key_suffix(k));
            if keys.first().map(|k| k == "value1").unwrap_or(false) {
                label_key = Some("value1".to_string());
                data_keys = keys.iter().skip(1).cloned().collect();
            } else {
                data_keys = keys.clone();
            }
            if periods.is_empty() && !data_keys.is_empty() {
                periods = data_keys.clone();
            }
        }
    }

    (label_key, data_keys, periods)
}

fn header_key(header: &Value) -> Option<&str> {
    for key in ["value", "field", "name", "key"] {
        if let Some(v) = header.get(key).and_then(Value::as_str) {
            let trimmed = v.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

fn row_label(row: &Value, label_key: Option<&str>) -> String {
    if let Some(label_key) = label_key {
        if let Some(v) = row.get(label_key).and_then(Value::as_str) {
            let trimmed = v.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    for key in ["name", "label", "rowTitle", "statementLine", "value1"] {
        if let Some(v) = row.get(key).and_then(Value::as_str) {
            let trimmed = v.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    String::new()
}

fn key_suffix(key: &str) -> u32 {
    key.trim_start_matches("value")
        .parse::<u32>()
        .unwrap_or(u32::MAX)
}

fn normalize_table(table: Option<ParsedTable>, periods: &[String]) -> NasdaqStatementTable {
    match table {
        Some(mut parsed) => {
            for row in &mut parsed.rows {
                if row.values.len() > periods.len() {
                    row.values.truncate(periods.len());
                } else if row.values.len() < periods.len() {
                    row.values
                        .resize(periods.len(), "-".to_string());
                }
            }
            NasdaqStatementTable { rows: parsed.rows }
        }
        None => NasdaqStatementTable { rows: Vec::new() },
    }
}

fn json_to_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                "-".to_string()
            } else {
                trimmed.to_string()
            }
        }
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Null) | None => "-".to_string(),
        Some(other) => {
            let rendered = other.to_string();
            if rendered.is_empty() || rendered == "null" {
                "-".to_string()
            } else {
                rendered
            }
        }
    }
}
