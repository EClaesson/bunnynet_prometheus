use std::collections::HashMap;
use std::fmt::{self, Display};
use std::time::Duration;

use anyhow::Result;
use chrono::NaiveDate;
use serde::{Deserialize, de::DeserializeOwned};
use tracing::debug;

const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
const API_BASE_URL: &str = "https://api.bunny.net";
const ACCESS_KEY_HEADER: &str = "AccessKey";
const MIN_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const MAX_RETRY_INTERVAL: Duration = Duration::from_mins(1);
const MAX_RETRY_DURATION: Duration = Duration::from_mins(5);
const ITEMS_PER_PAGE: u32 = 500;
const DATE_FORMAT: &str = "%Y-%m-%d";

pub struct ApiClient {
    client: reqwest_middleware::ClientWithMiddleware,
}

impl ApiClient {
    pub fn new(
        access_key: &str,
        api_request_timeout: Duration,
        poll_interval: Duration,
    ) -> Result<Self> {
        let mut headers = reqwest::header::HeaderMap::new();

        let accept_header_value = reqwest::header::HeaderValue::from_static("application/json");
        headers.insert(reqwest::header::ACCEPT, accept_header_value);

        let mut access_key_header_value = reqwest::header::HeaderValue::from_str(access_key)?;
        access_key_header_value.set_sensitive(true);
        headers.insert(ACCESS_KEY_HEADER, access_key_header_value);

        let raw_client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .default_headers(headers)
            .timeout(api_request_timeout)
            .build()?;

        let retry_duration_budget = (poll_interval * 4 / 5).min(MAX_RETRY_DURATION);
        let retry_policy = reqwest_retry::policies::ExponentialBackoff::builder()
            .jitter(reqwest_retry::Jitter::Bounded)
            .retry_bounds(MIN_RETRY_INTERVAL, MAX_RETRY_INTERVAL)
            .build_with_total_retry_duration(retry_duration_budget);

        let client = reqwest_middleware::ClientBuilder::new(raw_client)
            .with(reqwest_retry::RetryTransientMiddleware::new_with_policy(
                retry_policy,
            ))
            .build();

        Ok(Self { client })
    }

    async fn get_all_items<T>(&self, url_path: &str) -> Result<Vec<T>>
    where
        T: DeserializeOwned,
    {
        let mut page_num = 1;
        let mut items: Vec<T> = vec![];

        loop {
            let mut page = self
                .client
                .get(format!(
                    "{API_BASE_URL}/{url_path}?page={page_num}&perPage={ITEMS_PER_PAGE}"
                ))
                .send()
                .await?
                .error_for_status()?
                .json::<Page<T>>()
                .await?;

            let is_last_page = page.items.is_empty() || !page.has_more_items;
            items.append(&mut page.items);
            page_num += 1;

            if is_last_page {
                break;
            }
        }

        Ok(items)
    }

    async fn get_single<T>(&self, url_path: &str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        self.client
            .get(format!("{API_BASE_URL}/{url_path}"))
            .send()
            .await?
            .error_for_status()?
            .json::<T>()
            .await
            .map_err(Into::into)
    }

    pub async fn list_dns_zones(&self) -> Result<Vec<DnsZone>> {
        debug!("Fetching list of DNS zones");
        self.get_all_items::<DnsZone>("dnszone").await
    }

    pub async fn get_dns_zone_stats(
        &self,
        zone_id: u64,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<DnsZoneStats> {
        let from_date = from_date.format(DATE_FORMAT);
        let to_date = to_date.format(DATE_FORMAT);

        debug!(zone_id, "Fetching DNS zone stats");
        self.get_single::<DnsZoneStats>(&format!(
            "dnszone/{zone_id}/statistics?dateFrom={from_date}&dateTo={to_date}"
        ))
        .await
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Page<T> {
    has_more_items: bool,
    items: Vec<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DnsZone {
    pub id: u64,
    pub domain: String,
}

impl Display for DnsZone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.domain, self.id)
    }
}

type QueriesServedChart = HashMap<String, f64>;
pub type QueriesByTypeChart = HashMap<String, u64>;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(clippy::struct_field_names)]
pub struct DnsZoneStats {
    pub normal_queries_served_chart: QueriesServedChart,
    pub smart_queries_served_chart: QueriesServedChart,
    pub queries_by_type_chart: QueriesByTypeChart,
}
