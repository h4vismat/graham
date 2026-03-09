use reqwest::Client;
use scraper::{Html, Selector};

use crate::models::{NewsItem, QuarterlyReport};

const BASE_URL: &str = "https://www.fundamentus.com.br";
const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
                  AppleWebKit/537.36 (KHTML, like Gecko) \
                  Chrome/121.0.0.0 Safari/537.36";

fn make_absolute(href: &str) -> String {
    if href.starts_with("http") {
        href.to_string()
    } else {
        format!("{}/{}", BASE_URL, href.trim_start_matches('/'))
    }
}

/// Scrapes fatos relevantes (relevant facts / material disclosures) for a
/// B3 ticker from Fundamentus.  Returns `None` if no items were found.
pub async fn fetch_fatos_relevantes(client: &Client, ticker: &str) -> Option<Vec<NewsItem>> {
    let url = format!(
        "{}/fatos_relevantes.php?papel={}",
        BASE_URL,
        ticker.to_uppercase()
    );
    let text = client
        .get(&url)
        .header("User-Agent", UA)
        .header("Accept", "text/html,application/xhtml+xml,*/*;q=0.8")
        .header("Accept-Language", "pt-BR,pt;q=0.9")
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;

    // Parse synchronously inside a block so Html (!Send) is dropped before
    // this future is returned.
    let items: Vec<NewsItem> = {
        let html = Html::parse_document(&text);
        let row_sel = Selector::parse("table tr").ok()?;
        let td_sel = Selector::parse("td").ok()?;
        let a_sel = Selector::parse("a").ok()?;

        html.select(&row_sel)
            .filter_map(|row| {
                let tds: Vec<_> = row.select(&td_sel).collect();
                if tds.len() < 2 {
                    return None;
                }
                let date = tds[0].text().collect::<String>().trim().to_string();
                if date.is_empty() {
                    return None;
                }

                // Extract text cells (columns after the date), ignoring pure
                // link cells whose only text is "Exibir" / "Ver" / etc.
                let mut text_cols: Vec<String> = tds[1..]
                    .iter()
                    .filter_map(|td| {
                        let t = td.text().collect::<String>().trim().to_string();
                        if t.is_empty() {
                            return None;
                        }
                        // Skip navigation-only cells.
                        let lower = t.to_lowercase();
                        if lower == "exibir" || lower == "ver" || lower == "show" {
                            return None;
                        }
                        Some(t)
                    })
                    .collect();

                // The first column is often a short category code (e.g. "CO",
                // "FR"). The last column is the actual subject text. Use the
                // last column as the title; if there are more than two columns
                // everything in between forms the description.
                let title = text_cols.last()?.clone();
                let description = if text_cols.len() > 2 {
                    Some(text_cols[1..text_cols.len() - 1].join(" — "))
                } else {
                    None
                };

                let link = tds
                    .iter()
                    .find_map(|td| {
                        td.select(&a_sel)
                            .next()
                            .and_then(|a| a.value().attr("href"))
                    })
                    .map(make_absolute)
                    .unwrap_or_default();

                Some(NewsItem {
                    title,
                    link,
                    publisher: Some("CVM / Fundamentus".to_string()),
                    published_at: Some(date),
                    description,
                })
            })
            .collect()
    };

    if items.is_empty() { None } else { Some(items) }
}

