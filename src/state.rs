use std::future::Future;
use std::io::{self, Write};
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use serde::de::DeserializeOwned;
use tracing::{debug, info};

use crate::bunny::ApiClient;

pub type PollFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

pub trait State: Send {
    fn poll(&mut self, client: Arc<ApiClient>, concurrency: usize) -> PollFuture<'_>;
    fn serialize(&self) -> Result<String>;
}

pub fn read_state_from_file<T>(state_dir: &Path, file_name: &str) -> Result<T>
where
    T: DeserializeOwned + Default,
{
    let path = state_dir.join(file_name);

    match std::fs::read_to_string(&path) {
        Ok(json) => Ok(serde_json::from_str(&json)?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            info!(
                path = %path.display(),
                "No prior state found, starting fresh"
            );
            Ok(T::default())
        }
        Err(e) => Err(e.into()),
    }
}

pub fn write_state_to_file(state_dir: &Path, file_name: &str, json: &str) -> Result<()> {
    let tmp_path = state_dir.join(format!("{file_name}.tmp"));
    let path = state_dir.join(file_name);

    debug!(path = %path.display(), "Writing state to file");
    let mut file = std::fs::File::create(&tmp_path)?;
    file.write_all(json.as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp_path, path)?;
    std::fs::File::open(state_dir)?.sync_all()?;

    Ok(())
}
