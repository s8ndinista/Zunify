use crate::{app::Item, spotify_player::SpotifyPlayer};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use tiny_http::{Header, Method, Response, Server, StatusCode};
use url::Url;

pub const REDIRECT: &str = "http://127.0.0.1:4176/spotify/callback";
const SCOPE: &str = "user-library-read user-library-modify user-follow-read playlist-read-private playlist-read-collaborative user-read-playback-state user-modify-playback-state user-read-currently-playing";
const PLAYBACK_SCOPE: &str = "streaming";
const API: &str = "https://api.spotify.com/v1";
const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";

pub type Result<T> = std::result::Result<T, String>;

#[derive(Deserialize)]
struct Token {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}

pub struct Session {
    player: Option<SpotifyPlayer>,
    playback_token: Option<(Token, Instant)>,
    agent: ureq::Agent,
    client_id: String,
    token: Token,
    expires: Instant,
    retry_after: Instant,
}

fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(12)))
        .max_redirects(0)
        .build()
        .into()
}

fn random_token<const N: usize>() -> Result<String> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|_| "Could not generate Spotify sign-in credentials.")?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub fn authorize(client_id: String, cancelled: &AtomicBool) -> Result<Session> {
    let server = Server::http("127.0.0.1:4176").map_err(
        |_| "Spotify sign-in needs port 4176. Close the other app using it and try again.",
    )?;
    let agent = http_agent();
    let token = authorize_token(&agent, &client_id, SCOPE, REDIRECT, server, || {
        cancelled.load(Ordering::Relaxed)
    })?;
    Ok(Session {
        player: None,
        playback_token: None,
        agent,
        client_id,
        expires: Instant::now() + Duration::from_secs(token.expires_in),
        retry_after: Instant::now(),
        token,
    })
}

fn authorize_token(
    agent: &ureq::Agent,
    client_id: &str,
    scope: &str,
    redirect: &str,
    server: Server,
    cancelled: impl Fn() -> bool,
) -> Result<Token> {
    let callback = Url::parse(redirect).unwrap();
    let authority = &callback[url::Position::BeforeHost..url::Position::AfterPort];
    if cancelled() {
        return Err("Spotify sign-in cancelled.".into());
    }
    let state = random_token::<32>()?;
    let verifier = random_token::<64>()?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut login = Url::parse("https://accounts.spotify.com/authorize").unwrap();
    login.query_pairs_mut().extend_pairs([
        ("client_id", client_id),
        ("response_type", "code"),
        ("redirect_uri", redirect),
        ("scope", scope),
        ("state", &state),
        ("code_challenge_method", "S256"),
        ("code_challenge", &challenge),
    ]);
    open::that_detached(login.as_str())
        .map_err(|_| "Could not open Spotify sign-in in your browser.")?;
    let expires = Instant::now() + Duration::from_secs(180);
    let code = loop {
        if cancelled() {
            return Err("Spotify sign-in cancelled.".into());
        }
        if Instant::now() >= expires {
            return Err("Spotify sign-in expired. Please try again.".into());
        }
        let request = server
            .recv_timeout(Duration::from_millis(200))
            .map_err(|_| "Could not read Spotify’s sign-in response.")?;
        let Some(request) = request else { continue };
        let expected_host = request
            .headers()
            .iter()
            .any(|header| header.field.equiv("Host") && header.value.as_str() == authority);
        if request.method() != &Method::Get || !expected_host {
            reply(
                request,
                404,
                "This address is for the Zunify sign-in callback.",
            );
            continue;
        }
        match callback_result(&callback, request.url(), &state) {
            Callback::Code(code) => {
                reply(
                    request,
                    200,
                    "You can close this tab and return to Zunify to finish connecting.",
                );
                break code;
            }
            Callback::Cancelled => {
                reply(
                    request,
                    200,
                    "Spotify sign-in was cancelled. Return to Zunify.",
                );
                return Err("Spotify sign-in was cancelled.".into());
            }
            Callback::Invalid => {
                reply(
                    request,
                    400,
                    "The sign-in response did not match this request. Return to Zunify and try again.",
                );
            }
        }
    };
    drop(server);
    exchange(
        agent,
        [
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", redirect),
            ("client_id", client_id),
            ("code_verifier", &verifier),
        ],
    )
}

