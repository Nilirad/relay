use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// Payload that GitHub expects in the JWT.
#[derive(Debug, Serialize, Deserialize)]
struct GitHubClaims {
    /// Issued at time (UNIX time).
    iat: u64,
    /// Expiration time (UNIX time).
    exp: u64,
    /// Issuer: GitHub App's Client ID.
    iss: String,
}

/// Generates a JWT to authenticate access to the GitHub App.
///
/// Implementation based on [GitHub's documentation][jwt_docs].
///
/// [jwt_docs]: https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-json-web-token-jwt-for-a-github-app
pub(super) fn generate_gh_jwt(client_id: &str, pem_path: &str) -> Result<String, AppError> {
    // TODO: Check if you should use Tokio API.
    let pem = std::fs::read(pem_path)?;

    const CLOCK_DRIFT_BUFFER_SECS: u64 = 60;
    const TOKEN_VALIDITY_SECS: u64 = 5 * 60;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let claims = GitHubClaims {
        iat: now - CLOCK_DRIFT_BUFFER_SECS,
        exp: now + TOKEN_VALIDITY_SECS,
        iss: client_id.to_string(),
    };

    let header = Header::new(Algorithm::RS256);
    let key = EncodingKey::from_rsa_pem(&pem)?;

    let jwt = encode(&header, &claims, &key)?;
    Ok(jwt)
}
