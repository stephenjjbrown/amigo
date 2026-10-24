use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;

use crate::error::{ApiError, ErrorKind};
use crate::keychain;

use super::model::{now_unix, McpSession, MCP_ENDPOINT};

const AUTH_SERVER_METADATA: &str = "https://mcp.deepl.com/.well-known/oauth-authorization-server";
const DEFAULT_AUTHORIZATION_ENDPOINT: &str = "https://mcp.deepl.com/.idp/auth";
const DEFAULT_TOKEN_ENDPOINT: &str = "https://mcp.deepl.com/.idp/token";
const DEFAULT_REGISTRATION_ENDPOINT: &str = "https://mcp.deepl.com/.idp/register";
const DEFAULT_REVOCATION_ENDPOINT: &str = "https://mcp.deepl.com/.idp/revoke";
const CALLBACK_PATH: &str = "/callback";
const CALLBACK_PORTS: [u16; 5] = [53682, 53683, 53684, 53685, 53686];
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(180);
const CLIENT_NAME: &str = "Translation Amigo";
const SCOPE: &str = "offline_access";

#[derive(Debug, Clone)]
struct AuthServerMetadata {
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: Option<String>,
    revocation_endpoint: Option<String>,
}

impl AuthServerMetadata {
    fn fallback() -> Self {
        Self {
            authorization_endpoint: DEFAULT_AUTHORIZATION_ENDPOINT.to_string(),
            token_endpoint: DEFAULT_TOKEN_ENDPOINT.to_string(),
            registration_endpoint: Some(DEFAULT_REGISTRATION_ENDPOINT.to_string()),
            revocation_endpoint: Some(DEFAULT_REVOCATION_ENDPOINT.to_string()),
        }
    }
}

#[derive(Debug, Deserialize)]
struct MetadataResponse {
    authorization_endpoint: String,
    token_endpoint: String,
    #[serde(default)]
    registration_endpoint: Option<String>,
    #[serde(default)]
    revocation_endpoint: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ClientRegistration {
    client_id: String,
    #[serde(default)]
    client_secret: Option<String>,
    #[serde(default)]
    token_endpoint_auth_method: Option<String>,
    redirect_uri: String,
}

#[derive(Debug, Deserialize)]
struct RegistrationResponse {
    client_id: String,
    #[serde(default)]
    client_secret: Option<String>,
    #[serde(default)]
    token_endpoint_auth_method: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    scope: Option<String>,
}

struct Pkce {
    verifier: String,
    challenge: String,
    state: String,
}

struct CallbackQuery {
    code: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
    state: Option<String>,
}

fn discover(http: &reqwest::blocking::Client) -> AuthServerMetadata {
    let metadata = http
        .get(AUTH_SERVER_METADATA)
        .header("Accept", "application/json")
        .send()
        .ok()
        .and_then(|response| response.error_for_status().ok())
        .and_then(|response| response.json::<MetadataResponse>().ok())
        .map(|metadata| AuthServerMetadata {
            authorization_endpoint: metadata.authorization_endpoint,
            token_endpoint: metadata.token_endpoint,
            registration_endpoint: metadata.registration_endpoint,
            revocation_endpoint: metadata.revocation_endpoint,
        });

    match metadata {
        Some(metadata) => {
            log::debug!("OAuth metadata discovered");
            metadata
        }
        None => {
            log::warn!("OAuth metadata discovery failed; using built-in endpoints");
            AuthServerMetadata::fallback()
        }
    }
}

fn bind_listener() -> Result<(TcpListener, u16), ApiError> {
    for port in CALLBACK_PORTS {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) {
            return Ok((listener, port));
        }
    }

    Err(ApiError::system(
        "Could not open a local port for sign-in. Close any other Translation Amigo instance and try again.",
    ))
}

