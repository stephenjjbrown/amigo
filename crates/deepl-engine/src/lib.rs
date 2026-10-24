//! Command engine: owns the provider clients and OAuth session, and runs on its
//! own thread. The UI only sends commands and polls events, so network and
//! browser sign-in never block a frame.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use deepl_core::{
    fetch_status, is_supported_pair, mcp, session_store, ApiError, ErrorKind, Language, McpClient,
    McpSession, OllamaClient, OllamaStatus, Provider, Register, Sampling, SessionStatus,
    Translation, MAX_TRANSLATION_CHARS,
};

pub type RepaintCallback = Arc<dyn Fn() + Send + Sync>;

#[derive(Debug, Clone)]
pub enum Command {
    LoadSession,
    SignIn,
    SignOut,
    LoadOllamaStatus,
    Translate {
        request_id: u64,
        text: String,
        jobs: Vec<ProviderJob>,
    },
}

#[derive(Debug, Clone)]
pub enum ProviderJob {
    DeepL {
        slot_id: u64,
        source: Language,
        target: Language,
    },
    Ollama {
        slot_id: u64,
        source: Language,
        target: Language,
        model: String,
        register: Register,
        context: Option<String>,
        sampling: Sampling,
        count: usize,
    },
}

#[derive(Debug, Clone)]
pub enum Event {
    /// The engine could not start for a non-recoverable reason.
    Fatal(ApiError),
    SessionLoaded(Result<SessionStatus, ApiError>),
    SignInStarted,
    SignedIn(Result<SessionStatus, ApiError>),
    SignedOut(Result<SessionStatus, ApiError>),
    OllamaStatus(Result<OllamaStatus, ApiError>),
    ProviderTranslation {
        request_id: u64,
        slot_id: u64,
        provider: Provider,
        result: Result<Vec<Translation>, ApiError>,
        elapsed_ms: u128,
    },
}

pub struct Engine {
    commands: Sender<Command>,
    events: Receiver<Event>,
    cancel_sign_in: Arc<AtomicBool>,
}

impl Engine {
    pub fn spawn(notify: RepaintCallback) -> Self {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let cancel_sign_in = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel_sign_in.clone();

        thread::Builder::new()
            .name("translation-engine".to_string())
            .spawn(move || {
                let http = match reqwest::blocking::Client::builder()
                    .user_agent(concat!("TranslationAmigo/", env!("CARGO_PKG_VERSION")))
                    .timeout(Duration::from_secs(30))
                    .build()
                {
                    Ok(http) => http,
                    Err(error) => {
                        let _ = event_tx.send(Event::Fatal(ApiError::network(format!(
                            "Could not start the HTTP client: {error}"
                        ))));
                        notify();
                        return;
                    }
                };

                Worker {
                    http,
                    session: None,
                    client: None,
                    cancel_sign_in: worker_cancel,
                }
                .run(command_rx, event_tx, notify);
            })
            .expect("failed to spawn the translation engine thread");

        Self {
            commands: command_tx,
            events: event_rx,
            cancel_sign_in,
        }
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// Signal the in-flight browser sign-in to stop. This is set directly
    /// because the engine thread is blocked waiting for the OAuth callback.
    pub fn cancel_sign_in(&self) {
        self.cancel_sign_in.store(true, Ordering::SeqCst);
    }

    pub fn try_recv(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }
}

struct Worker {
    http: reqwest::blocking::Client,
    session: Option<McpSession>,
    client: Option<McpClient>,
    cancel_sign_in: Arc<AtomicBool>,
}

impl Worker {
    fn run(mut self, commands: Receiver<Command>, events: Sender<Event>, notify: RepaintCallback) {
        while let Ok(command) = commands.recv() {
            self.handle(command, &events, &notify);
            notify();
        }
    }

