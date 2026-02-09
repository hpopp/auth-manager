import http from "k6/http";
import { check } from "k6";
import { Counter } from "k6/metrics";
import { discoverCluster, jsonHeaders } from "./helpers.js";

const validationsPerformed = new Counter("validations_performed");

const SEED_COUNT = 200;

export const options = {
  scenarios: {
    reads: {
      executor: "constant-vus",
      vus: 100,
      duration: "60s",
    },
  },
  thresholds: {
    http_req_duration: ["p(95)<100", "p(99)<200"],
    http_req_failed: ["rate<0.05"],
  },
};

export function setup() {
  const cluster = discoverCluster();

  // Seed sessions on the leader for followers to read
  const tokens = [];
  for (let i = 0; i < SEED_COUNT; i++) {
    const res = http.post(
      `${cluster.leader}/sessions`,
      JSON.stringify({
        subject_id: `seed-user-${i}`,
        ttl_seconds: 600,
        user_agent: "k6-seed/1.0",
      }),
      jsonHeaders,
    );

    if (res.status === 200) {
      tokens.push(res.json().data.token);
    }
  }

  console.log(`Seeded ${tokens.length} sessions`);

  // Brief pause for replication to propagate
  // k6 doesn't have a sleep in setup, so we use a sync http call as a delay
  http.get(`${cluster.leader}/_internal/health`);

  return { ...cluster, tokens };
}

export default function (data) {
  const { followers, tokens } = data;

  if (tokens.length === 0) return;

  // Pick a random follower and token
  const follower = followers[Math.floor(Math.random() * followers.length)];
  const token = tokens[Math.floor(Math.random() * tokens.length)];

  const res = http.post(
    `${follower}/sessions/verify`,
    JSON.stringify({ token }),
    jsonHeaders,
  );

  const ok = check(res, {
    "status 200": (r) => r.status === 200,
  });

  if (ok) {
    validationsPerformed.add(1);
  }
}