#[derive(Debug, PartialEq, Eq)]
enum Callback {
    Code(String),
    Cancelled,
    Invalid,
}

fn callback_result(redirect: &Url, target: &str, expected_state: &str) -> Callback {
    let Ok(url) = redirect.join(target) else {
        return Callback::Invalid;
    };
    if url.origin() != redirect.origin() || url.path() != redirect.path() {
        return Callback::Invalid;
    }
    let mut states = url.query_pairs().filter(|(key, _)| key == "state");
    if states
        .next()
        .is_none_or(|(_, value)| value != expected_state)
        || states.next().is_some()
    {
        return Callback::Invalid;
    }
    if url.query_pairs().any(|(key, _)| key == "error") {
        return Callback::Cancelled;
    }
    let mut codes = url.query_pairs().filter(|(key, _)| key == "code");
    let Some((_, code)) = codes.next() else {
        return Callback::Invalid;
    };
    if code.is_empty() || codes.next().is_some() {
        return Callback::Invalid;
    }
    Callback::Code(code.into_owned())
}

fn reply(request: tiny_http::Request, status: u16, message: &str) {
    let response = Response::from_string(message)
        .with_status_code(StatusCode(status))
        .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
        .with_header(Header::from_bytes("Referrer-Policy", "no-referrer").unwrap());
    let _ = request.respond(response);
}

fn exchange<'a>(
    agent: &ureq::Agent,
    form: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<Token> {
    agent
        .post(TOKEN_URL)
        .send_form(form)
        .map_err(
            |_| "Spotify rejected sign-in. Check the Client ID and redirect URL, then try again.",
        )?
        .body_mut()
        .read_json()
        .map_err(|_| "Spotify returned an invalid sign-in response.".into())
}

#[derive(Clone, Copy)]
pub enum Http {
    Get,
    Put,
    Post,
    Delete,
}

