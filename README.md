# bunnynet_prometheus

Expose [Bunny.net](https://bunny.net) statistics as a scrapable Prometheus endpoint.

_bunnynet_prometheus_ is a daemon that periodically polls the [Bunny.net API](https://docs.bunny.net/api-reference) for usage and performance statistics and re-exposes it as Prometheus metrics on an HTTP endpoint.

All available statistics endpoints in the Bunny.net API are covered: Magic Containers, DNS zones, Edge scripts, Storage zones, Video libraries, Video transcribing, Video DRM, Pull zones, Pull zone optimizers, Pull zone origin shield queues, Pull zone SafeHops and Shield zones.
Each category of statistics is implemented as a separate collector that can be individually enabled. Details about available collectors and the metrics they emit can be found under [Collectors](#collectors).

Counter values are persisted to disk between poll cycles, so restarts don't reset totals and day rollovers are correctly handled. If the daemon has been stopped for an extended period, missed days will be backfilled.

## Installation

_bunnynet_prometheus_ can be compiled with cargo or you can download prebuilt [releases](https://github.com/EClaesson/bunnynet_prometheus/releases).

It is also available on [crates.io](https://crates.io/crates/bunnynet_prometheus):

```
cargo install bunnynet_prometheus
```

## Usage

```
bunnynet_prometheus [OPTIONS] --collectors <COLLECTORS>...

Options:
  -v, --verbose
          Enable verbose output
  -q, --quiet
          Only output warnings and errors
  -k, --access-key <ACCESS_KEY>
          Bunny.net API access key (Can also be set by environment variable BUNNYNET_ACCESS_KEY)
  -f, --access-key-file <ACCESS_KEY_FILE>
          Path to a file containing a Bunny.net API access key
  -r, --api-request-timeout <API_REQUEST_TIMEOUT>
          Timeout in seconds for Bunny.net API requests [default: 10]
  -i, --poll-interval <POLL_INTERVAL>
          Update interval in seconds [default: 300]
  -s, --state-dir <STATE_DIR>
          Path to a directory to store persistent state files in [default: ~/.local/share/bunnynet_prometheus/state]
  -a, --bind-addr <BIND_ADDR>
          HTTP server bind address [default: 0.0.0.0]
  -p, --bind-port <BIND_PORT>
          HTTP server bind port [default: 9000]
  -c, --collectors <COLLECTORS>...
          Comma-separated list of categories of statistics to poll [possible values: application, dns_zone, edge_script, storage_zone, video_library, video_library_transcribing, video_library_drm, pull_zone, pull_zone_optimizer, pull_zone_origin_shield_queue, pull_zone_safehop, shield_zone]
  -h, --help
          Print help
  -V, --version
          Print version
```

You can create your API access key in the [Bunny.net dashboard](https://dash.bunny.net/account/api-key).

The HTTP endpoint will respond on any path.

### Example

Load the access key from a file, poll the dns_zone and storage_zone collectors and expose metrics on the default port (9000).

```
bunnynet_prometheus -f ~/.bunnynet.key -c dns_zone,storage_zone
```

## Internal metrics

These metrics are always enabled and expose the health and age of the collectors.

| Name                                                | Type  | Tags      |
| --------------------------------------------------- | ----- | --------- |
| `bunnynet_last_update_attempt_timestamp_seconds`    | Gauge |           |
| `bunnynet_last_successful_update_timestamp_seconds` | Gauge |           |
| `bunnynet_last_collector_update_timestamp_seconds`  | Gauge | collector |

## Collectors

### application

| Name                                   | Type    | Tags                 |
| -------------------------------------- | ------- | -------------------- |
| `bunnynet_application_target_latency`  | Gauge   | app_id, name         |
| `bunnynet_application_active_regions`  | Gauge   | app_id, name         |
| `bunnynet_application_latency`         | Gauge   | app_id, name         |
| `bunnynet_application_instances`       | Gauge   | app_id, name         |
| `bunnynet_application_cpu_usage`       | Gauge   | app_id, name         |
| `bunnynet_application_ram_usage`       | Gauge   | app_id, name         |
| `bunnynet_application_traffic`         | Counter | app_id, name         |
| `bunnynet_application_volume_usage`    | Gauge   | app_id, name, volume |
| `bunnynet_application_volume_capacity` | Gauge   | app_id, name, volume |

### dns_zone

| Name                                | Type    | Tags                  |
| ----------------------------------- | ------- | --------------------- |
| `bunnynet_dns_zone_normal_queries`  | Counter | zone_id, domain       |
| `bunnynet_dns_zone_smart_queries`   | Counter | zone_id, domain       |
| `bunnynet_dns_zone_queries_by_type` | Counter | zone_id, domain, type |

### edge_script

| Name                                    | Type    | Tags            |
| --------------------------------------- | ------- | --------------- |
| `bunnynet_edge_script_requests_served`  | Counter | script_id, name |
| `bunnynet_edge_script_cpu_time`         | Counter | script_id, name |
| `bunnynet_edge_script_average_cpu_time` | Gauge   | script_id, name |

### storage_zone

| Name                                 | Type  | Tags          |
| ------------------------------------ | ----- | ------------- |
| `bunnynet_storage_zone_storage_used` | Gauge | zone_id, name |
| `bunnynet_storage_zone_file_count`   | Gauge | zone_id, name |

### video_library

| Name                                        | Type    | Tags                      |
| ------------------------------------------- | ------- | ------------------------- |
| `bunnynet_video_library_views`              | Counter | library_id, name          |
| `bunnynet_video_library_watch_time`         | Counter | library_id, name          |
| `bunnynet_video_library_country_views`      | Counter | library_id, name, country |
| `bunnynet_video_library_country_watch_time` | Counter | library_id, name, country |

### video_library_drm

| Name                                         | Type    | Tags             |
| -------------------------------------------- | ------- | ---------------- |
| `bunnynet_video_library_drm_licenses_issued` | Counter | library_id, name |

### video_library_transcribing

| Name                                          | Type    | Tags             |
| --------------------------------------------- | ------- | ---------------- |
| `bunnynet_video_library_transcribing_seconds` | Counter | library_id, name |

### pull_zone

| Name                                                       | Type    | Tags                  |
| ---------------------------------------------------------- | ------- | --------------------- |
| `bunnynet_pull_zone_bandwidth_used`                        | Counter | zone_id, name         |
| `bunnynet_pull_zone_bandwidth_cached`                      | Counter | zone_id, name         |
| `bunnynet_pull_zone_requests_served`                       | Counter | zone_id, name         |
| `bunnynet_pull_zone_pull_requests_pulled`                  | Counter | zone_id, name         |
| `bunnynet_pull_zone_origin_shield_bandwidth_used`          | Counter | zone_id, name         |
| `bunnynet_pull_zone_origin_shield_internal_bandwidth_used` | Counter | zone_id, name         |
| `bunnynet_pull_zone_origin_traffic`                        | Counter | zone_id, name         |
| `bunnynet_pull_zone_errors_3xx`                            | Counter | zone_id, name         |
| `bunnynet_pull_zone_errors_4xx`                            | Counter | zone_id, name         |
| `bunnynet_pull_zone_errors_5xx`                            | Counter | zone_id, name         |
| `bunnynet_pull_zone_origin_response_time`                  | Gauge   | zone_id, name         |
| `bunnynet_pull_zone_cache_hit_rate`                        | Gauge   | zone_id, name         |
| `bunnynet_pull_zone_geo_traffic`                           | Counter | zone_id, name, region |

### pull_zone_optimizer

| Name                                                   | Type    | Tags          |
| ------------------------------------------------------ | ------- | ------------- |
| `bunnynet_pull_zone_optimizer_requests_optimized`      | Counter | zone_id, name |
| `bunnynet_pull_zone_optimizer_traffic_saved`           | Counter | zone_id, name |
| `bunnynet_pull_zone_optimizer_average_compression`     | Gauge   | zone_id, name |
| `bunnynet_pull_zone_optimizer_average_processing_time` | Gauge   | zone_id, name |

### pull_zone_origin_shield_queue

| Name                                                         | Type  | Tags          |
| ------------------------------------------------------------ | ----- | ------------- |
| `bunnynet_pull_zone_origin_shield_queue_concurrent_requests` | Gauge | zone_id, name |
| `bunnynet_pull_zone_origin_shield_queue_queued_requests`     | Gauge | zone_id, name |

### pull_zone_safehop

| Name                                          | Type    | Tags          |
| --------------------------------------------- | ------- | ------------- |
| `bunnynet_pull_zone_safehop_requests_retried` | Counter | zone_id, name |
| `bunnynet_pull_zone_safehop_requests_saved`   | Counter | zone_id, name |

### shield_zone

| Name                                                | Type    | Tags                                           |
| --------------------------------------------------- | ------- | ---------------------------------------------- |
| `bunnynet_shield_zone_requests`                     | Counter | shield_zone_id, pull_zone_id, category, action |
| `bunnynet_shield_zone_clean_requests_limit`         | Gauge   | shield_zone_id, pull_zone_id                   |
| `bunnynet_shield_zone_billable_requests_this_month` | Gauge   | shield_zone_id, pull_zone_id                   |
