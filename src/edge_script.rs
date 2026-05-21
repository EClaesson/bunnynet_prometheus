use anyhow::Context;
use chrono::NaiveDate;
use metrics::{counter, gauge};
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, EdgeScript};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, f64_to_u64, find_chart_value_for_date,
};

pub type EdgeScriptStatsState = EntityStatsState<EdgeScriptKind>;

const REQUESTS_SERVED: &str = "requests_served";
const TOTAL_CPU_TIME: &str = "total_cpu_time";
const AVERAGE_CPU_TIME: &str = "average_cpu_time";

pub struct EdgeScriptKind;

impl EntityType for EdgeScriptKind {
    type Entity = EdgeScript;
    type DayData = EdgeScriptDayData;

    const LOG_LABEL: &'static str = "Edge script";

    fn entity_id(entity: &EdgeScript) -> u64 {
        entity.id
    }

    fn entity_label(entity: &EdgeScript) -> String {
        entity.name.clone()
    }

    fn list(client: &ApiClient) -> FetchFuture<'_, Vec<EdgeScript>> {
        Box::pin(async move { client.list_edge_scripts().await })
    }

    fn fetch_day<'a>(
        client: &'a ApiClient,
        script: &'a EdgeScript,
        date: NaiveDate,
    ) -> FetchFuture<'a, EdgeScriptDayData> {
        Box::pin(async move {
            let stats = client.get_edge_script_stats(script.id, date, date).await?;

            let requests_served = f64_to_u64(
                find_chart_value_for_date(&stats.requests_served_chart, date)
                    .context(REQUESTS_SERVED)?,
            );
            let total_cpu_time = f64_to_u64(
                find_chart_value_for_date(&stats.total_cpu_time_chart, date)
                    .context(TOTAL_CPU_TIME)?,
            );
            let average_cpu_time = find_chart_value_for_date(&stats.average_cpu_time_chart, date)
                .context(AVERAGE_CPU_TIME)?;

            Ok(EdgeScriptDayData {
                requests_served,
                total_cpu_time,
                average_cpu_time,
            })
        })
    }

    fn emit_metrics(
        id: u64,
        name: &str,
        last: &EdgeScriptDayData,
        current: &EdgeScriptDayData,
    ) {
        let labels = [("script_id", id.to_string()), ("name", name.to_string())];

        counter!("bunnynet.edge_script.requests_served", &labels)
            .absolute(last.requests_served + current.requests_served);
        counter!("bunnynet.edge_script.cpu_time", &labels)
            .absolute(last.total_cpu_time + current.total_cpu_time);
        gauge!("bunnynet.edge_script.average_cpu_time", &labels)
            .set(current.average_cpu_time);
    }
}

#[derive(Serialize, Deserialize, Default)]
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

    fn merge_latest(&mut self, snap: Self) {
        self.requests_served = self.requests_served.max(snap.requests_served);
        self.total_cpu_time = self.total_cpu_time.max(snap.total_cpu_time);
        self.average_cpu_time = snap.average_cpu_time;
    }
}
