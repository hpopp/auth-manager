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

  // Create an API key
  const createRes = http.post(
    `${leader}/api-keys`,
    JSON.stringify({
      name: `bench-key-${__VU}-${__ITER}`,
      subject_id: `bench-user-${__VU}`,
      description: "Benchmark test key",
      scopes: ["read", "write"],
    }),
    jsonHeaders,
  );

  const createOk = check(createRes, {
    "create: status 200": (r) => r.status === 200,
    "create: has key": (r) => r.json().data.key !== undefined,
  });

  if (!createOk) {
    sleep(0.1);
    return;
  }

  const { id, key } = createRes.json().data;

  // Validate the API key
  const validateRes = http.post(
    `${leader}/api-keys/verify`,
    JSON.stringify({ key }),
    jsonHeaders,
  );

  check(validateRes, {
    "validate: status 200": (r) => r.status === 200,
    "validate: correct id": (r) => r.json().data.id === id,
  });

  // Update the API key
  const updateRes = http.put(
    `${leader}/api-keys/${id}`,
    JSON.stringify({
      name: `bench-key-${__VU}-${__ITER}-updated`,
      description: "Updated by benchmark",
      scopes: ["read", "write", "admin"],
    }),
    jsonHeaders,
  );

  check(updateRes, {
    "update: status 200": (r) => r.status === 200,
  });

  // Revoke the API key
  const revokeRes = http.del(`${leader}/api-keys/${id}`);

  check(revokeRes, {
    "revoke: status 200": (r) => r.status === 200,
  });

  sleep(0.1);
}