fn load_or_register_client(
    http: &reqwest::blocking::Client,
    metadata: &AuthServerMetadata,
    redirect_uri: &str,
) -> Result<ClientRegistration, ApiError> {
    if let Some(stored) = keychain::load_mcp_client()? {
        if let Ok(registration) = serde_json::from_str::<ClientRegistration>(&stored) {
            if registration.redirect_uri == redirect_uri {
                log::info!("reusing OAuth client registration");
                return Ok(registration);
            }
        }
    }

    log::info!("registering OAuth client with DeepL");

    let endpoint = metadata.registration_endpoint.as_deref().ok_or_else(|| {
        ApiError::api("DeepL's OAuth server does not expose a registration endpoint.")
    })?;

    let response = http
        .post(endpoint)
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "client_name": CLIENT_NAME,
            "redirect_uris": [redirect_uri],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
            "application_type": "native",
            "scope": SCOPE,
        }))
        .send()
        .map_err(ApiError::from)?;

    let status = response.status();
    let body = response.text().map_err(ApiError::from)?;

    if !status.is_success() {
        return Err(ApiError::api(format!(
            "DeepL rejected the app registration (HTTP {}): {}",
            status.as_u16(),
            error_detail(&body)
        )));
    }

    let parsed: RegistrationResponse = serde_json::from_str(&body).map_err(|error| {
        ApiError::api(format!(
            "DeepL sent a registration response this app could not read ({error})."
        ))
    })?;

    let registration = ClientRegistration {
        client_id: parsed.client_id,
        client_secret: parsed.client_secret,
        token_endpoint_auth_method: parsed.token_endpoint_auth_method,
        redirect_uri: redirect_uri.to_string(),
    };

    let encoded = serde_json::to_string(&registration).map_err(|error| {
        ApiError::system(format!("Could not encode the client registration: {error}"))
    })?;
    keychain::save_mcp_client(&encoded)?;
    log::info!("OAuth client registration stored");

    Ok(registration)
}

fn generate_pkce() -> Result<Pkce, ApiError> {
    let mut verifier_bytes = [0u8; 32];
    getrandom::fill(&mut verifier_bytes).map_err(|error| {
        ApiError::system(format!("Could not generate secure random data: {error}"))
    })?;
    let verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));

    let mut state_bytes = [0u8; 16];
    getrandom::fill(&mut state_bytes).map_err(|error| {
        ApiError::system(format!("Could not generate secure random data: {error}"))
    })?;
    let state = URL_SAFE_NO_PAD.encode(state_bytes);

    Ok(Pkce {
        verifier,
        challenge,
        state,
    })
}

fn authorization_url(
    metadata: &AuthServerMetadata,
    client_id: &str,
    redirect_uri: &str,
    pkce: &Pkce,
) -> Result<Url, ApiError> {
    let mut url = Url::parse(&metadata.authorization_endpoint).map_err(|error| {
        ApiError::api(format!(
            "DeepL's authorization endpoint is invalid: {error}"
        ))
    })?;

    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", SCOPE)
        .append_pair("state", &pkce.state)
        .append_pair("code_challenge", &pkce.challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("resource", MCP_ENDPOINT);

    Ok(url)
}

fn open_browser(url: &str) -> Result<(), ApiError> {
    #[cfg(target_os = "macos")]
    let outcome = std::process::Command::new("open").arg(url).spawn();

    #[cfg(target_os = "windows")]
    let outcome = std::process::Command::new("cmd")
        .arg("/C")
        .arg("start")
        .arg("")
        .arg(url)
        .spawn();

    #[cfg(target_os = "linux")]
    let outcome = std::process::Command::new("xdg-open").arg(url).spawn();

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let outcome: std::io::Result<std::process::Child> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Opening a browser is not supported on this platform.",
    ));

    outcome
        .map(|_| ())
        .map_err(|error| ApiError::system(format!("Could not open your browser: {error}")))
}

