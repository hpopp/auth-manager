# User Agent E2E Tests
# Parse user agent strings

# Parse Chrome on macOS
POST http://{{host}}:{{port}}/user-agents/parse
Content-Type: application/json
{
  "user_agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
}
HTTP 200
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.browser" == "Chrome"
jsonpath "$.data.kind" == "Desktop"
jsonpath "$.data.os" == "Mac OS X"


# Parse Safari on iOS
POST http://{{host}}:{{port}}/user-agents/parse
Content-Type: application/json
{
  "user_agent": "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1"
}
HTTP 200
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.browser" == "Safari"
jsonpath "$.data.kind" == "Mobile"


# Empty user agent returns 400
POST http://{{host}}:{{port}}/user-agents/parse
Content-Type: application/json
{
  "user_agent": ""
}
HTTP 400
[Asserts]
jsonpath "$.status" == "fail"
