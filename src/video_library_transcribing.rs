use anyhow::Context;
use chrono::NaiveDate;
use metrics::counter;
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, VideoLibrary};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, f64_to_u64, find_chart_value_for_date,
};

pub type VideoLibraryTranscribingStatsState = EntityStatsState<VideoLibraryTranscribingKind>;

pub struct VideoLibraryTranscribingKind;

impl EntityType for VideoLibraryTranscribingKind {
    type Entity = VideoLibrary;
    type DayData = VideoLibraryTranscribingDayData;

    const LOG_LABEL: &'static str = "Video library transcribing";

    fn entity_id(entity: &VideoLibrary) -> u64 {
        entity.id
    }

    fn entity_label(entity: &VideoLibrary) -> &str {
        &entity.name
    }

    fn list(client: &ApiClient) -> FetchFuture<'_, Vec<VideoLibrary>> {
        Box::pin(async move { client.list_video_libraries().await })
    }

    fn fetch_day<'a>(
        client: &'a ApiClient,
        library: &'a VideoLibrary,
        date: NaiveDate,
    ) -> FetchFuture<'a, VideoLibraryTranscribingDayData> {
        Box::pin(async move {
            let stats = client
                .get_video_library_transcribing_stats(library.id, date, date)
                .await?;

            let transcription_seconds = f64_to_u64(
                find_chart_value_for_date(&stats.transcription_seconds_chart, date)
                    .context("Transcription seconds")?,
            );

            Ok(VideoLibraryTranscribingDayData {
                transcription_seconds,
            })
        })
    }

    fn emit_metrics(
        id: u64,
        name: &str,
        last: &VideoLibraryTranscribingDayData,
        current: &VideoLibraryTranscribingDayData,
    ) {
        let labels = [("library_id", id.to_string()), ("name", name.to_string())];

        counter!("bunnynet.video_library_transcribing.seconds", &labels)
            .absolute(last.transcription_seconds + current.transcription_seconds);
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct VideoLibraryTranscribingDayData {
    pub transcription_seconds: u64,
}

impl DayData for VideoLibraryTranscribingDayData {
    fn accumulate(&mut self, day: Self) {
        self.transcription_seconds += day.transcription_seconds;
    }

    fn merge_latest(&mut self, snap: Self) {
        self.transcription_seconds = self.transcription_seconds.max(snap.transcription_seconds);
    }
}
