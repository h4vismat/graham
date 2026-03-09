use sqlx::FromRow;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

#[derive(Debug, Clone, FromRow)]
pub struct Trade {
    pub id: i64,
    pub ticker: String,
    pub side: String,
    pub shares: f64,
    pub price: f64,
    pub date: String,
}

pub async fn init_db() -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename("positions.db")
        .create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS trades (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ticker TEXT NOT NULL,
            side TEXT NOT NULL,
            shares REAL NOT NULL,
            price REAL NOT NULL,
            date TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}

pub async fn list_trades(pool: &SqlitePool) -> Result<Vec<Trade>, sqlx::Error> {
    sqlx::query_as::<_, Trade>(
        "SELECT id, ticker, side, shares, price, date FROM trades ORDER BY date ASC, id ASC",
    )
    .fetch_all(pool)
    .await
}

pub async fn insert_trade(
    pool: &SqlitePool,
    ticker: &str,
    side: &str,
    shares: f64,
    price: f64,
    date: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO trades (ticker, side, shares, price, date) VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(ticker)
    .bind(side)
    .bind(shares)
    .bind(price)
    .bind(date)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_trade(
    pool: &SqlitePool,
    id: i64,
    ticker: &str,
    side: &str,
    shares: f64,
    price: f64,
    date: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE trades SET ticker = ?1, side = ?2, shares = ?3, price = ?4, date = ?5 WHERE id = ?6",
    )
    .bind(ticker)
    .bind(side)
    .bind(shares)
    .bind(price)
    .bind(date)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_trade(pool: &SqlitePool, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM trades WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
