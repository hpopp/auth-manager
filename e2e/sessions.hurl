# Sessions E2E Tests
# Lifecycle: purge -> create -> get -> list -> validate -> revoke -> verify revoked

# Purge all data
DELETE http://{{host}}:{{port}}/admin/purge
HTTP 200
[Asserts]
jsonpath "$.status" == "success"


# Create a session
POST http://{{host}}:{{port}}/sessions
Content-Type: application/json
{
  "subject_id": "test-user",
  "ip_address": "10.0.0.1",
  "user_agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
  "ttl_seconds": 3600,
  "metadata": {
    "name": "Test User",
    "email": "test@example.com",
    "role": "admin"
  }
}
HTTP 200
[Captures]
session_id: jsonpath "$.data.id"
session_token: jsonpath "$.data.token"
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.id" isString
jsonpath "$.data.token" isString
jsonpath "$.data.expires_at" isString


# Get session by ID
GET http://{{host}}:{{port}}/sessions/{{session_id}}
HTTP 200
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.id" == {{session_id}}
jsonpath "$.data.subject_id" == "test-user"
jsonpath "$.data.ip_address" == "10.0.0.1"
jsonpath "$.data.device_info.browser" == "Chrome"
jsonpath "$.data.device_info.kind" == "Desktop"
jsonpath "$.data.metadata.name" == "Test User"
jsonpath "$.data.metadata.email" == "test@example.com"
jsonpath "$.data.metadata.role" == "admin"
jsonpath "$.data.renewable" == false


# List all sessions (unfiltered)
GET http://{{host}}:{{port}}/sessions
HTTP 200
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.items" count >= 1
jsonpath "$.data.pagination.total" >= 1


# List sessions filtered by subject_id
GET http://{{host}}:{{port}}/sessions?subject_id=test-user
HTTP 200
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.items" count >= 1


# List sessions with nonexistent subject_id
GET http://{{host}}:{{port}}/sessions?subject_id=nonexistent
HTTP 200
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.items" count == 0
jsonpath "$.data.pagination.total" == 0


# Validate session token
POST http://{{host}}:{{port}}/sessions/verify
Content-Type: application/json
{
  "token": "{{session_token}}"
}
HTTP 200
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.id" == {{session_id}}
jsonpath "$.data.subject_id" == "test-user"
jsonpath "$.data.metadata.name" == "Test User"
jsonpath "$.data.metadata.email" == "test@example.com"
jsonpath "$.data.renewable" == false


# Revoke session
DELETE http://{{host}}:{{port}}/sessions/{{session_id}}
HTTP 200
[Asserts]
jsonpath "$.status" == "success"


# Verify revoked session returns 404
POST http://{{host}}:{{port}}/sessions/verify
Content-Type: application/json
{
  "token": "{{session_token}}"
}
HTTP 404
[Asserts]
jsonpath "$.status" == "fail"


# Create a renewable session
POST http://{{host}}:{{port}}/sessions
Content-Type: application/json
{
  "subject_id": "renewable-user",
  "ttl_seconds": 3600,
  "renewable": true
}
HTTP 200
[Captures]
renewable_session_id: jsonpath "$.data.id"
renewable_session_token: jsonpath "$.data.token"
renewable_expires_at: jsonpath "$.data.expires_at"
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.id" isString
jsonpath "$.data.token" isString


# Get renewable session and verify renewable flag
GET http://{{host}}:{{port}}/sessions/{{renewable_session_id}}
HTTP 200
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.renewable" == true
jsonpath "$.data.subject_id" == "renewable-user"


# Validate renewable session (triggers sliding-window renewal)
POST http://{{host}}:{{port}}/sessions/verify
Content-Type: application/json
{
  "token": "{{renewable_session_token}}"
}
HTTP 200
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.id" == {{renewable_session_id}}
jsonpath "$.data.renewable" == true
jsonpath "$.data.expires_at" isString


# Create a session to revoke by token
POST http://{{host}}:{{port}}/sessions
Content-Type: application/json
{
  "subject_id": "revoke-by-token-user",
  "ttl_seconds": 3600
}
HTTP 200
[Captures]
revoke_token: jsonpath "$.data.token"
[Asserts]
jsonpath "$.status" == "success"


# Revoke by token
POST http://{{host}}:{{port}}/sessions/revoke
Content-Type: application/json
{
  "token": "{{revoke_token}}"
}
HTTP 200
[Asserts]
jsonpath "$.status" == "success"


# Verify revoked-by-token session returns 404
POST http://{{host}}:{{port}}/sessions/verify
Content-Type: application/json
{
  "token": "{{revoke_token}}"
}
HTTP 404
[Asserts]
jsonpath "$.status" == "fail"


# Revoke with empty token returns 400
POST http://{{host}}:{{port}}/sessions/revoke
Content-Type: application/json
{
  "token": ""
}
HTTP 400
[Asserts]
jsonpath "$.status" == "fail"
