use keyring::Entry;

use crate::error::ApiError;

const SERVICE: &str = "com.stephen.deeplclient";
const API_KEY_ACCOUNT: &str = "deepl-api-key";
const MCP_SESSION_ACCOUNT: &str = "mcp-session";
const MCP_CLIENT_ACCOUNT: &str = "mcp-client";

fn entry(account: &str) -> Result<Entry, ApiError> {
    Entry::new(SERVICE, account).map_err(|error| {
        ApiError::storage(format!(
            "Could not open the system credential store: {error}"
        ))
    })
}

fn save_secret(account: &str, value: &str) -> Result<(), ApiError> {
    entry(account)?.set_password(value).map_err(|error| {
        ApiError::storage(format!(
            "Could not save credentials in the system credential store: {error}"
        ))
    })
}

fn load_secret(account: &str) -> Result<Option<String>, ApiError> {
    match entry(account)?.get_password() {
        Ok(value) if value.trim().is_empty() => Ok(None),
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(ApiError::storage(format!(
            "Could not read credentials from the system credential store: {error}"
        ))),
    }
}

fn delete_secret(account: &str) -> Result<(), ApiError> {
    match entry(account)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(ApiError::storage(format!(
            "Could not remove credentials from the system credential store: {error}"
        ))),
    }
}

// --- DeepL API key (the REST backend is currently shelved) -----------------

pub fn save_api_key(api_key: &str) -> Result<(), ApiError> {
    save_secret(API_KEY_ACCOUNT, api_key)
}

pub fn load_api_key() -> Result<Option<String>, ApiError> {
    load_secret(API_KEY_ACCOUNT)
}

pub fn delete_api_key() -> Result<(), ApiError> {
    delete_secret(API_KEY_ACCOUNT)
}

// --- MCP OAuth session ------------------------------------------------------

pub fn save_mcp_session(value: &str) -> Result<(), ApiError> {
    save_secret(MCP_SESSION_ACCOUNT, value)
}

pub fn load_mcp_session() -> Result<Option<String>, ApiError> {
    load_secret(MCP_SESSION_ACCOUNT)
}

pub fn delete_mcp_session() -> Result<(), ApiError> {
    delete_secret(MCP_SESSION_ACCOUNT)
}

// --- MCP dynamic client registration ---------------------------------------

pub fn save_mcp_client(value: &str) -> Result<(), ApiError> {
    save_secret(MCP_CLIENT_ACCOUNT, value)
}

pub fn load_mcp_client() -> Result<Option<String>, ApiError> {
    load_secret(MCP_CLIENT_ACCOUNT)
}

pub fn delete_mcp_client() -> Result<(), ApiError> {
    delete_secret(MCP_CLIENT_ACCOUNT)
}
