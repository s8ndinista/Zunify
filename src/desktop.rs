use crate::{
    app::{self, Item, Model},
    audio::Audio,
    listen::Listener,
    spotify::{self, Http, Session, item, query, split_uri, str_field},
};
use ferroui_base::{
    Ref,
    threading::{DispatcherPriority, DispatcherTimer},
};
use ferroui_controls::Window;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};

pub use spotify::REDIRECT;

const TRACK_AUDIO: [&[u8]; 6] = [
    include_bytes!("../wwwroot/audio/sample-1.wav"),
    include_bytes!("../wwwroot/audio/sample-2.wav"),
    include_bytes!("../wwwroot/audio/sample-3.wav"),
    include_bytes!("../wwwroot/audio/sample-4.wav"),
    include_bytes!("../wwwroot/audio/sample-5.wav"),
    include_bytes!("../wwwroot/audio/sample-6.wav"),
];

#[derive(Deserialize)]
struct Demo {
    tracks: Vec<Item>,
    mixes: Vec<Mix>,
}

#[derive(Deserialize)]
struct Mix {
    #[serde(flatten)]
    item: Item,
    tracks: Vec<String>,
}

fn demo_audio(id: &str) -> spotify::Result<&'static [u8]> {
    id.strip_prefix("demo:")
        .and_then(|number| number.parse::<usize>().ok())
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| TRACK_AUDIO.get(index).copied())
        .ok_or_else(|| "This demo track is unavailable.".into())
}

enum Update {
    SignedIn,
    Library {
        playlists: Vec<Item>,
        saved: Vec<Item>,
        warning: Option<String>,
    },
    View(Vec<Item>, String),
    Playback(Value),
    Queue(Vec<Item>),
    Liked(Item, bool),
    Device {
        id: String,
        started_playback: bool,
    },
    Command {
        refresh_queue: bool,
    },
}

#[derive(Clone, Copy)]
struct Request {
    generation: u64,
    page: Option<u64>,
    silent: bool,
    playback_poll: bool,
}

type Job = Box<dyn FnOnce(&mut Option<Session>) -> spotify::Result<Update> + Send>;

struct Network {
    generation: Arc<AtomicU64>,
    send: Sender<(Request, Job)>,
    updates: Receiver<(Request, spotify::Result<Update>)>,
    art_requests: Sender<String>,
    images: Receiver<(String, Vec<u8>)>,
}

impl Network {
    fn new() -> Self {
        let (send, work) = mpsc::channel::<(Request, Job)>();
        let (updates, receive) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(0));
        let current = generation.clone();
        std::thread::spawn(move || {
            let mut session = None;
            for (request, job) in work {
                if request.generation != current.load(Ordering::Acquire) {
                    continue;
                }
                let result = job(&mut session);
                if updates.send((request, result)).is_err() {
                    break;
                }
            }
        });
        let (art_requests, art_work) = mpsc::channel::<String>();
        let (art_results, images) = mpsc::channel();
        std::thread::spawn(move || {
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(6)))
                .max_redirects(0)
                .build()
                .into();
            for url in art_work {
                let bytes = spotify::artwork(&agent, &url).unwrap_or_default();
                if art_results.send((url, bytes)).is_err() {
                    break;
                }
            }
        });
        Self {
            generation,
            send,
            updates: receive,
            art_requests,
            images,
        }
    }
}

fn pin_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let config = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Application Support"));
    #[cfg(target_os = "windows")]
    let config = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    config.map(|dir| dir.join("SpotiFe").join("quickplay-pins.json"))
}

struct Desktop {
    model: Model,
    demo: Demo,
    demo_liked: HashSet<String>,
    demo_pins: Vec<String>,
    liked: HashSet<String>,
    known: HashMap<String, Item>,
    history: Vec<Item>,
    upcoming: VecDeque<Item>,
    order: Vec<Item>,
    audio: Option<Audio>,
    // Hears Spotify playing on this computer, which the app itself cannot decode.
    listener: Option<Listener>,
    network: Network,
    requested_art: HashSet<String>,
    selected_device: String,
    active_device: String,
    pending_play: Option<Value>,
    generation: u64,
    page_version: u64,
    sign_in: Option<Arc<AtomicBool>>,
    last_poll: Instant,
    poll_pending: bool,
    search_at: Option<Instant>,
    seek_at: Option<(Instant, f64)>,
    volume_at: Option<Instant>,
    position: f64,
    position_at: Instant,
}

thread_local! {
    static STATE: RefCell<Desktop> = RefCell::new(Desktop::new());
}

