#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::todo,
    clippy::unimplemented,
    clippy::indexing_slicing
)]

use crate::{
    polling::git::GitFetcher,
    trigger::{Authenticator, error::AuthError},
};
use async_trait::async_trait;
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};

pub struct MockGitFetcher {
    pub hash: String,
}

#[async_trait]
impl GitFetcher for MockGitFetcher {
    async fn get_latest_hash(
        &self,
        _repo: &str,
        _branch: &str,
    ) -> Result<String, crate::error::CommitHashError> {
        Ok(self.hash.clone())
    }
}

pub struct MockAuthenticator {
    pub iat: String,
}

#[async_trait]
impl Authenticator for MockAuthenticator {
    async fn request_installation_token(
        &self,
        _sub: &crate::model::Subscriber,
    ) -> Result<String, AuthError> {
        Ok(self.iat.clone())
    }
}

pub async fn create_test_db() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .expect("Failed to create in-memory database");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    pool
}
