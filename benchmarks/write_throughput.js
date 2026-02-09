import http from "k6/http";
import { check } from "k6";
import { Counter } from "k6/metrics";
import { discoverCluster, jsonHeaders } from "./helpers.js";

const sessionsCreated = new Counter("sessions_created");

export const options = {
  scenarios: {
    writes: {
      executor: "constant-vus",
      vus: 100,
      duration: "60s",
    },
  },
  thresholds: {
    http_req_duration: ["p(95)<200", "p(99)<500"],
    http_req_failed: ["rate<0.01"],
  },
};

export function setup() {
  return discoverCluster();
}

export default function (data) {
  const { leader } = data;

  const res = http.post(
    `${leader}/sessions`,
    JSON.stringify({
      subject_id: `bench-user-${__VU}`,
      ttl_seconds: 60,
      user_agent: "k6-write-bench/1.0",
    }),
    jsonHeaders,
  );

  const ok = check(res, {
    "status 200": (r) => r.status === 200,
  });

  if (ok) {
    sessionsCreated.add(1);
  }
}
