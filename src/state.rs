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

    #[cfg(unix)]
    std::fs::File::open(state_dir)?.sync_all()?;

    Ok(())
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
    use serde::{Deserialize, Serialize};
    use std::path::PathBuf;

    #[derive(Default, Serialize, Deserialize, PartialEq, Eq, Debug)]
    struct Sample {
        n: u64,
        name: String,
    }

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("bunnynet_prometheus_tests")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_then_read_roundtrips() {
        let dir = test_dir("write_then_read_roundtrips");
        let original = Sample {
            n: 42,
            name: "hello".to_string(),
        };
        let json = serde_json::to_string(&original).unwrap();
        write_state_to_file(&dir, "sample.json", &json).unwrap();
        let read: Sample = read_state_from_file(&dir, "sample.json").unwrap();
        assert_eq!(read, original);
    }

    #[test]
    fn read_missing_file_returns_default() {
        let dir = test_dir("read_missing_file_returns_default");
        let read: Sample = read_state_from_file(&dir, "missing.json").unwrap();
        assert_eq!(read, Sample::default());
    }

    #[test]
    fn write_leaves_no_tmp_file_on_success() {
        let dir = test_dir("write_leaves_no_tmp_file_on_success");
        write_state_to_file(&dir, "sample.json", r#"{"n":1,"name":"x"}"#).unwrap();
        assert!(dir.join("sample.json").exists());
        assert!(!dir.join("sample.json.tmp").exists());
    }

    #[test]
    fn write_replaces_existing_file() {
        let dir = test_dir("write_replaces_existing_file");
        write_state_to_file(&dir, "sample.json", r#"{"n":1,"name":"a"}"#).unwrap();
        write_state_to_file(&dir, "sample.json", r#"{"n":2,"name":"b"}"#).unwrap();
        let read: Sample = read_state_from_file(&dir, "sample.json").unwrap();
        assert_eq!(
            read,
            Sample {
                n: 2,
                name: "b".to_string()
            }
        );
    }
}
