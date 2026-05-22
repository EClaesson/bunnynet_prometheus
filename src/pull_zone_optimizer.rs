use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use metrics::{counter, gauge};
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, PullZone};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, find_chart_value_for_date,
};

pub type PullZoneOptimizerStatsState = EntityStatsState<PullZoneOptimizerKind>;

const REQUESTS_OPTIMIZED: &str = "requests_optimized";
const TRAFFIC_SAVED: &str = "traffic_saved";
const AVERAGE_COMPRESSION: &str = "average_compression";
const AVERAGE_PROCESSING_TIME: &str = "average_processing_time";

pub struct PullZoneOptimizerKind;

impl EntityType for PullZoneOptimizerKind {
    type Entity = PullZone;
    type DayData = PullZoneOptimizerDayData;

    const LOG_LABEL: &'static str = "pull_zone_optimizer";

    fn entity_id(entity: &PullZone) -> String {
        entity.id.to_string()
    }

    fn entity_label(entity: &PullZone) -> String {
        entity.name.clone()
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
                chart_value_or_default(stats.requests_optimized_chart.as_ref(), date)
                    .context(REQUESTS_OPTIMIZED)?;
            let traffic_saved =
                chart_value_or_default(stats.traffic_saved_chart.as_ref(), date)
                    .context(TRAFFIC_SAVED)?;
            let average_compression =
                chart_value_or_default(stats.average_compression_chart.as_ref(), date)
                    .context(AVERAGE_COMPRESSION)?;
            let average_processing_time =
                chart_value_or_default(stats.average_processing_time_chart.as_ref(), date)
                    .context(AVERAGE_PROCESSING_TIME)?;

            Ok(PullZoneOptimizerDayData {
                requests_optimized,
                traffic_saved,
                average_compression,
                average_processing_time,
            })
        })
    }

    fn emit_metrics(
        id: &str,
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

fn chart_value_or_default<V: Copy + Default>(
    chart: Option<&HashMap<String, V>>,
    date: NaiveDate,
) -> Result<V> {
    chart.map_or_else(|| Ok(V::default()), |c| find_chart_value_for_date(c, date))
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
