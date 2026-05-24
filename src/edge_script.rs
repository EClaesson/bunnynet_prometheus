use std::sync::Arc;

use anyhow::Context;
use chrono::NaiveDate;
use metrics::{counter, gauge};
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, EdgeScript};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, f64_to_u64, find_chart_value_for_date,
    sum_chart_f64_as_u64,
};

pub type EdgeScriptStatsState = EntityStatsState<EdgeScriptKind>;

pub struct EdgeScriptKind;

impl EntityType for EdgeScriptKind {
    type Entity = EdgeScript;
    type DayData = EdgeScriptDayData;

    const LOG_LABEL: &'static str = "edge_script";

    fn entity_id(entity: &EdgeScript) -> String {
        entity.id.to_string()
    }

    fn entity_label(entity: &EdgeScript) -> String {
        entity.name.clone()
    }

    fn list(api_client: &ApiClient) -> FetchFuture<'_, Arc<Vec<EdgeScript>>> {
        Box::pin(async move { api_client.list_edge_scripts().await })
    }

    fn fetch_day<'a>(
        client: &'a ApiClient,
        script: &'a EdgeScript,
        date: NaiveDate,
    ) -> FetchFuture<'a, EdgeScriptDayData> {
        Box::pin(async move {
            let statistics = client.get_edge_script_stats(script.id, date, date).await?;

            let requests_served = f64_to_u64(
                find_chart_value_for_date(&statistics.requests_served_chart, date)
                    .context("requests_served")?,
            );
            let total_cpu_time = f64_to_u64(
                find_chart_value_for_date(&statistics.total_cpu_time_chart, date)
                    .context("total_cpu_time")?,
            );
            let average_cpu_time =
                find_chart_value_for_date(&statistics.average_cpu_time_chart, date)
                    .context("average_cpu_time")?;

            Ok(EdgeScriptDayData {
                requests_served,
                total_cpu_time,
                average_cpu_time,
            })
        })
    }

    fn fetch_range<'a>(
        api_client: &'a ApiClient,
        script: &'a EdgeScript,
        from: NaiveDate,
        to: NaiveDate,
    ) -> FetchFuture<'a, EdgeScriptDayData> {
        Box::pin(async move {
            let statistics = api_client
                .get_edge_script_stats(script.id, from, to)
                .await?;

            let requests_served = sum_chart_f64_as_u64(&statistics.requests_served_chart);
            let total_cpu_time = sum_chart_f64_as_u64(&statistics.total_cpu_time_chart);

            Ok(EdgeScriptDayData {
                requests_served,
                total_cpu_time,
                average_cpu_time: 0.0,
            })
        })
    }

    fn emit_metrics(id: &str, name: &str, last: &EdgeScriptDayData, current: &EdgeScriptDayData) {
        let labels = [("script_id", id.to_string()), ("name", name.to_string())];

        counter!("bunnynet.edge_script.requests_served", &labels)
            .absolute(last.requests_served + current.requests_served);
        counter!("bunnynet.edge_script.cpu_time", &labels)
            .absolute(last.total_cpu_time + current.total_cpu_time);
        gauge!("bunnynet.edge_script.average_cpu_time", &labels).set(current.average_cpu_time);
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct EdgeScriptDayData {
    pub requests_served: u64,
    pub total_cpu_time: u64,
    pub average_cpu_time: f64,
}

impl DayData for EdgeScriptDayData {
    fn accumulate(&mut self, day: Self) {
        self.requests_served += day.requests_served;
        self.total_cpu_time += day.total_cpu_time;
    }

    fn merge_latest(&mut self, snapshot: Self) {
        self.requests_served = self.requests_served.max(snapshot.requests_served);
        self.total_cpu_time = self.total_cpu_time.max(snapshot.total_cpu_time);
        self.average_cpu_time = snapshot.average_cpu_time;
    }
}
