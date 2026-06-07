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
        allow_missing: bool,
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
                let day = Self::fetch_day(api_client, entity, cursor, false).await?;
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

        if days == 1 {
            debug!(
                collector = E::LOG_LABEL,
                entity_id = %id,
                entity_label = %entry.label,
                date = %yesterday,
                "Finalizing day rollover"
            );
        } else {
            info!(
                collector = E::LOG_LABEL,
                entity_id = %id,
                entity_label = %entry.label,
                from = %first,
                to = %yesterday,
                days,
                "Backfilling missed days",
            );
        }

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
    let snapshot = E::fetch_day(api_client, entity, today, true).await?;
    entry.current_day_state.merge_latest(snapshot);

    Ok(())
}

pub fn find_chart_value_for_date<V: Copy + Default>(
    chart: &HashMap<String, V>,
    date: NaiveDate,
    allow_missing: bool,
) -> Result<V> {
    let date_str = date.format(DATE_FORMAT).to_string();
    let mut iter = chart.iter();
    match (iter.next(), iter.next()) {
        (Some((key, value)), None) if key.starts_with(&date_str) => Ok(*value),
        (None, None) if allow_missing => Ok(V::default()),
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

pub fn find_chart_value_for_date_lenient<V: Copy + Default>(
    chart: &HashMap<String, V>,
    date: NaiveDate,
    allow_missing: bool,
) -> Result<V> {
    let date_str = date.format(DATE_FORMAT).to_string();
    match chart.iter().find(|(key, _)| key.starts_with(&date_str)) {
        Some((_, value)) => Ok(*value),
        None if allow_missing => Ok(V::default()),
        None => Err(anyhow::anyhow!(
            "No entry starting with {date_str} found in chart"
        )),
    }
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

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc,
    clippy::float_cmp,
    clippy::await_holding_lock
)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::Duration;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn f64_to_u64_truncates_positives_and_clamps_negatives_and_nan() {
        assert_eq!(f64_to_u64(42.9), 42);
        assert_eq!(f64_to_u64(0.0), 0);
        assert_eq!(f64_to_u64(-1.0), 0);
        assert_eq!(f64_to_u64(f64::NAN), 0);
        assert_eq!(f64_to_u64(f64::INFINITY), u64::MAX);
    }

    #[test]
    fn find_chart_value_for_date_accepts_bare_and_timestamped_keys() {
        let mut chart = HashMap::new();
        chart.insert("2026-05-24".to_string(), 7u64);
        assert_eq!(
            find_chart_value_for_date(&chart, date(2026, 5, 24), false).unwrap(),
            7
        );

        let mut ts = HashMap::new();
        ts.insert("2026-05-24T00:00:00".to_string(), 9u64);
        assert_eq!(
            find_chart_value_for_date(&ts, date(2026, 5, 24), false).unwrap(),
            9
        );
    }

    #[test]
    fn find_chart_value_for_date_errors_unless_exactly_one_matching_entry() {
        let empty: HashMap<String, u64> = HashMap::new();
        assert!(find_chart_value_for_date(&empty, date(2026, 5, 24), false).is_err());

        let mut wrong = HashMap::new();
        wrong.insert("2026-05-25".to_string(), 1u64);
        assert!(find_chart_value_for_date(&wrong, date(2026, 5, 24), false).is_err());

        let mut multi = HashMap::new();
        multi.insert("2026-05-24".to_string(), 1u64);
        multi.insert("2026-05-25".to_string(), 2u64);
        assert!(find_chart_value_for_date(&multi, date(2026, 5, 24), false).is_err());
    }

    #[test]
    fn find_chart_value_for_date_allow_missing_returns_zero_only_when_empty() {
        let empty: HashMap<String, u64> = HashMap::new();
        assert_eq!(
            find_chart_value_for_date(&empty, date(2026, 5, 24), true).unwrap(),
            0
        );

        let mut present = HashMap::new();
        present.insert("2026-05-24".to_string(), 7u64);
        assert_eq!(
            find_chart_value_for_date(&present, date(2026, 5, 24), true).unwrap(),
            7
        );

        let mut wrong = HashMap::new();
        wrong.insert("2026-05-25".to_string(), 1u64);
        assert!(find_chart_value_for_date(&wrong, date(2026, 5, 24), true).is_err());

        let mut multi = HashMap::new();
        multi.insert("2026-05-24".to_string(), 1u64);
        multi.insert("2026-05-25".to_string(), 2u64);
        assert!(find_chart_value_for_date(&multi, date(2026, 5, 24), true).is_err());
    }

    #[test]
    fn find_chart_value_for_date_lenient_matches_prefix_only_at_start() {
        let mut chart = HashMap::new();
        chart.insert("2026-05-24T00:00:00".to_string(), 11u64);
        chart.insert("2026-05-25T00:00:00".to_string(), 22u64);
        assert_eq!(
            find_chart_value_for_date_lenient(&chart, date(2026, 5, 24), false).unwrap(),
            11
        );

        let mut wrong_position = HashMap::new();
        wrong_position.insert("xyz-2026-05-24".to_string(), 1u64);
        assert!(
            find_chart_value_for_date_lenient(&wrong_position, date(2026, 5, 24), false).is_err()
        );
    }

    #[test]
    fn find_chart_value_for_date_lenient_allow_missing_returns_zero_when_not_found() {
        let mut chart = HashMap::new();
        chart.insert("2026-05-25T00:00:00".to_string(), 22u64);
        assert_eq!(
            find_chart_value_for_date_lenient(&chart, date(2026, 5, 24), true).unwrap(),
            0
        );
        assert!(find_chart_value_for_date_lenient(&chart, date(2026, 5, 24), false).is_err());

        assert_eq!(
            find_chart_value_for_date_lenient(&chart, date(2026, 5, 25), true).unwrap(),
            22
        );
    }

    #[test]
    fn sum_chart_f64_as_u64_clamps_negatives_per_entry() {
        let mut chart = HashMap::new();
        chart.insert("a".to_string(), 1.5);
        chart.insert("b".to_string(), 2.7);
        chart.insert("c".to_string(), -10.0);
        assert_eq!(sum_chart_f64_as_u64(&chart), 3);
    }

    #[test]
    fn sum_chart_values_in_range_is_inclusive_and_accepts_timestamped_keys() {
        let mut chart = HashMap::new();
        chart.insert("2026-05-23".to_string(), 1u64);
        chart.insert("2026-05-24T00:00:00".to_string(), 10u64);
        chart.insert("2026-05-25T12:00:00".to_string(), 100u64);
        chart.insert("2026-05-26".to_string(), 1000u64);
        let total = sum_chart_values_in_range(&chart, date(2026, 5, 24), date(2026, 5, 25));
        assert_eq!(total, 110);
    }

    #[test]
    fn sum_chart_values_in_range_skips_keys_shorter_than_date_prefix() {
        let mut chart = HashMap::new();
        chart.insert("foo".to_string(), 5u64);
        chart.insert("2026-05-24".to_string(), 10u64);
        let total = sum_chart_values_in_range(&chart, date(2026, 5, 24), date(2026, 5, 24));
        assert_eq!(total, 10);
    }

    static TRY_POLL_LOCK: Mutex<()> = Mutex::new(());
    static FAKE_LIST: Mutex<Option<Arc<Vec<Arc<FakeEntity>>>>> = Mutex::new(None);

    #[derive(Default, Clone, Serialize, Deserialize)]
    struct FakeDayData {
        value: u64,
    }

    impl DayData for FakeDayData {
        fn accumulate(&mut self, day: Self) {
            self.value += day.value;
        }
        fn merge_latest(&mut self, snapshot: Self) {
            self.value = snapshot.value;
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        FetchDay(NaiveDate, bool),
        FetchRange(NaiveDate, NaiveDate),
    }

    #[derive(Default)]
    struct FakeEntity {
        id: String,
        label: String,
        day_values: HashMap<NaiveDate, u64>,
        range_value: Option<u64>,
        error_on_day: Option<NaiveDate>,
        calls: Mutex<Vec<Call>>,
    }

    impl FakeEntity {
        fn new(id: &str, label: &str) -> Self {
            Self {
                id: id.to_string(),
                label: label.to_string(),
                ..Default::default()
            }
        }
        fn calls(&self) -> Vec<Call> {
            self.calls.lock().unwrap().clone()
        }
    }

    struct RecordingType;

    impl EntityType for RecordingType {
        type Entity = Arc<FakeEntity>;
        type DayData = FakeDayData;
        const LOG_LABEL: &'static str = "fake-recording";

        fn entity_id(e: &Self::Entity) -> String {
            e.id.clone()
        }
        fn entity_label(e: &Self::Entity) -> String {
            e.label.clone()
        }

        fn list(_: &ApiClient) -> FetchFuture<'_, Arc<Vec<Self::Entity>>> {
            Box::pin(async move {
                let guard = FAKE_LIST.lock().unwrap();
                Ok(guard.as_ref().expect("FAKE_LIST not set").clone())
            })
        }

        fn fetch_day<'a>(
            _: &'a ApiClient,
            e: &'a Self::Entity,
            date: NaiveDate,
            allow_missing: bool,
        ) -> FetchFuture<'a, Self::DayData> {
            Box::pin(async move {
                e.calls
                    .lock()
                    .unwrap()
                    .push(Call::FetchDay(date, allow_missing));
                if e.error_on_day == Some(date) {
                    bail!("fake error for {date}");
                }
                Ok(FakeDayData {
                    value: e.day_values.get(&date).copied().unwrap_or(0),
                })
            })
        }

        fn fetch_range<'a>(
            _: &'a ApiClient,
            e: &'a Self::Entity,
            from: NaiveDate,
            to: NaiveDate,
        ) -> FetchFuture<'a, Self::DayData> {
            Box::pin(async move {
                e.calls.lock().unwrap().push(Call::FetchRange(from, to));
                Ok(FakeDayData {
                    value: e.range_value.unwrap_or(0),
                })
            })
        }

        fn emit_metrics(_: &str, _: &str, _: &Self::DayData, _: &Self::DayData) {}
    }

    struct DefaultRangeType;

    impl EntityType for DefaultRangeType {
        type Entity = Arc<FakeEntity>;
        type DayData = FakeDayData;
        const LOG_LABEL: &'static str = "fake-default-range";

        fn entity_id(e: &Self::Entity) -> String {
            e.id.clone()
        }
        fn entity_label(e: &Self::Entity) -> String {
            e.label.clone()
        }

        fn list(_: &ApiClient) -> FetchFuture<'_, Arc<Vec<Self::Entity>>> {
            Box::pin(async move { unreachable!("list not used in these tests") })
        }

        fn fetch_day<'a>(
            _: &'a ApiClient,
            e: &'a Self::Entity,
            date: NaiveDate,
            allow_missing: bool,
        ) -> FetchFuture<'a, Self::DayData> {
            Box::pin(async move {
                e.calls
                    .lock()
                    .unwrap()
                    .push(Call::FetchDay(date, allow_missing));
                Ok(FakeDayData {
                    value: e.day_values.get(&date).copied().unwrap_or(0),
                })
            })
        }

        fn emit_metrics(_: &str, _: &str, _: &Self::DayData, _: &Self::DayData) {}
    }

    fn fake_api_client() -> Arc<ApiClient> {
        Arc::new(ApiClient::new("test", Duration::from_secs(10), Duration::from_mins(1)).unwrap())
    }

    fn set_fake_list(entities: Vec<Arc<FakeEntity>>) {
        *FAKE_LIST.lock().unwrap() = Some(Arc::new(entities));
    }

    #[tokio::test]
    async fn update_entry_no_prior_state_fetches_only_today() {
        let today = date(2026, 5, 26);
        let yesterday = date(2026, 5, 25);
        let mut entity_data = FakeEntity::new("e1", "Entity 1");
        entity_data.day_values.insert(today, 10);
        let entity = Arc::new(entity_data);
        let mut entry = EntityEntry::<RecordingType>::default();
        let client = fake_api_client();

        update_entry::<RecordingType>(&client, &entity, "e1", &mut entry, today, yesterday)
            .await
            .unwrap();

        assert_eq!(entity.calls(), vec![Call::FetchDay(today, true)]);
        assert_eq!(entry.last_complete_day, Some(yesterday));
        assert_eq!(entry.last_complete_day_state.value, 0);
        assert_eq!(entry.current_day_state.value, 10);
    }

    #[tokio::test]
    async fn update_entry_caught_up_no_backfill() {
        let today = date(2026, 5, 26);
        let yesterday = date(2026, 5, 25);
        let mut entity_data = FakeEntity::new("e1", "Entity 1");
        entity_data.day_values.insert(today, 7);
        let entity = Arc::new(entity_data);

        let mut entry = EntityEntry::<RecordingType> {
            last_complete_day: Some(yesterday),
            last_complete_day_state: FakeDayData { value: 100 },
            current_day_state: FakeDayData { value: 50 },
            ..Default::default()
        };

        let client = fake_api_client();
        update_entry::<RecordingType>(&client, &entity, "e1", &mut entry, today, yesterday)
            .await
            .unwrap();

        assert_eq!(entity.calls(), vec![Call::FetchDay(today, true)]);
        assert_eq!(entry.last_complete_day, Some(yesterday));
        assert_eq!(entry.last_complete_day_state.value, 100);
        assert_eq!(entry.current_day_state.value, 7);
    }

    #[tokio::test]
    async fn update_entry_backfills_missing_days_then_today() {
        let today = date(2026, 5, 26);
        let yesterday = date(2026, 5, 25);
        let last = date(2026, 5, 20);
        let mut entity_data = FakeEntity::new("e1", "Entity 1");
        entity_data.day_values.insert(today, 3);
        entity_data.range_value = Some(42);
        let entity = Arc::new(entity_data);

        let mut entry = EntityEntry::<RecordingType> {
            last_complete_day: Some(last),
            last_complete_day_state: FakeDayData { value: 100 },
            current_day_state: FakeDayData { value: 99 },
            ..Default::default()
        };

        let client = fake_api_client();
        update_entry::<RecordingType>(&client, &entity, "e1", &mut entry, today, yesterday)
            .await
            .unwrap();

        assert_eq!(
            entity.calls(),
            vec![
                Call::FetchRange(date(2026, 5, 21), yesterday),
                Call::FetchDay(today, true),
            ]
        );
        assert_eq!(entry.last_complete_day, Some(yesterday));
        assert_eq!(entry.last_complete_day_state.value, 142);
        assert_eq!(entry.current_day_state.value, 3);
    }

    #[tokio::test]
    async fn fetch_range_default_iterates_day_by_day_and_accumulates() {
        let today = date(2026, 5, 26);
        let yesterday = date(2026, 5, 25);
        let last = date(2026, 5, 22);
        let mut entity_data = FakeEntity::new("e1", "Entity 1");
        entity_data.day_values.insert(date(2026, 5, 23), 1);
        entity_data.day_values.insert(date(2026, 5, 24), 2);
        entity_data.day_values.insert(date(2026, 5, 25), 4);
        entity_data.day_values.insert(today, 8);
        let entity = Arc::new(entity_data);

        let mut entry = EntityEntry::<DefaultRangeType> {
            last_complete_day: Some(last),
            last_complete_day_state: FakeDayData { value: 100 },
            ..Default::default()
        };

        let client = fake_api_client();
        update_entry::<DefaultRangeType>(&client, &entity, "e1", &mut entry, today, yesterday)
            .await
            .unwrap();

        assert_eq!(
            entity.calls(),
            vec![
                Call::FetchDay(date(2026, 5, 23), false),
                Call::FetchDay(date(2026, 5, 24), false),
                Call::FetchDay(date(2026, 5, 25), false),
                Call::FetchDay(today, true),
            ]
        );
        assert_eq!(entry.last_complete_day, Some(yesterday));
        assert_eq!(entry.last_complete_day_state.value, 107);
        assert_eq!(entry.current_day_state.value, 8);
    }

    #[tokio::test]
    async fn try_poll_adds_unknown_entities() {
        let _guard = TRY_POLL_LOCK.lock().unwrap();
        let entity_a = Arc::new(FakeEntity::new("a", "Entity A"));
        let entity_b = Arc::new(FakeEntity::new("b", "Entity B"));
        set_fake_list(vec![Arc::clone(&entity_a), Arc::clone(&entity_b)]);

        let mut state = EntityStatsState::<RecordingType>::default();
        let client = fake_api_client();
        state.try_poll(client, 2).await.unwrap();

        assert!(state.entities.contains_key("a"));
        assert!(state.entities.contains_key("b"));
        assert_eq!(state.entities["a"].label, "Entity A");
        assert_eq!(state.entities["b"].label, "Entity B");
        assert_eq!(entity_a.calls().len(), 1);
        assert_eq!(entity_b.calls().len(), 1);
    }

    #[tokio::test]
    async fn try_poll_prunes_missing_entities() {
        let _guard = TRY_POLL_LOCK.lock().unwrap();
        let entity_a = Arc::new(FakeEntity::new("a", "Entity A"));
        set_fake_list(vec![Arc::clone(&entity_a)]);

        let mut state = EntityStatsState::<RecordingType>::default();
        state
            .entities
            .insert("a".to_string(), EntityEntry::default());
        state
            .entities
            .insert("stale".to_string(), EntityEntry::default());

        let client = fake_api_client();
        state.try_poll(client, 1).await.unwrap();

        assert!(state.entities.contains_key("a"));
        assert!(!state.entities.contains_key("stale"));
    }

    #[tokio::test]
    async fn poll_rolls_back_state_on_error() {
        let _guard = TRY_POLL_LOCK.lock().unwrap();
        let today = chrono::Utc::now().date_naive();
        let mut a_data = FakeEntity::new("a", "Entity A");
        a_data.error_on_day = Some(today);
        let entity_a = Arc::new(a_data);
        let entity_b = Arc::new(FakeEntity::new("b", "Entity B"));
        set_fake_list(vec![Arc::clone(&entity_a), Arc::clone(&entity_b)]);

        let mut state = EntityStatsState::<RecordingType>::default();
        state.entities.insert(
            "preexisting".to_string(),
            EntityEntry {
                label: "Before".to_string(),
                last_complete_day_state: FakeDayData { value: 7 },
                ..Default::default()
            },
        );
        state.last_entity_count = Some(99);

        let client = fake_api_client();
        let result = state.poll(client, 1).await;
        assert!(result.is_err());
        assert_eq!(state.entities.len(), 1);
        assert_eq!(state.entities["preexisting"].label, "Before");
        assert_eq!(
            state.entities["preexisting"].last_complete_day_state.value,
            7
        );
        assert_eq!(state.last_entity_count, Some(99));
    }
}