/// Scrapes the list of quarterly / annual ITR–DFP filings available for a
/// B3 ticker and resolves each "Exibir" link to the final rad.cvm.gov.br URL.
pub async fn fetch_quarterly_reports(
    client: &Client,
    ticker: &str,
) -> Option<Vec<QuarterlyReport>> {
    let url = format!(
        "{}/resultados_trimestrais.php?papel={}&tipo=1",
        BASE_URL,
        ticker.to_uppercase()
    );
    let text = client
        .get(&url)
        .header("User-Agent", UA)
        .header("Accept", "text/html,application/xhtml+xml,*/*;q=0.8")
        .header("Accept-Language", "pt-BR,pt;q=0.9")
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;

    // Parse all rows synchronously inside a block so that Html (which is
    // !Send) is dropped completely before any subsequent .await calls.
    let rows: Vec<(String, String, Option<String>)> = {
        let html = Html::parse_document(&text);
        let row_sel = match Selector::parse("table tr") {
            Ok(s) => s,
            Err(_) => return None,
        };
        let td_sel = match Selector::parse("td") {
            Ok(s) => s,
            Err(_) => return None,
        };
        let a_sel = match Selector::parse("a") {
            Ok(s) => s,
            Err(_) => return None,
        };

        html.select(&row_sel)
            .filter_map(|row| {
                let tds: Vec<_> = row.select(&td_sel).collect();
                if tds.len() < 2 {
                    return None;
                }
                let period = tds[0].text().collect::<String>().trim().to_string();
                if period.is_empty() {
                    return None;
                }
                let published = tds[1].text().collect::<String>().trim().to_string();
                let href = tds
                    .iter()
                    .find_map(|td| {
                        td.select(&a_sel)
                            .next()
                            .and_then(|a| a.value().attr("href"))
                    })
                    .map(make_absolute);
                Some((period, published, href))
            })
            .collect()
        // `html` is dropped here — before any .await
    };

    if rows.is_empty() {
        return None;
    }

    // Resolve redirects only for the most recent reports.
    const MAX_REPORTS: usize = 8;
    let rows: Vec<_> = rows.into_iter().take(MAX_REPORTS).collect();

    // Resolve all redirects in parallel.  Each "Exibir" href is a Fundamentus
    // path that redirects to rad.cvm.gov.br; we follow it once so the stored
    // link opens directly.  A 429 response means we're rate-limited — in that
    // case we fall back to storing the original Fundamentus URL.
    let futures: Vec<_> = rows
        .into_iter()
        .map(|(period, published, raw_href)| {
            let client = client.clone();
            async move {
                let link = match raw_href {
                    Some(ref href) if !href.is_empty() => {
                        match client.get(href).header("User-Agent", UA).send().await {
                            Ok(resp) if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS => {
                                href.clone()
                            }
                            Ok(resp) => resp.url().to_string(),
                            Err(_) => href.clone(),
                        }
                    }
                    Some(href) => href,
                    None => String::new(),
                };
                QuarterlyReport { period, published, link }
            }
        })
        .collect();

    let reports = futures::future::join_all(futures).await;

    if reports.is_empty() { None } else { Some(reports) }
}

// ─── In-app report viewer ─────────────────────────────────────────────────────

/// Fetches a CVM report page and extracts the `ctl00_cphPopUp_tbDados` table.
///
/// The CVM RAD portal (`frmGerenciaPaginaFRE.aspx`) embeds financial data
/// inside a dynamically-built iframe (`frmDemonstracaoFinanceiraITR.aspx`).
/// The iframe URL including the `Hash` token lives in the parent page's
/// inline JavaScript as `window.frames[0].location='...'`.  The sub-page
/// also requires the ASP.NET session cookie established by the parent request
/// and the parent page URL as a `Referer` header.
///
/// Two HTTP requests are therefore needed:
///   1. GET the parent page → capture session cookies + extract iframe URL.
///   2. GET the iframe URL with `Referer` → parse `ctl00_cphPopUp_tbDados`.
///
/// Returns `None` only on unrecoverable network errors.
pub async fn fetch_cvm_report(_client: &Client, url: &str) -> Option<Vec<Vec<String>>> {
    if url.is_empty() {
        return None;
    }

    // A dedicated client with a cookie jar so the ASP.NET session cookie
    // established by the parent-page request is automatically sent with the
    // sub-page request.
    let cvm_client = reqwest::Client::builder()
        .cookie_store(true)
        .user_agent(UA)
        .build()
        .ok()?;

    // ── Request 1: parent page ────────────────────────────────────────────────
    let parent_resp = cvm_client
        .get(url)
        .header("Accept", "text/html,application/xhtml+xml,*/*;q=0.8")
        .header("Accept-Language", "pt-BR,pt;q=0.9")
        .send()
        .await
        .ok()?;

    if parent_resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Some(vec![vec![
            "Rate-limited by CVM (HTTP 429). Please wait and try again.".to_string(),
        ]]);
    }
    if !parent_resp.status().is_success() {
        return Some(vec![vec![format!("HTTP error: {}", parent_resp.status())]]);
    }

    // The final URL (after any redirects) is needed as the Referer.
    let parent_url = parent_resp.url().to_string();
    let parent_text = parent_resp.text().await.ok()?;

    // Extract the iframe URL from JS: window.frames[0].location='...'
    let iframe_rel = extract_frames_location(&parent_text)?;
    let sub_url = make_url_absolute(&iframe_rel, &parent_url);

    // ── Request 2: financial data sub-page ────────────────────────────────────
    let sub_resp = cvm_client
        .get(&sub_url)
        .header("Referer", &parent_url)
        .header("Accept", "text/html,application/xhtml+xml,*/*;q=0.8")
        .header("Accept-Language", "pt-BR,pt;q=0.9")
        .send()
        .await
        .ok()?;

    if !sub_resp.status().is_success() {
        return Some(vec![vec![format!(
            "HTTP error fetching sub-page: {}",
            sub_resp.status()
        )]]);
    }

    let sub_text = sub_resp.text().await.ok()?;

    let rows = {
        let html = Html::parse_document(&sub_text);
        extract_dados_table(&html)
    };

    if rows.is_empty() {
        Some(vec![vec!["No financial data found in this report.".to_string()]])
    } else {
        Some(rows)
    }
}

