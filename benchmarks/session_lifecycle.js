import http from "k6/http";
import { check, sleep } from "k6";
import { discoverCluster, jsonHeaders } from "./helpers.js";

export const options = {
  scenarios: {
    lifecycle: {
      executor: "ramping-vus",
      startVUs: 0,
      stages: [
        { duration: "15s", target: 25 },
        { duration: "60s", target: 50 },
        { duration: "15s", target: 0 },
      ],
    },
  },
  thresholds: {
    http_req_duration: ["p(95)<200"],
    http_req_failed: ["rate<0.01"],
  },
};

export function setup() {
  return discoverCluster();
}

export default function (data) {
  const { leader } = data;

  // Create a session
  const createRes = http.post(
    `${leader}/sessions`,
    JSON.stringify({
      subject_id: `bench-user-${__VU}`,
      ttl_seconds: 300,
      user_agent: "k6-benchmark/1.0",
    }),
    jsonHeaders,
  );

  const createOk = check(createRes, {
    "create: status 200": (r) => r.status === 200,
    "create: has token": (r) => r.json().data.token !== undefined,
  });

  if (!createOk) {
    sleep(0.1);
    return;
  }

  const { id, token } = createRes.json().data;

  // Validate the session
  const validateRes = http.post(
    `${leader}/sessions/verify`,
    JSON.stringify({ token }),
    jsonHeaders,
  );

  check(validateRes, {
    "validate: status 200": (r) => r.status === 200,
    "validate: correct id": (r) => r.json().data.id === id,
  });

  // Revoke the session
  const revokeRes = http.del(`${leader}/sessions/${id}`);

  check(revokeRes, {
    "revoke: status 200": (r) => r.status === 200,
  });

  sleep(0.1);
}
