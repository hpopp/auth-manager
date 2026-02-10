# Cluster E2E Tests
# Health check and cluster status

# Health check
GET http://{{host}}:{{port}}/_internal/health
HTTP 200
[Asserts]
jsonpath "$.status" == "success"


# Cluster status
GET http://{{host}}:{{port}}/_internal/cluster/status
HTTP 200
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.node_id" isString
jsonpath "$.data.role" isString
jsonpath "$.data.term" isInteger
jsonpath "$.data.sequence" isInteger
