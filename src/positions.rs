use sqlx::FromRow;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

#[derive(Debug, Clone, FromRow)]
pub struct Position {
    pub id: i64,
    pub ticker: String,
    pub shares: f64,
    pub avg_cost: f64,
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
        "CREATE TABLE IF NOT EXISTS positions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ticker TEXT NOT NULL,
            shares REAL NOT NULL,
            avg_cost REAL NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}

pub async fn list_positions(pool: &SqlitePool) -> Result<Vec<Position>, sqlx::Error> {
    sqlx::query_as::<_, Position>(
        "SELECT id, ticker, shares, avg_cost FROM positions ORDER BY ticker ASC, id ASC",
    )
    .fetch_all(pool)
    .await
}

pub async fn insert_position(
    pool: &SqlitePool,
    ticker: &str,
    shares: f64,
    avg_cost: f64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO positions (ticker, shares, avg_cost) VALUES (?1, ?2, ?3)",
    )
    .bind(ticker)
    .bind(shares)
    .bind(avg_cost)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_position(
    pool: &SqlitePool,
    id: i64,
    ticker: &str,
    shares: f64,
    avg_cost: f64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE positions SET ticker = ?1, shares = ?2, avg_cost = ?3 WHERE id = ?4",
    )
    .bind(ticker)
    .bind(shares)
    .bind(avg_cost)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_position(pool: &SqlitePool, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM positions WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
