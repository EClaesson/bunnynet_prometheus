use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{Result, bail};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::{debug, info};

use crate::bunny::ApiClient;
use crate::state::{PollFuture, State};

const DATE_FORMAT: &str = "%Y-%m-%d";

pub type FetchFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

pub trait DayData: Default + Clone + Serialize + DeserializeOwned + Send + 'static {
    fn accumulate(&mut self, day: Self);
    fn merge_latest(&mut self, snapshot: Self);
}

pub trait EntityType: Sized + Send + Sync + 'static {
    type Entity: Send + Sync + 'static;
    type DayData: DayData;

    const LOG_LABEL: &'static str;

    fn entity_id(entity: &Self::Entity) -> String;
    fn entity_label(entity: &Self::Entity) -> String;

    fn list(api_client: &ApiClient) -> FetchFuture<'_, Arc<Vec<Self::Entity>>>;
    fn fetch_day<'a>(
        api_client: &'a ApiClient,
        entity: &'a Self::Entity,
        date: NaiveDate,
    ) -> FetchFuture<'a, Self::DayData>;

    fn fetch_range<'a>(
        api_client: &'a ApiClient,
        entity: &'a Self::Entity,
        from: NaiveDate,
        to: NaiveDate,
    ) -> FetchFuture<'a, Self::DayData> {
        Box::pin(async move {
            let mut accumulator = Self::DayData::default();
            let mut cursor = from;
            while cursor <= to {
                let day = Self::fetch_day(api_client, entity, cursor).await?;
                accumulator.accumulate(day);
                cursor += chrono::TimeDelta::days(1);
            }
            Ok(accumulator)
        })
    }

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
    fn poll(&mut self, api_client: Arc<ApiClient>, concurrency: usize) -> PollFuture<'_> {
        Box::pin(async move {
            let mut new_state = Self {
                entities: self.entities.clone(),
                last_entity_count: self.last_entity_count,
            };
            new_state.try_poll(api_client, concurrency).await?;
            *self = new_state;
            Ok(())
        })
    }

    fn serialize(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }
}

impl<E: EntityType> EntityStatsState<E> {
    async fn try_poll(&mut self, client: Arc<ApiClient>, concurrency: usize) -> Result<()> {
        debug!(collector = E::LOG_LABEL, "Polling stats");
        let today = chrono::Utc::now().date_naive();
        let yesterday = today - chrono::Days::new(1);

        let entities = E::list(&client).await?;
        let count = entities.len();

        match self.last_entity_count {
            None => info!(collector = E::LOG_LABEL, count, "Monitoring entities",),
            Some(previous) if previous != count => info!(
                collector = E::LOG_LABEL,
                previous = previous,
                current = count,
                "Entity count changed",
            ),
            _ => {}
        }
        self.last_entity_count = Some(count);

        let live_ids: HashSet<String> = entities.iter().map(E::entity_id).collect();

        let semaphore = Arc::new(Semaphore::new(concurrency.max(1)));
        let mut join_set: JoinSet<Result<(String, EntityEntry<E>)>> = JoinSet::new();

        for (idx, entity) in entities.iter().enumerate() {
            let id = E::entity_id(entity);
            let mut entry = self.entities.get(&id).cloned().unwrap_or_default();
            entry.label = E::entity_label(entity);

            let entities = Arc::clone(&entities);
            let semaphore = Arc::clone(&semaphore);
            let client = Arc::clone(&client);

            join_set.spawn(async move {
                let _permit = semaphore.acquire().await?;
                let entity = &entities[idx];
                update_entry::<E>(&client, entity, &id, &mut entry, today, yesterday).await?;
                Ok((id, entry))
            });
        }

        let mut first_error: Option<anyhow::Error> = None;
        while let Some(joined) = join_set.join_next().await {
            match joined {
                Ok(Ok((id, entry))) if first_error.is_none() => {
                    self.entities.insert(id, entry);
                }
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    if first_error.is_none() {
                        first_error = Some(e);
                        join_set.abort_all();
                    }
                }
                Err(e) => {
                    if first_error.is_none() {
                        first_error = Some(e.into());
                        join_set.abort_all();
                    }
                }
            }
        }

        if let Some(e) = first_error {
            return Err(e);
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

async fn update_entry<E: EntityType>(
    api_client: &ApiClient,
    entity: &E::Entity,
    id: &str,
    entry: &mut EntityEntry<E>,
    today: NaiveDate,
    yesterday: NaiveDate,
) -> Result<()> {
    if let Some(last) = entry.last_complete_day
        && last < yesterday
    {
        let first = last + chrono::TimeDelta::days(1);
        let days = (yesterday - last).num_days();
        info!(
            collector = E::LOG_LABEL,
            entity_id = %id,
            entity_label = %entry.label,
            from = %first,
            to = %yesterday,
            days,
            "Backfilling missed days",
        );
        let backfill = E::fetch_range(api_client, entity, first, yesterday).await?;
        entry.last_complete_day_state.accumulate(backfill);
        entry.last_complete_day = Some(yesterday);
        entry.current_day_state = E::DayData::default();
    } else {
        entry.last_complete_day = Some(yesterday);
    }

    debug!(
        collector = E::LOG_LABEL,
        entity_id = %id,
        entity_label = %entry.label,
        date = %today,
        "Fetching day (current)",
    );
    let snapshot = E::fetch_day(api_client, entity, today).await?;
    entry.current_day_state.merge_latest(snapshot);

    Ok(())
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

pub fn sum_chart_values<V>(chart: &HashMap<String, V>) -> V
where
    V: Copy + std::iter::Sum<V>,
{
    chart.values().copied().sum()
}

pub fn sum_chart_f64_as_u64(chart: &HashMap<String, f64>) -> u64 {
    chart.values().copied().map(f64_to_u64).sum()
}

pub fn sum_chart_values_in_range<V>(chart: &HashMap<String, V>, from: NaiveDate, to: NaiveDate) -> V
where
    V: Copy + std::iter::Sum<V>,
{
    let from_key = from.format(DATE_FORMAT).to_string();
    let to_key = to.format(DATE_FORMAT).to_string();
    chart
        .iter()
        .filter_map(|(key, value)| {
            let prefix = key.get(..from_key.len())?;
            (prefix >= from_key.as_str() && prefix <= to_key.as_str()).then_some(*value)
        })
        .sum()
}

pub fn find_chart_value_for_date_lenient<V: Copy>(
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
pub const fn f64_to_u64(value: f64) -> u64 {
    if value < 0.0 { 0 } else { value as u64 }
}

pub fn emit_labeled_counter(
    metric: &'static str,
    last: &HashMap<String, u64>,
    current: &HashMap<String, u64>,
    label_key: &'static str,
    fixed_labels: &[(&'static str, String)],
) {
    let keys: HashSet<&String> = last.keys().chain(current.keys()).collect();
    for key in keys {
        let total = last.get(key).copied().unwrap_or(0) + current.get(key).copied().unwrap_or(0);
        let mut labels: Vec<(&'static str, String)> = Vec::with_capacity(fixed_labels.len() + 1);
        labels.extend_from_slice(fixed_labels);
        labels.push((label_key, key.clone()));
        metrics::counter!(metric, &labels).absolute(total);
    }
}
