//! Authentication and authorization to request services.

use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{
    error::FatalError,
    model::Subscriber,
    trigger::error::{AuthError, RequestError},
};

/// Credentials required for authentication.
#[derive(Debug, Clone)]
pub struct AuthCredentials {
    /// GitHub App's Client ID.
    pub client_id: String,
    /// Path to the GitHub App's private key.
    pub pem_path: String,
}

/// Payload that GitHub expects in the JWT.
///
/// Read more on [GitHub's documentation][jwt_docs].
///
/// <!-- LINKS -->
/// [jwt_docs]: https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-json-web-token-jwt-for-a-github-app
#[derive(Debug, Serialize, Deserialize)]
struct GitHubClaims {
    /// Issued at time (UNIX time).
    iat: u64,

    /// Expiration time (UNIX time).
    exp: u64,

    /// Issuer: GitHub App's Client ID.
    iss: String,
}

/// Reads the environment variables for authentication.
pub fn get_auth_credentials() -> Result<AuthCredentials, FatalError> {
    let client_id = std::env::var("GH_CLIENT_ID")
        .map_err(|_| FatalError::EnvVarNotSet("GH_CLIENT_ID".to_string()))?;
    let pem_path = std::env::var("GH_APP_KEY_PATH")
        .map_err(|_| FatalError::EnvVarNotSet("GH_APP_KEY_PATH".to_string()))?;

    Ok(AuthCredentials {
        client_id,
        pem_path,
    })
}

/// Generates a JWT to authenticate access to the GitHub App.
///
/// Implementation based on [GitHub's documentation][jwt_docs].
///
/// <!-- LINKS -->
/// [jwt_docs]: https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-json-web-token-jwt-for-a-github-app
pub(super) fn generate_gh_jwt(creds: &AuthCredentials) -> Result<String, AuthError> {
    // TODO: Check if you should use Tokio API.
    let pem = std::fs::read(&creds.pem_path)?;

    const CLOCK_DRIFT_BUFFER_SECS: u64 = 60;
    const TOKEN_VALIDITY_SECS: u64 = 5 * 60;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let claims = GitHubClaims {
        iat: now - CLOCK_DRIFT_BUFFER_SECS,
        exp: now + TOKEN_VALIDITY_SECS,
        iss: creds.client_id.to_string(),
    };

    let header = Header::new(Algorithm::RS256);
    let key = EncodingKey::from_rsa_pem(&pem)?;

    let jwt = encode(&header, &claims, &key)?;
    Ok(jwt)
}

/// Requests an Installation Access Token (IAT)
/// to operate on a GitHub App installation.
///
/// Implementation based on [GitHub's documentation][iat_docs].
///
/// <!-- LINKS -->
/// [iat_docs]: https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-an-installation-access-token-for-a-github-app
pub(super) async fn request_iat(
    http_client: &Client,
    jwt: &str,
    sub: &Subscriber,
) -> Result<String, AuthError> {
    #[derive(serde::Deserialize)]
    struct IatResponse {
        token: String,
    }

    let api_url = format!(
        "https://api.github.com/app/installations/{}/access_tokens",
        sub.gh_app_installation_id
    );
    let response = http_client
        .post(&api_url)
        .bearer_auth(jwt)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2026-03-10")
        .send()
        .await?;

    if response.status().is_success() {
        let response_json = response.json::<IatResponse>().await?;
        info!("IAT received for subscriber {}", sub.target_repo);
        Ok(response_json.token)
    } else {
        Err(AuthError::Server(RequestError::Response {
            status: response.status(),
            text: response.text().await?,
        }))
    }
}
