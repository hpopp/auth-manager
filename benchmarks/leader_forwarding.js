import http from "k6/http";
import { check, sleep } from "k6";
import { Trend } from "k6/metrics";
import { discoverCluster, jsonHeaders } from "./helpers.js";

const directLatency = new Trend("direct_write_latency", true);
const forwardedLatency = new Trend("forwarded_write_latency", true);

export const options = {
  scenarios: {
    comparison: {
      executor: "constant-vus",
      vus: 20,
      duration: "60s",
    },
  },
  thresholds: {
    http_req_failed: ["rate<0.01"],
  },
};

export function setup() {
  const cluster = discoverCluster();

  if (cluster.followers.length === 0) {
    throw new Error("No followers found. Need a multi-node cluster.");
  }

  return cluster;
}

export default function (data) {
  const { leader, followers } = data;
  const follower = followers[0];

  // Write directly to leader
  const directRes = http.post(
    `${leader}/sessions`,
    JSON.stringify({
      subject_id: `direct-${__VU}-${__ITER}`,
      ttl_seconds: 60,
    }),
    jsonHeaders,
  );

  check(directRes, {
    "direct: status 200": (r) => r.status === 200,
  });

  if (directRes.status === 200) {
    directLatency.add(directRes.timings.duration);
  }

  // Write to follower (forwarded to leader via middleware)
  const forwardedRes = http.post(
    `${follower}/sessions`,
    JSON.stringify({
      subject_id: `forwarded-${__VU}-${__ITER}`,
      ttl_seconds: 60,
    }),
    jsonHeaders,
  );

  check(forwardedRes, {
    "forwarded: status 200": (r) => r.status === 200,
  });

  if (forwardedRes.status === 200) {
    forwardedLatency.add(forwardedRes.timings.duration);
  }

  sleep(0.1);
}