impl Session {
    pub(crate) fn local_playback_notice(&self) -> Option<&'static str> {
        self.player.as_ref().and_then(SpotifyPlayer::playback_notice)
    }

    pub(crate) fn local_device(&self) -> Option<&str> {
        self.player.as_ref().filter(|player| player.is_running()).map(SpotifyPlayer::device_id)
    }

    pub(crate) fn start_local_player(
        &mut self,
        volume: f64,
        cancelled: impl Fn() -> bool,
    ) -> Result<String> {
        if let Some(device) = self.local_device() {
            return Ok(device.to_owned());
        }
        self.player = None;
        let access_token = self.local_access_token(&cancelled)?;
        if cancelled() {
            return Err("Spotify playback authorization cancelled.".into());
        }
        let player = SpotifyPlayer::connect(access_token, volume).inspect_err(|_| {
            // Allow a fresh authorization after Spotify rejects the credential.
            self.playback_token = None;
        })?;
        let id = player.device_id().to_owned();
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            if cancelled() {
                return Err("Spotify playback authorization cancelled.".into());
            }
            let devices = self.get("/me/player/devices")?;
            if devices["devices"].as_array().is_some_and(|devices| {
                devices.iter().any(|device| device["id"].as_str() == Some(id.as_str()))
            }) {
                self.player = Some(player);
                return Ok(id);
            }
            if !player.is_running() || Instant::now() >= deadline {
                return Err("Spotify did not make Zunify available. Use the same Spotify account for sign-in and playback authorization.".into());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    fn local_access_token(&mut self, cancelled: impl Fn() -> bool) -> Result<String> {
        // Spotify's native login accepts credentials issued to the player client.
        // The Web API authorization is tied to Zunify's developer app instead.
        let client = librespot_core::SessionConfig::default().client_id;
        if let Some((token, expiry)) = &mut self.playback_token {
            if *expiry > Instant::now() + Duration::from_secs(30) {
                return Ok(token.access_token.clone());
            }
            if let Some(refresh) = token.refresh_token.as_deref()
                && let Ok(mut renewed) = exchange(
                    &self.agent,
                    [
                        ("grant_type", "refresh_token"),
                        ("refresh_token", refresh),
                        ("client_id", &client),
                    ],
                )
            {
                renewed.refresh_token = renewed.refresh_token.or_else(|| token.refresh_token.take());
                *expiry = Instant::now() + Duration::from_secs(renewed.expires_in);
                *token = renewed;
                return Ok(token.access_token.clone());
            }
        }
        let server = Server::http("127.0.0.1:0")
            .map_err(|_| "Could not open the Spotify playback authorization callback.")?;
        let addr = server.server_addr().to_ip().expect("IPv4 loopback");
        let redirect = format!("http://{addr}/login");
        let token = authorize_token(&self.agent, &client, PLAYBACK_SCOPE, &redirect, server, cancelled)
            .map_err(|error| format!("Could not authorize local Spotify playback. {error}"))?;
        let access_token = token.access_token.clone();
        let expires = Instant::now() + Duration::from_secs(token.expires_in);
        self.playback_token = Some((token, expires));
        Ok(access_token)
    }

    fn refresh(&mut self) -> Result<()> {
        let refresh = self
            .token
            .refresh_token
            .as_deref()
            .ok_or("Your Spotify session expired. Sign in again.")?;
        let mut token = exchange(
            &self.agent,
            [
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh),
                ("client_id", &self.client_id),
            ],
        )?;
        token.refresh_token = token
            .refresh_token
            .or_else(|| self.token.refresh_token.take());
        self.expires = Instant::now() + Duration::from_secs(token.expires_in);
        self.token = token;
        Ok(())
    }

    pub fn request(&mut self, method: Http, path: &str, body: Option<Value>) -> Result<Value> {
        if Instant::now() < self.retry_after {
            return Err("Spotify is limiting requests. Try again in a moment.".into());
        }
        if self.expires <= Instant::now() + Duration::from_secs(30) {
            self.refresh()?;
        }
        let url = format!("{API}{path}");
        for attempt in 0..2 {
            let authorization = format!("Bearer {}", self.token.access_token);
            let result = match method {
                Http::Get => self
                    .agent
                    .get(&url)
                    .header("Authorization", &authorization)
                    .call(),
                Http::Put => {
                    let request = self.agent.put(&url).header("Authorization", &authorization);
                    match &body {
                        Some(value) => request.send_json(value),
                        None => request.send_empty(),
                    }
                }
                Http::Post => self
                    .agent
                    .post(&url)
                    .header("Authorization", &authorization)
                    .send_empty(),
                Http::Delete => self
                    .agent
                    .delete(&url)
                    .header("Authorization", &authorization)
                    .call(),
            };
            match result {
                // Write endpoints acknowledge the command without returning data we use.
                Ok(response) if !matches!(method, Http::Get) || response.status().as_u16() == 204 => {
                    return Ok(Value::Null);
                }
                Ok(mut response) => {
                    let bytes = response.body_mut().with_config().limit(8 * 1024 * 1024).read_to_vec()
                        .map_err(|_| "Could not read Spotify’s response.")?;
                    return if bytes.is_empty() { Ok(Value::Null) } else {
                        serde_json::from_slice(&bytes).map_err(|_| "Spotify returned an unreadable response.".into())
                    };
                }
                Err(ureq::Error::StatusCode(401)) if attempt == 0 => self.refresh()?,
                Err(ureq::Error::StatusCode(403)) => return Err("Spotify denied the request. Check Premium and access to your developer app.".into()),
                Err(ureq::Error::StatusCode(404)) => return Err("Spotify could not find the item or playback device. Open Spotify on a device and try again.".into()),
                Err(ureq::Error::StatusCode(429)) => {
                    self.retry_after = Instant::now() + Duration::from_secs(15);
                    return Err("Spotify is limiting requests. Try again in a moment.".into());
                }
                Err(error) => return Err(format!("Spotify request failed: {error}")),
            }
        }
        Err("Your Spotify session expired. Sign in again.".into())
    }

    pub fn get(&mut self, path: &str) -> Result<Value> {
        self.request(Http::Get, path, None)
    }

    pub fn items(&mut self, path: &str, field: Option<&str>) -> Result<Vec<Value>> {
        self.items_up_to(path, field, 100)
    }

    pub fn items_up_to(&mut self, path: &str, field: Option<&str>, limit: usize) -> Result<Vec<Value>> {
        let mut next = Some(path.to_owned());
        let mut items = Vec::new();
        for _ in 0..10 {
            let Some(path) = next.take() else { break };
            let data = self.get(&path)?;
            let page = field.map_or(&data, |field| &data[field]);
            if let Some(rows) = page["items"].as_array() {
                items.extend(rows.iter().take(limit - items.len()).cloned());
            }
            if items.len() >= limit {
                break;
            }
            next = page["next"]
                .as_str()
                .and_then(|next| next.strip_prefix(API))
                .filter(|next| next.starts_with('/') && *next != path)
                .map(str::to_owned);
        }
        Ok(items)
    }
}

