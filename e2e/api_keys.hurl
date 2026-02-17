# API Keys E2E Tests
# Lifecycle: purge -> create -> get -> list -> update -> validate -> revoke -> verify revoked

# Purge all data
DELETE http://{{host}}:{{port}}/admin/purge
HTTP 200
[Asserts]
jsonpath "$.status" == "success"


# Create an API key
POST http://{{host}}:{{port}}/api-keys
Content-Type: application/json
{
  "name": "Test Key",
  "subject_id": "test-user",
  "description": "E2E test key",
  "scopes": ["read", "write"]
}
HTTP 200
[Captures]
api_key_id: jsonpath "$.data.id"
api_key: jsonpath "$.data.key"
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.id" isString
jsonpath "$.data.key" isString
jsonpath "$.data.name" == "Test Key"
jsonpath "$.data.subject_id" == "test-user"
jsonpath "$.data.description" == "E2E test key"
jsonpath "$.data.scopes" count == 2


# Create an API key with a custom key value
POST http://{{host}}:{{port}}/api-keys
Content-Type: application/json
{
  "name": "Custom Key",
  "subject_id": "test-user",
  "key": "my-custom-key-value",
  "scopes": ["view"]
}
HTTP 200
[Captures]
custom_key_id: jsonpath "$.data.id"
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.key" == "my-custom-key-value"


# Get API key by ID
GET http://{{host}}:{{port}}/api-keys/{{api_key_id}}
HTTP 200
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.id" == {{api_key_id}}
jsonpath "$.data.name" == "Test Key"
jsonpath "$.data.subject_id" == "test-user"
jsonpath "$.data.scopes" count == 2


# List all API keys (unfiltered)
GET http://{{host}}:{{port}}/api-keys
HTTP 200
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.items" count >= 2
jsonpath "$.data.pagination.total" >= 2


# List API keys filtered by subject_id
GET http://{{host}}:{{port}}/api-keys?subject_id=test-user
HTTP 200
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.items" count >= 2


# List API keys with nonexistent subject_id
GET http://{{host}}:{{port}}/api-keys?subject_id=nonexistent
HTTP 200
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.items" count == 0
jsonpath "$.data.pagination.total" == 0


# Update API key
PUT http://{{host}}:{{port}}/api-keys/{{api_key_id}}
Content-Type: application/json
{
  "name": "Updated Key",
  "description": "Updated description",
  "scopes": ["read", "write", "admin"]
}
HTTP 200
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.name" == "Updated Key"
jsonpath "$.data.description" == "Updated description"
jsonpath "$.data.scopes" count == 3


# Clear description (set to null)
PUT http://{{host}}:{{port}}/api-keys/{{api_key_id}}
Content-Type: application/json
{
  "description": null
}
HTTP 200
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.name" == "Updated Key"
jsonpath "$.data.description" == null
jsonpath "$.data.scopes" count == 3


# Validate API key
POST http://{{host}}:{{port}}/api-keys/verify
Content-Type: application/json
{
  "key": "{{api_key}}"
}
HTTP 200
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.id" == {{api_key_id}}
jsonpath "$.data.name" == "Updated Key"
jsonpath "$.data.scopes" count == 3


# Validate custom key
POST http://{{host}}:{{port}}/api-keys/verify
Content-Type: application/json
{
  "key": "my-custom-key-value"
}
HTTP 200
[Asserts]
jsonpath "$.status" == "success"
jsonpath "$.data.id" == {{custom_key_id}}
jsonpath "$.data.name" == "Custom Key"


# Revoke API key
DELETE http://{{host}}:{{port}}/api-keys/{{api_key_id}}
HTTP 200
[Asserts]
jsonpath "$.status" == "success"


# Verify revoked API key returns 404
POST http://{{host}}:{{port}}/api-keys/verify
Content-Type: application/json
{
  "key": "{{api_key}}"
}
HTTP 404
[Asserts]
jsonpath "$.status" == "fail"


# Clean up custom key
DELETE http://{{host}}:{{port}}/api-keys/{{custom_key_id}}
HTTP 200
[Asserts]
jsonpath "$.status" == "success"
