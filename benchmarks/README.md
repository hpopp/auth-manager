# Benchmarks

Load tests for auth-manager using [k6](https://k6.io/).

## Prerequisites

- [k6](https://k6.io/docs/get-started/installation/) installed locally
- Docker Compose cluster running (`docker compose up -d --build`)
- Cluster stabilized (wait ~15 seconds for leader election)

## Scripts

| Script                 | Description                                           | VUs   | Duration |
| ---------------------- | ----------------------------------------------------- | ----- | -------- |
| `session_lifecycle.js` | Create, validate, and revoke sessions                 | 10-50 | 90s      |
| `api_key_lifecycle.js` | Create, validate, update, and revoke API keys         | 10-50 | 90s      |
| `write_throughput.js`  | Peak write throughput (session creates on leader)     | 100   | 60s      |
| `read_throughput.js`   | Peak read throughput (session validates on followers) | 100   | 60s      |
| `leader_forwarding.js` | Compare direct vs forwarded write latency             | 20    | 60s      |

## Usage

```bash
# Start the cluster
docker compose up -d --build

# Wait for leader election
sleep 15

# Run a single benchmark
k6 run benchmarks/session_lifecycle.js

# Run all benchmarks
for f in benchmarks/*.js; do
  [ "$(basename "$f")" = "helpers.js" ] && continue
  echo "--- Running $f ---"
  k6 run "$f"
  echo ""
done
```

## Thresholds

Each script includes built-in pass/fail thresholds:

- **Reads**: p95 < 100ms
- **Writes**: p95 < 200ms
- **Error rate**: < 1%

## Custom Metrics

- `write_throughput.js` reports `sessions_created` (total writes)
- `read_throughput.js` reports `validations_performed` (total reads)
- `leader_forwarding.js` reports `direct_write_latency` and `forwarded_write_latency` for comparison

## Notes

- All scripts auto-discover the leader node via `/_internal/cluster/status`
- `read_throughput.js` seeds 200 sessions before running, then validates against follower nodes
- `leader_forwarding.js` requires at least one follower node
- After `write_throughput.js`, consider purging data: `curl -X DELETE localhost:8081/admin/purge`
