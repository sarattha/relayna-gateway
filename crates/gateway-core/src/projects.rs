use crate::{GatewayError, GatewayResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ProjectCreateRequest {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct ProjectPatchRequest {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProjectResponse {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[async_trait]
pub trait AdminProjectStore: Send + Sync {
    async fn create_project(&self, request: ProjectCreateRequest)
        -> GatewayResult<ProjectResponse>;
    async fn list_projects(&self) -> GatewayResult<Vec<ProjectResponse>>;
    async fn get_project(&self, project_id: Uuid) -> GatewayResult<Option<ProjectResponse>>;
    async fn patch_project(
        &self,
        project_id: Uuid,
        patch: ProjectPatchRequest,
    ) -> GatewayResult<Option<ProjectResponse>>;
    async fn delete_project(&self, project_id: Uuid) -> GatewayResult<bool>;
}

#[async_trait]
impl<T> AdminProjectStore for std::sync::Arc<T>
where
    T: AdminProjectStore + ?Sized,
{
    async fn create_project(
        &self,
        request: ProjectCreateRequest,
    ) -> GatewayResult<ProjectResponse> {
        (**self).create_project(request).await
    }

    async fn list_projects(&self) -> GatewayResult<Vec<ProjectResponse>> {
        (**self).list_projects().await
    }

    async fn get_project(&self, project_id: Uuid) -> GatewayResult<Option<ProjectResponse>> {
        (**self).get_project(project_id).await
    }

    async fn patch_project(
        &self,
        project_id: Uuid,
        patch: ProjectPatchRequest,
    ) -> GatewayResult<Option<ProjectResponse>> {
        (**self).patch_project(project_id, patch).await
    }

    async fn delete_project(&self, project_id: Uuid) -> GatewayResult<bool> {
        (**self).delete_project(project_id).await
    }
}

impl ProjectCreateRequest {
    pub fn validate(&self) -> GatewayResult<()> {
        validate_project_name(&self.name)
    }
}

impl ProjectPatchRequest {
    pub fn validate(&self) -> GatewayResult<()> {
        if let Some(name) = self.name.as_deref() {
            validate_project_name(name)?;
        }
        Ok(())
    }
}

pub fn validate_project_name(name: &str) -> GatewayResult<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 120 {
        return Err(GatewayError::InvalidProjectPayload);
    }
    Ok(())
}
