//! Authorization-code sign-in. The client keeps the PKCE verifier across the
//! system-browser round trip; provider tokens never reach the client or logs.

use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    extract::{ConnectInfo, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{Redirect, Response},
    routing::post,
    Extension, Form, Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use fred::prelude::{KeysInterface, LuaInterface};
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::{AuthState, Dest};

const TTL: i64 = 600;
type Error = StatusCode;
const DOWN: Error = StatusCode::SERVICE_UNAVAILABLE;
const INVALID: Error = StatusCode::UNAUTHORIZED;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
enum Provider {
    Google,
    Github,
    Apple,
}

struct Config {
    client_id: String,
    secret: Secret,
    authorize: String,
    token: String,
    // GitHub's verified-email API, or the OIDC provider's JWKS endpoint.
    identity: String,
}

enum Secret {
    Shared(String),
    Apple { team: String, kid: String, key: jsonwebtoken::EncodingKey },
}

/// Disabled providers return 503. Configure once at startup with `from_env`.
#[derive(Default)]
pub struct OAuth {
    providers: HashMap<Provider, Config>,
    client: Client,
    // Serialized refresh prevents a burst of sign-ins from fetching JWKS repeatedly.
    jwks: Mutex<HashMap<Provider, (tokio::time::Instant, JwkSet)>>,
}

impl OAuth {
    /// A provider is disabled when all its variables are absent. Partial config
    /// fails startup. Apple signs a fresh, short-lived ES256 client secret per exchange.
    pub fn from_env() -> Result<Self, String> {
        let mut oauth = Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("gh-core-auth")
                .build()
                .map_err(|_| "cannot build OAuth HTTP client")?,
            providers: HashMap::new(),
            jwks: Mutex::default(),
        };
        for (provider, prefix, authorize, token, identity) in [
            (
                Provider::Google,
                "GOOGLE",
                "https://accounts.google.com/o/oauth2/v2/auth",
                "https://oauth2.googleapis.com/token",
                "https://www.googleapis.com/oauth2/v3/certs",
            ),
            (
                Provider::Github,
                "GITHUB",
                "https://github.com/login/oauth/authorize",
                "https://github.com/login/oauth/access_token",
                "https://api.github.com/user/emails",
            ),
            (
                Provider::Apple,
                "APPLE",
                "https://appleid.apple.com/auth/authorize",
                "https://appleid.apple.com/auth/token",
                "https://appleid.apple.com/auth/keys",
            ),
        ] {
            let suffixes: &[&str] = if provider == Provider::Apple {
                &["CLIENT_ID", "TEAM_ID", "KEY_ID", "PRIVATE_KEY"]
            } else {
                &["CLIENT_ID", "CLIENT_SECRET"]
            };
            let vars: Vec<_> = suffixes.iter().map(|s| std::env::var(format!("OAUTH_{prefix}_{s}")).ok()).collect();
            if vars.iter().all(Option::is_none) {
                continue;
            }
            let vars: Vec<String> = vars
                .into_iter()
                .collect::<Option<_>>()
                .filter(|v: &Vec<String>| v.iter().all(|s| !s.trim().is_empty()))
                .ok_or_else(|| format!("incomplete OAUTH_{prefix} configuration"))?;
            let secret = if provider == Provider::Apple {
                Secret::Apple {
                    team: vars[1].clone(),
                    kid: vars[2].clone(),
                    key: jsonwebtoken::EncodingKey::from_ec_pem(vars[3].as_bytes())
                        .map_err(|_| "invalid OAUTH_APPLE_PRIVATE_KEY")?,
                }
            } else {
                Secret::Shared(vars[1].clone())
            };
            oauth.providers.insert(
                provider,
                Config {
                    client_id: vars[0].clone(),
                    secret,
                    authorize: authorize.into(),
                    token: token.into(),
                    identity: identity.into(),
                },
            );
        }
        Ok(oauth)
    }

    async fn email(
        &self,
        provider: Provider,
        config: &Config,
        tokens: Value,
        nonce: &str,
        now: i64,
    ) -> Result<Dest, Error> {
        let email = if provider == Provider::Github {
            #[derive(Deserialize)]
            struct Email {
                email: String,
                verified: bool,
                primary: bool,
            }
            let token = tokens["access_token"].as_str().filter(|t| !t.is_empty()).ok_or(INVALID)?;
            let response = self
                .client
                .get(&config.identity)
                .bearer_auth(token)
                .header("Accept", "application/vnd.github+json")
                .send()
                .await
                .map_err(|_| DOWN)?;
            let emails: Vec<Email> = provider_json(response).await?;
            // Never trust /user.email: it doesn't carry a verified flag.
            emails.into_iter().find(|e| e.primary && e.verified).ok_or(INVALID)?.email
        } else {
            let token = tokens["id_token"].as_str().ok_or(INVALID)?;
            let header = decode_header(token).map_err(|_| INVALID)?;
            if header.alg != Algorithm::RS256 {
                return Err(INVALID);
            }
            let kid = header.kid.ok_or(INVALID)?;
            let mut cache = self.jwks.lock().await;
            if cache.get(&provider).is_none_or(|(until, _)| *until <= tokio::time::Instant::now()) {
                let response = self.client.get(&config.identity).send().await.map_err(|_| DOWN)?;
                let keys: JwkSet = provider_json(response).await?;
                cache.insert(provider, (tokio::time::Instant::now() + Duration::from_secs(300), keys));
            }
            let key = cache[&provider].1.find(&kid).ok_or(INVALID)?;
            let key = DecodingKey::from_jwk(key).map_err(|_| INVALID)?;
            drop(cache);
            let mut validation = Validation::new(Algorithm::RS256);
            validation.set_audience(&[&config.client_id]);
            let issuers: &[&str] = if provider == Provider::Google {
                &["https://accounts.google.com", "accounts.google.com"]
            } else {
                &["https://appleid.apple.com"]
            };
            validation.set_issuer(issuers);
            validation.set_required_spec_claims(&["iss", "aud", "sub", "exp", "iat"]);
            // Use AuthState's clock, including in tests, rather than the JWT library's clock.
            validation.validate_exp = false;
            let claims = decode::<Value>(token, &key, &validation).map_err(|_| INVALID)?.claims;
            let exp = claims["exp"].as_i64().ok_or(INVALID)?;
            let iat = claims["iat"].as_i64().ok_or(INVALID)?;
            if exp <= now
                || iat > now + 60
                || exp <= iat
                || claims["nonce"].as_str() != Some(nonce)
                || claims["sub"].as_str().is_none_or(str::is_empty)
                || !(claims["email_verified"] == true
                    || (provider == Provider::Apple && claims["email_verified"] == "true"))
                || ((claims.get("azp").is_some() || claims["aud"].as_array().is_some_and(|a| a.len() > 1))
                    && claims["azp"] != config.client_id)
            {
                return Err(INVALID);
            }
            claims["email"].as_str().ok_or(INVALID)?.to_owned()
        };
        // The same address normalization/validation as code sign-in.
        Dest::email(&email).ok_or(INVALID)
    }
}