impl Desktop {
    fn new() -> Self {
        let demo: Demo = serde_json::from_str(include_str!("../wwwroot/demo-library.json"))
            .expect("bundled demo library is valid");
        let demo_pins = pin_path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok())
            .unwrap_or_else(|| {
                demo.mixes
                    .iter()
                    .take(2)
                    .map(|mix| mix.item.id.clone())
                    .collect()
            });
        let mut state = Self {
            model: Model {
                volume: 65.0,
                client_id: configured_client().unwrap_or_default(),
                ..Model::default()
            },
            demo,
            demo_pins,
            demo_liked: ["demo:1".to_owned(), "demo:3".to_owned()].into(),
            liked: HashSet::new(),
            known: HashMap::new(),
            history: Vec::new(),
            upcoming: VecDeque::new(),
            order: Vec::new(),
            audio: None,
            listener: None,
            network: Network::new(),
            requested_art: HashSet::new(),
            selected_device: String::new(),
            active_device: String::new(),
            pending_play: None,
            generation: 0,
            page_version: 0,
            sign_in: None,
            last_poll: Instant::now(),
            poll_pending: false,
            search_at: None,
            seek_at: None,
            volume_at: None,
            position: 0.0,
            position_at: Instant::now(),
        };
        state.reset_demo();
        state
    }

    fn reset_demo(&mut self) {
        self.known.clear();
        self.liked.clone_from(&self.demo_liked);
        self.model.page = "home".into();
        self.model.query.clear();
        self.model.detail.clear();
        self.model.status.clear();
        self.model.caption.clear();
        self.model.items = self.demo.tracks.clone();
        self.model.collections = self.demo.mixes.iter().map(|mix| mix.item.clone()).collect();
        self.known.extend(
            self.model
                .items
                .iter()
                .chain(&self.model.collections)
                .map(|item| (item.id.clone(), item.clone())),
        );
        self.model.pins = self
            .demo_pins
            .iter()
            .filter_map(|id| self.known.get(id).cloned())
            .take(6)
            .collect();
        self.model.recent.clear();
        self.model.current = self.demo.tracks[0].clone();
        self.order = self.demo.tracks.clone();
        self.upcoming = self.order.iter().skip(1).cloned().collect();
        self.model.queue = self.upcoming.iter().cloned().collect();
        self.model.connected = false;
        self.model.connecting = false;
        self.model.playing = false;
        self.model.repeat = false;
        self.model.shuffle = false;
        self.position = 0.0;
        self.listener = None;
        self.audio = None;
        self.history.clear();
        self.selected_device.clear();
        self.active_device.clear();
        self.pending_play = None;
    }

    fn display_model(&self) -> Model {
        let mut model = self.model.clone();
        for item in model
            .items
            .iter_mut()
            .chain(&mut model.queue)
            .chain(std::iter::once(&mut model.current))
        {
            item.liked = self.liked.contains(&item.id);
        }
        model
    }

    fn render(&self, body: bool, player: bool, reset: bool) {
        app::apply_model(
            self.display_model(),
            body || (player && matches!(self.model.page.as_str(), "home" | "queue")),
            player,
            reset,
        );
        if player {
            app::tick(self.position);
        }
    }

    fn status(&mut self, message: impl Into<String>) {
        self.model.status = message.into();
        self.render(true, false, false);
    }

    fn remember(&mut self, items: &[Item]) {
        self.known
            .extend(items.iter().map(|item| (item.id.clone(), item.clone())));
    }

    fn request(&self, page: Option<u64>, silent: bool, playback_poll: bool, job: Job) {
        let request = Request {
            generation: self.generation,
            page,
            silent,
            playback_poll,
        };
        let _ = self.network.send.send((request, job));
    }

    fn api<F>(&self, page: Option<u64>, silent: bool, playback_poll: bool, job: F)
    where
        F: FnOnce(&mut Session) -> spotify::Result<Update> + Send + 'static,
    {
        self.request(
            page,
            silent,
            playback_poll,
            Box::new(move |session| {
                let session = session
                    .as_mut()
                    .ok_or("Connect your Spotify account first.")?;
                job(session)
            }),
        );
    }

    fn connect(&mut self, client: &str) {
        let client = client.trim();
        self.model.client_id = client.to_owned();
        if client.len() != 32 || !client.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            self.status("Enter the 32-character Client ID from your Spotify developer app.");
            return;
        }
        if self.sign_in.is_some() || self.model.connected {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.sign_in = Some(cancel.clone());
        self.model.connecting = true;
        self.model.status.clear();
        self.render(true, false, false);
        let client = client.to_owned();
        self.request(
            None,
            false,
            false,
            Box::new(move |session| {
                let signed_in = spotify::authorize(client, &cancel)?;
                if cancel.load(Ordering::Relaxed) {
                    return Err("Spotify sign-in cancelled.".into());
                }
                *session = Some(signed_in);
                Ok(Update::SignedIn)
            }),
        );
    }

    fn disconnect(&mut self) {
        if let Some(cancel) = self.sign_in.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.generation += 1;
        self.network
            .generation
            .store(self.generation, Ordering::Release);
        self.page_version += 1;
        self.poll_pending = false;
        self.search_at = None;
        self.seek_at = None;
        self.volume_at = None;
        self.request(
            None,
            true,
            false,
            Box::new(|session| {
                *session = None;
                Ok(Update::Command {
                    refresh_queue: false,
                })
            }),
        );
        self.reset_demo();
        self.render(true, true, true);
    }

    fn saved_tracks(&self) -> Vec<Item> {
        let mut items: Vec<_> = self
            .liked
            .iter()
            .filter_map(|id| self.known.get(id))
            .cloned()
            .collect();
        items.sort_by(|left, right| left.title.cmp(&right.title));
        items
    }

    fn navigate(&mut self, page: &str, id: &str) {
        self.search_at = None;
        if page != "devices" {
            self.pending_play = None;
        }
        self.page_version += 1;
        self.model.page = page.into();
        self.model.detail = id.into();
        self.model.title = match page {
            "home" => "Quickplay",
            "search" => "Browse",
            "library" => "Collection",
            "liked" => "Liked songs",
            "devices" => "Playback devices",
            "detail" => self
                .known
                .get(id)
                .map_or("Collection", |item| item.title.as_str()),
            "account" => "Spotify account",
            _ => "Queue",
        }
        .into();
        self.model.status.clear();
        self.model.caption.clear();
        self.model.items = match page {
            "home" if self.model.connected => self.saved_tracks().into_iter().take(12).collect(),
            "home" => self.demo.tracks.clone(),
            "liked" => self.saved_tracks(),
            "library" => {
                let mut items = vec![section("liked", "Liked songs", "Saved tracks")];
                if self.model.connected {
                    items.push(section("albums", "Albums", "Saved albums"));
                    items.push(section("artists", "Artists", "Followed artists"));
                }
                self.remember(&items);
                items.extend(if self.model.connected {
                    self.saved_tracks()
                } else {
                    self.demo.tracks.clone()
                });
                items
            }
            "search" if !self.model.connected => self.search_demo(),
            "detail" if !self.model.connected => self
                .demo
                .mixes
                .iter()
                .find(|mix| mix.item.id == id)
                .map_or_else(Vec::new, |mix| {
                    mix.tracks
                        .iter()
                        .filter_map(|id| self.known.get(id).cloned())
                        .collect()
                }),
            _ => Vec::new(),
        };
        match page {
            "liked" if self.model.items.is_empty() => self.model.caption = "Tracks you save appear here.".into(),
            "search" if self.model.query.is_empty() => self.model.caption = "Search songs, artists, albums, or playlists.".into(),
            "search" if self.model.connected => self.model.caption = "Looking for music…".into(),
            "search" => self.model.caption = if self.model.items.is_empty() { "No results in the demo library." } else { "Results in the demo library" }.into(),
            "devices" if self.model.connected => self.model.caption = "Choose an active Spotify Connect device. Open Spotify on this computer to listen here.".into(),
            "devices" => self.model.caption = "The demo plays through this computer’s audio output.".into(),
            "detail" => self.model.caption = self.known.get(id).map_or_else(String::new, |item| item.artist.clone()),
            _ => {}
        }
        self.render(true, false, true);
        if !self.model.connected {
            return;
        }
        let version = self.page_version;
        match page {
            "search" if !self.model.query.is_empty() => {
                let terms = self.model.query.clone();
                self.api(Some(version), false, false, move |session| {
                    let data = session.get(&format!(
                        "/search?{}",
                        query(&[
                            ("q", &terms),
                            ("type", "track,album,artist,playlist"),
                            ("limit", "10")
                        ])
                    ))?;
                    let results = ["tracks", "albums", "artists", "playlists"]
                        .iter()
                        .flat_map(|kind| data[kind]["items"].as_array().into_iter().flatten())
                        .filter_map(item)
                        .collect::<Vec<_>>();
                    let caption = if results.is_empty() {
                        "No results found."
                    } else {
                        "Spotify search results"
                    };
                    Ok(Update::View(results, caption.into()))
                });
            }
            "detail" => {
                let target = self.known.get(id).cloned();
                if let Some(target) = target {
                    self.api(Some(version), false, false, move |session| {
                        collection(session, target)
                    });
                }
            }
            "devices" => {
                self.api(Some(version), false, false, |session| {
                    let data = session.get("/me/player/devices")?;
                    let devices = data["devices"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|device| device["is_restricted"].as_bool() != Some(true))
                        .filter_map(|device| {
                            let id = device["id"].as_str()?.to_owned();
                            let suffix = if device["is_active"].as_bool() == Some(true) {
                                " · Active"
                            } else {
                                ""
                            };
                            Some(Item {
                                id,
                                title: format!("{}{suffix}", str_field(device, "name")),
                                ..Item::default()
                            })
                        })
                        .collect::<Vec<_>>();
                    let caption = if devices.is_empty() {
                        "No devices found. Open Spotify on a device and refresh."
                    } else {
                        "Choose a Spotify Connect device."
                    };
                    Ok(Update::View(devices, caption.into()))
                });
            }
            _ => {}
        }
    }

    fn search_demo(&self) -> Vec<Item> {
        if self.model.query.is_empty() {
            return self.model.collections.clone();
        }
        let query = self.model.query.to_lowercase();
        self.demo
            .tracks
            .iter()
            .chain(&self.model.collections)
            .filter(|item| {
                let text = format!("{} {} {}", item.title, item.artist, item.album).to_lowercase();
                query.split_whitespace().all(|word| text.contains(word))
            })
            .cloned()
            .collect()
    }

    fn local_play(&mut self, item: Item) -> spotify::Result<()> {
        let bytes = demo_audio(&item.id)?;
        if self.audio.is_none() {
            self.audio = Some(Audio::new()?);
        }
        self.audio
            .as_ref()
            .unwrap()
            .play(bytes, self.model.volume)?;
        self.position = 0.0;
        self.model.playing = true;
        item.record_in(&mut self.model.recent);
        self.model.current = item;
        self.model.queue = self.upcoming.iter().cloned().collect();
        self.model.status.clear();
        self.render(false, true, false);
        Ok(())
    }

    fn play(&mut self, id: &str) -> spotify::Result<()> {
        let id = if id.is_empty() {
            &self.model.detail
        } else {
            id
        };
        let target = self.known.get(id).cloned().or_else(|| {
            self.model
                .items
                .iter()
                .find(|item| item.kind == "track")
                .cloned()
        });
        let Some(target) = target else { return Ok(()) };
        if self.model.connected {
            let payload = match target.kind.as_str() {
                "track" => json!({ "uris": [target.id] }),
                "album" | "playlist" => json!({ "context_uri": target.id }),
                _ => {
                    json!({ "uris": self.model.items.iter().filter(|item| item.kind == "track").take(50).map(|item| &item.id).collect::<Vec<_>>() })
                }
            };
            self.start_playback(payload);
            return Ok(());
        }
        let mix = self.demo.mixes.iter().find(|mix| mix.item.id == target.id);
        if let Some(mix) = mix {
            self.order = mix
                .tracks
                .iter()
                .filter_map(|id| self.known.get(id).cloned())
                .collect();
            self.upcoming = self.order.iter().skip(1).cloned().collect();
            self.history.clear();
            if let Some(first) = self.order.first().cloned() {
                self.local_play(first)?;
            }
        } else {
            if self.model.page == "queue" {
                if let Some(index) = self.upcoming.iter().position(|item| item.id == target.id) {
                    self.upcoming.drain(..=index);
                }
            } else {
                let items: Vec<_> = self
                    .model
                    .items
                    .iter()
                    .filter(|item| item.kind == "track")
                    .cloned()
                    .collect();
                self.order = if items.iter().any(|item| item.id == target.id) {
                    items
                } else {
                    self.demo.tracks.clone()
                };
                self.upcoming = self
                    .order
                    .iter()
                    .skip_while(|item| item.id != target.id)
                    .skip(1)
                    .cloned()
                    .collect();
                self.history.clear();
            }
            self.local_play(target)?;
        }
        Ok(())
    }

    fn next(&mut self, previous: bool, ended: bool) -> spotify::Result<()> {
        if self.model.connected {
            self.control(
                if previous { "previous" } else { "next" },
                Http::Post,
                &[],
                None,
            );
            return Ok(());
        }
        if previous {
            if self.position > 3.0 {
                return self.seek(0.0);
            }
            if let Some(item) = self.history.pop() {
                self.upcoming.push_front(self.model.current.clone());
                return self.local_play(item);
            }
            return self.seek(0.0);
        }
        if self.upcoming.is_empty() && self.model.repeat {
            self.upcoming = self.order.iter().cloned().collect();
        }
        let index = if self.model.shuffle && !self.upcoming.is_empty() {
            (getrandom::u64().unwrap_or_default() % self.upcoming.len() as u64) as usize
        } else {
            0
        };
        if let Some(item) = self.upcoming.remove(index) {
            self.history.push(self.model.current.clone());
            return self.local_play(item);
        }
        if ended {
            self.model.playing = false;
            self.position = self.model.current.duration;
            self.render(false, true, false);
        }
        Ok(())
    }

    fn toggle(&mut self) -> spotify::Result<()> {
        if self.model.connected {
            if self.model.current.id.is_empty() {
                if self.model.items.iter().any(|item| item.kind == "track") {
                    return self.play("");
                }
                self.status("Choose a track or playlist to start playback.");
                return Ok(());
            }
            if !self.model.playing && self.playback_device().is_none() {
                self.start_playback(json!({
                    "uris": [&self.model.current.id],
                    "position_ms": (self.position * 1000.0) as u64,
                }));
                return Ok(());
            }
            self.control(
                if self.model.playing { "pause" } else { "play" },
                Http::Put,
                &[],
                None,
            );
        } else if let Some(audio) = &self.audio
            && !audio.sink.empty()
        {
            self.model.playing = !self.model.playing;
            if self.model.playing {
                audio.sink.play();
            } else {
                audio.sink.pause();
            }
            self.render(false, true, false);
        } else {
            self.local_play(self.model.current.clone())?;
        }
        Ok(())
    }

    fn seek(&mut self, value: f64) -> spotify::Result<()> {
        if self.model.connected {
            self.seek_at = Some((Instant::now() + Duration::from_millis(180), value));
        } else if let Some(audio) = &self.audio {
            audio
                .sink
                .try_seek(Duration::from_secs_f64(value))
                .map_err(|error| format!("Could not seek: {error}"))?;
            self.position = value;
        }
        Ok(())
    }

    fn playback_device(&self) -> Option<&str> {
        [&self.selected_device, &self.active_device]
            .into_iter()
            .find(|id| !id.is_empty())
            .map(String::as_str)
    }

    fn start_playback(&mut self, payload: Value) {
        if self.playback_device().is_some() {
            self.control("play", Http::Put, &[], Some(payload));
        } else {
            self.pending_play = Some(payload);
            self.navigate("devices", "");
            self.status("Choose a playback device to start your selection.");
        }
    }

    fn control(&self, endpoint: &str, method: Http, params: &[(&str, &str)], body: Option<Value>) {
        let mut params = params.to_vec();
        if let Some(device) = self.playback_device() {
            params.push(("device_id", device));
        }
        let suffix = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", query(&params))
        };
        let path = format!("/me/player/{endpoint}{suffix}");
        let refresh_queue = matches!(endpoint, "queue" | "next" | "previous" | "play");
        self.api(None, false, false, move |session| {
            session.request(method, &path, body)?;
            Ok(Update::Command { refresh_queue })
        });
    }

    fn like(&mut self, id: &str) {
        let Some(item) = self.known.get(id).cloned() else {
            return;
        };
        let add = !self.liked.contains(id);
        if self.model.connected {
            let path = format!("/me/library?{}", query(&[("uris", id)]));
            self.api(None, false, false, move |session| {
                session.request(if add { Http::Put } else { Http::Delete }, &path, None)?;
                Ok(Update::Liked(item, add))
            });
        } else {
            self.apply_like(item, add);
            self.demo_liked.clone_from(&self.liked);
        }
    }

    fn apply_like(&mut self, item: Item, add: bool) {
        if add {
            self.liked.insert(item.id);
        } else {
            self.liked.remove(&item.id);
        }
        if self.model.page == "liked"
            || (matches!(self.model.page.as_str(), "home" | "library") && self.model.connected)
        {
            self.model.items = self.saved_tracks();
        }
        self.render(true, true, false);
    }

    fn poll_playback(&mut self) {
        if self.poll_pending {
            return;
        }
        self.last_poll = Instant::now();
        self.poll_pending = true;
        self.api(None, true, true, |session| {
            session.get("/me/player").map(Update::Playback)
        });
    }

    fn fetch_queue(&self) {
        self.api(None, true, false, |session| {
            let data = session.get("/me/player/queue")?;
            Ok(Update::Queue(
                data["queue"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(item)
                    .take(20)
                    .collect(),
            ))
        });
    }

    fn action(&mut self, action: &str, value: &str) -> spotify::Result<()> {
        match action {
            "art" => self.load_art(value),
            "settings" => {
                self.navigate("account", "");
                if !self.model.connected
                    && let Some(client) = configured_client()
                {
                    self.connect(&client);
                }
            }
            "dashboard" => {
                let _ = open::that_detached("https://developer.spotify.com/dashboard");
            }
            "connect" => self.connect(value),
            "disconnect" => self.disconnect(),
            "navigate" => {
                self.navigate(value, "");
                if value == "search" {
                    app::focus_search();
                }
            }
            "query" => {
                let value = value.trim();
                if self.model.query != value {
                    self.model.query = value.into();
                    self.search_at = Some(Instant::now() + Duration::from_millis(260));
                    self.page_version += 1;
                }
            }
            "open" if value == "section:liked" => self.navigate("liked", ""),
            "open" => self.navigate("detail", value),
            "pin" => {
                if let Some(index) = self.model.pins.iter().position(|item| item.id == value) {
                    self.model.pins.remove(index);
                } else if let Some(item) = self.known.get(value) {
                    if self.model.pins.len() >= 6 {
                        self.status("Quickplay holds six pins. Remove one to make room.");
                        return Ok(());
                    }
                    self.model.pins.push(item.clone());
                }
                if !self.model.connected {
                    self.demo_pins = self.model.pins.iter().map(|item| item.id.clone()).collect();
                    if let Some(path) = pin_path()
                        && let Some(parent) = path.parent()
                        && std::fs::create_dir_all(parent).is_ok()
                        && let Ok(bytes) = serde_json::to_vec(&self.demo_pins)
                    {
                        let temporary = path.with_extension("new");
                        if std::fs::write(&temporary, bytes).is_ok() {
                            let _ = std::fs::rename(temporary, path);
                        }
                    }
                }
                self.render(true, false, false);
            }
            "play" => self.play(value)?,
            "toggle" => self.toggle()?,
            "next" => self.next(false, false)?,
            "previous" => self.next(true, false)?,
            "like" => self.like(value),
            "enqueue" if self.model.connected => {
                self.control("queue", Http::Post, &[("uri", value)], None)
            }
            "enqueue" => {
                if let Some(item) = self.known.get(value).cloned() {
                    self.upcoming.push_back(item);
                    self.model.queue = self.upcoming.iter().cloned().collect();
                    self.render(false, true, false);
                }
            }
            "devices" => self.navigate("devices", ""),
            "transfer" if self.model.connected => {
                let device = value.to_owned();
                let started_playback = self.pending_play.is_some();
                let (path, body) = device_selection_request(
                    &device,
                    self.pending_play.clone(),
                    self.model.playing,
                );
                self.api(None, false, false, move |session| {
                    session.request(Http::Put, &path, Some(body))?;
                    Ok(Update::Device {
                        id: device,
                        started_playback,
                    })
                });
            }
            "seek" => {
                if let Some(value) = number(value, self.model.current.duration) {
                    self.seek(value)?;
                }
            }
            "volume" => {
                if let Some(value) = number(value, 100.0) {
                    self.model.volume = value.round();
                    if self.model.connected {
                        self.volume_at = Some(Instant::now() + Duration::from_millis(180));
                    } else if let Some(audio) = &self.audio {
                        audio.sink.set_volume((value / 100.0) as f32);
                    }
                }
            }
            "shuffle" => {
                self.model.shuffle = !self.model.shuffle;
                if self.model.connected {
                    self.control(
                        "shuffle",
                        Http::Put,
                        &[("state", if self.model.shuffle { "true" } else { "false" })],
                        None,
                    );
                }
                self.render(false, true, false);
            }
            "repeat" => {
                self.model.repeat = !self.model.repeat;
                if self.model.connected {
                    self.control(
                        "repeat",
                        Http::Put,
                        &[("state", if self.model.repeat { "context" } else { "off" })],
                        None,
                    );
                }
                self.render(false, true, false);
            }
            "spotify-link" => {
                if let Some((kind, id)) = split_uri(value) {
                    let _ = open::that_detached(format!("https://open.spotify.com/{kind}/{id}"));
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn load_art(&mut self, url: &str) {
        let bytes: Option<&[u8]> = match url {
            "art/hero.jpg" => Some(include_bytes!("../wwwroot/art/hero.jpg")),
            "art/mix-1.jpg" => Some(include_bytes!("../wwwroot/art/mix-1.jpg")),
            "art/mix-2.jpg" => Some(include_bytes!("../wwwroot/art/mix-2.jpg")),
            "art/mix-3.jpg" => Some(include_bytes!("../wwwroot/art/mix-3.jpg")),
            "art/mix-4.jpg" => Some(include_bytes!("../wwwroot/art/mix-4.jpg")),
            _ => None,
        };
        if let Some(bytes) = bytes {
            app::set_image(url, bytes);
        } else if self.requested_art.insert(url.to_owned()) {
            let _ = self.network.art_requests.send(url.to_owned());
        }
    }

    fn update(&mut self, request: Request, result: spotify::Result<Update>) {
        if request.generation != self.generation {
            return;
        }
        if request.playback_poll {
            self.poll_pending = false;
        }
        if request.page.is_some_and(|page| page != self.page_version) {
            return;
        }
        let update = match result {
            Ok(update) => update,
            Err(error) => {
                if self.model.connecting {
                    self.sign_in = None;
                    self.model.connecting = false;
                }
                if !request.silent {
                    self.status(error);
                }
                return;
            }
        };
        match update {
            Update::SignedIn => {
                self.sign_in = None;
                self.model.connecting = false;
                self.model.connected = true;
                self.model.current = Item::default();
                self.model.collections.clear();
                self.model.pins.clear();
                self.model.recent.clear();
                self.model.queue.clear();
                self.model.playing = false;
                self.position = 0.0;
                self.known.clear();
                self.liked.clear();
                self.audio = None;
                // Without the capture the fire falls back to its ambient glow.
                self.listener = Listener::start().ok();
                self.navigate("home", "");
                self.api(None, false, false, |session| {
                    let playlists = session.items("/me/playlists?limit=50", None);
                    let saved = session.items("/me/tracks?limit=50", None);
                    let warning = playlists.as_ref().err().or(saved.as_ref().err()).cloned();
                    let playlists = playlists
                        .unwrap_or_default()
                        .iter()
                        .filter_map(item)
                        .collect();
                    let saved = saved
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|row| {
                            item(row.get("track").or_else(|| row.get("item")).unwrap_or(row))
                        })
                        .collect();
                    Ok(Update::Library {
                        playlists,
                        saved,
                        warning,
                    })
                });
                self.poll_playback();
                self.fetch_queue();
            }
            Update::Library {
                playlists,
                saved,
                warning,
            } => {
                self.remember(&playlists);
                self.remember(&saved);
                self.model.collections = playlists;
                self.liked = saved.iter().map(|item| item.id.clone()).collect();
                let page = self.model.page.clone();
                if matches!(page.as_str(), "home" | "library" | "liked") {
                    self.navigate(&page, "");
                }
                if let Some(warning) = warning {
                    self.status(warning);
                }
            }
            Update::View(items, caption) => {
                self.remember(&items);
                self.model.items = items;
                self.model.caption = caption;
                self.render(true, false, false);
            }
            Update::Playback(data) => {
                let track = item(&data["item"]);
                let playing = data["is_playing"].as_bool().unwrap_or(false);
                let changed_track = track
                    .as_ref()
                    .is_some_and(|item| item.id != self.model.current.id);
                let volume = if self.volume_at.is_some() {
                    self.model.volume
                } else {
                    data["device"]["volume_percent"]
                        .as_f64()
                        .unwrap_or(self.model.volume)
                };
                let shuffle = data["shuffle_state"]
                    .as_bool()
                    .unwrap_or(self.model.shuffle);
                let repeat = data["repeat_state"]
                    .as_str()
                    .map_or(self.model.repeat, |state| state != "off");
                let changed = changed_track
                    || playing != self.model.playing
                    || volume != self.model.volume
                    || shuffle != self.model.shuffle
                    || repeat != self.model.repeat;
                self.active_device = str_field(&data["device"], "id").to_owned();
                self.position = data["progress_ms"]
                    .as_f64()
                    .unwrap_or(self.position * 1000.0)
                    / 1000.0;
                self.position_at = Instant::now();
                self.model.playing = playing;
                self.model.volume = volume;
                self.model.shuffle = shuffle;
                self.model.repeat = repeat;
                if let Some(track) = track {
                    self.remember(std::slice::from_ref(&track));
                    if playing {
                        track.record_in(&mut self.model.recent);
                    }
                    self.model.current = track;
                }
                if changed {
                    self.render(false, true, false);
                }
                if changed_track {
                    self.fetch_queue();
                }
            }
            Update::Queue(queue) => {
                self.remember(&queue);
                if queue.iter().map(|item| &item.id).ne(self
                    .model
                    .queue
                    .iter()
                    .map(|item| &item.id))
                {
                    self.model.queue = queue;
                    self.render(false, true, false);
                }
            }
            Update::Liked(item, add) => self.apply_like(item, add),
            Update::Device {
                id,
                started_playback,
            } => {
                self.selected_device = id;
                if started_playback {
                    self.pending_play = None;
                    self.status("Playback sent to the selected device.");
                    self.fetch_queue();
                } else {
                    self.status("Playback device selected.");
                }
                self.poll_playback();
            }
            Update::Command { refresh_queue } => {
                if self.model.connected {
                    self.poll_playback();
                    if refresh_queue {
                        self.fetch_queue();
                    }
                }
            }
        }
    }

    fn clock(&mut self) {
        while let Ok((request, update)) = self.network.updates.try_recv() {
            self.update(request, update);
        }
        while let Ok((url, bytes)) = self.network.images.try_recv() {
            self.requested_art.remove(&url);
            app::set_image(&url, &bytes);
        }
        let now = Instant::now();
        if self.search_at.is_some_and(|time| time <= now) {
            self.navigate(
                if self.model.query.is_empty() {
                    "home"
                } else {
                    "search"
                },
                "",
            );
        }
        if self.model.connected {
            if self.last_poll.elapsed() >= Duration::from_secs(7) {
                self.poll_playback();
            }
            if let Some((at, value)) = self.seek_at
                && at <= now
            {
                self.seek_at = None;
                self.position = value;
                self.position_at = now;
                self.control(
                    "seek",
                    Http::Put,
                    &[("position_ms", &((value * 1000.0) as u64).to_string())],
                    None,
                );
            }
            if self.volume_at.is_some_and(|at| at <= now) {
                self.volume_at = None;
                self.control(
                    "volume",
                    Http::Put,
                    &[("volume_percent", &self.model.volume.to_string())],
                    None,
                );
            }
            if self.seek_at.is_none() {
                let elapsed = if self.model.playing {
                    self.position_at.elapsed().as_secs_f64()
                } else {
                    0.0
                };
                app::tick((self.position + elapsed).min(self.model.current.duration));
            }
        } else if let Some(audio) = &self.audio {
            if self.model.playing && audio.sink.empty() {
                if let Err(error) = self.next(false, true) {
                    self.status(error);
                }
            } else if !audio.sink.empty() {
                self.position = audio.sink.get_pos().as_secs_f64();
                app::tick(self.position.min(self.model.current.duration));
            }
        }
    }
}

fn collection(session: &mut Session, target: Item) -> spotify::Result<Update> {
    let (rows, field) = if target.id == "section:albums" {
        (session.items("/me/albums?limit=50", None)?, Some("album"))
    } else if target.id == "section:artists" {
        (
            session.items("/me/following?type=artist&limit=50", Some("artists"))?,
            None,
        )
    } else {
        let (kind, id) = split_uri(&target.id).ok_or("This collection is unavailable.")?;
        match kind {
            "playlist" => (
                session.items(&format!("/playlists/{id}/items?limit=50"), None)?,
                Some("item"),
            ),
            "album" => (
                session.items(&format!("/albums/{id}/tracks?limit=50"), None)?,
                None,
            ),
            "artist" => {
                let term = format!("artist:\"{}\"", target.title);
                let response = session.get(&format!(
                    "/search?{}",
                    query(&[("q", &term), ("type", "track"), ("limit", "10")])
                ))?;
                (
                    response["tracks"]["items"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default(),
                    None,
                )
            }
            _ => return Err("This collection is unavailable.".into()),
        }
    };
    let items = rows
        .iter()
        .filter_map(|row| {
            let raw = field
                .and_then(|field| row.get(field))
                .or_else(|| row.get("track"))
                .unwrap_or(row);
            let mut item = item(raw)?;
            if item.art.is_empty() {
                item.art.clone_from(&target.art);
            }
            if item.album.is_empty() {
                item.album.clone_from(&target.title);
            }
            Some(item)
        })
        .collect::<Vec<_>>();
    let caption = if items.is_empty() {
        "Nothing here yet.".into()
    } else {
        target.artist
    };
    Ok(Update::View(items, caption))
}

fn device_selection_request(
    device: &str,
    pending: Option<Value>,
    playing: bool,
) -> (String, Value) {
    if let Some(payload) = pending {
        let params = query(&[("device_id", device)]);
        (format!("/me/player/play?{params}"), payload)
    } else {
        (
            "/me/player".into(),
            json!({"device_ids": [device], "play": playing}),
        )
    }
}

fn section(id: &str, title: &str, description: &str) -> Item {
    Item {
        id: format!("section:{id}"),
        title: title.into(),
        artist: description.into(),
        kind: "section".into(),
        ..Item::default()
    }
}

fn number(value: &str, max: f64) -> Option<f64> {
    value
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(0.0, max.max(0.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choosing_device_starts_the_pending_track_and_targets_the_device() {
        let payload = json!({"uris": ["spotify:track:123"]});
        assert_eq!(
            device_selection_request("connect-device", Some(payload.clone()), false),
            ("/me/player/play?device_id=connect-device".into(), payload),
        );
        assert_eq!(
            device_selection_request("connect-device", None, false),
            (
                "/me/player".into(),
                json!({"device_ids": ["connect-device"], "play": false}),
            ),
        );
    }
}

// The public OAuth client ID is safe to embed. Sign-in uses PKCE.
const DEFAULT_SPOTIFY_CLIENT_ID: &str = "50955714b13a4baab990559658592bca";

pub fn configured_client() -> Option<String> {
    std::env::var("SPOTIFY_CLIENT_ID")
        .ok()
        .or_else(|| option_env!("SPOTIFY_CLIENT_ID").map(str::to_owned))
        .or_else(|| Some(DEFAULT_SPOTIFY_CLIENT_ID.to_owned()))
        .filter(|value| !value.is_empty())
}

pub fn initial_model() -> Model {
    STATE.with(|state| state.borrow().display_model())
}

pub fn dispatch(action: &str, value: &str) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if let Err(error) = state.action(action, value) {
            state.status(error);
        }
    });
}

pub fn window_ready(window: &Ref<Window>) {
    let weak = window.downgrade();
    window
        .opened(move || {
            DispatcherTimer::run(
                {
                    let weak = weak.clone();
                    move || {
                        if weak.upgrade().is_none_or(|window| !window.is_visible()) {
                            return false;
                        }
                        STATE.with(|state| state.borrow_mut().clock());
                        true
                    }
                },
                Duration::from_millis(100),
                DispatcherPriority::NORMAL,
            )
            .detach();
        })
        .detach();
    window
        .closed(|| {
            STATE.with(|state| {
                let mut state = state.borrow_mut();
                if let Some(cancel) = state.sign_in.take() {
                    cancel.store(true, Ordering::Relaxed);
                }
                state.listener = None;
                state.audio = None;
            });
        })
        .detach();
}
