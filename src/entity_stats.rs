use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;

use anyhow::{Result, bail};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tracing::{debug, info};

use crate::bunny::ApiClient;
use crate::state::{PollFuture, State};

const DATE_FORMAT: &str = "%Y-%m-%d";

pub type FetchFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

pub trait DayData: Default + Clone + Serialize + DeserializeOwned + Send {
    fn accumulate(&mut self, day: Self);
    fn merge_latest(&mut self, snap: Self);
}

pub trait EntityType: Sized + Send + Sync + 'static {
    type Entity: Send + Sync;
    type DayData: DayData;

    const LOG_LABEL: &'static str;

    fn entity_id(entity: &Self::Entity) -> String;
    fn entity_label(entity: &Self::Entity) -> String;

    fn list(client: &ApiClient) -> FetchFuture<'_, Vec<Self::Entity>>;
    fn fetch_day<'a>(
        client: &'a ApiClient,
        entity: &'a Self::Entity,
        date: NaiveDate,
    ) -> FetchFuture<'a, Self::DayData>;

    fn emit_metrics(id: &str, label: &str, last: &Self::DayData, current: &Self::DayData);
}

#[derive(Serialize, Deserialize)]
#[serde(bound(
    serialize = "E::DayData: Serialize",
    deserialize = "E::DayData: Deserialize<'de>"
))]
pub struct EntityStatsState<E: EntityType> {
    entities: HashMap<String, EntityEntry<E>>,
    #[serde(skip)]
    last_entity_count: Option<usize>,
}

impl<E: EntityType> Default for EntityStatsState<E> {
    fn default() -> Self {
        Self {
            entities: HashMap::new(),
            last_entity_count: None,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(bound(
    serialize = "E::DayData: Serialize",
    deserialize = "E::DayData: Deserialize<'de>"
))]
struct EntityEntry<E: EntityType> {
    #[serde(skip)]
    label: String,
    last_complete_day: Option<NaiveDate>,
    last_complete_day_state: E::DayData,
    current_day_state: E::DayData,
}

impl<E: EntityType> Default for EntityEntry<E> {
    fn default() -> Self {
        Self {
            label: String::new(),
            last_complete_day: None,
            last_complete_day_state: E::DayData::default(),
            current_day_state: E::DayData::default(),
        }
    }
}

impl<E: EntityType> Clone for EntityEntry<E> {
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            last_complete_day: self.last_complete_day,
            last_complete_day_state: self.last_complete_day_state.clone(),
            current_day_state: self.current_day_state.clone(),
        }
    }
}

impl<E: EntityType> State for EntityStatsState<E> {
    fn poll<'a>(&'a mut self, client: &'a ApiClient) -> PollFuture<'a> {
        Box::pin(async move {
            let mut new_state = Self {
                entities: self.entities.clone(),
                last_entity_count: self.last_entity_count,
            };
            new_state.try_poll(client).await?;
            *self = new_state;
            Ok(())
        })
    }

    fn serialize(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }
}

impl<E: EntityType> EntityStatsState<E> {
    async fn try_poll(&mut self, client: &ApiClient) -> Result<()> {
        debug!(collector = E::LOG_LABEL, "Polling stats");
        let today = chrono::Utc::now().date_naive();
        let yesterday = today - chrono::Days::new(1);

        let entities = E::list(client).await?;
        let count = entities.len();

        match self.last_entity_count {
            None => info!(collector = E::LOG_LABEL, count, "Monitoring entities",),
            Some(prev) if prev != count => info!(
                collector = E::LOG_LABEL,
                previous = prev,
                current = count,
                "Entity count changed",
            ),
            _ => {}
        }
        self.last_entity_count = Some(count);

        let mut live_ids: HashSet<String> = HashSet::with_capacity(entities.len());
        for entity in &entities {
            let id = E::entity_id(entity);
            let label = E::entity_label(entity);
            live_ids.insert(id.clone());
            let entry = self.entities.entry(id.clone()).or_default();
            entry.label.clone_from(&label);

            if let Some(last) = entry.last_complete_day
                && last < yesterday
            {
                let first = last + chrono::TimeDelta::days(1);
                let days = (yesterday - last).num_days();
                info!(
                    collector = E::LOG_LABEL,
                    entity_id = %id,
                    entity_label = %label,
                    from = %first,
                    to = %yesterday,
                    days,
                    "Backfilling missed days",
                );
                let mut cursor = first;
                while cursor <= yesterday {
                    debug!(
                        collector = E::LOG_LABEL,
                        entity_id = %id,
                        entity_label = %label,
                        date = %cursor,
                        "Fetching day (backfill)",
                    );
                    let day = E::fetch_day(client, entity, cursor).await?;
                    entry.last_complete_day_state.accumulate(day);
                    entry.last_complete_day = Some(cursor);
                    cursor += chrono::TimeDelta::days(1);
                }
                entry.current_day_state = E::DayData::default();
            } else {
                entry.last_complete_day = Some(yesterday);
            }

            debug!(
                collector = E::LOG_LABEL,
                entity_id = %id,
                entity_label = %label,
                date = %today,
                "Fetching day (current)",
            );
            let snap = E::fetch_day(client, entity, today).await?;
            entry.current_day_state.merge_latest(snap);
        }

        self.entities.retain(|id, _| live_ids.contains(id));

        for (id, entry) in &self.entities {
            E::emit_metrics(
                id,
                &entry.label,
                &entry.last_complete_day_state,
                &entry.current_day_state,
            );
        }

        Ok(())
    }
}

pub fn find_chart_value_for_date<V: Copy>(
    chart: &HashMap<String, V>,
    date: NaiveDate,
) -> Result<V> {
    let date_str = date.format(DATE_FORMAT).to_string();
    let mut iter = chart.iter();
    match (iter.next(), iter.next()) {
        (Some((key, value)), None) if key.starts_with(&date_str) => Ok(*value),
        _ => bail!(
            "Expected exactly one entry starting with {date_str}, got {} entries",
            chart.len()
        ),
    }
}

pub fn find_chart_value_for_date_multi<V: Copy>(
    chart: &HashMap<String, V>,
    date: NaiveDate,
) -> Result<V> {
    let date_str = date.format(DATE_FORMAT).to_string();
    chart
        .iter()
        .find(|(key, _)| key.starts_with(&date_str))
        .map(|(_, value)| *value)
        .ok_or_else(|| anyhow::anyhow!("No entry starting with {date_str} found in chart"))
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub const fn f64_to_u64(v: f64) -> u64 {
    if v < 0.0 { 0 } else { v as u64 }
}
