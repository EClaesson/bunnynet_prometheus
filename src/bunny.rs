use std::collections::HashMap;
use std::fmt::{self, Display};
use std::time::Duration;

use anyhow::Result;
use chrono::NaiveDate;
use serde::{Deserialize, de::DeserializeOwned};
use tracing::debug;

const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
const API_BASE_URL: &str = "https://api.bunny.net";
const STREAM_API_BASE_URL: &str = "https://video.bunnycdn.com";
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

    fn build_get(
        &self,
        url: String,
        access_key: Option<&str>,
    ) -> Result<reqwest_middleware::RequestBuilder> {
        let mut req = self.client.get(url);
        if let Some(key) = access_key {
            let mut header = reqwest::header::HeaderValue::from_str(key)?;
            header.set_sensitive(true);
            req = req.header(ACCESS_KEY_HEADER, header);
        }
        Ok(req)
    }

    async fn get_all_items<T>(
        &self,
        base_url: &str,
        url_path: &str,
        access_key: Option<&str>,
    ) -> Result<Vec<T>>
    where
        T: DeserializeOwned,
    {
        let mut page_num = 1;
        let mut items: Vec<T> = vec![];

        loop {
            let mut page = self
                .build_get(
                    format!(
                        "{base_url}/{url_path}?page={page_num}&perPage={ITEMS_PER_PAGE}"
                    ),
                    access_key,
                )?
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

    async fn get_all_data_items<T>(
        &self,
        base_url: &str,
        url_path: &str,
        access_key: Option<&str>,
    ) -> Result<Vec<T>>
    where
        T: DeserializeOwned,
    {
        let mut page_num = 1;
        let mut items: Vec<T> = vec![];

        loop {
            let mut page = self
                .build_get(
                    format!(
                        "{base_url}/{url_path}?page={page_num}&perPage={ITEMS_PER_PAGE}"
                    ),
                    access_key,
                )?
                .send()
                .await?
                .error_for_status()?
                .json::<DataPage<T>>()
                .await?;

            items.append(&mut page.data);
            match page.page.next_page {
                Some(next) => page_num = next,
                None => break,
            }
        }

        Ok(items)
    }

    async fn get_single<T>(
        &self,
        base_url: &str,
        url_path: &str,
        access_key: Option<&str>,
    ) -> Result<T>
    where
        T: DeserializeOwned,
    {
        self.build_get(format!("{base_url}/{url_path}"), access_key)?
            .send()
            .await?
            .error_for_status()?
            .json::<T>()
            .await
            .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    async fn get_entity_statistics<I, T>(
        &self,
        base_url: &str,
        url_path: &str,
        id: I,
        stats_path: &str,
        from_date: NaiveDate,
        to_date: NaiveDate,
        access_key: Option<&str>,
    ) -> Result<T>
    where
        I: ToString + Display + tracing::Value,
        T: DeserializeOwned,
    {
        let from_date = from_date.format(DATE_FORMAT).to_string();
        let to_date = to_date.format(DATE_FORMAT).to_string();
        debug!(
            base_url,
            url_path,
            id,
            stats_path,
            from_date,
            to_date,
            "Fetching statistics"
        );

        self.get_single::<T>(
            base_url,
            &format!("{url_path}/{id}/{stats_path}?dateFrom={from_date}&dateTo={to_date}"),
            access_key,
        )
        .await
    }

    pub async fn list_dns_zones(&self) -> Result<Vec<DnsZone>> {
        debug!("Fetching list of DNS zones");
        self.get_all_items::<DnsZone>(API_BASE_URL, "dnszone", None)
            .await
    }

    pub async fn get_dns_zone_stats(
        &self,
        zone_id: u64,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<DnsZoneStats> {
        self.get_entity_statistics(
            API_BASE_URL,
            "dnszone",
            zone_id,
            "statistics",
            from_date,
            to_date,
            None,
        )
        .await
    }

    pub async fn list_storage_zones(&self) -> Result<Vec<StorageZone>> {
        debug!("Fetching list of storage zones");
        self.get_all_items::<StorageZone>(API_BASE_URL, "storagezone", None)
            .await
    }

    pub async fn get_storage_zone_stats(
        &self,
        zone_id: u64,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<StorageZoneStats> {
        self.get_entity_statistics(
            API_BASE_URL,
            "storagezone",
            zone_id,
            "statistics",
            from_date,
            to_date,
            None,
        )
        .await
    }

    pub async fn list_video_libraries(&self) -> Result<Vec<VideoLibrary>> {
        debug!("Fetching list of video libraries");
        self.get_all_items::<VideoLibrary>(API_BASE_URL, "videolibrary", None)
            .await
    }

    pub async fn get_video_library_transcribing_stats(
        &self,
        library_id: u64,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<VideoLibraryTranscribingStats> {
        self.get_entity_statistics(
            API_BASE_URL,
            "videolibrary",
            library_id,
            "transcribing/statistics",
            from_date,
            to_date,
            None,
        )
        .await
    }

    pub async fn get_video_library_drm_stats(
        &self,
        library_id: u64,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<VideoLibraryDrmStats> {
        self.get_entity_statistics(
            API_BASE_URL,
            "videolibrary",
            library_id,
            "drm/statistics",
            from_date,
            to_date,
            None,
        )
        .await
    }

    pub async fn get_video_library_stats(
        &self,
        library_id: u64,
        access_key: Option<&str>,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<VideoLibraryStats> {
        self.get_entity_statistics(
            STREAM_API_BASE_URL,
            "library",
            library_id,
            "statistics",
            from_date,
            to_date,
            access_key,
        )
        .await
    }

    pub async fn list_pull_zones(&self) -> Result<Vec<PullZone>> {
        debug!("Fetching list of pull zones");
        self.get_all_items::<PullZone>(API_BASE_URL, "pullzone", None)
            .await
    }

    pub async fn get_pull_zone_stats(
        &self,
        zone_id: u64,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<PullZoneStats> {
        let from_date = from_date.format(DATE_FORMAT).to_string();
        let to_date = to_date.format(DATE_FORMAT).to_string();
        debug!(zone_id, from_date, to_date, "Fetching pull zone statistics");

        self.get_single::<PullZoneStats>(
            API_BASE_URL,
            &format!(
                "statistics?dateFrom={from_date}&dateTo={to_date}&pullZone={zone_id}&loadErrors=true&loadOriginResponseTimes=true&loadOriginTraffic=true&loadRequestsServed=true&loadBandwidthUsed=true&loadOriginShieldBandwidth=true&loadGeographicTrafficDistribution=true"
            ),
            None,
        )
        .await
    }

    pub async fn get_pull_zone_optimizer_stats(
        &self,
        zone_id: u64,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<PullZoneOptimizerStats> {
        self.get_entity_statistics(
            API_BASE_URL,
            "pullzone",
            zone_id,
            "optimizer/statistics",
            from_date,
            to_date,
            None,
        )
        .await
    }

    pub async fn get_pull_zone_origin_shield_queue_stats(
        &self,
        zone_id: u64,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<PullZoneOriginShieldQueueStats> {
        self.get_entity_statistics(
            API_BASE_URL,
            "pullzone",
            zone_id,
            "originshield/queuestatistics",
            from_date,
            to_date,
            None,
        )
        .await
    }

    pub async fn get_pull_zone_safehop_stats(
        &self,
        zone_id: u64,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<PullZoneSafeHopStats> {
        self.get_entity_statistics(
            API_BASE_URL,
            "pullzone",
            zone_id,
            "safehop/statistics",
            from_date,
            to_date,
            None,
        )
        .await
    }

    pub async fn list_edge_scripts(&self) -> Result<Vec<EdgeScript>> {
        debug!("Fetching list of edge scripts");
        self.get_all_items::<EdgeScript>(API_BASE_URL, "compute/script", None)
            .await
    }

    pub async fn get_edge_script_stats(
        &self,
        script_id: u64,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<EdgeScriptStats> {
        self.get_entity_statistics(
            API_BASE_URL,
            "compute/script",
            script_id,
            "statistics",
            from_date,
            to_date,
            None,
        )
        .await
    }

    pub async fn list_shield_zones(&self) -> Result<Vec<ShieldZone>> {
        debug!("Fetching list of shield zones");
        self.get_all_data_items::<ShieldZone>(API_BASE_URL, "shield/shield-zones", None)
            .await
    }

    pub async fn get_shield_metrics(
        &self,
        shield_zone_id: u64,
        from_date: NaiveDate,
    ) -> Result<ShieldMetrics> {
        let from_date = from_date.format(DATE_FORMAT).to_string();
        debug!(shield_zone_id, from_date, "Fetching shield zone metrics");

        let wrapper = self
            .get_single::<DataEnvelope<ShieldMetrics>>(
                API_BASE_URL,
                &format!(
                    "shield/metrics/overview/{shield_zone_id}/detailed?startdate={from_date}Z&resolution=4"
                ),
                None,
            )
            .await?;
        Ok(wrapper.data)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Page<T> {
    has_more_items: bool,
    items: Vec<T>,
}

#[derive(Deserialize)]
struct DataPage<T> {
    data: Vec<T>,
    page: DataPageInfo,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataPageInfo {
    next_page: Option<u32>,
}

#[derive(Deserialize)]
struct DataEnvelope<T> {
    data: T,
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
    pub read_only_api_key: String,
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

pub type ViewsChart = HashMap<String, u64>;
pub type WatchTimeChart = HashMap<String, u64>;
pub type CountryViewCounts = HashMap<String, u64>;
pub type CountryWatchTime = HashMap<String, u64>;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(clippy::struct_field_names)]
pub struct VideoLibraryStats {
    pub views_chart: ViewsChart,
    pub watch_time_chart: WatchTimeChart,
    pub country_view_counts: CountryViewCounts,
    pub country_watch_time: CountryWatchTime,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct EdgeScript {
    pub id: u64,
    pub name: String,
}

impl Display for EdgeScript {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.name, self.id)
    }
}

pub type EdgeScriptRequestsServedChart = HashMap<String, f64>;
pub type EdgeScriptAverageCpuTimeChart = HashMap<String, f64>;
pub type EdgeScriptTotalCpuTimeChart = HashMap<String, f64>;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(clippy::struct_field_names)]
pub struct EdgeScriptStats {
    pub requests_served_chart: EdgeScriptRequestsServedChart,
    pub average_cpu_time_chart: EdgeScriptAverageCpuTimeChart,
    pub total_cpu_time_chart: EdgeScriptTotalCpuTimeChart,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShieldZone {
    pub shield_zone_id: u64,
    pub pull_zone_id: Option<u64>,
}

impl Display for ShieldZone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.pull_zone_id {
            Some(pz) => write!(f, "shield zone {} (pull zone {pz})", self.shield_zone_id),
            None => write!(f, "shield zone {}", self.shield_zone_id),
        }
    }
}

pub type ShieldMetricChart = HashMap<String, u64>;

#[derive(Deserialize, Default)]
pub struct ShieldCategoryMetrics {
    pub metrics: HashMap<String, ShieldMetricChart>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShieldMetrics {
    pub waf: ShieldCategoryMetrics,
    pub ddos: ShieldCategoryMetrics,
    pub rate_limit: ShieldCategoryMetrics,
    pub access_lists: ShieldCategoryMetrics,
    pub bot_detection: ShieldCategoryMetrics,
    pub upload_scanning: ShieldCategoryMetrics,
    pub api_guardian: ShieldCategoryMetrics,
    pub total_clean_requests_limit: u64,
    pub total_billable_requests_this_month: u64,
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
pub type AverageCompressionChart = HashMap<String, f64>;
pub type AverageProcessingTimeChart = HashMap<String, f64>;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(clippy::struct_field_names)]
pub struct PullZoneOptimizerStats {
    pub requests_optimized_chart: RequestsOptimizedChart,
    pub traffic_saved_chart: TrafficSavedChart,
    pub average_compression_chart: AverageCompressionChart,
    pub average_processing_time_chart: AverageProcessingTimeChart,
}

pub type ConcurrentRequestsChart = HashMap<String, u64>;
pub type QueuedRequestsChart = HashMap<String, u64>;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(clippy::struct_field_names)]
pub struct PullZoneOriginShieldQueueStats {
    pub concurrent_requests_chart: ConcurrentRequestsChart,
    pub queued_requests_chart: QueuedRequestsChart,
}

pub type RequestsRetriedChart = HashMap<String, u64>;
pub type RequestsSavedChart = HashMap<String, u64>;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(clippy::struct_field_names)]
pub struct PullZoneSafeHopStats {
    pub requests_retried_chart: RequestsRetriedChart,
    pub requests_saved_chart: RequestsSavedChart,
}

pub type OriginResponseTimeChart = HashMap<String, f64>;
pub type CacheHitRateChart = HashMap<String, f64>;
pub type BandwidthUsedChart = HashMap<String, u64>;
pub type BandwidthCachedChart = HashMap<String, u64>;
pub type RequestsServedChart = HashMap<String, u64>;
pub type PullRequestsPulledChart = HashMap<String, u64>;
pub type OriginShieldBandwidthUsedChart = HashMap<String, u64>;
pub type OriginShieldInternalBandwidthUsedChart = HashMap<String, u64>;
pub type OriginTrafficChart = HashMap<String, u64>;
pub type GeoTrafficDistribution = HashMap<String, u64>;
pub type ErrorChart = HashMap<String, u64>;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(clippy::struct_field_names)]
pub struct PullZoneStats {
    pub origin_response_time_chart: OriginResponseTimeChart,
    pub cache_hit_rate_chart: CacheHitRateChart,
    pub bandwidth_used_chart: BandwidthUsedChart,
    pub bandwidth_cached_chart: BandwidthCachedChart,
    pub requests_served_chart: RequestsServedChart,
    pub pull_requests_pulled_chart: PullRequestsPulledChart,
    pub origin_shield_bandwidth_used_chart: OriginShieldBandwidthUsedChart,
    pub origin_shield_internal_bandwidth_used_chart: OriginShieldInternalBandwidthUsedChart,
    pub origin_traffic_chart: OriginTrafficChart,
    pub geo_traffic_distribution: GeoTrafficDistribution,
    #[serde(rename = "Error3xxChart")]
    pub errors_3xx_chart: ErrorChart,
    #[serde(rename = "Error4xxChart")]
    pub errors_4xx_chart: ErrorChart,
    #[serde(rename = "Error5xxChart")]
    pub errors_5xx_chart: ErrorChart,
}
