use axum::extract::{Path, State};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::{replication_error, PaginationParams};
use crate::api::response::{ApiError, AppJson, AppQuery, JSend, JSendPaginated, Pagination};
use crate::cluster::replicate_write;
use crate::storage::models::{ApiKey as ApiKeyModel, WriteOp};
use crate::tokens::{
    api_key,
    generator::{generate_hex, hash_key},
};
use crate::AppState;

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateApiKeyRequest {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    pub name: String,
    pub subject_id: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateApiKeyResponse {
    pub description: Option<String>,
    pub expires_at: Option<String>,
    pub id: String,
    pub key: String,
    pub name: String,
    pub subject_id: String,
    pub scopes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ApiKeyResponse {
    pub created_at: String,
    pub description: Option<String>,
    pub expires_at: Option<String>,
    pub id: String,
    pub last_used_at: Option<String>,
    pub name: String,
    pub subject_id: String,
    pub scopes: Vec<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateApiKeyRequest {
    #[serde(default)]
    pub description: Option<Option<String>>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct VerifyApiKeyRequest {
    pub key: String,
}

// ============================================================================
// Handlers
// ============================================================================

pub async fn create_api_key(
    State(state): State<Arc<AppState>>,
    AppJson(req): AppJson<CreateApiKeyRequest>,
) -> Result<Json<JSend<CreateApiKeyResponse>>, ApiError> {
    validate_create_api_key(&req)?;

    let expires_at = req
        .expires_at
        .as_deref()
        .map(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|_| ApiError::bad_request("expires_at must be a valid RFC 3339 datetime"))
        })
        .transpose()?;

    let key = generate_hex(24, Some("am_"));
    let key_hash = hash_key(&key);
    let now = Utc::now();

    let api_key_record = ApiKeyModel {
        created_at: now,
        description: req.description.clone(),
        expires_at,
        id: uuid::Uuid::new_v4().to_string(),
        key_hash: key_hash.clone(),
        last_used_at: None,
        name: req.name.clone(),
        subject_id: req.subject_id.clone(),
        scopes: req.scopes.clone(),
        updated_at: Some(now),
    };

    let operation = WriteOp::CreateApiKey(api_key_record.clone());
    replicate_write(Arc::clone(&state), operation)
        .await
        .map_err(replication_error)?;

    state
        .db
        .put_api_key(&api_key_record)
        .map_err(|e| ApiError::internal(format!("Failed to store API key: {e}")))?;

    tracing::debug!(key_id = %api_key_record.id, name = %req.name, "Created API key");

    Ok(JSend::success(CreateApiKeyResponse {
        description: api_key_record.description,
        expires_at: api_key_record.expires_at.map(|t| t.to_rfc3339()),
        id: api_key_record.id,
        key,
        name: api_key_record.name,
        subject_id: api_key_record.subject_id,
        scopes: api_key_record.scopes,
    }))
}

pub async fn update_api_key(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    AppJson(req): AppJson<UpdateApiKeyRequest>,
) -> Result<Json<JSend<ApiKeyResponse>>, ApiError> {
    validate_update_api_key(&req)?;

    let key_hash = state
        .db
        .get_api_key_hash_by_id(&id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("API key not found"))?;

    let operation = WriteOp::UpdateApiKey {
        key_hash: key_hash.clone(),
        description: req.description.clone(),
        name: req.name.clone(),
        scopes: req.scopes.clone(),
    };
    replicate_write(Arc::clone(&state), operation)
        .await
        .map_err(replication_error)?;

    state
        .db
        .update_api_key(
            &key_hash,
            req.name.as_deref(),
            req.description.as_ref().map(|d| d.as_deref()),
            req.scopes.as_deref(),
        )
        .map_err(|e| ApiError::internal(format!("Failed to update API key: {e}")))?;

    let api_key_record = state
        .db
        .get_api_key(&key_hash)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::internal("API key not found after update".to_string()))?;

    tracing::debug!(id = %id, "Updated API key");
    Ok(JSend::success(api_key_to_response(&api_key_record)))
}

pub async fn get_api_key(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<JSend<ApiKeyResponse>>, ApiError> {
    let api_key = state
        .db
        .get_api_key_by_id(&id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("API key not found"))?;

    // Filter out expired keys
    if let Some(expires_at) = api_key.expires_at {
        if expires_at < Utc::now() {
            return Err(ApiError::not_found("API key not found"));
        }
    }

    Ok(JSend::success(api_key_to_response(&api_key)))
}

pub async fn validate_api_key(
    State(state): State<Arc<AppState>>,
    AppJson(req): AppJson<VerifyApiKeyRequest>,
) -> Result<Json<JSend<ApiKeyResponse>>, ApiError> {
    if req.key.trim().is_empty() {
        return Err(ApiError::bad_request("key is required"));
    }

    let key = req.key;
    match api_key::validate(&state.db, &key) {
        Ok(Some(api_key_record)) => Ok(JSend::success(api_key_to_response(&api_key_record))),
        Ok(None) => Err(ApiError::not_found("API key not found or expired")),
        Err(e) => Err(ApiError::internal(e.to_string())),
    }
}

pub async fn revoke_api_key(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<JSend<()>>, ApiError> {
    // Resolve UUID id to the internal key_hash
    let key_hash = state
        .db
        .get_api_key_hash_by_id(&id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("API key not found"))?;

    let operation = WriteOp::RevokeApiKey {
        key_id: key_hash.clone(),
    };
    replicate_write(Arc::clone(&state), operation)
        .await
        .map_err(replication_error)?;

    state
        .db
        .delete_api_key(&key_hash)
        .map_err(|e| ApiError::internal(format!("Failed to delete API key: {e}")))?;

    tracing::debug!(id = %id, "Revoked API key");
    Ok(JSend::success(()))
}

pub async fn list_api_keys(
    State(state): State<Arc<AppState>>,
    Path(subject_id): Path<String>,
    AppQuery(params): AppQuery<PaginationParams>,
) -> Result<Json<JSendPaginated<ApiKeyResponse>>, ApiError> {
    params.validate()?;

    match api_key::list_by_subject(&state.db, &subject_id) {
        Ok(keys) => {
            let total = keys.len() as u64;
            let items: Vec<ApiKeyResponse> = keys
                .iter()
                .skip(params.offset as usize)
                .take(params.limit as usize)
                .map(api_key_to_response)
                .collect();

            Ok(JSendPaginated::success(
                items,
                Pagination {
                    limit: params.limit,
                    offset: params.offset,
                    total,
                },
            ))
        }
        Err(e) => Err(ApiError::internal(e.to_string())),
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn validate_create_api_key(req: &CreateApiKeyRequest) -> Result<(), ApiError> {
    if req.name.trim().is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }
    if req.subject_id.trim().is_empty() {
        return Err(ApiError::bad_request("subject_id is required"));
    }
    validate_scopes(&req.scopes)?;
    Ok(())
}

fn validate_update_api_key(req: &UpdateApiKeyRequest) -> Result<(), ApiError> {
    if req.name.is_none() && req.description.is_none() && req.scopes.is_none() {
        return Err(ApiError::bad_request(
            "at least one field (name, description, scopes) must be provided",
        ));
    }
    if let Some(ref name) = req.name {
        if name.trim().is_empty() {
            return Err(ApiError::bad_request("name must not be empty"));
        }
    }
    if let Some(ref scopes) = req.scopes {
        validate_scopes(scopes)?;
    }
    Ok(())
}

fn validate_scopes(scopes: &[String]) -> Result<(), ApiError> {
    for scope in scopes {
        if scope.trim().is_empty() {
            return Err(ApiError::bad_request("scope values must not be empty"));
        }
    }
    Ok(())
}

fn api_key_to_response(api_key: &ApiKeyModel) -> ApiKeyResponse {
    ApiKeyResponse {
        created_at: api_key.created_at.to_rfc3339(),
        description: api_key.description.clone(),
        expires_at: api_key.expires_at.map(|t| t.to_rfc3339()),
        id: api_key.id.clone(),
        last_used_at: api_key.last_used_at.map(|t| t.to_rfc3339()),
        name: api_key.name.clone(),
        subject_id: api_key.subject_id.clone(),
        scopes: api_key.scopes.clone(),
        updated_at: api_key.updated_at.map(|t| t.to_rfc3339()),
    }
}
