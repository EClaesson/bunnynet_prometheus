use std::sync::Arc;

use anyhow::Context;
use chrono::NaiveDate;
use metrics::counter;
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, CountryViewCounts, CountryWatchTime, VideoLibrary};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, emit_labeled_counter,
    find_chart_value_for_date, sum_chart_values,
};

pub type VideoLibraryStatsState = EntityStatsState<VideoLibraryKind>;

pub struct VideoLibraryKind;

impl EntityType for VideoLibraryKind {
    type Entity = VideoLibrary;
    type DayData = VideoLibraryDayData;

    const LOG_LABEL: &'static str = "video_library";

    fn entity_id(entity: &VideoLibrary) -> String {
        entity.id.to_string()
    }

    fn entity_label(entity: &VideoLibrary) -> String {
        entity.name.clone()
    }

    fn list(api_client: &ApiClient) -> FetchFuture<'_, Arc<Vec<VideoLibrary>>> {
        Box::pin(async move { api_client.list_video_libraries().await })
    }

    fn fetch_day<'a>(
        api_client: &'a ApiClient,
        library: &'a VideoLibrary,
        date: NaiveDate,
        allow_missing: bool,
    ) -> FetchFuture<'a, VideoLibraryDayData> {
        Box::pin(async move {
            let statistics = api_client
                .get_video_library_stats(library.id, Some(&library.read_only_api_key), date, date)
                .await?;

            let views = find_chart_value_for_date(&statistics.views_chart, date, allow_missing)
                .context("views")?;
            let watch_time =
                find_chart_value_for_date(&statistics.watch_time_chart, date, allow_missing)
                    .context("watch_time")?;

            Ok(VideoLibraryDayData {
                views,
                watch_time,
                country_views: statistics.country_view_counts,
                country_watch_time: statistics.country_watch_time,
            })
        })
    }

    fn fetch_range<'a>(
        api_client: &'a ApiClient,
        library: &'a VideoLibrary,
        from: NaiveDate,
        to: NaiveDate,
    ) -> FetchFuture<'a, VideoLibraryDayData> {
        Box::pin(async move {
            let statistics = api_client
                .get_video_library_stats(library.id, Some(&library.read_only_api_key), from, to)
                .await?;

            Ok(VideoLibraryDayData {
                views: sum_chart_values(&statistics.views_chart),
                watch_time: sum_chart_values(&statistics.watch_time_chart),
                country_views: statistics.country_view_counts,
                country_watch_time: statistics.country_watch_time,
            })
        })
    }

    fn emit_metrics(
        id: &str,
        name: &str,
        last: &VideoLibraryDayData,
        current: &VideoLibraryDayData,
    ) {
        let labels = [("library_id", id.to_string()), ("name", name.to_string())];

        counter!("bunnynet.video_library.views", &labels).absolute(last.views + current.views);
        counter!("bunnynet.video_library.watch_time", &labels)
            .absolute(last.watch_time + current.watch_time);

        emit_labeled_counter(
            "bunnynet.video_library.country_views",
            &last.country_views,
            &current.country_views,
            "country",
            &labels,
        );

        emit_labeled_counter(
            "bunnynet.video_library.country_watch_time",
            &last.country_watch_time,
            &current.country_watch_time,
            "country",
            &labels,
        );
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct VideoLibraryDayData {
    pub views: u64,
    pub watch_time: u64,
    pub country_views: CountryViewCounts,
    pub country_watch_time: CountryWatchTime,
}

impl DayData for VideoLibraryDayData {
    fn accumulate(&mut self, day: Self) {
        self.views += day.views;
        self.watch_time += day.watch_time;
        for (country, value) in day.country_views {
            *self.country_views.entry(country).or_default() += value;
        }
        for (country, value) in day.country_watch_time {
            *self.country_watch_time.entry(country).or_default() += value;
        }
    }

    fn merge_latest(&mut self, snapshot: Self) {
        self.views = self.views.max(snapshot.views);
        self.watch_time = self.watch_time.max(snapshot.watch_time);
        for (country, value) in snapshot.country_views {
            let entry = self.country_views.entry(country).or_default();
            *entry = (*entry).max(value);
        }
        for (country, value) in snapshot.country_watch_time {
            let entry = self.country_watch_time.entry(country).or_default();
            *entry = (*entry).max(value);
        }
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

    fn data(
        views: u64,
        watch_time: u64,
        views_per_country: &[(&str, u64)],
        watch_time_per_country: &[(&str, u64)],
    ) -> VideoLibraryDayData {
        let mut country_views = CountryViewCounts::new();
        for (k, v) in views_per_country {
            country_views.insert((*k).to_string(), *v);
        }
        let mut country_watch_time = CountryWatchTime::new();
        for (k, v) in watch_time_per_country {
            country_watch_time.insert((*k).to_string(), *v);
        }
        VideoLibraryDayData {
            views,
            watch_time,
            country_views,
            country_watch_time,
        }
    }

    #[test]
    fn accumulate_sums_counters_and_maps() {
        let mut state = data(10, 100, &[("US", 5), ("DE", 2)], &[("US", 50)]);
        state.accumulate(data(3, 30, &[("US", 1), ("FR", 7)], &[("DE", 8)]));
        assert_eq!(state.views, 13);
        assert_eq!(state.watch_time, 130);
        assert_eq!(state.country_views.get("US"), Some(&6));
        assert_eq!(state.country_views.get("DE"), Some(&2));
        assert_eq!(state.country_views.get("FR"), Some(&7));
        assert_eq!(state.country_watch_time.get("US"), Some(&50));
        assert_eq!(state.country_watch_time.get("DE"), Some(&8));
    }

    #[test]
    fn merge_latest_takes_max_of_counters() {
        let mut state = data(50, 500, &[], &[]);
        state.merge_latest(data(30, 1000, &[], &[]));
        assert_eq!(state.views, 50);
        assert_eq!(state.watch_time, 1000);
    }

    #[test]
    fn merge_latest_maps_take_max_per_key_and_keep_existing() {
        let mut state = data(0, 0, &[("US", 10), ("DE", 3)], &[("US", 100)]);
        state.merge_latest(data(
            0,
            0,
            &[("US", 5), ("FR", 7)],
            &[("US", 80), ("DE", 9)],
        ));
        assert_eq!(state.country_views.get("US"), Some(&10));
        assert_eq!(state.country_views.get("DE"), Some(&3));
        assert_eq!(state.country_views.get("FR"), Some(&7));
        assert_eq!(state.country_watch_time.get("US"), Some(&100));
        assert_eq!(state.country_watch_time.get("DE"), Some(&9));
    }
}
