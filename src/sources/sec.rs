use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use chrono::{DateTime, FixedOffset, Utc};
use rayon::prelude::*;
use reqwest::header::{ACCEPT, HeaderValue, USER_AGENT};
use scraper::{Html, Selector};
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::HashMap;

const SEC_TICKER_URL: &str = "https://www.sec.gov/files/company_tickers.json";
const REFRESH_INTERVAL_SECS: u64 = 24 * 60 * 60;
const RELEVANT_FORMS: [&'static str; 3] = ["10-K", "10-Q", "8-K"];

const GRAHAM_USER_AGENT: &'static str = "Graham/v0.1 bot@usegraham.io";

#[derive(Debug, serde::Deserialize, sqlx::FromRow)]
struct Company {
    #[serde(rename = "cik_str")]
    pub cik: i32,
    pub ticker: String,
    pub title: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Filings {
    pub accession_number: Vec<String>,
    pub filing_date: Vec<String>,
    pub form: Vec<String>,
    pub primary_document: Vec<String>,
}

#[derive(serde::Deserialize)]
struct RecentFilings {
    pub recent: Filings,
}

#[derive(serde::Deserialize)]
struct Submissions {
    pub cik: String,
    #[serde(rename = "entityType")]
    pub entity_type: String,
    pub name: String,
    pub filings: RecentFilings,
}

type Companies = HashMap<String, Company>;

struct Filing {
    pub cik: i32,
    pub accession_number: String,
    pub filing_date: chrono::DateTime<chrono::Utc>,
    pub form: String,
    pub primary_document: String,
}

impl Filing {
    #[inline]
    pub fn document_path(&self) -> String {
        let clean_doc = self.primary_document.replace("-", "");
        let doc_path = format!("{}/{}", clean_doc, self.primary_document);

        doc_path
    }
}

pub struct SecApi {
    client: reqwest::Client,
    pool: SqlitePool,
}

impl SecApi {
    async fn retrieve_company(&self, ticker: &str) -> Result<Option<Company>> {
        let company: Option<Company> =
            sqlx::query_as("SELECT cik, name, ticker FROM sec_companies WHERE ticker = ?1")
                .bind(ticker)
                .fetch_optional(&self.pool)
                .await?;

        Ok(company)
    }

    async fn retrieve_submission(&self, company: &Company) -> Result<Submissions> {
        let cik = format!("{:0>10}", company.cik);
        let url = format!("https://data.sec.gov/submissions/CIK{}.json", cik);

        let response = self
            .client
            .get(url)
            .header("User-Agent", "Graham/v0.1 reader@usegraham.io")
            .send()
            .await?
            .text()
            .await?;

        let submissions: Submissions = serde_json::from_str(&response)?;

        Ok(submissions)
    }

    async fn lookup_relevant_filings(
        &self,
        cik: i32,
        submissions: Submissions,
    ) -> Result<Vec<Filing>> {
        let mut filings = vec![];

        for (idx, form) in submissions.filings.recent.form.iter().enumerate() {
            if RELEVANT_FORMS.contains(&form.as_str()) {
                let filing = Filing {
                    cik,
                    accession_number: submissions.filings.recent.accession_number[idx].clone(),
                    filing_date: format!(
                        "{}T00:00:00Z",
                        submissions.filings.recent.filing_date[idx].clone()
                    )
                    .parse::<DateTime<Utc>>()?,
                    primary_document: submissions.filings.recent.primary_document[idx].clone(),
                    form: form.clone(),
                };

                filings.push(filing);
            }
        }

        Ok(filings)
    }

    /// Parse the table of contents and return the available section numbers
    /// (e.g. `["1", "1A", "1B", "2", "7", "7A", …]`) in TOC order.
    pub fn lookup_sections(page: &str) -> Vec<String> {
        Self::section_anchor_map(page).0
    }

    /// Extract the plain-text content of a named section (e.g. `"1A"`, `"7"`)
    /// from an already-fetched 10-K page.  Returns `None` if the section is
    /// not present in the document.
    pub fn extract_section(page: &str, section: &str) -> Option<String> {
        let (order, map) = Self::section_anchor_map(page);
        let anchor = map.get(section)?;

        // Resolve each section's byte offset in the raw HTML so we can slice
        // between the start of the target section and the start of the next one.
        let positions: Vec<usize> = order
            .iter()
            .filter_map(|sec| {
                let id = map.get(sec)?;
                page.find(&format!("id=\"{}\"", id))
            })
            .collect();

        let start = page.find(&format!("id=\"{}\"", anchor))?;
        let idx = positions.iter().position(|&p| p == start)?;
        let end = positions.get(idx + 1).copied().unwrap_or(page.len());

        let fragment = Html::parse_fragment(&page[start..end]);
        let text = fragment
            .root_element()
            .text()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n");

        Some(text)
    }

    // ── internal helpers ────────────────────────────────────────────────────

    /// Build a mapping of section number -> anchor id by scraping the TOC.
    /// Also returns section numbers in their TOC order.
    ///
    /// The TOC contains anchors like:
    ///   `<a href="#i7bfbfbe…_52">Item 1A.</a>`
    /// and the body contains matching:
    ///   `<div id="i7bfbfbe…_52"></div>`
    fn section_anchor_map(page: &str) -> (Vec<String>, HashMap<String, String>) {
        let doc = Html::parse_document(page);
        let sel = Selector::parse("a[href]").unwrap();

        let mut order: Vec<String> = Vec::new();
        let mut map: HashMap<String, String> = HashMap::new();

        for el in doc.select(&sel) {
            // Normalize non-breaking spaces (&#160;) to regular spaces before matching.
            // Some filings (e.g. Meta) use &nbsp; between "Item" and the section number.
            let text: String = el.text().collect::<String>().replace('\u{00a0}', " ");
            let text = text.trim().to_string();

            // Match "Item 1.", "Item 1A.", "Item 7A.", …
            let Some(rest) = text.strip_prefix("Item ") else {
                continue;
            };
            let num = rest.trim_end_matches('.');
            // Must start with a digit to exclude "Part I" / "Part II" headings
            if !num
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
            {
                continue;
            }
            // Skip duplicate TOC entries (the same item appears 3× in Workiva docs)
            if map.contains_key(num) {
                continue;
            }

            let Some(href) = el.value().attr("href") else {
                continue;
            };
            let Some(anchor) = href.strip_prefix('#') else {
                continue;
            };

            order.push(num.to_string());
            map.insert(num.to_string(), anchor.to_string());
        }

        (order, map)
    }

    pub async fn retrieve_filing(&self, filing: &Filing) -> Result<String> {
        const SEC_BASE_URL: &str = "https://www.sec.gov/Archives/edgar/data/{}/{}/{}";
        let accession_number = filing.accession_number.replace("-", "");

        let url = format!(
            "https://www.sec.gov/Archives/edgar/data/{}/{}/{}",
            format!("{:0>10}", filing.cik),
            filing.accession_number.replace("-", ""),
            filing.primary_document
        );

        let res = self
            .client
            .post(url)
            .header("USER_AGENT", GRAHAM_USER_AGENT)
            .send()
            .await?
            .text()
            .await?;

        Ok(res)
    }

    pub async fn retrieve_company_filings(&self, ticker: &str) -> Result<Option<Vec<Filing>>> {
        let company = match self.retrieve_company(ticker).await? {
            Some(company) => company,
            None => return Ok(None),
        };

        let submissions = self.retrieve_submission(&company).await?;
        let filings = self
            .lookup_relevant_filings(company.cik, submissions)
            .await?;

        Ok(Some(filings))
    }
}

pub async fn migrate_sec_tables(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sec_companies (
            cik      INTEGER NOT NULL,
            name     TEXT NOT NULL,
            ticker   TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_sec_companies_ticker ON sec_companies (ticker)")
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
    let row: Option<(i64,)> = sqlx::query_as("SELECT fetched_at FROM sec_fetch_log WHERE id = 1")
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(ts,)| ts as u64))
}

async fn fetch_and_store(pool: &SqlitePool) -> Result<()> {
    let client = reqwest::Client::new();
    let body = client
        .get(SEC_TICKER_URL)
        .header(USER_AGENT, HeaderValue::from_str(GRAHAM_USER_AGENT)?)
        .header(ACCEPT, HeaderValue::from_static("application/json"))
        .send()
        .await?
        .text()
        .await?;

    dbg!("here");
    dbg!(
        "first 200 chars: {:?}",
        &body.chars().take(200).collect::<String>()
    );

    let parsed: Companies = serde_json::from_str(&body)?;
    dbg!("here 2");

    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM sec_companies")
        .execute(&mut *tx)
        .await?;

    for rec in parsed.values() {
        sqlx::query("INSERT INTO sec_companies (cik, name, ticker) VALUES (?1, ?2, ?3)")
            .bind(rec.cik)
            .bind(&rec.title)
            .bind(&rec.ticker)
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
