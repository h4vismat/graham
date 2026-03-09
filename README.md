# Graham

Graham is a terminal UI (TUI) for quick, fundamentals-first stock analysis. It fetches public indicator data for B3 and US tickers, renders valuation and quality metrics, and optionally adds a short AI summary.

**Features**
- B3 and US ticker support with automatic market detection
- Fundamentals dashboard across valuation, debt, efficiency, profitability, and growth
- Price chart with multiple periods (30d, 6m, 1y, 5y)
- Optional AI commentary via OpenRouter
- Clean separation of data fetching, domain models, and rendering

**Architecture**
The code keeps side effects isolated and pushes most logic into pure, testable functions.
- `scraper::scrape_stock` fetches and normalizes data into `models::StockIndicators`
- `models` defines the domain data structures and history shapes
- `ui` renders read-only views from `App` state
- `app` is a small state machine controlling navigation and view selection
- `ai` is an optional adapter that turns stock data into a short summary

This makes it straightforward to swap data providers or AI backends without changing the core UI or domain model.

**Requirements**
- Rust toolchain with Cargo
- Network access to fetch data

**Quick Start**
1. `cargo run`
2. Enter a ticker and press Enter

Example tickers:
- `VALE3` or `PETR4` for B3
- `AAPL` or `MSFT` for US

**Configuration**
- `OPENROUTER_API_KEY` enables AI summaries via OpenRouter
- `OPENROUTER_MODEL` allows you to choose which model to use.

**Controls**
- `Ctrl+C` quit
- `Esc` clear input or go back to search
- `q` back to search (from data view)
- `Left/Right` or `Tab/Shift+Tab` change tab
- `1-4` or `,` `.` change price period on Overview
- `o v d e p g` jump to tabs (Overview, Valuation, Debt, Efficiency, Profitability, Growth)

**Data Sources**
- Public endpoints and HTML from Status Invest for fundamentals and prices

**Notes**
- The data source is upstream and can change without notice
- AI summaries are optional and require an API key
- This tool is for informational purposes only and is not investment advice
