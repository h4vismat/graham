use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize)]
pub struct MappingResponse {
    pub data: Option<Vec<FigiDataPoint>>,
    pub warning: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct FigiDataPoint {
    pub figi: String,
    pub ticker: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "exchCode")]
    pub exch_code: Option<String>,
    #[serde(rename = "marketSector")]
    pub market_sector: Option<String>,
    #[serde(rename = "securityType")]
    pub security_type: Option<String>,
    #[serde(rename = "securityType2")]
    pub security_type2: Option<String>,
    #[serde(rename = "shareClassFIGI")]
    pub share_class_figi: Option<String>,
    #[serde(rename = "compositeFIGI")]
    pub composite_figi: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MappingJob<'a> {
    id_type: &'a str,
    id_value: &'a str,
    exch_code: &'a str,
    market_sec_des: &'a str,
}

#[derive(Deserialize)]
struct SearchResponse {
    pub data: Vec<FigiDataPoint>,
    pub next: Option<String>,
}

pub struct OpenFigi {
    client: reqwest::Client,
}

impl OpenFigi {
    pub fn new() -> Self {
        let client = reqwest::Client::new();

        Self { client }
    }

    pub async fn map_stock_ticker(&self, tickers: &[String]) -> Result<Option<FigiDataPoint>> {
        let mappings: Vec<MappingJob> = tickers
            .iter()
            .map(|i| MappingJob {
                id_type: "TICKER",
                id_value: i.as_str(),
                exch_code: "US",
                market_sec_des: "Equity",
            })
            .collect();

        let res: Vec<MappingResponse> = self
            .client
            .post("https://api.openfigi.com/v3/mapping")
            .json(&mappings)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let first = res.into_iter().next();

        let figi = first
            .and_then(|item| item.data)
            .and_then(|mut data| data.drain(..).next());

        Ok(figi)
    }
}