    fn handle(&mut self, command: Command, events: &Sender<Event>, notify: &RepaintCallback) {
        match command {
            Command::LoadSession => {
                let result = self.load_session();
                match &result {
                    Ok(status) => log::info!("session loaded: signed_in={}", status.signed_in),
                    Err(error) => log::warn!("session load failed: {error}"),
                }
                let _ = events.send(Event::SessionLoaded(result));
            }
            Command::SignIn => {
                self.cancel_sign_in.store(false, Ordering::SeqCst);
                log::info!("sign-in started");
                let _ = events.send(Event::SignInStarted);
                notify();
                let result = self.sign_in();
                match &result {
                    Ok(_) => log::info!("sign-in completed"),
                    Err(error) => log::warn!("sign-in failed: {error}"),
                }
                let _ = events.send(Event::SignedIn(result));
            }
            Command::SignOut => {
                log::info!("sign-out requested");
                let result = self.sign_out();
                match &result {
                    Ok(_) => log::info!("signed out"),
                    Err(error) => log::warn!("sign-out failed: {error}"),
                }
                let _ = events.send(Event::SignedOut(result));
            }
            Command::LoadOllamaStatus => {
                let result = self.ollama_status();
                match &result {
                    Ok(status) => log::info!(
                        "Ollama status: running={}, models={}",
                        status.running,
                        status.models.len()
                    ),
                    Err(error) => log::warn!("Ollama status check failed: {error}"),
                }
                let _ = events.send(Event::OllamaStatus(result));
            }
            Command::Translate {
                request_id,
                text,
                jobs,
            } => {
                log::info!(
                    "translate request {request_id}: {} provider(s), {} chars",
                    jobs.len(),
                    text.chars().count()
                );

                for job in jobs {
                    let started = Instant::now();
                    let (slot_id, provider, result) = match job {
                        ProviderJob::DeepL {
                            slot_id,
                            source,
                            target,
                        } => (
                            slot_id,
                            Provider::DeepL,
                            self.translate_deepl(&text, source, target)
                                .map(|translation| vec![translation]),
                        ),
                        ProviderJob::Ollama {
                            slot_id,
                            source,
                            target,
                            model,
                            register,
                            context,
                            sampling,
                            count,
                        } => (
                            slot_id,
                            Provider::Ollama,
                            self.translate_ollama(
                                &text,
                                source,
                                target,
                                &model,
                                register,
                                context.as_deref(),
                                sampling,
                                count,
                            ),
                        ),
                    };
                    let elapsed_ms = started.elapsed().as_millis();

                    match &result {
                        Ok(translations) => log::info!(
                            "{} translation for request {request_id} (slot {slot_id}) succeeded in {elapsed_ms}ms: {} variant(s)",
                            provider.label(),
                            translations.len()
                        ),
                        Err(error) => log::warn!(
                            "{} translation for request {request_id} (slot {slot_id}) failed in {elapsed_ms}ms: {error}",
                            provider.label()
                        ),
                    }

                    let _ = events.send(Event::ProviderTranslation {
                        request_id,
                        slot_id,
                        provider,
                        result,
                        elapsed_ms,
                    });
                    notify();
                }
            }
        }
    }

    fn load_session(&mut self) -> Result<SessionStatus, ApiError> {
        log::info!("loading saved session");

        let Some(stored) = session_store::load_mcp_session()? else {
            log::info!("no saved session");
            return Ok(SessionStatus::signed_out());
        };

        log::debug!("saved session found; decoding");

        let mut session: McpSession = serde_json::from_str(&stored).map_err(|error| {
            ApiError::storage(format!("The stored DeepL session is invalid: {error}"))
        })?;

        match mcp::refresh_session(&self.http, &mut session) {
            Ok(changed) => {
                if changed {
                    self.save_session(&session)?;
                }
            }
            Err(error) if error.kind == ErrorKind::SessionExpired => {
                let _ = session_store::delete_mcp_session();
                self.session = None;
                self.client = None;
                return Ok(SessionStatus::signed_out());
            }
            Err(_) => {
                // Offline at launch: keep the session and retry on demand.
            }
        }

        self.install_session(session)?;
        Ok(SessionStatus::signed_in())
    }

