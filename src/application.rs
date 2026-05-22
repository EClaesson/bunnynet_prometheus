use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use metrics::{counter, gauge};
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, Application, ApplicationVolumeChart};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, f64_to_u64, find_chart_value_for_date,
};

pub type ApplicationStatsState = EntityStatsState<ApplicationKind>;

const TARGET_LATENCY: &str = "target_latency";
const ACTIVE_REGIONS: &str = "active_regions";
const LATENCY: &str = "latency";
const INSTANCES: &str = "instances";
const CPU_USAGE: &str = "cpu_usage";
const RAM_USAGE: &str = "ram_usage";
const TRAFFIC: &str = "traffic";
const VOLUME_USAGE: &str = "volume_usage";
const VOLUME_CAPACITY: &str = "volume_capacity";

pub struct ApplicationKind;

impl EntityType for ApplicationKind {
    type Entity = Application;
    type DayData = ApplicationDayData;

    const LOG_LABEL: &'static str = "application";

    fn entity_id(entity: &Application) -> String {
        entity.id.clone()
    }

    fn entity_label(entity: &Application) -> String {
        entity.name.clone()
    }

    fn list(client: &ApiClient) -> FetchFuture<'_, Vec<Application>> {
        Box::pin(async move { client.list_applications().await })
    }

    fn fetch_day<'a>(
        client: &'a ApiClient,
        app: &'a Application,
        date: NaiveDate,
    ) -> FetchFuture<'a, ApplicationDayData> {
        Box::pin(async move {
            let stats = client.get_application_stats(&app.id, date, date).await?;

            let target_latency = find_chart_value_for_date(&stats.target_latency_chart, date)
                .context(TARGET_LATENCY)?;
            let active_regions = find_chart_value_for_date(&stats.active_regions_chart, date)
                .context(ACTIVE_REGIONS)?
                .unwrap_or(0.0);
            let latency =
                find_chart_value_for_date(&stats.latency_chart, date).context(LATENCY)?;
            let instances = find_chart_value_for_date(&stats.instances_chart, date)
                .context(INSTANCES)?
                .unwrap_or(0.0);
            let cpu_usage =
                find_chart_value_for_date(&stats.cpu_usage_chart, date).context(CPU_USAGE)?;
            let ram_usage =
                find_chart_value_for_date(&stats.ram_usage_chart, date).context(RAM_USAGE)?;
            let traffic = f64_to_u64(
                find_chart_value_for_date(&stats.traffic_chart, date).context(TRAFFIC)?,
            );
            let volume_usage =
                extract_volume_chart_for_date(&stats.volumes_split_usage_chart, date)
                    .context(VOLUME_USAGE)?;
            let volume_capacity =
                extract_volume_chart_for_date(&stats.volumes_split_capacity_chart, date)
                    .context(VOLUME_CAPACITY)?;

            Ok(ApplicationDayData {
                target_latency,
                active_regions,
                latency,
                instances,
                cpu_usage,
                ram_usage,
                traffic,
                volume_usage,
                volume_capacity,
            })
        })
    }

    fn emit_metrics(
        id: &str,
        name: &str,
        last: &ApplicationDayData,
        current: &ApplicationDayData,
    ) {
        let labels = [("app_id", id.to_string()), ("name", name.to_string())];

        gauge!("bunnynet.application.target_latency", &labels).set(current.target_latency);
        gauge!("bunnynet.application.active_regions", &labels).set(current.active_regions);
        gauge!("bunnynet.application.latency", &labels).set(current.latency);
        gauge!("bunnynet.application.instances", &labels).set(current.instances);
        gauge!("bunnynet.application.cpu_usage", &labels).set(current.cpu_usage);
        gauge!("bunnynet.application.ram_usage", &labels).set(current.ram_usage);
        counter!("bunnynet.application.traffic", &labels)
            .absolute(last.traffic + current.traffic);

        for (volume, value) in &current.volume_usage {
            gauge!(
                "bunnynet.application.volume_usage",
                "app_id" => id.to_string(),
                "name" => name.to_string(),
                "volume" => volume.clone(),
            )
            .set(*value);
        }

        for (volume, value) in &current.volume_capacity {
            gauge!(
                "bunnynet.application.volume_capacity",
                "app_id" => id.to_string(),
                "name" => name.to_string(),
                "volume" => volume.clone(),
            )
            .set(*value);
        }
    }
}

fn extract_volume_chart_for_date(
    chart: &ApplicationVolumeChart,
    date: NaiveDate,
) -> Result<HashMap<String, f64>> {
    let mut result = HashMap::with_capacity(chart.len());
    for (volume, time_series) in chart {
        let value = find_chart_value_for_date(time_series, date)
            .with_context(|| format!("volume {volume}"))?;
        result.insert(volume.clone(), value);
    }
    Ok(result)
}

#[derive(Serialize, Deserialize, Default)]
pub struct ApplicationDayData {
    pub target_latency: f64,
    pub active_regions: f64,
    pub latency: f64,
    pub instances: f64,
    pub cpu_usage: f64,
    pub ram_usage: f64,
    pub traffic: u64,
    pub volume_usage: HashMap<String, f64>,
    pub volume_capacity: HashMap<String, f64>,
}

impl DayData for ApplicationDayData {
    fn accumulate(&mut self, day: Self) {
        self.traffic += day.traffic;
    }

    fn merge_latest(&mut self, snap: Self) {
        self.target_latency = snap.target_latency;
        self.active_regions = snap.active_regions;
        self.latency = snap.latency;
        self.instances = snap.instances;
        self.cpu_usage = snap.cpu_usage;
        self.ram_usage = snap.ram_usage;
        self.traffic = self.traffic.max(snap.traffic);
        self.volume_usage = snap.volume_usage;
        self.volume_capacity = snap.volume_capacity;
    }
}