/// Extract rows from the `ctl00_cphPopUp_tbDados` table as structured cells.
///
/// Each returned `Vec<String>` represents one row:
///   - single element  → section-header label
///   - 2+ elements     → [account_code, description, value1, value2, …]
///                       (account_code is empty for the column-header row)
fn extract_dados_table(html: &Html) -> Vec<Vec<String>> {
    let Ok(table_sel) = Selector::parse("#ctl00_cphPopUp_tbDados") else {
        return vec![];
    };
    let Ok(tr_sel) = Selector::parse("tr") else { return vec![] };
    let Ok(cell_sel) = Selector::parse("td, th") else { return vec![] };

    let Some(table) = html.select(&table_sel).next() else {
        return vec![];
    };

    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut last_section = String::new();

    for row in table.select(&tr_sel) {
        let cells: Vec<String> = row
            .select(&cell_sel)
            .map(|c| ws_collapse(c.text().collect::<String>().as_str()))
            .collect();

        if cells.iter().all(|c| c.is_empty()) {
            continue;
        }

        let non_empty_count = cells.iter().filter(|c| !c.is_empty()).count();

        // Single non-empty cell → section header (deduplicated).
        if non_empty_count == 1 {
            let header = cells.into_iter().find(|c| !c.is_empty()).unwrap();
            if header != last_section {
                last_section = header.clone();
                rows.push(vec![header]);
            }
            continue;
        }

        // Multi-cell row: cells are already [code, description, val1, val2, …]
        rows.push(cells);
    }

    rows
}

/// Find `window.frames[0].location='<url>';` in a page's inline JavaScript
/// and return the URL (may be relative).
fn extract_frames_location(html: &str) -> Option<String> {
    for marker in [
        "window.frames[0].location='",
        "window.frames[0].location=\"",
    ] {
        if let Some(pos) = html.find(marker) {
            let start = pos + marker.len();
            let quote = marker.chars().last().unwrap(); // ' or "
            if let Some(end) = html[start..].find(quote) {
                return Some(html[start..start + end].to_string());
            }
        }
    }
    None
}

/// Resolve a potentially-relative `href` against the directory of `base_url`.
///
/// Examples:
///   `frmDemo.aspx?x=1`  + `https://host/ENET/parent.aspx?y=2`
///     → `https://host/ENET/frmDemo.aspx?x=1`
fn make_url_absolute(href: &str, base_url: &str) -> String {
    if href.starts_with("http") {
        return href.to_string();
    }
    if href.starts_with('/') {
        // Absolute path — prepend the origin (scheme + host).
        if let Some(i) = base_url.find("://") {
            if let Some(j) = base_url[i + 3..].find('/') {
                return format!("{}{}", &base_url[..i + 3 + j], href);
            }
        }
        return href.to_string();
    }
    // Relative path — resolve against the directory part of base_url
    // (strip query string first, then everything after the last '/').
    let path_end = base_url.find('?').unwrap_or(base_url.len());
    let dir_end = base_url[..path_end].rfind('/').unwrap_or(path_end);
    format!("{}/{}", &base_url[..dir_end], href)
}

