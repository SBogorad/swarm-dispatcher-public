//! Optional S3-compatible content-addressed artifact storage for remote workers.

use crate::artifacts::sha256_ref;
use crate::error::{Result, WorkerError};
use aws_sdk_s3::primitives::ByteStream;
use aws_credential_types::Credentials;
use aws_sdk_s3::config::Builder as S3ConfigBuilder;
use aws_sdk_s3::Client;
use aws_types::region::Region;
use std::path::Path;

pub struct ArtifactStore {
    runtime: tokio::runtime::Runtime,
    client: Client,
    bucket: String,
    prefix: String,
}

impl ArtifactStore {
    /// Returns None when the worker should use its existing local-artifact mode.
    pub fn from_env() -> Result<Option<Self>> {
        let endpoint = std::env::var("SWARM_ARTIFACT_S3_ENDPOINT").ok();
        let bucket = std::env::var("SWARM_ARTIFACT_S3_BUCKET").ok();
        let access = std::env::var("SWARM_ARTIFACT_S3_ACCESS_KEY").ok();
        let secret = std::env::var("SWARM_ARTIFACT_S3_SECRET_KEY").ok();
        let configured = [endpoint.as_ref(), bucket.as_ref(), access.as_ref(), secret.as_ref()]
            .iter()
            .filter(|value| value.is_some())
            .count();
        if configured == 0 {
            return Ok(None);
        }
        if configured != 4 {
            return Err(WorkerError::Artifact(
                "incomplete SWARM_ARTIFACT_S3 configuration".into(),
            ));
        }

        let region = std::env::var("SWARM_ARTIFACT_S3_REGION")
            .unwrap_or_else(|_| "us-east-1".into());
        let prefix = std::env::var("SWARM_ARTIFACT_S3_PREFIX")
            .unwrap_or_else(|_| "artifacts".into());
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|error| WorkerError::Artifact(format!("create S3 runtime: {error}")))?;
        let client = runtime.block_on(async {
            let credentials = Credentials::new(
                access.as_deref().unwrap_or_default(),
                secret.as_deref().unwrap_or_default(),
                None,
                None,
                "swarm-worker",
            );
            let shared = aws_config::defaults(aws_config::BehaviorVersion::latest())
                .region(Region::new(region))
                .endpoint_url(endpoint.as_deref().unwrap_or_default())
                .credentials_provider(credentials)
                .load()
                .await;
            S3ConfigBuilder::from(&shared).force_path_style(true).build()
        });

        Ok(Some(Self {
            runtime,
            client: Client::from_conf(client),
            bucket: bucket.unwrap_or_default(),
            prefix,
        }))
    }

    fn key(&self, artifact_ref: &str) -> Result<String> {
        let digest = artifact_ref
            .strip_prefix("sha256:")
            .ok_or_else(|| WorkerError::Artifact("artifact ref must use sha256: prefix".into()))?;
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(WorkerError::Artifact("artifact ref has invalid SHA-256".into()));
        }
        Ok(format!("{}/sha256/{digest}", self.prefix.trim_end_matches('/')))
    }

    pub fn put_verified(
        &self,
        artifact_ref: &str,
        path: &Path,
        content_type: &str,
    ) -> Result<String> {
        let bytes = std::fs::read(path)
            .map_err(|error| WorkerError::Artifact(format!("read artifact for upload: {error}")))?;
        if sha256_ref(&bytes) != artifact_ref {
            return Err(WorkerError::Artifact("local artifact SHA-256 mismatch".into()));
        }
        let key = self.key(artifact_ref)?;
        let client = self.client.clone();
        let bucket = self.bucket.clone();
        self.runtime.block_on(async move {
            client
                .put_object()
                .bucket(bucket)
                .key(&key)
                .content_type(content_type)
                .content_length(bytes.len() as i64)
                .body(ByteStream::from(bytes))
                .send()
                .await
                .map_err(|error| WorkerError::Artifact(format!("S3 upload failed: {error}")))
        })?;
        self.verify(artifact_ref)
    }

    pub fn verify(&self, artifact_ref: &str) -> Result<String> {
        let key = self.key(artifact_ref)?;
        let response = self
            .runtime
            .block_on(self.client.get_object().bucket(&self.bucket).key(&key).send())
            .map_err(|error| WorkerError::Artifact(format!("S3 download failed: {error}")))?;
        let bytes = self
            .runtime
            .block_on(response.body.collect())
            .map_err(|error| WorkerError::Artifact(format!("S3 response read failed: {error}")))?
            .into_bytes();
        if sha256_ref(&bytes) != artifact_ref {
            return Err(WorkerError::Artifact("remote artifact SHA-256 mismatch".into()));
        }
        Ok(format!("s3://{}/{}", self.bucket, key))
    }
}

#[cfg(test)]
mod tests {
    use super::ArtifactStore;

    #[test]
    fn disabled_without_configuration() {
        for key in [
            "SWARM_ARTIFACT_S3_ENDPOINT",
            "SWARM_ARTIFACT_S3_BUCKET",
            "SWARM_ARTIFACT_S3_ACCESS_KEY",
            "SWARM_ARTIFACT_S3_SECRET_KEY",
        ] {
            std::env::remove_var(key);
        }
        assert!(ArtifactStore::from_env().unwrap().is_none());
    }
}
