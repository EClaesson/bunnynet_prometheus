use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use metrics::{counter, gauge};
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, PullZone};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, find_chart_value_for_date, sum_chart_values,
};

pub type PullZoneOptimizerStatsState = EntityStatsState<PullZoneOptimizerKind>;

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

    fn list(api_client: &ApiClient) -> FetchFuture<'_, Arc<Vec<PullZone>>> {
        Box::pin(async move { api_client.list_pull_zones().await })
    }

    fn fetch_day<'a>(
        api_client: &'a ApiClient,
        zone: &'a PullZone,
        date: NaiveDate,
        allow_missing: bool,
    ) -> FetchFuture<'a, PullZoneOptimizerDayData> {
        Box::pin(async move {
            let statistics = api_client
                .get_pull_zone_optimizer_stats(zone.id, date, date)
                .await?;

            let requests_optimized = chart_value_or_default(
                statistics.requests_optimized_chart.as_ref(),
                date,
                allow_missing,
            )
            .context("requests_optimized")?;
            let traffic_saved = chart_value_or_default(
                statistics.traffic_saved_chart.as_ref(),
                date,
                allow_missing,
            )
            .context("traffic_saved")?;
            let average_compression = chart_value_or_default(
                statistics.average_compression_chart.as_ref(),
                date,
                allow_missing,
            )
            .context("average_compression")?;
            let average_processing_time = chart_value_or_default(
                statistics.average_processing_time_chart.as_ref(),
                date,
                allow_missing,
            )
            .context("average_processing_time")?;

            Ok(PullZoneOptimizerDayData {
                requests_optimized,
                traffic_saved,
                average_compression,
                average_processing_time,
            })
        })
    }

    fn fetch_range<'a>(
        api_client: &'a ApiClient,
        zone: &'a PullZone,
        from: NaiveDate,
        to: NaiveDate,
    ) -> FetchFuture<'a, PullZoneOptimizerDayData> {
        Box::pin(async move {
            let statistics = api_client
                .get_pull_zone_optimizer_stats(zone.id, from, to)
                .await?;

            let requests_optimized = statistics
                .requests_optimized_chart
                .as_ref()
                .map(sum_chart_values)
                .unwrap_or_default();
            let traffic_saved = statistics
                .traffic_saved_chart
                .as_ref()
                .map(sum_chart_values)
                .unwrap_or_default();

            Ok(PullZoneOptimizerDayData {
                requests_optimized,
                traffic_saved,
                average_compression: 0.0,
                average_processing_time: 0.0,
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
        gauge!(
            "bunnynet.pull_zone_optimizer.average_processing_time",
            &labels
        )
        .set(current.average_processing_time);
    }
}

fn chart_value_or_default<V: Copy + Default>(
    chart: Option<&HashMap<String, V>>,
    date: NaiveDate,
    allow_missing: bool,
) -> Result<V> {
    chart.map_or_else(
        || Ok(V::default()),
        |chart_inner| find_chart_value_for_date(chart_inner, date, allow_missing),
    )
}

#[derive(Serialize, Deserialize, Default, Clone)]
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

    fn merge_latest(&mut self, snapshot: Self) {
        self.requests_optimized = self.requests_optimized.max(snapshot.requests_optimized);
        self.traffic_saved = self.traffic_saved.max(snapshot.traffic_saved);
        self.average_compression = snapshot.average_compression;
        self.average_processing_time = snapshot.average_processing_time;
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc,
    clippy::float_cmp
)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn accumulate_sums_counters_and_leaves_gauges() {
        let mut state = PullZoneOptimizerDayData {
            requests_optimized: 10,
            traffic_saved: 100,
            average_compression: 0.5,
            average_processing_time: 2.0,
        };
        state.accumulate(PullZoneOptimizerDayData {
            requests_optimized: 3,
            traffic_saved: 30,
            average_compression: 99.0,
            average_processing_time: 99.0,
        });
        assert_eq!(state.requests_optimized, 13);
        assert_eq!(state.traffic_saved, 130);
        assert_eq!(state.average_compression, 0.5);
        assert_eq!(state.average_processing_time, 2.0);
    }

    #[test]
    fn merge_latest_max_counters_and_overwrite_gauges_even_when_smaller() {
        let mut state = PullZoneOptimizerDayData {
            requests_optimized: 50,
            traffic_saved: 500,
            average_compression: 9.9,
            average_processing_time: 9.9,
        };
        state.merge_latest(PullZoneOptimizerDayData {
            requests_optimized: 30,
            traffic_saved: 1000,
            average_compression: 0.1,
            average_processing_time: 0.1,
        });
        assert_eq!(state.requests_optimized, 50);
        assert_eq!(state.traffic_saved, 1000);
        assert_eq!(state.average_compression, 0.1);
        assert_eq!(state.average_processing_time, 0.1);
    }

    #[test]
    fn chart_value_or_default_returns_default_for_none_else_delegates() {
        let absent: u64 = chart_value_or_default(None, date(2026, 5, 24), false).unwrap();
        assert_eq!(absent, 0);

        let mut chart = HashMap::new();
        chart.insert("2026-05-24".to_string(), 42u64);
        assert_eq!(
            chart_value_or_default(Some(&chart), date(2026, 5, 24), false).unwrap(),
            42
        );

        let mut wrong_date = HashMap::new();
        wrong_date.insert("2026-05-25".to_string(), 42u64);
        assert!(chart_value_or_default(Some(&wrong_date), date(2026, 5, 24), false).is_err());
    }

    #[test]
    fn chart_value_or_default_allow_missing_returns_zero_for_empty_chart() {
        let empty: HashMap<String, u64> = HashMap::new();
        assert_eq!(
            chart_value_or_default(Some(&empty), date(2026, 5, 24), true).unwrap(),
            0
        );
        assert!(chart_value_or_default(Some(&empty), date(2026, 5, 24), false).is_err());
    }
}