async fn provider_json<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> Result<T, Error> {
    let status = response.status();
    if status.as_u16() == 429 {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    if status.is_server_error() || status.is_redirection() {
        return Err(DOWN);
    }
    let body: Value = response.json().await.map_err(|_| DOWN)?;
    if let Some(error) = body.get("error") {
        return Err(match error.as_str() {
            Some("invalid_grant" | "bad_verification_code" | "access_denied") => INVALID,
            _ => DOWN, // Invalid client credentials/configuration aren't the user's fault.
        });
    }
    if !status.is_success() {
        return Err(INVALID);
    }
    serde_json::from_value(body).map_err(|_| DOWN)
}

#[derive(Serialize, Deserialize)]
struct Pending {
    provider: Provider,
    challenge: String,
    nonce: String,
    web: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Start {
    code_challenge: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Complete {
    state: String,
    code: String,
    code_verifier: String,
}

fn random() -> Result<String, Error> {
    let mut bytes = [0; 32];
    getrandom::getrandom(&mut bytes).map_err(|_| DOWN)?;
    Ok(B64.encode(bytes))
}

fn opaque(s: &str) -> bool {
    s.len() == 43 && B64.decode(s).is_ok_and(|b| b.len() == 32)
}
fn web(state: &AuthState, headers: &HeaderMap) -> bool {
    headers.get(header::ORIGIN).is_some_and(|o| o == state.origin.as_str())
}
fn callback(state: &AuthState) -> String {
    format!("{}/auth/callback", state.origin)
}

pub(crate) fn routes() -> Router<Arc<AuthState>> {
    Router::new()
        .route("/api/v1/auth/oauth/{provider}/start", post(start))
        .route("/api/v1/auth/oauth/{provider}/complete", post(complete))
        .route("/auth/callback", post(apple_callback))
        .layer(axum::middleware::map_response(|mut res: Response| async {
            res.headers_mut().insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
            res.headers_mut().insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
            res
        }))
}

async fn start(
    State(state): State<Arc<AuthState>>,
    Path(provider): Path<Provider>,
    headers: HeaderMap,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    Json(input): Json<Start>,
) -> Result<Json<Value>, Error> {
    let config = state.oauth.providers.get(&provider).ok_or(DOWN)?;
    if !opaque(&input.code_challenge) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let ip = crate::routes::client_ip(&headers, peer).ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let count: i64 = state
        .sessions
        .redis
        .eval(
            "local n = redis.call('INCR', KEYS[1]); if n == 1 then redis.call('EXPIRE', KEYS[1], 3600) end; return n",
            vec![format!("gh:oauth:rl:{ip}:{}", (state.now)() / 3600)],
            Vec::<String>::new(),
        )
        .await
        .map_err(|_| DOWN)?;
    if count > 300 {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    let id = random()?;
    let pending = Pending { provider, challenge: input.code_challenge, nonce: random()?, web: web(&state, &headers) };
    let mut url = Url::parse(&config.authorize).map_err(|_| DOWN)?;
    let redirect = callback(&state);
    {
        let mut query = url.query_pairs_mut();
        query.extend_pairs([
            ("client_id", config.client_id.as_str()),
            ("redirect_uri", &redirect),
            ("response_type", "code"),
            ("state", &id),
            ("nonce", &pending.nonce),
        ]);
        if provider == Provider::Apple {
            query.extend_pairs([("scope", "email"), ("response_mode", "form_post")]);
        } else {
            query.extend_pairs([
                ("scope", if provider == Provider::Google { "openid email" } else { "user:email" }),
                ("code_challenge_method", "S256"),
                ("code_challenge", &pending.challenge),
            ]);
        }
    }
    state
        .sessions
        .redis
        .set::<(), _, _>(
            format!("gh:oauth:{id}"),
            serde_json::to_string(&pending).map_err(|_| DOWN)?,
            Some(fred::types::Expiration::EX(TTL)),
            None,
            false,
        )
        .await
        .map_err(|_| DOWN)?;
    Ok(Json(json!({"url": url.as_str(), "state": id})))
}

async fn complete(
    State(state): State<Arc<AuthState>>,
    Path(provider): Path<Provider>,
    headers: HeaderMap,
    Json(input): Json<Complete>,
) -> Result<Response, Error> {
    let config = state.oauth.providers.get(&provider).ok_or(DOWN)?;
    if !opaque(&input.state)
        || input.code.is_empty()
        || input.code.len() > 4096
        || !(43..=128).contains(&input.code_verifier.len())
        || !input.code_verifier.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let key = format!("gh:oauth:{}", input.state);
    let raw: String = state.sessions.redis.get::<Option<String>, _>(&key).await.map_err(|_| DOWN)?.ok_or(INVALID)?;
    let pending: Pending = serde_json::from_str(&raw).map_err(|_| DOWN)?;
    if pending.provider != provider
        || pending.challenge != B64.encode(Sha256::digest(input.code_verifier.as_bytes()))
        || pending.web != web(&state, &headers)
    {
        return Err(INVALID);
    }
    // Compare-and-delete: exactly one parallel completion can exchange the code.
    let used: i64 = state
        .sessions
        .redis
        .eval(
            "if redis.call('GET', KEYS[1]) == ARGV[1] then return redis.call('DEL', KEYS[1]) else return 0 end",
            vec![key],
            vec![raw],
        )
        .await
        .map_err(|_| DOWN)?;
    if used != 1 {
        return Err(INVALID);
    }
    let now = (state.now)();
    let secret = match &config.secret {
        Secret::Shared(secret) => secret.clone(),
        Secret::Apple { team, kid, key } => {
            let mut header = jsonwebtoken::Header::new(Algorithm::ES256);
            header.kid = Some(kid.clone());
            jsonwebtoken::encode(
                &header,
                &json!({"iss": team, "sub": config.client_id,
                "aud": "https://appleid.apple.com", "iat": now, "exp": now + 300}),
                key,
            )
            .map_err(|_| DOWN)?
        }
    };
    let redirect = callback(&state);
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("client_id", &config.client_id),
        ("client_secret", &secret),
        ("redirect_uri", &redirect),
        ("code", &input.code),
    ];
    if provider != Provider::Apple {
        form.push(("code_verifier", &input.code_verifier));
    }
    let response = state
        .oauth
        .client
        .post(&config.token)
        .header("Accept", "application/json")
        .form(&form)
        .send()
        .await
        .map_err(|_| DOWN)?;
    let tokens = provider_json(response).await?;
    let to = state.oauth.email(provider, config, tokens, &pending.nonce, now).await?;
    crate::routes::sign_in(&state, &to, now, pending.web).await
}

#[derive(Deserialize)]
struct AppleCallback {
    state: String,
    code: Option<String>,
    error: Option<String>,
}

/// Apple POSTs here; GET /auth/callback remains the client/Universal Link route.
/// No session is created by this relay. Only the client holding the verifier can
/// complete sign-in. Ignore Apple's unsigned `user` and front-channel ID token.
async fn apple_callback(
    State(state): State<Arc<AuthState>>,
    Form(input): Form<AppleCallback>,
) -> Result<Redirect, Error> {
    if !opaque(&input.state) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut url = Url::parse(&callback(&state)).map_err(|_| DOWN)?;
    url.query_pairs_mut().extend_pairs([("provider", "apple"), ("state", &input.state)]);
    if let Some(code) = input.code.filter(|c| !c.is_empty() && c.len() <= 4096) {
        url.query_pairs_mut().append_pair("code", &code);
    } else if input.error.is_some() {
        url.query_pairs_mut().append_pair("error", "access_denied");
    } else {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(Redirect::to(url.as_str()))
}

#[cfg(test)]
#[path = "oauth/tests.rs"]
mod tests;
