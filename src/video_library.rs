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
    ) -> FetchFuture<'a, VideoLibraryDayData> {
        Box::pin(async move {
            let statistics = api_client
                .get_video_library_stats(library.id, Some(&library.read_only_api_key), date, date)
                .await?;

            let views =
                find_chart_value_for_date(&statistics.views_chart, date).context("views")?;
            let watch_time = find_chart_value_for_date(&statistics.watch_time_chart, date)
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
