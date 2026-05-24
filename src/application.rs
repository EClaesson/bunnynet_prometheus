use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use metrics::{counter, gauge};
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, Application, ApplicationVolumeChart};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, f64_to_u64, find_chart_value_for_date,
    sum_chart_f64_as_u64,
};

pub type ApplicationStatsState = EntityStatsState<ApplicationKind>;

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

    fn list(api_client: &ApiClient) -> FetchFuture<'_, Arc<Vec<Application>>> {
        Box::pin(async move { api_client.list_applications().await })
    }

    fn fetch_day<'a>(
        api_client: &'a ApiClient,
        application: &'a Application,
        date: NaiveDate,
    ) -> FetchFuture<'a, ApplicationDayData> {
        Box::pin(async move {
            let statistics = api_client
                .get_application_stats(&application.id, date, date)
                .await?;

            let target_latency = find_chart_value_for_date(&statistics.target_latency_chart, date)
                .context("target_latency")?;
            let active_regions = find_chart_value_for_date(&statistics.active_regions_chart, date)
                .context("active_regions")?
                .unwrap_or(0.0);
            let latency =
                find_chart_value_for_date(&statistics.latency_chart, date).context("latency")?;
            let instances = find_chart_value_for_date(&statistics.instances_chart, date)
                .context("instances")?
                .unwrap_or(0.0);
            let cpu_usage = find_chart_value_for_date(&statistics.cpu_usage_chart, date)
                .context("cpu_usage")?;
            let ram_usage = find_chart_value_for_date(&statistics.ram_usage_chart, date)
                .context("ram_usage")?;
            let traffic = f64_to_u64(
                find_chart_value_for_date(&statistics.traffic_chart, date).context("traffic")?,
            );
            let volume_usage =
                extract_volume_chart_for_date(&statistics.volumes_split_usage_chart, date)
                    .context("volume_usage")?;
            let volume_capacity =
                extract_volume_chart_for_date(&statistics.volumes_split_capacity_chart, date)
                    .context("volume_capacity")?;

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

    fn fetch_range<'a>(
        api_client: &'a ApiClient,
        application: &'a Application,
        from: NaiveDate,
        to: NaiveDate,
    ) -> FetchFuture<'a, ApplicationDayData> {
        Box::pin(async move {
            let statistics = api_client
                .get_application_stats(&application.id, from, to)
                .await?;

            Ok(ApplicationDayData {
                traffic: sum_chart_f64_as_u64(&statistics.traffic_chart),
                ..ApplicationDayData::default()
            })
        })
    }

    fn emit_metrics(id: &str, name: &str, last: &ApplicationDayData, current: &ApplicationDayData) {
        let application_id = id.to_string();
        let application_name = name.to_string();
        let labels = [
            ("app_id", application_id.clone()),
            ("name", application_name.clone()),
        ];

        gauge!("bunnynet.application.target_latency", &labels).set(current.target_latency);
        gauge!("bunnynet.application.active_regions", &labels).set(current.active_regions);
        gauge!("bunnynet.application.latency", &labels).set(current.latency);
        gauge!("bunnynet.application.instances", &labels).set(current.instances);
        gauge!("bunnynet.application.cpu_usage", &labels).set(current.cpu_usage);
        gauge!("bunnynet.application.ram_usage", &labels).set(current.ram_usage);
        counter!("bunnynet.application.traffic", &labels).absolute(last.traffic + current.traffic);

        for (volume, value) in &current.volume_usage {
            gauge!(
                "bunnynet.application.volume_usage",
                "app_id" => application_id.clone(),
                "name" => application_name.clone(),
                "volume" => volume.clone(),
            )
            .set(*value);
        }

        for (volume, value) in &current.volume_capacity {
            gauge!(
                "bunnynet.application.volume_capacity",
                "app_id" => application_id.clone(),
                "name" => application_name.clone(),
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

#[derive(Serialize, Deserialize, Default, Clone)]
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

    fn merge_latest(&mut self, snapshot: Self) {
        self.target_latency = snapshot.target_latency;
        self.active_regions = snapshot.active_regions;
        self.latency = snapshot.latency;
        self.instances = snapshot.instances;
        self.cpu_usage = snapshot.cpu_usage;
        self.ram_usage = snapshot.ram_usage;
        self.traffic = self.traffic.max(snapshot.traffic);
        self.volume_usage = snapshot.volume_usage;
        self.volume_capacity = snapshot.volume_capacity;
    }
}