fn parse_callback(path: &str) -> Option<CallbackQuery> {
    let url = Url::parse(&format!("http://127.0.0.1{path}")).ok()?;
    if url.path() != CALLBACK_PATH {
        return None;
    }

    let mut query = CallbackQuery {
        code: None,
        error: None,
        error_description: None,
        state: None,
    };

    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => query.code = Some(value.into_owned()),
            "error" => query.error = Some(value.into_owned()),
            "error_description" => query.error_description = Some(value.into_owned()),
            "state" => query.state = Some(value.into_owned()),
            _ => {}
        }
    }

    Some(query)
}

fn write_response(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn wait_for_callback(
    listener: &TcpListener,
    expected_state: &str,
    cancel: &AtomicBool,
) -> Result<String, ApiError> {
    listener.set_nonblocking(true).map_err(|error| {
        ApiError::system(format!("Could not start the sign-in listener: {error}"))
    })?;

    let deadline = Instant::now() + SIGN_IN_TIMEOUT;

    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(ApiError::cancelled());
        }
        if Instant::now() >= deadline {
            return Err(ApiError::system(
                "Sign-in timed out. Return to the app and try again.",
            ));
        }

        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

                let clone = match stream.try_clone() {
                    Ok(clone) => clone,
                    Err(_) => continue,
                };
                let mut reader = BufReader::new(clone);
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    write_response(&mut stream, "400 Bad Request", "<h1>Bad request</h1>");
                    continue;
                }

                let path = request_line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .to_string();

                let Some(query) = parse_callback(&path) else {
                    write_response(&mut stream, "404 Not Found", "<h1>Not found</h1>");
                    continue;
                };

                if let Some(error) = &query.error {
                    let description = query
                        .error_description
                        .as_deref()
                        .filter(|value| !value.is_empty())
                        .unwrap_or(error);
                    write_response(
                        &mut stream,
                        "200 OK",
                        "<h1>Sign-in failed</h1><p>Return to Translation Amigo and try again.</p>",
                    );
                    return Err(ApiError::auth(format!(
                        "DeepL sign-in failed: {description}"
                    )));
                }

                if query.state.as_deref() != Some(expected_state) {
                    write_response(
                        &mut stream,
                        "200 OK",
                        "<h1>Sign-in failed</h1><p>Return to Translation Amigo and try again.</p>",
                    );
                    return Err(ApiError::auth(
                        "DeepL sign-in returned an unexpected state. Try again.",
                    ));
                }

                let Some(code) = query.code else {
                    write_response(
                        &mut stream,
                        "200 OK",
                        "<h1>Sign-in failed</h1><p>Return to Translation Amigo and try again.</p>",
                    );
                    return Err(ApiError::auth(
                        "DeepL sign-in did not return an authorization code.",
                    ));
                };

                write_response(
                    &mut stream,
                    "200 OK",
                    "<h1>Signed in</h1><p>You can close this window and return to Translation Amigo.</p>",
                );
                return Ok(code);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => {
                return Err(ApiError::system(format!(
                    "The sign-in listener failed: {error}"
                )));
            }
        }
    }
}

