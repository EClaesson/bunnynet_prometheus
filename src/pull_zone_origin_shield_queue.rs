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

    fn list(api_client: &ApiClient) -> FetchFuture<'_, Arc<Vec<PullZone>>> {
        Box::pin(async move { api_client.list_pull_zones().await })
    }

    fn fetch_day<'a>(
        client: &'a ApiClient,
        zone: &'a PullZone,
        date: NaiveDate,
        allow_missing: bool,
    ) -> FetchFuture<'a, PullZoneOriginShieldQueueDayData> {
        Box::pin(async move {
            let statistics = client
                .get_pull_zone_origin_shield_queue_stats(zone.id, date, date)
                .await?;

            let concurrent_requests = find_chart_value_for_date(
                &statistics.concurrent_requests_chart,
                date,
                allow_missing,
            )
            .context("concurrent_requests")?;
            let queued_requests =
                find_chart_value_for_date(&statistics.queued_requests_chart, date, allow_missing)
                    .context("queued_requests")?;

            Ok(PullZoneOriginShieldQueueDayData {
                concurrent_requests,
                queued_requests,
            })
        })
    }

    fn fetch_range<'a>(
        _api_client: &'a ApiClient,
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

    fn merge_latest(&mut self, snapshot: Self) {
        self.concurrent_requests = snapshot.concurrent_requests;
        self.queued_requests = snapshot.queued_requests;
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc
)]
mod tests {
    use super::*;

    #[test]
    fn accumulate_is_noop() {
        let mut state = PullZoneOriginShieldQueueDayData {
            concurrent_requests: 50,
            queued_requests: 10,
        };
        state.accumulate(PullZoneOriginShieldQueueDayData {
            concurrent_requests: 999,
            queued_requests: 999,
        });
        assert_eq!(state.concurrent_requests, 50);
        assert_eq!(state.queued_requests, 10);
    }

    #[test]
    fn merge_latest_overwrites_with_snapshot_even_when_smaller() {
        let mut state = PullZoneOriginShieldQueueDayData {
            concurrent_requests: 500,
            queued_requests: 200,
        };
        state.merge_latest(PullZoneOriginShieldQueueDayData {
            concurrent_requests: 5,
            queued_requests: 2,
        });
        assert_eq!(state.concurrent_requests, 5);
        assert_eq!(state.queued_requests, 2);
    }
}
