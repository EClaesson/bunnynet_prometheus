mod application;
mod bunny;
mod cli;
mod collector;
mod dns_zone;
mod edge_script;
mod entity_stats;
mod pull_zone;
mod pull_zone_optimizer;
mod pull_zone_origin_shield_queue;
mod pull_zone_safehop;
mod shield_zone;
mod state;
mod storage_zone;
mod video_library;
mod video_library_drm;
mod video_library_transcribing;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Parser;
use metrics::gauge;
use metrics_exporter_prometheus::PrometheusBuilder;
use tracing::{debug, error, info, warn};

use crate::bunny::ApiClient;
use crate::cli::CliArgs;
use crate::collector::Collector;
use crate::state::State;

const ENV_ACCESS_KEY: &str = "BUNNYNET_ACCESS_KEY";

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let args = CliArgs::parse();

    let log_filter = if args.quiet {
        tracing_subscriber::EnvFilter::new("warn")
    } else if args.verbose {
        tracing_subscriber::EnvFilter::new(
            "debug,reqwest=info,hyper=info,hyper_util=info,h2=info,rustls=info,reqwest_middleware=info,reqwest_retry=info",
        )
    } else {
        tracing_subscriber::EnvFilter::new("info")
    };

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(log_filter)
        .with_target(false)
        .init();

    match run_server(&args).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            error!("Server failed: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run_server(args: &CliArgs) -> Result<()> {
    let poll_interval = Duration::from_secs(args.poll_interval);
    let access_key = resolve_access_key(args)?;
    let api_request_timeout = Duration::from_secs(args.api_request_timeout);
    let client = Arc::new(create_api_client(
        &access_key,
        api_request_timeout,
        poll_interval,
    )?);
    std::fs::create_dir_all(&args.state_dir)
        .with_context(|| format!("Failed to create state dir: {}", args.state_dir.display()))?;
    start_prometheus_listener(&args.bind_addr, args.bind_port)?;
    start_poller_loop(
        client,
        &args.collectors,
        &args.state_dir,
        poll_interval,
        args.api_concurrency,
    )
    .await?;

    Ok(())
}

fn create_api_client(
    access_key: &str,
    api_request_timeout: Duration,
    poll_interval: Duration,
) -> Result<ApiClient> {
    ApiClient::new(access_key, api_request_timeout, poll_interval)
        .context("Failed to create Bunny.net API client")
}

fn start_prometheus_listener(bind_addr: &str, bind_port: u16) -> Result<()> {
    info!(
        bind_addr,
        bind_port, "Starting Prometheus HTTP endpoint listener"
    );
    PrometheusBuilder::new()
        .with_http_listener(std::net::SocketAddr::new(bind_addr.parse()?, bind_port))
        .with_recommended_naming(true)
        .install()?;

    Ok(())
}

async fn start_poller_loop(
    client: Arc<ApiClient>,
    collectors: &[Collector],
    state_dir: &Path,
    poll_interval: Duration,
    api_concurrency: usize,
) -> Result<()> {
    let mut states: Vec<(Collector, Box<dyn State>)> = collectors
        .iter()
        .map(|c| c.load_state(state_dir).map(|s| (*c, s)))
        .collect::<Result<_>>()?;

    let enabled: Vec<&str> = collectors.iter().map(|c| c.name()).collect();
    info!(
        poll_interval_seconds = poll_interval.as_secs(),
        api_concurrency,
        state_dir = %state_dir.display(),
        collectors = enabled.join(","),
        "Starting bunny.net poller"
    );
    let mut interval = tokio::time::interval(poll_interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut shutdown = std::pin::pin!(shutdown_signal());
    let mut cycle: u64 = 0;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                cycle += 1;
                let cycle_start = std::time::Instant::now();
                debug!(cycle, "Poll cycle starting");
                let total = states.len();
                let mut succeeded: usize = 0;

                for (collector, state) in &mut states {
                    if let Err(e) = state.poll(Arc::clone(&client), api_concurrency).await {
                        error!(collector = collector.name(), "Poll failed: {e:#}");
                        continue;
                    }

                    if let Err(e) = collector.save_state(state.as_ref(), state_dir) {
                        warn!(collector = collector.name(), "Failed to save state. Metrics may be incorrect after program restart: {e:#}");
                    } else {
                        succeeded += 1;
                    }

                    gauge!("bunnynet.last_collector_update.timestamp_seconds", "collector" => collector.name()).set(now());
                }

                gauge!("bunnynet.last_update_attempt.timestamp_seconds").set(now());
                if succeeded == total {
                    gauge!("bunnynet.last_successful_update.timestamp_seconds").set(now());
                }

                debug!(
                    cycle,
                    succeeded,
                    total,
                    elapsed_seconds = cycle_start.elapsed().as_secs_f64(),
                    "Poll cycle complete",
                );

                client.clear_cycle_cache();
            }
            () = &mut shutdown => {
                info!("Shutdown signal received, exiting");
                break;
            }
        }
    }

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}

fn resolve_access_key(args: &CliArgs) -> Result<String> {
    let raw = if let Some(key) = &args.access_key {
        debug!("Read access key from cli parameter");
        key.clone()
    } else if let Some(path) = &args.access_key_file {
        debug!("Read access key from file");
        std::fs::read_to_string(path).context("Failed to read access key from file")?
    } else if let Ok(key) = std::env::var(ENV_ACCESS_KEY) {
        debug!("Read access key from env");
        key
    } else {
        bail!("No Bunny.net API access key specified. Run with --help to show help.");
    };

    let key = raw.trim();
    if key.is_empty() {
        bail!("Bunny.net API access key is empty");
    }

    Ok(key.to_string())
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
