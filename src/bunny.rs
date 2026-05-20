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

    async fn get_entity_statistics<I, T>(
        &self,
        url_path: &str,
        id: I,
        sub_url_path: Option<&str>,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<T>
    where
        I: ToString + Display + tracing::Value,
        T: DeserializeOwned,
    {
        let from_date = from_date.format(DATE_FORMAT).to_string();
        let to_date = to_date.format(DATE_FORMAT).to_string();
        debug!(
            url_path,
            id, sub_url_path, from_date, to_date, "Fetching statistics"
        );

        let sub_path = sub_url_path.map(|s| format!("/{s}")).unwrap_or_default();
        self.get_single::<T>(&format!(
            "{url_path}/{id}{sub_path}/statistics?dateFrom={from_date}&dateTo={to_date}"
        ))
        .await
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
        self.get_entity_statistics("dnszone", zone_id, None, from_date, to_date)
            .await
    }

    pub async fn list_storage_zones(&self) -> Result<Vec<StorageZone>> {
        debug!("Fetching list of storage zones");
        self.get_all_items::<StorageZone>("storagezone").await
    }

    pub async fn get_storage_zone_stats(
        &self,
        zone_id: u64,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<StorageZoneStats> {
        self.get_entity_statistics("storagezone", zone_id, None, from_date, to_date)
            .await
    }

    pub async fn list_video_libraries(&self) -> Result<Vec<VideoLibrary>> {
        debug!("Fetching list of video libraries");
        self.get_all_items::<VideoLibrary>("videolibrary").await
    }

    pub async fn get_video_library_transcribing_stats(
        &self,
        library_id: u64,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<VideoLibraryTranscribingStats> {
        self.get_entity_statistics(
            "videolibrary",
            library_id,
            Some("transcribing"),
            from_date,
            to_date,
        )
        .await
    }

    pub async fn get_video_library_drm_stats(
        &self,
        library_id: u64,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<VideoLibraryDrmStats> {
        self.get_entity_statistics("videolibrary", library_id, Some("drm"), from_date, to_date)
            .await
    }

    pub async fn list_pull_zones(&self) -> Result<Vec<PullZone>> {
        debug!("Fetching list of pull zones");
        self.get_all_items::<PullZone>("pullzone").await
    }

    pub async fn get_pull_zone_optimizer_stats(
        &self,
        zone_id: u64,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<PullZoneOptimizerStats> {
        self.get_entity_statistics("pullzone", zone_id, Some("optimizer"), from_date, to_date)
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

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct StorageZone {
    pub id: u64,
    pub name: String,
}

impl Display for StorageZone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.name, self.id)
    }
}

pub type StorageUsedChart = HashMap<String, u64>;
pub type FileCountChart = HashMap<String, u64>;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct StorageZoneStats {
    pub storage_used_chart: StorageUsedChart,
    pub file_count_chart: FileCountChart,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct VideoLibrary {
    pub id: u64,
    pub name: String,
}

impl Display for VideoLibrary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.name, self.id)
    }
}

pub type TranscriptionSecondsChart = HashMap<String, f64>;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct VideoLibraryTranscribingStats {
    pub transcription_seconds_chart: TranscriptionSecondsChart,
}

pub type LicensesIssuedChart = HashMap<String, f64>;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct VideoLibraryDrmStats {
    pub licenses_issued_chart: LicensesIssuedChart,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PullZone {
    pub id: u64,
    pub name: String,
}

impl Display for PullZone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.name, self.id)
    }
}

pub type RequestsOptimizedChart = HashMap<String, u64>;
pub type TrafficSavedChart = HashMap<String, u64>;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PullZoneOptimizerStats {
    pub requests_optimized_chart: RequestsOptimizedChart,
    pub traffic_saved_chart: TrafficSavedChart,
}
