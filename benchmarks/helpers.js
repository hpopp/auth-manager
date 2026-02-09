import http from "k6/http";

const NODES = [
  "http://localhost:8081",
  "http://localhost:8082",
  "http://localhost:8083",
];

export function discoverCluster() {
  let leader = null;
  let followers = [];

  for (const node of NODES) {
    const res = http.get(`${node}/_internal/cluster/status`);
    if (res.status !== 200) continue;

    const data = res.json().data;
    if (data.role === "Leader") {
      leader = node;
    } else {
      followers.push(node);
    }
  }

  if (!leader) {
    throw new Error("No leader found. Is the cluster running?");
  }

  console.log(`Leader: ${leader}`);
  console.log(`Followers: ${followers.join(", ")}`);

  return { leader, followers };
}

export const jsonHeaders = {
  headers: { "Content-Type": "application/json" },
};
