use reqwest::Client;
use rss::Channel;

use crate::models::NewsItem;
use crate::sources::yahoo::yahoo_symbols_for_ticker;

const YAHOO_RSS_TEMPLATE: &str =
    "https://feeds.finance.yahoo.com/rss/2.0/headline?s={symbol}&region=US&lang=en-US";

pub async fn fetch_yahoo_news_for_ticker(client: &Client, ticker: &str) -> Option<Vec<NewsItem>> {
    let symbols = yahoo_symbols_for_ticker(ticker);
    fetch_yahoo_news(client, &symbols).await
}

pub async fn fetch_yahoo_news(client: &Client, symbols: &[String]) -> Option<Vec<NewsItem>> {
    for symbol in symbols {
        if let Some(items) = fetch_symbol_news(client, symbol).await {
            if !items.is_empty() {
                return Some(items);
            }
        }
    }
    None
}

async fn fetch_symbol_news(client: &Client, symbol: &str) -> Option<Vec<NewsItem>> {
    let url = YAHOO_RSS_TEMPLATE.replace("{symbol}", symbol);
    let resp = client
        .get(url)
        .header(
            "Accept",
            "application/rss+xml, application/xml;q=0.9, */*;q=0.8",
        )
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let text = resp.text().await.ok()?;
    let channel = Channel::read_from(text.as_bytes()).ok()?;

    let items = channel
        .items()
        .iter()
        .filter_map(|item| news_item_from_rss(item))
        .collect::<Vec<_>>();

    Some(items)
}

fn news_item_from_rss(item: &rss::Item) -> Option<NewsItem> {
    let title = item.title()?.trim();
    let link = item.link()?.trim();

    if is_filtered(item, title, link) {
        return None;
    }

    // Strip HTML tags from the RSS description if present.
    let description = item.description().and_then(|d| {
        let stripped = strip_html(d.trim());
        if stripped.is_empty() {
            None
        } else {
            Some(stripped)
        }
    });

    Some(NewsItem {
        title: title.to_string(),
        link: link.to_string(),
        publisher: item.source().and_then(|s| s.title()).map(|t| t.to_string()),
        published_at: item.pub_date().map(|d| d.to_string()),
        description,
    })
}

/// Remove HTML tags from a string, collapsing whitespace.
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_filtered(item: &rss::Item, title: &str, link: &str) -> bool {
    let description = item.description().unwrap_or("");
    let source = item.source().and_then(|s| s.title()).unwrap_or("");

    let mut haystack =
        String::with_capacity(title.len() + link.len() + description.len() + source.len() + 3);
    haystack.push_str(title);
    haystack.push(' ');
    haystack.push_str(link);
    haystack.push(' ');
    haystack.push_str(description);
    haystack.push(' ');
    haystack.push_str(source);

    let haystack = haystack.to_lowercase();

    let blocked_phrases = [
        "polymarket",
        "prediction market",
        "sponsored",
        "advertisement",
        "ad:",
        "promoted",
    ];

    blocked_phrases
        .iter()
        .any(|phrase| haystack.contains(phrase))
}
