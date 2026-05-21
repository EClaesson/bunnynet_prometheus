use std::path::Path;

use anyhow::Result;
use clap::ValueEnum;

use crate::dns_zone::DnsZoneStatsState;
use crate::edge_script::EdgeScriptStatsState;
use crate::pull_zone::PullZoneStatsState;
use crate::pull_zone_optimizer::PullZoneOptimizerStatsState;
use crate::pull_zone_origin_shield_queue::PullZoneOriginShieldQueueStatsState;
use crate::pull_zone_safehop::PullZoneSafeHopStatsState;
use crate::shield_zone::ShieldZoneStatsState;
use crate::state::{State, read_state_from_file, write_state_to_file};
use crate::storage_zone::StorageZoneStatsState;
use crate::video_library::VideoLibraryStatsState;
use crate::video_library_drm::VideoLibraryDrmStatsState;
use crate::video_library_transcribing::VideoLibraryTranscribingStatsState;

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum Collector {
    DnsZone,
    EdgeScript,
    StorageZone,
    VideoLibrary,
    VideoLibraryTranscribing,
    VideoLibraryDrm,
    PullZone,
    PullZoneOptimizer,
    PullZoneOriginShieldQueue,
    PullZoneSafehop,
    ShieldZone,
}

impl Collector {
    pub fn state_file_name(self) -> String {
        let name = self.name();
        format!("{name}.json")
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::DnsZone => "dns_zone",
            Self::EdgeScript => "edge_script",
            Self::StorageZone => "storage_zone",
            Self::VideoLibrary => "video_library",
            Self::VideoLibraryTranscribing => "video_library_transcribing",
            Self::VideoLibraryDrm => "video_library_drm",
            Self::PullZone => "pull_zone",
            Self::PullZoneOptimizer => "pull_zone_optimizer",
            Self::PullZoneOriginShieldQueue => "pull_zone_origin_shield_queue",
            Self::PullZoneSafehop => "pull_zone_safehop",
            Self::ShieldZone => "shield_zone",
        }
    }

    pub fn load_state(self, state_dir: &Path) -> Result<Box<dyn State>> {
        match self {
            Self::DnsZone => Ok(Box::new(read_state_from_file::<DnsZoneStatsState>(
                state_dir,
                &self.state_file_name(),
            )?)),
            Self::EdgeScript => Ok(Box::new(read_state_from_file::<EdgeScriptStatsState>(
                state_dir,
                &self.state_file_name(),
            )?)),
            Self::StorageZone => Ok(Box::new(read_state_from_file::<StorageZoneStatsState>(
                state_dir,
                &self.state_file_name(),
            )?)),
            Self::VideoLibrary => Ok(Box::new(read_state_from_file::<VideoLibraryStatsState>(
                state_dir,
                &self.state_file_name(),
            )?)),
            Self::VideoLibraryTranscribing => Ok(Box::new(read_state_from_file::<
                VideoLibraryTranscribingStatsState,
            >(
                state_dir, &self.state_file_name()
            )?)),
            Self::VideoLibraryDrm => {
                Ok(Box::new(read_state_from_file::<VideoLibraryDrmStatsState>(
                    state_dir,
                    &self.state_file_name(),
                )?))
            }
            Self::PullZone => Ok(Box::new(read_state_from_file::<PullZoneStatsState>(
                state_dir,
                &self.state_file_name(),
            )?)),
            Self::PullZoneOptimizer => Ok(Box::new(read_state_from_file::<
                PullZoneOptimizerStatsState,
            >(
                state_dir, &self.state_file_name()
            )?)),
            Self::PullZoneOriginShieldQueue => Ok(Box::new(read_state_from_file::<
                PullZoneOriginShieldQueueStatsState,
            >(
                state_dir, &self.state_file_name()
            )?)),
            Self::PullZoneSafehop => Ok(Box::new(read_state_from_file::<
                PullZoneSafeHopStatsState,
            >(
                state_dir, &self.state_file_name()
            )?)),
            Self::ShieldZone => Ok(Box::new(read_state_from_file::<ShieldZoneStatsState>(
                state_dir,
                &self.state_file_name(),
            )?)),
        }
    }

    pub fn save_state(self, state: &dyn State, state_dir: &Path) -> Result<()> {
        write_state_to_file(state_dir, &self.state_file_name(), &state.serialize()?)
    }
}