pub fn str_field<'a>(value: &'a Value, name: &str) -> &'a str {
    value[name].as_str().unwrap_or_default()
}

pub fn item(value: &Value) -> Option<Item> {
    let id = str_field(value, "uri");
    let (kind, _) = split_uri(id)?;
    if value["is_local"].as_bool() == Some(true) || value["is_playable"].as_bool() == Some(false) {
        return None;
    }
    let artist = value["artists"]
        .as_array()
        .map(|artists| {
            artists
                .iter()
                .map(|artist| str_field(artist, "name"))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| str_field(&value["owner"], "display_name").to_owned());
    let images = value["album"]["images"]
        .as_array()
        .or_else(|| value["images"].as_array());
    let art = images
        .and_then(|images| {
            images
                .iter()
                .filter_map(|image| image["url"].as_str())
                .next()
        })
        .unwrap_or_default();
    Some(Item {
        id: id.to_owned(),
        kind: kind.to_owned(),
        title: str_field(value, "name").to_owned(),
        artist,
        album: str_field(&value["album"], "name").to_owned(),
        art: art.to_owned(),
        duration: value["duration_ms"].as_f64().unwrap_or_default() / 1000.0,
        liked: false,
    })
}

pub fn split_uri(uri: &str) -> Option<(&str, &str)> {
    let (kind, id) = uri.strip_prefix("spotify:")?.split_once(':')?;
    (matches!(kind, "track" | "album" | "artist" | "playlist")
        && !id.is_empty()
        && id.bytes().all(|byte| byte.is_ascii_alphanumeric()))
    .then_some((kind, id))
}

pub fn query(pairs: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs.iter().copied())
        .finish()
}

pub fn artwork(agent: &ureq::Agent, url: &str) -> Option<Vec<u8>> {
    let parsed = Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    if parsed.scheme() != "https"
        || !["scdn.co", "spotifycdn.com", "spotify.com"]
            .iter()
            .any(|base| host == *base || host.ends_with(&format!(".{base}")))
    {
        return None;
    }
    agent
        .get(parsed.as_str())
        .call()
        .ok()?
        .body_mut()
        .with_config()
        .limit(4 * 1024 * 1024)
        .read_to_vec()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_callback_requires_the_current_state_and_exact_redirect() {
        let redirect = Url::parse(REDIRECT).unwrap();
        let state = "session-state";
        assert_eq!(
            callback_result(
                &redirect,
                "/spotify/callback?code=one-time&state=session-state",
                state
            ),
            Callback::Code("one-time".into()),
        );
        assert_eq!(
            callback_result(
                &redirect,
                "/spotify/callback?error=access_denied&state=session-state",
                state
            ),
            Callback::Cancelled,
        );
        for target in [
            "/spotify/callback?code=one-time",
            "/spotify/callback?code=one-time&state=other",
            "/spotify/callback?code=one-time&state=session-state&state=other",
            "/spotify/callback?code=one-time&code=duplicate&state=session-state",
            "/different?code=one-time&state=session-state",
            "http://127.0.0.1:4177/spotify/callback?code=one-time&state=session-state",
        ] {
            assert_eq!(
                callback_result(&redirect, target, state),
                Callback::Invalid,
                "{target}"
            );
        }
    }
}
