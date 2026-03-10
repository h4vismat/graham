pub fn is_brazil_ticker(ticker: &str) -> bool {
    ticker
        .chars()
        .last()
        .map_or(false, |c| c.is_ascii_digit())
}

pub fn yahoo_symbols_for_ticker(ticker: &str) -> Vec<String> {
    let mut symbols = Vec::with_capacity(2);
    let upper = ticker.trim().to_uppercase();

    if is_brazil_ticker(&upper) {
        if upper.contains('.') {
            symbols.push(upper.clone());
        } else {
            symbols.push(format!("{upper}.SA"));
        }
    }

    symbols.push(upper);
    symbols
}

pub fn yahoo_profile_url(ticker: &str) -> String {
    let symbol = yahoo_symbols_for_ticker(ticker)
        .into_iter()
        .next()
        .unwrap_or_else(|| ticker.trim().to_uppercase());
    format!("https://finance.yahoo.com/quote/{symbol}/profile/")
}