fn ws_collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Unit tests ────────────────────────────────────────────────────────────

    #[test]
    fn test_extract_frames_location_single_quotes() {
        let html = r#"<script>window.frames[0].location='frmDemo.aspx?Hash=abc123';</script>"#;
        assert_eq!(
            extract_frames_location(html),
            Some("frmDemo.aspx?Hash=abc123".to_string())
        );
    }

    #[test]
    fn test_extract_frames_location_double_quotes() {
        let html = r#"<script>window.frames[0].location="frmDemo.aspx?Hash=xyz";</script>"#;
        assert_eq!(
            extract_frames_location(html),
            Some("frmDemo.aspx?Hash=xyz".to_string())
        );
    }

    #[test]
    fn test_extract_frames_location_missing() {
        let html = "<script>var x = 1;</script>";
        assert_eq!(extract_frames_location(html), None);
    }

    #[test]
    fn test_extract_frames_location_real_pattern() {
        // Pattern as it appears in frmGerenciaPaginaFRE.aspx
        let html = concat!(
            "window.frames[0].location='frmDemonstracaoFinanceiraITR.aspx",
            "?Informacao=2&Demonstracao=4&Periodo=0&Hash=tUdnS47Euc6MAyAP';"
        );
        let result = extract_frames_location(html).unwrap();
        assert!(result.starts_with("frmDemonstracaoFinanceiraITR.aspx"));
        assert!(result.contains("Hash=tUdnS47Euc6MAyAP"));
    }

    #[test]
    fn test_make_url_absolute_relative() {
        let href = "frmDemonstracaoFinanceiraITR.aspx?Hash=abc";
        let base = "https://www.rad.cvm.gov.br/ENET/frmGerenciaPaginaFRE.aspx?NumeroSeq=123";
        assert_eq!(
            make_url_absolute(href, base),
            "https://www.rad.cvm.gov.br/ENET/frmDemonstracaoFinanceiraITR.aspx?Hash=abc"
        );
    }

    #[test]
    fn test_make_url_absolute_already_absolute() {
        let href = "https://example.com/page.aspx?x=1";
        let base = "https://other.com/path/parent.aspx";
        assert_eq!(make_url_absolute(href, base), href);
    }

    #[test]
    fn test_make_url_absolute_root_relative() {
        let href = "/ENET/frmDemo.aspx?x=1";
        let base = "https://www.rad.cvm.gov.br/ENET/parent.aspx?y=2";
        assert_eq!(
            make_url_absolute(href, base),
            "https://www.rad.cvm.gov.br/ENET/frmDemo.aspx?x=1"
        );
    }

    #[test]
    fn test_extract_dados_table_empty_html() {
        let html = Html::parse_document("<html><body></body></html>");
        assert!(extract_dados_table(&html).is_empty());
    }

    #[test]
    fn test_extract_dados_table_with_data() {
        let html_str = r#"
            <table id="ctl00_cphPopUp_tbDados">
                <tr><td>3.01</td><td>Receita de Venda</td><td>10.000</td><td>9.500</td></tr>
                <tr><td>3.02</td><td>Custo dos Bens</td><td>-4.000</td><td>-3.800</td></tr>
            </table>
        "#;
        let html = Html::parse_document(html_str);
        let rows = extract_dados_table(&html);
        assert!(!rows.is_empty());
        assert_eq!(rows[0][0], "3.01");
        assert_eq!(rows[0][1], "Receita de Venda");
        assert_eq!(rows[0][2], "10.000");
        assert_eq!(rows[0][3], "9.500");
    }

    #[test]
    fn test_extract_dados_table_section_headers() {
        let html_str = r#"
            <table id="ctl00_cphPopUp_tbDados">
                <tr><td>Demonstração do Resultado</td></tr>
                <tr><td>3.01</td><td>Receita</td><td>100</td></tr>
            </table>
        "#;
        let html = Html::parse_document(html_str);
        let rows = extract_dados_table(&html);
        // Section header row has exactly one element.
        assert!(rows.iter().any(|r| r.len() == 1 && r[0] == "Demonstração do Resultado"));
        // Data row contains the account code.
        assert!(rows.iter().any(|r| r.len() > 1 && r[0] == "3.01"));
    }

    // ── Integration test (requires network; skipped by default) ───────────────

    /// Verifies the full two-request flow against the live CVM RAD portal.
    /// Run with:  cargo test -- --ignored test_fetch_cvm_report_live
    #[tokio::test]
    #[ignore]
    async fn test_fetch_cvm_report_live() {
        let client = reqwest::Client::new();
        // VALE3 DFP 2024 — a stable, publicly accessible document.
        let url = "https://www.rad.cvm.gov.br/ENET/frmGerenciaPaginaFRE.aspx\
                   ?NumeroSequencialDocumento=154745&CodigoTipoInstituicao=1";

        let result = fetch_cvm_report(&client, url).await;
        assert!(result.is_some(), "fetch_cvm_report returned None");

        let rows = result.unwrap();
        assert!(!rows.is_empty(), "Expected non-empty rows");
        assert!(
            rows.iter()
                .filter(|r| r.len() > 1)
                .any(|r| r[0].contains('.')),
            "Expected account codes (e.g. 3.01) in output"
        );

        // Dump the first 30 rows for visual inspection.
        println!("── CVM report (first 30 rows) ──");
        for row in rows.iter().take(30) {
            println!("{}", row.join("  |  "));
        }
    }
}
