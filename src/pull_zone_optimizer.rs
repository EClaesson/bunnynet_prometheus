use anyhow::Context;
use chrono::NaiveDate;
use metrics::{counter, gauge};
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, PullZone};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, find_chart_value_for_date,
};

pub type PullZoneOptimizerStatsState = EntityStatsState<PullZoneOptimizerKind>;

pub struct PullZoneOptimizerKind;

impl EntityType for PullZoneOptimizerKind {
    type Entity = PullZone;
    type DayData = PullZoneOptimizerDayData;

    const LOG_LABEL: &'static str = "Pull zone optimizer";

    fn entity_id(entity: &PullZone) -> u64 {
        entity.id
    }

    fn entity_label(entity: &PullZone) -> &str {
        &entity.name
    }

    fn list(client: &ApiClient) -> FetchFuture<'_, Vec<PullZone>> {
        Box::pin(async move { client.list_pull_zones().await })
    }

    fn fetch_day<'a>(
        client: &'a ApiClient,
        zone: &'a PullZone,
        date: NaiveDate,
    ) -> FetchFuture<'a, PullZoneOptimizerDayData> {
        Box::pin(async move {
            let stats = client
                .get_pull_zone_optimizer_stats(zone.id, date, date)
                .await?;

            let requests_optimized =
                find_chart_value_for_date(&stats.requests_optimized_chart, date)
                    .context("Requests optimized")?;
            let traffic_saved = find_chart_value_for_date(&stats.traffic_saved_chart, date)
                .context("Traffic saved")?;
            let average_compression =
                find_chart_value_for_date(&stats.average_compression_chart, date)
                    .context("Average compression")?;
            let average_processing_time =
                find_chart_value_for_date(&stats.average_processing_time_chart, date)
                    .context("Average processing time")?;

            Ok(PullZoneOptimizerDayData {
                requests_optimized,
                traffic_saved,
                average_compression,
                average_processing_time,
            })
        })
    }

    fn emit_metrics(
        id: u64,
        name: &str,
        last: &PullZoneOptimizerDayData,
        current: &PullZoneOptimizerDayData,
    ) {
        let labels = [("zone_id", id.to_string()), ("name", name.to_string())];

        counter!("bunnynet.pull_zone_optimizer.requests_optimized", &labels)
            .absolute(last.requests_optimized + current.requests_optimized);
        counter!("bunnynet.pull_zone_optimizer.traffic_saved", &labels)
            .absolute(last.traffic_saved + current.traffic_saved);
        gauge!("bunnynet.pull_zone_optimizer.average_compression", &labels)
            .set(current.average_compression);
        gauge!("bunnynet.pull_zone_optimizer.average_processing_time", &labels)
            .set(current.average_processing_time);
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct PullZoneOptimizerDayData {
    pub requests_optimized: u64,
    pub traffic_saved: u64,
    pub average_compression: f64,
    pub average_processing_time: f64,
}

impl DayData for PullZoneOptimizerDayData {
    fn accumulate(&mut self, day: Self) {
        self.requests_optimized += day.requests_optimized;
        self.traffic_saved += day.traffic_saved;
    }

    fn merge_latest(&mut self, snap: Self) {
        self.requests_optimized = self.requests_optimized.max(snap.requests_optimized);
        self.traffic_saved = self.traffic_saved.max(snap.traffic_saved);
        self.average_compression = snap.average_compression;
        self.average_processing_time = snap.average_processing_time;
    }
}
