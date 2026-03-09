use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::HashMap;

const SEC_TICKER_URL: &str = "https://www.sec.gov/files/company_tickers_exchange.json";
const REFRESH_INTERVAL_SECS: u64 = 24 * 60 * 60;

#[derive(serde::Deserialize)]
struct SecTickerFile {
    fields: Vec<String>,
    data: Vec<Vec<Value>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CompanyRecord {
    pub cik: i64,
    pub name: String,
    pub ticker: String,
    pub exchange: String,
}

impl SecTickerFile {
    fn parse_records(self) -> Result<Vec<CompanyRecord>> {
        let field_index: HashMap<&str, usize> = self
            .fields
            .iter()
            .enumerate()
            .map(|(i, f)| (f.as_str(), i))
            .collect();

        let cik_idx = *field_index
            .get("cik")
            .ok_or_else(|| anyhow::anyhow!("missing field: cik"))?;
        let name_idx = *field_index
            .get("name")
            .ok_or_else(|| anyhow::anyhow!("missing field: name"))?;
        let ticker_idx = *field_index
            .get("ticker")
            .ok_or_else(|| anyhow::anyhow!("missing field: ticker"))?;
        let exchange_idx = *field_index
            .get("exchange")
            .ok_or_else(|| anyhow::anyhow!("missing field: exchange"))?;

        let mut out = Vec::with_capacity(self.data.len());

        for row in self.data {
            let cik = row
                .get(cik_idx)
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow::anyhow!("invalid cik in row: {:?}", row))?
                as i64;

            let name = row
                .get(name_idx)
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("invalid name in row: {:?}", row))?
                .to_string();

            let ticker = row
                .get(ticker_idx)
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("invalid ticker in row: {:?}", row))?
                .to_string();

            let exchange = row
                .get(exchange_idx)
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("invalid exchange in row: {:?}", row))?
                .to_string();

            out.push(CompanyRecord {
                cik,
                name,
                ticker,
                exchange,
            });
        }

        Ok(out)
    }
}

pub async fn migrate_sec_tables(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sec_companies (
            cik      INTEGER NOT NULL,
            name     TEXT NOT NULL,
            ticker   TEXT NOT NULL,
            exchange TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_sec_companies_ticker ON sec_companies (ticker)",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sec_fetch_log (
            id         INTEGER PRIMARY KEY CHECK (id = 1),
            fetched_at INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    Ok(())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

async fn last_fetched_at(pool: &SqlitePool) -> Result<Option<u64>> {
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT fetched_at FROM sec_fetch_log WHERE id = 1")
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(ts,)| ts as u64))
}

async fn fetch_and_store(pool: &SqlitePool) -> Result<()> {
    let body = reqwest::get(SEC_TICKER_URL).await?.text().await?;
    let parsed: SecTickerFile = serde_json::from_str(&body)?;
    let records = parsed.parse_records()?;

    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM sec_companies")
        .execute(&mut *tx)
        .await?;

    for rec in &records {
        sqlx::query(
            "INSERT INTO sec_companies (cik, name, ticker, exchange) VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(rec.cik)
        .bind(&rec.name)
        .bind(&rec.ticker)
        .bind(&rec.exchange)
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query(
        "INSERT INTO sec_fetch_log (id, fetched_at) VALUES (1, ?1)
         ON CONFLICT(id) DO UPDATE SET fetched_at = excluded.fetched_at",
    )
    .bind(now_secs() as i64)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Download SEC company data if the local cache is absent or older than 24 hours.
pub async fn sync_sec_companies(pool: &SqlitePool) -> Result<()> {
    let needs_refresh = match last_fetched_at(pool).await? {
        None => true,
        Some(ts) => now_secs().saturating_sub(ts) >= REFRESH_INTERVAL_SECS,
    };

    if needs_refresh {
        fetch_and_store(pool).await?;
    }

    Ok(())
}

/// Look up a company by ticker symbol. Returns `None` if the ticker is not found.
pub async fn lookup_cik(pool: &SqlitePool, ticker: &str) -> Result<Option<CompanyRecord>> {
    let rec = sqlx::query_as::<_, CompanyRecord>(
        "SELECT cik, name, ticker, exchange FROM sec_companies WHERE ticker = ?1 LIMIT 1",
    )
    .bind(ticker.to_uppercase())
    .fetch_optional(pool)
    .await?;

    Ok(rec)
}
