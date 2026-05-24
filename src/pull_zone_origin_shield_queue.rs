use std::sync::Arc;

use anyhow::Context;
use chrono::NaiveDate;
use metrics::gauge;
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, PullZone};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, find_chart_value_for_date,
};

pub type PullZoneOriginShieldQueueStatsState = EntityStatsState<PullZoneOriginShieldQueueKind>;

const CONCURRENT_REQUESTS: &str = "concurrent_requests";
const QUEUED_REQUESTS: &str = "queued_requests";

pub struct PullZoneOriginShieldQueueKind;

impl EntityType for PullZoneOriginShieldQueueKind {
    type Entity = PullZone;
    type DayData = PullZoneOriginShieldQueueDayData;

    const LOG_LABEL: &'static str = "pull_zone_origin_shield_queue";

    fn entity_id(entity: &PullZone) -> String {
        entity.id.to_string()
    }

    fn entity_label(entity: &PullZone) -> String {
        entity.name.clone()
    }

    fn list(client: &ApiClient) -> FetchFuture<'_, Arc<Vec<PullZone>>> {
        Box::pin(async move { client.list_pull_zones().await })
    }

    fn fetch_day<'a>(
        client: &'a ApiClient,
        zone: &'a PullZone,
        date: NaiveDate,
    ) -> FetchFuture<'a, PullZoneOriginShieldQueueDayData> {
        Box::pin(async move {
            let stats = client
                .get_pull_zone_origin_shield_queue_stats(zone.id, date, date)
                .await?;

            let concurrent_requests =
                find_chart_value_for_date(&stats.concurrent_requests_chart, date)
                    .context(CONCURRENT_REQUESTS)?;
            let queued_requests = find_chart_value_for_date(&stats.queued_requests_chart, date)
                .context(QUEUED_REQUESTS)?;

            Ok(PullZoneOriginShieldQueueDayData {
                concurrent_requests,
                queued_requests,
            })
        })
    }

    fn fetch_range<'a>(
        _client: &'a ApiClient,
        _zone: &'a PullZone,
        _from: NaiveDate,
        _to: NaiveDate,
    ) -> FetchFuture<'a, PullZoneOriginShieldQueueDayData> {
        Box::pin(async move { Ok(PullZoneOriginShieldQueueDayData::default()) })
    }

    #[allow(clippy::cast_precision_loss)]
    fn emit_metrics(
        id: &str,
        name: &str,
        _last: &PullZoneOriginShieldQueueDayData,
        current: &PullZoneOriginShieldQueueDayData,
    ) {
        let labels = [("zone_id", id.to_string()), ("name", name.to_string())];

        gauge!(
            "bunnynet.pull_zone_origin_shield_queue.concurrent_requests",
            &labels
        )
        .set(current.concurrent_requests as f64);
        gauge!(
            "bunnynet.pull_zone_origin_shield_queue.queued_requests",
            &labels
        )
        .set(current.queued_requests as f64);
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct PullZoneOriginShieldQueueDayData {
    pub concurrent_requests: u64,
    pub queued_requests: u64,
}

impl DayData for PullZoneOriginShieldQueueDayData {
    fn accumulate(&mut self, _day: Self) {}

    fn merge_latest(&mut self, snap: Self) {
        self.concurrent_requests = snap.concurrent_requests;
        self.queued_requests = snap.queued_requests;
    }
}
