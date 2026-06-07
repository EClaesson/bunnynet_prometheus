use std::sync::Arc;

use anyhow::Context;
use chrono::NaiveDate;
use metrics::counter;
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, PullZone};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, find_chart_value_for_date, sum_chart_values,
};

pub type PullZoneSafeHopStatsState = EntityStatsState<PullZoneSafeHopKind>;

pub struct PullZoneSafeHopKind;

impl EntityType for PullZoneSafeHopKind {
    type Entity = PullZone;
    type DayData = PullZoneSafeHopDayData;

    const LOG_LABEL: &'static str = "pull_zone_safehop";

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
    ) -> FetchFuture<'a, PullZoneSafeHopDayData> {
        Box::pin(async move {
            let statistics = api_client
                .get_pull_zone_safehop_stats(zone.id, date, date)
                .await?;

            let requests_retried =
                find_chart_value_for_date(&statistics.requests_retried_chart, date, allow_missing)
                    .context("requests_retried")?;
            let requests_saved =
                find_chart_value_for_date(&statistics.requests_saved_chart, date, allow_missing)
                    .context("requests_saved")?;

            Ok(PullZoneSafeHopDayData {
                requests_retried,
                requests_saved,
            })
        })
    }

    fn fetch_range<'a>(
        api_client: &'a ApiClient,
        zone: &'a PullZone,
        from: NaiveDate,
        to: NaiveDate,
    ) -> FetchFuture<'a, PullZoneSafeHopDayData> {
        Box::pin(async move {
            let statistics = api_client
                .get_pull_zone_safehop_stats(zone.id, from, to)
                .await?;

            Ok(PullZoneSafeHopDayData {
                requests_retried: sum_chart_values(&statistics.requests_retried_chart),
                requests_saved: sum_chart_values(&statistics.requests_saved_chart),
            })
        })
    }

    fn emit_metrics(
        id: &str,
        name: &str,
        last: &PullZoneSafeHopDayData,
        current: &PullZoneSafeHopDayData,
    ) {
        let labels = [("zone_id", id.to_string()), ("name", name.to_string())];

        counter!("bunnynet.pull_zone_safehop.requests_retried", &labels)
            .absolute(last.requests_retried + current.requests_retried);
        counter!("bunnynet.pull_zone_safehop.requests_saved", &labels)
            .absolute(last.requests_saved + current.requests_saved);
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct PullZoneSafeHopDayData {
    pub requests_retried: u64,
    pub requests_saved: u64,
}

impl DayData for PullZoneSafeHopDayData {
    fn accumulate(&mut self, day: Self) {
        self.requests_retried += day.requests_retried;
        self.requests_saved += day.requests_saved;
    }

    fn merge_latest(&mut self, snapshot: Self) {
        self.requests_retried = self.requests_retried.max(snapshot.requests_retried);
        self.requests_saved = self.requests_saved.max(snapshot.requests_saved);
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
    fn accumulate_sums_counters() {
        let mut state = PullZoneSafeHopDayData {
            requests_retried: 10,
            requests_saved: 20,
        };
        state.accumulate(PullZoneSafeHopDayData {
            requests_retried: 3,
            requests_saved: 4,
        });
        assert_eq!(state.requests_retried, 13);
        assert_eq!(state.requests_saved, 24);
    }

    #[test]
    fn merge_latest_takes_max_per_field() {
        let mut state = PullZoneSafeHopDayData {
            requests_retried: 50,
            requests_saved: 5,
        };
        state.merge_latest(PullZoneSafeHopDayData {
            requests_retried: 30,
            requests_saved: 100,
        });
        assert_eq!(state.requests_retried, 50);
        assert_eq!(state.requests_saved, 100);
    }
}