fn exchange_code(
    http: &reqwest::blocking::Client,
    metadata: &AuthServerMetadata,
    registration: &ClientRegistration,
    redirect_uri: &str,
    pkce: &Pkce,
    code: &str,
) -> Result<McpSession, ApiError> {
    let mut form: Vec<(&str, &str)> = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", registration.client_id.as_str()),
        ("code_verifier", pkce.verifier.as_str()),
        ("resource", MCP_ENDPOINT),
    ];
    if let Some(secret) = &registration.client_secret {
        form.push(("client_secret", secret.as_str()));
    }

    let response = http
        .post(&metadata.token_endpoint)
        .header("Accept", "application/json")
        .form(&form)
        .send()
        .map_err(ApiError::from)?;

    let status = response.status();
    let body = response.text().map_err(ApiError::from)?;

    if !status.is_success() {
        let error = token_error(status.as_u16(), &body);
        log::warn!("token exchange failed: {error}");
        return Err(error);
    }

    let tokens: TokenResponse = serde_json::from_str(&body).map_err(|error| {
        ApiError::api(format!(
            "DeepL sent a token response this app could not read ({error})."
        ))
    })?;

    log::info!(
        "OAuth tokens received (expires in {}s, refresh token: {})",
        tokens.expires_in.unwrap_or(3600),
        if tokens.refresh_token.is_some() {
            "yes"
        } else {
            "no"
        }
    );

    Ok(McpSession {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        expires_at: now_unix() + tokens.expires_in.unwrap_or(3600),
        client_id: registration.client_id.clone(),
        client_secret: registration.client_secret.clone(),
        token_endpoint: metadata.token_endpoint.clone(),
        revocation_endpoint: metadata.revocation_endpoint.clone(),
        token_endpoint_auth_method: registration.token_endpoint_auth_method.clone(),
        resource: MCP_ENDPOINT.to_string(),
        scope: tokens.scope.or_else(|| Some(SCOPE.to_string())),
    })
}

/// Run the full browser-based OAuth 2.1 + PKCE flow. This blocks for as long as
/// it takes the user to finish in the browser, so it runs on the engine thread
/// with a cancellation flag shared with the UI.
pub fn sign_in(
    cancel: Arc<AtomicBool>,
    http: &reqwest::blocking::Client,
) -> Result<McpSession, ApiError> {
    if cancel.load(Ordering::SeqCst) {
        return Err(ApiError::cancelled());
    }

    let metadata = discover(http);
    let (listener, port) = bind_listener()?;
    log::debug!("OAuth callback listening on 127.0.0.1:{port}");
    let redirect_uri = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");
    let registration = load_or_register_client(http, &metadata, &redirect_uri)?;
    let pkce = generate_pkce()?;
    let url = authorization_url(&metadata, &registration.client_id, &redirect_uri, &pkce)?;

    log::info!("opening browser for DeepL sign-in");
    open_browser(url.as_str())?;
    let code = wait_for_callback(&listener, &pkce.state, cancel.as_ref())?;
    log::info!("authorization callback received");

    exchange_code(http, &metadata, &registration, &redirect_uri, &pkce, &code)
}

/// Refresh an expiring session. Returns whether the tokens changed.
pub fn refresh_session(
    http: &reqwest::blocking::Client,
    session: &mut McpSession,
) -> Result<bool, ApiError> {
    if !session.is_expiring() {
        return Ok(false);
    }

    log::info!("refreshing saved session");
    log::debug!("refreshing OAuth access token");

    let refresh_token = session
        .refresh_token
        .clone()
        .ok_or_else(ApiError::auth_expired)?;

    let mut form: Vec<(&str, &str)> = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token.as_str()),
        ("client_id", session.client_id.as_str()),
        ("resource", session.resource.as_str()),
    ];
    if let Some(secret) = &session.client_secret {
        form.push(("client_secret", secret.as_str()));
    }

    let response = http
        .post(&session.token_endpoint)
        .header("Accept", "application/json")
        .form(&form)
        .send()
        .map_err(ApiError::from)?;

    let status = response.status();
    let body = response.text().map_err(ApiError::from)?;

    if !status.is_success() {
        let error = token_error(status.as_u16(), &body);
        log::warn!("token refresh failed: {error}");
        return Err(if matches!(error.kind, ErrorKind::Auth) {
            ApiError::auth_expired()
        } else {
            error
        });
    }

    let tokens: TokenResponse = serde_json::from_str(&body).map_err(|error| {
        ApiError::api(format!(
            "DeepL sent a token response this app could not read ({error})."
        ))
    })?;

    session.access_token = tokens.access_token;
    if let Some(refresh_token) = tokens.refresh_token {
        session.refresh_token = Some(refresh_token);
    }
    session.expires_at = now_unix() + tokens.expires_in.unwrap_or(3600);
    if let Some(scope) = tokens.scope {
        session.scope = Some(scope);
    }

    log::info!("OAuth access token refreshed");
    Ok(true)
}