    fn sign_in(&mut self) -> Result<SessionStatus, ApiError> {
        let session = mcp::sign_in(self.cancel_sign_in.clone(), &self.http)?;

        let mut client = McpClient::new(&session)?;
        client.verify()?;

        self.save_session(&session)?;
        self.session = Some(session);
        self.client = Some(client);

        Ok(SessionStatus::signed_in())
    }

    fn sign_out(&mut self) -> Result<SessionStatus, ApiError> {
        if let Some(session) = &self.session {
            mcp::revoke(&self.http, session);
        }

        self.session = None;
        self.client = None;

        session_store::delete_mcp_session()?;
        Ok(SessionStatus::signed_out())
    }

    fn ollama_status(&mut self) -> Result<OllamaStatus, ApiError> {
        fetch_status()
    }

    fn translate_deepl(
        &mut self,
        text: &str,
        source: Language,
        target: Language,
    ) -> Result<Translation, ApiError> {
        validate_request(text, source, target)?;

        let refresh = {
            let session = self.session.as_mut().ok_or_else(|| {
                ApiError::new(
                    ErrorKind::NotConfigured,
                    "Sign in with DeepL to translate with DeepL.",
                )
            })?;
            mcp::refresh_session(&self.http, session)
        };

        match refresh {
            Ok(true) => {
                let refreshed = self.session.clone().ok_or_else(|| {
                    ApiError::new(
                        ErrorKind::NotConfigured,
                        "Sign in with DeepL to translate with DeepL.",
                    )
                })?;
                self.save_session(&refreshed)?;
                self.client = Some(McpClient::new(&refreshed)?);
            }
            Ok(false) => {}
            Err(error) if error.kind == ErrorKind::SessionExpired => {
                let _ = session_store::delete_mcp_session();
                self.session = None;
                self.client = None;
                return Err(ApiError::auth_expired());
            }
            Err(error) => return Err(error),
        }

        let session = self.session.as_ref().ok_or_else(|| {
            ApiError::new(
                ErrorKind::NotConfigured,
                "Sign in with DeepL to translate with DeepL.",
            )
        })?;

        if self.client.is_none() {
            self.client = Some(McpClient::new(session)?);
        }

        let result = self
            .client
            .as_mut()
            .expect("MCP client initialized")
            .translate(text, source, target);

        if let Err(error) = &result {
            if error.kind == ErrorKind::SessionExpired {
                let _ = session_store::delete_mcp_session();
                self.session = None;
                self.client = None;
            }
        }

        result
    }

    #[allow(clippy::too_many_arguments)]
    fn translate_ollama(
        &mut self,
        text: &str,
        source: Language,
        target: Language,
        model: &str,
        register: Register,
        context: Option<&str>,
        sampling: Sampling,
        count: usize,
    ) -> Result<Vec<Translation>, ApiError> {
        validate_request(text, source, target)?;

        let client = OllamaClient::new(model)?;
        client.translate_variants(text, source, target, register, context, sampling, count)
    }

    fn install_session(&mut self, session: McpSession) -> Result<(), ApiError> {
        let client = McpClient::new(&session)?;
        self.session = Some(session);
        self.client = Some(client);
        Ok(())
    }

    fn save_session(&self, session: &McpSession) -> Result<(), ApiError> {
        let encoded = serde_json::to_string(session).map_err(|error| {
            ApiError::storage(format!("Could not encode the DeepL session: {error}"))
        })?;
        session_store::save_mcp_session(&encoded)
    }
}

fn validate_request(text: &str, source: Language, target: Language) -> Result<(), ApiError> {
    if text.trim().is_empty() {
        return Err(ApiError::input("There is no text to translate."));
    }

    if !is_supported_pair(source, target) {
        return Err(ApiError::input(
            "That language pair is not supported by this version of the app.",
        ));
    }

    let characters = text.chars().count();
    if characters > MAX_TRANSLATION_CHARS {
        return Err(ApiError::input(format!(
            "That text is {characters} characters long. Requests are limited to {MAX_TRANSLATION_CHARS} characters."
        )));
    }

    Ok(())
}