/// Best-effort token revocation when the user signs out.
pub fn revoke(http: &reqwest::blocking::Client, session: &McpSession) {
    let Some(endpoint) = &session.revocation_endpoint else {
        return;
    };

    let token = session
        .refresh_token
        .as_deref()
        .unwrap_or(session.access_token.as_str());

    let mut form: Vec<(&str, &str)> =
        vec![("token", token), ("client_id", session.client_id.as_str())];
    if let Some(secret) = &session.client_secret {
        form.push(("client_secret", secret.as_str()));
    }

    let _ = http
        .post(endpoint)
        .header("Accept", "application/json")
        .form(&form)
        .send();
    log::debug!("OAuth token revocation requested");
}

fn token_error(status: u16, body: &str) -> ApiError {
    let detail = error_detail(body);

    match status {
        400 | 401 | 403 => ApiError::auth(format!("DeepL sign-in failed: {detail}")),
        429 => ApiError::rate_limit(
            "DeepL is receiving too many sign-in attempts. Wait a moment and try again.",
        ),
        500..=599 => ApiError::api(format!(
            "DeepL's sign-in service is having trouble right now (HTTP {status})."
        )),
        other => ApiError::api(format!(
            "DeepL's sign-in service returned an unexpected error (HTTP {other}): {detail}"
        )),
    }
}

fn error_detail(body: &str) -> String {
    #[derive(Deserialize)]
    struct ErrorBody {
        message: Option<String>,
        error: Option<String>,
        error_description: Option<String>,
    }

    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "no details".to_string();
    }

    if let Ok(parsed) = serde_json::from_str::<ErrorBody>(trimmed) {
        if let Some(description) = parsed
            .error_description
            .filter(|value| !value.trim().is_empty())
        {
            return truncate(description.trim(), 240);
        }
        if let Some(message) = parsed.message.filter(|value| !value.trim().is_empty()) {
            return truncate(message.trim(), 240);
        }
        if let Some(error) = parsed.error.filter(|value| !value.trim().is_empty()) {
            return truncate(error.trim(), 240);
        }
    }

    truncate(trimmed, 240)
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        let shortened: String = value.chars().take(max).collect();
        format!("{shortened}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_s256_of_verifier() {
        let pkce = generate_pkce().unwrap();
        let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(pkce.verifier.as_bytes()));

        assert_eq!(pkce.challenge, expected);
        assert!(pkce.verifier.len() >= 43);
        assert!(pkce.state.len() >= 16);
    }

    #[test]
    fn authorization_url_contains_pkce_and_resource() {
        let pkce = generate_pkce().unwrap();
        let url = authorization_url(
            &AuthServerMetadata::fallback(),
            "client-123",
            "http://127.0.0.1:53682/callback",
            &pkce,
        )
        .unwrap();
        let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();

        assert!(pairs.contains(&("client_id".into(), "client-123".into())));
        assert!(pairs.contains(&("code_challenge".into(), pkce.challenge.clone())));
        assert!(pairs.contains(&("code_challenge_method".into(), "S256".into())));
        assert!(pairs.contains(&("resource".into(), MCP_ENDPOINT.into())));
        assert!(pairs.contains(&("scope".into(), SCOPE.into())));
    }

    #[test]
    fn parses_the_loopback_callback() {
        let callback = parse_callback("/callback?code=abc&state=xyz").unwrap();
        assert_eq!(callback.code.as_deref(), Some("abc"));
        assert_eq!(callback.state.as_deref(), Some("xyz"));

        assert!(parse_callback("/favicon.ico").is_none());
    }

    #[test]
    fn extracts_oauth_error_descriptions() {
        let detail =
            error_detail(r#"{"error":"invalid_grant","error_description":"Code expired"}"#);
        assert_eq!(detail, "Code expired");
    }
}
