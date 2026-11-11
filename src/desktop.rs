use crate::{
    app::{self, Item, Model},
    audio::Audio,
    local_media::{self, Cover, Library},
    spectrum,
    spotify::{self, Http, Session, item, query, split_uri, str_field},
    spotify_player::{LOCAL_DEVICE, is_playback_notice},
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
    LocalCover(String, Vec<u8>),
    Playback(Value, bool, Option<&'static str>),
    Queue(Vec<Item>),
    Enqueued(Vec<Item>, Option<String>),
    Liked(Item, bool),
    StartedPlayback(String, bool),
    PlaybackNeedsDevice(Value, String),
    Device {
        id: String,
        local: bool,
        started_playback: bool,
    },
    Command {
        refresh_queue: bool,
    },
    Seek {
        target: SeekTarget,
        result: spotify::Result<()>,
    },
}

struct SeekTarget {
    track: String,
    device: String,
    seconds: f64,
    due: Instant,
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
    art_requests: Sender<(String, Option<Cover>)>,
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
        let (art_requests, art_work) = mpsc::channel::<(String, Option<Cover>)>();
        let (art_results, images) = mpsc::channel();
        std::thread::spawn(move || {
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(6)))
                .max_redirects(0)
                .build()
                .into();
            for (url, local) in art_work {
                let bytes = match local {
                    Some(cover) => cover.read(),
                    None => spotify::artwork(&agent, &url),
                }.unwrap_or_default();
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

struct Desktop {
    model: Model,
    demo: Demo,
    demo_liked: HashSet<String>,
    liked: HashSet<String>,
    known: HashMap<String, Item>,
    history: Vec<Item>,
    upcoming: VecDeque<Item>,
    order: Vec<Item>,
    audio: Option<Audio>,
    files: Library,
    file_playback: bool,
    folder_version: u64,
    importing: bool,
    folder_result: Option<Receiver<Result<Library, String>>>,
    cover_requested_at: Option<Instant>,
    network: Network,
    requested_art: HashSet<String>,
    selected_device: String,
    active_device: String,
    pending_play: Option<Value>,
    generation: u64,
    page_version: u64,
    back_stack: VecDeque<(String, String, String)>,
    page_query: String,
    sign_in: Option<Arc<AtomicBool>>,
    last_poll: Instant,
    poll_pending: bool,
    poll_again_at: Option<Instant>,
    search_at: Option<Instant>,
    seek_at: Option<SeekTarget>,
    seek_pending: bool,
    seek_hold_until: Option<Instant>,
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
        let mut state = Self {
            model: Model {
                volume: 65.0,
                client_id: configured_client().unwrap_or_default(),
                ..Model::default()
            },
            demo,
            demo_liked: ["demo:1".to_owned(), "demo:3".to_owned()].into(),
            liked: HashSet::new(),
            known: HashMap::new(),
            history: Vec::new(),
            upcoming: VecDeque::new(),
            order: Vec::new(),
            audio: None,
            files: Library::default(),
            file_playback: false,
            folder_version: 0,
            importing: false,
            folder_result: None,
            cover_requested_at: None,
            network: Network::new(),
            requested_art: HashSet::new(),
            selected_device: String::new(),
            active_device: String::new(),
            pending_play: None,
            generation: 0,
            page_version: 0,
            back_stack: VecDeque::new(),
            page_query: String::new(),
            sign_in: None,
            last_poll: Instant::now(),
            poll_pending: false,
            poll_again_at: None,
            search_at: None,
            seek_at: None,
            seek_pending: false,
            seek_hold_until: None,
            volume_at: None,
            position: 0.0,
            position_at: Instant::now(),
        };
        state.reset_demo();
        state
    }

    fn reset_demo(&mut self) {
        self.known.clear();
        self.remember_local_files();
        self.file_playback = false;
        self.liked.clone_from(&self.demo_liked);
        self.model.page = "home".into();
        self.back_stack.clear();
        self.page_query.clear();
        self.model.can_go_back = false;
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
        spectrum::set_listening(true);
        self.audio = None;
        self.history.clear();
        self.selected_device.clear();
        self.active_device.clear();
        self.pending_play = None;
    }

    fn display_model(&self) -> Model {
        let mut model = self.model.clone();
        if model.page == "home" {
            model.liked_music = self.saved_tracks().into_iter().take(6).collect();
        }
        if model.page == "local" {
            model.collections.clone_from(&self.files.albums);
        }
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
        if player && (!self.seek_pending || self.seek_at.is_some()) {
            app::tick(
                self.seek_at
                    .as_ref()
                    .map_or(self.position, |seek| seek.seconds),
            );
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

    fn remember_local_files(&mut self) {
        self.known.extend(self.files.tracks.iter().chain(&self.files.albums)
            .map(|item| (item.id.clone(), item.clone())));
    }

    fn import_folder(&mut self) {
        if self.importing {
            return;
        }
        self.importing = true;
        self.folder_version += 1;
        let version = self.folder_version;
        if let Err(error) = app::pick_music_folder(move |result| STATE.with(|state| {
            let mut state = state.borrow_mut();
            match result {
                Ok(Some(path)) => {
                    let (sender, receiver) = mpsc::channel();
                    state.folder_result = Some(receiver);
                    if state.model.page == "local" {
                        state.status("Reading music folder…");
                    }
                    std::thread::spawn(move || {
                        let _ = sender.send(local_media::scan(&PathBuf::from(path), version));
                    });
                }
                Ok(None) => state.importing = false,
                Err(error) => { state.importing = false; state.status(error); }
            }
        })) {
            self.importing = false;
            self.status(error);
        }
    }

    fn finish_import(&mut self, result: Result<Library, String>) {
        self.importing = false;
        self.folder_result = None;
        let files = match result {
            Ok(files) if !files.tracks.is_empty() => files,
            Ok(_) => { self.status("No supported audio files were found in that folder."); return; }
            Err(error) => { self.status(error); return; }
        };
        if self.file_playback {
            self.audio = None;
            self.model.playing = false;
            self.model.current = Item::default();
            self.model.queue.clear();
            self.position = 0.0;
            self.file_playback = false;
        }
        self.upcoming.retain(|track| !track.id.starts_with("local:"));
        self.model.queue.retain(|track| !track.id.starts_with("local:"));
        self.history.retain(|track| !track.id.starts_with("local:"));
        self.order.retain(|track| !track.id.starts_with("local:"));
        self.model.recent.retain(|track| !track.id.starts_with("local:"));
        self.known.retain(|id, _| !id.starts_with("local:"));
        self.liked.retain(|id| !id.starts_with("local:"));
        self.files = files;
        self.remember_local_files();
        if self.model.page == "local" || self.model.detail.starts_with("local:") {
            self.navigate("local", "");
            if self.files.unreadable > 0 {
                self.status(format!("Imported {} tracks. Some entries could not be read.", self.files.tracks.len()));
            }
        }
        self.render(false, true, false);
    }

    fn find_local_cover(&mut self, id: &str) {
        let Some(album) = self.files.albums.iter().find(|album| album.id == id).cloned() else { return };
        if !album.art.is_empty() { return; }
        if album.artist.is_empty() {
            self.status("Add an album artist tag to look up this cover.");
            return;
        }
        if self.cover_requested_at.is_some_and(|time| time.elapsed() < Duration::from_secs(1)) { return; }
        self.cover_requested_at = Some(Instant::now());
        self.status("Looking up cover art in MusicBrainz…");
        self.request(Some(self.page_version), false, false, Box::new(move |_| {
            let bytes = local_media::find_cover(&album)?;
            Ok(Update::LocalCover(album.id, bytes))
        }));
    }

    fn set_local_cover(&mut self, id: &str, bytes: Vec<u8>) {
        let Some(album) = self.files.albums.iter_mut().find(|album| album.id == id) else { return };
        let Some(tracks) = self.files.album_tracks.get_mut(id) else { return };
        let art = format!("local-cover:{id}");
        album.art.clone_from(&art);
        self.known.insert(id.to_owned(), album.clone());
        let track_ids: HashSet<_> = tracks.iter().map(|track| track.id.clone()).collect();
        let update = |track: &mut Item| {
            if track_ids.contains(&track.id) { track.art.clone_from(&art); }
        };
        for track in tracks.iter_mut().chain(&mut self.files.tracks).chain(&mut self.model.items)
            .chain(&mut self.model.queue).chain(&mut self.model.recent).chain(&mut self.order)
            .chain(&mut self.history).chain(&mut self.upcoming).chain(std::iter::once(&mut self.model.current)) {
            update(track);
        }
        for track_id in track_ids.iter() {
            if let Some(track) = self.known.get_mut(track_id) { update(track); }
        }
        self.files.covers.insert(art, Cover::Downloaded(bytes.into()));
        self.model.status.clear();
        self.render(true, true, false);
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
        self.poll_again_at = None;
        self.search_at = None;
        self.seek_at = None;
        self.seek_pending = false;
        self.seek_hold_until = None;
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
        if self.model.page != page || self.model.detail != id {
            if self.back_stack.len() == 64 { self.back_stack.pop_front(); }
            self.back_stack.push_back((self.model.page.clone(), self.model.detail.clone(), self.page_query.clone()));
        }
        self.show_page(page, id);
    }

    fn go_back(&mut self) {
        let Some((page, id, query)) = self.back_stack.pop_back() else { return };
        self.model.query = query;
        if id.starts_with("local:") && !self.known.contains_key(&id) {
            self.show_page("local", "");
        } else {
            self.show_page(&page, &id);
        }
        app::set_search_text(&self.model.query);
    }

    fn show_page(&mut self, page: &str, id: &str) {
        self.search_at = None;
        if page != "devices" {
            self.pending_play = None;
        }
        self.page_version += 1;
        self.page_query.clone_from(&self.model.query);
        self.model.can_go_back = !self.back_stack.is_empty();
        self.model.page = page.into();
        self.model.detail = id.into();
        self.model.title = match page {
            "home" => "Quickplay",
            "search" => "Browse",
            "library" => "Collection",
            "local" => "Local music",
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
            "local" => self.files.tracks.clone(),
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
            "detail" if id.starts_with("local:") => self.files.album_tracks.get(id).cloned().unwrap_or_default(),
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
            "local" if self.files.tracks.is_empty() => self.model.status = "Choose a music folder to import your albums and tracks.".into(),
            "liked" if self.model.items.is_empty() => self.model.caption = "Tracks you save appear here.".into(),
            "search" if self.model.query.is_empty() => self.model.caption = "Search songs, artists, albums, or playlists.".into(),
            "search" if self.model.connected => self.model.caption = "Looking for music…".into(),
            "search" => self.model.caption = if self.model.items.is_empty() { "No results in the demo library." } else { "Results in the demo library" }.into(),
            "devices" if self.model.connected => self.model.caption = "Choose a Spotify Connect device.".into(),
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
            "detail" if !id.starts_with("local:") => {
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
                    Ok(Update::View(
                        playback_devices(&data, session.local_device()),
                        "Choose a Spotify Connect device.".into(),
                    ))
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

    fn local_play(&mut self, mut item: Item) -> spotify::Result<()> {
        if self.audio.is_none() {
            self.audio = Some(Audio::new()?);
        }
        let audio = self.audio.as_ref().unwrap();
        let imported = item.id.starts_with("local:");
        if imported {
            let path = self.files.files.get(&item.id).ok_or("This track is no longer in the selected folder.")?;
            if let Some(duration) = audio.play_file(path, self.model.volume)? && item.duration == 0.0 {
                item.duration = duration.as_secs_f64();
            }
            if !self.file_playback && self.model.connected && self.model.playing {
                self.control("pause", Http::Put, &[], None);
            }
        } else {
            audio.play(demo_audio(&item.id)?, self.model.volume)?;
        }
        self.file_playback = imported;
        spectrum::set_listening(true);
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
        if target.id.starts_with("local:") {
            if let Some(tracks) = self.files.album_tracks.get(&target.id).cloned() {
                return self.play_file_list(tracks, None);
            }
            let tracks = if self.model.page == "detail" && self.model.detail.starts_with("local:") {
                self.model.items.clone()
            } else {
                self.files.tracks.clone()
            };
            return self.play_file_list(tracks, Some(target));
        }
        if self.model.connected {
            let list = if self.model.page == "queue" {
                &self.model.queue
            } else {
                &self.model.items
            };
            let context = (self.model.page == "detail").then_some(self.model.detail.as_str());
            let payload = playback_payload(&target, list, context);
            if payload["uris"].as_array().is_some_and(Vec::is_empty) {
                return Ok(());
            }
            self.start_playback(payload, false);
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

    fn play_file_list(&mut self, tracks: Vec<Item>, target: Option<Item>) -> spotify::Result<()> {
        let Some(target) = target.or_else(|| tracks.first().cloned()) else { return Ok(()) };
        let start = tracks.iter().position(|track| track.id == target.id).unwrap_or(0);
        self.local_play(target)?;
        self.upcoming = tracks.iter().skip(start + 1).cloned().collect();
        self.history = tracks[..start].to_vec();
        self.order = tracks;
        self.model.queue = self.upcoming.iter().cloned().collect();
        self.render(false, true, false);
        Ok(())
    }

    fn next(&mut self, previous: bool, ended: bool) -> spotify::Result<()> {
        if self.model.connected && !self.file_playback {
            self.cancel_seek();
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
        if self.model.connected && !self.file_playback {
            if !self.model.playing && let Some(payload) = self.pending_play.take() {
                self.start_playback(payload, false);
                return Ok(());
            }
            if self.model.current.id.is_empty() {
                if self.model.items.iter().any(|item| item.kind == "track") {
                    return self.play("");
                }
                self.status("Choose a track or playlist to start playback.");
                return Ok(());
            }
            if !self.model.playing {
                self.start_playback(json!({
                    "uris": [&self.model.current.id],
                    "position_ms": (self.position * 1000.0) as u64,
                }), true);
                return Ok(());
            }
            self.control("pause", Http::Put, &[], None);
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
        } else if !self.model.current.id.is_empty() {
            self.local_play(self.model.current.clone())?;
        } else if self.model.page == "local" {
            self.play("")?;
        }
        Ok(())
    }

    fn seek(&mut self, value: f64) -> spotify::Result<()> {
        if self.model.connected && !self.file_playback {
            if self.model.current.id.is_empty() || self.model.current.duration <= 0.0 {
                return Ok(());
            }
            self.seek_at = Some(SeekTarget {
                track: self.model.current.id.clone(),
                device: self.playback_device().unwrap_or_default().to_owned(),
                seconds: value.min((self.model.current.duration - 0.001).max(0.0)),
                due: Instant::now() + Duration::from_millis(160),
            });
        } else if let Some(audio) = &self.audio {
            audio
                .sink
                .try_seek(Duration::from_secs_f64(value))
                .map_err(|error| format!("Could not seek: {error}"))?;
            self.position = value;
        }
        Ok(())
    }

    fn cancel_seek(&mut self) {
        self.seek_at = None;
        self.seek_hold_until = None;
    }

    fn matches_seek(&self, seek: &SeekTarget) -> bool {
        self.model.current.id == seek.track
            && self.playback_device().unwrap_or_default() == seek.device
    }

    fn playback_device(&self) -> Option<&str> {
        [&self.selected_device, &self.active_device]
            .into_iter()
            .find(|id| !id.is_empty())
            .map(String::as_str)
    }

    fn start_playback(&mut self, payload: Value, resume: bool) {
        if self.file_playback {
            self.file_playback = false;
            self.audio = None;
            self.model.playing = false;
            self.model.current = Item::default();
            self.model.queue.clear();
            self.render(false, true, false);
        }
        self.cancel_seek();
        self.pending_play = None;
        let selected = self.selected_device.clone();
        let active = self.active_device.clone();
        let volume = self.model.volume;
        let local = selected.is_empty();
        if local && active.is_empty() {
            self.status("Starting Zunify playback. If Spotify opens, authorize with the same account.");
        }
        let generation = self.generation;
        let current = self.network.generation.clone();
        self.api(None, false, false, move |session| {
            let device = if local {
                match session.start_local_player(volume, || current.load(Ordering::Acquire) != generation) {
                    Ok(id) => id,
                    Err(error) => return Ok(Update::PlaybackNeedsDevice(payload, error)),
                }
            } else {
                selected
            };
            if current.load(Ordering::Acquire) != generation {
                return Err("Spotify sign-in cancelled.".into());
            }
            let params = query(&[("device_id", &device)]);
            let body = (!(resume && device == active)).then_some(payload);
            session.request(Http::Put, &format!("/me/player/play?{params}"), body)?;
            Ok(Update::StartedPlayback(device, local))
        });
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
        if id.starts_with("local:") {
            self.apply_like(item, add);
        } else if self.model.connected {
            let path = format!("/me/library?{}", query(&[("uris", id)]));
            self.api(None, false, false, move |session| {
                session.request(if add { Http::Put } else { Http::Delete }, &path, None)?;
                Ok(Update::Liked(item, add))
            });
        } else {
            self.apply_like(item, add);
            self.demo_liked = self.liked.iter().filter(|id| id.starts_with("demo:")).cloned().collect();
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
        if self.poll_pending || self.file_playback {
            return;
        }
        self.last_poll = Instant::now();
        self.poll_pending = true;
        self.api(None, true, true, |session| {
            let data = session.get("/me/player")?;
            let external = !str_field(&data["device"], "id").is_empty()
                && session.local_device() != data["device"]["id"].as_str();
            let notice = (!external).then(|| session.local_playback_notice()).flatten();
            Ok(Update::Playback(data, external, notice))
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

    fn enqueue(&mut self, id: &str) {
        if id.starts_with("local:") {
            let tracks = self.files.album_tracks.get(id).cloned()
                .or_else(|| self.known.get(id).cloned().map(|track| vec![track]));
            if let Some(tracks) = tracks { self.enqueue_files(tracks); }
            return;
        }
        if self.model.connected && self.file_playback {
            self.status("Play a Spotify track to use its queue.");
            return;
        }
        if !self.model.connected {
            if let Some(mix) = self.demo.mixes.iter().find(|mix| mix.item.id == id) {
                self.upcoming.extend(mix.tracks.iter().filter_map(|id| self.known.get(id).cloned()));
            } else if let Some(track) = self.known.get(id).filter(|item| item.kind == "track") {
                self.upcoming.push_back(track.clone());
            }
            self.model.queue = self.upcoming.iter().cloned().collect();
            self.render(false, true, false);
            return;
        }
        let Some(target) = self.known.get(id).cloned() else { return };
        let output = self.playback_device().map(str::to_owned);
        self.api(None, false, false, move |session| {
            let tracks = match split_uri(&target.id) {
                Some(("track", _)) => vec![target],
                Some(("album", id)) => {
                    let rows = session.items_up_to(&format!("/albums/{id}/tracks?limit=50"), None, usize::MAX)?;
                    rows.iter()
                        .filter_map(item)
                        .filter(|track| track.kind == "track")
                        .map(|mut track| {
                            track.art.clone_from(&target.art);
                            track.album.clone_from(&target.title);
                            track
                        })
                        .collect()
                }
                Some(("artist", _)) => catalog_artist_tracks(session, &target)?,
                _ => return Ok(Update::Enqueued(Vec::new(), None)),
            };
            if tracks.is_empty() {
                return Err("There are no playable tracks to queue.".into());
            }
            Ok(enqueue_spotify(session, tracks, output.as_deref()))
        });
    }

    fn artist_action(&mut self, artist: &str, play: bool) -> spotify::Result<()> {
        let tracks: Vec<_> = self.model.items.iter()
            .filter(|item| item.kind == "track" && item.artist == artist)
            .cloned()
            .collect();
        let Some(first) = tracks.first().cloned() else { return Ok(()) };
        if first.id.starts_with("local:") {
            if play { return self.play_file_list(tracks, None); }
            self.enqueue_files(tracks);
            return Ok(());
        }
        if !play && self.model.connected && self.file_playback {
            self.status("Play a Spotify track to use its queue.");
            return Ok(());
        }
        match (play, self.model.connected) {
            (true, true) => self.start_playback(playback_payload(&first, &tracks, None), false),
            (true, false) => {
                self.upcoming = tracks.iter().skip(1).cloned().collect();
                self.order = tracks;
                self.history.clear();
                self.local_play(first)?;
            }
            (false, true) => {
                let output = self.playback_device().map(str::to_owned);
                self.api(None, false, false, move |session| {
                    Ok(enqueue_spotify(session, tracks, output.as_deref()))
                });
            }
            (false, false) => {
                self.upcoming.extend(tracks);
                self.model.queue = self.upcoming.iter().cloned().collect();
                self.render(false, true, false);
            }
        }
        Ok(())
    }

    fn enqueue_files(&mut self, tracks: Vec<Item>) {
        if !self.file_playback && self.model.connected {
            self.status("Play a local track to start a local queue.");
            return;
        }
        self.upcoming.extend(tracks);
        self.model.queue = self.upcoming.iter().cloned().collect();
        self.render(false, true, false);
    }

    fn edit_queue(&mut self, action: &str, entry: &str) -> spotify::Result<()> {
        if action == "queue-clear" {
            self.upcoming.clear();
            self.order.clear();
        } else {
            let Some((index, id)) = entry.split_once(':') else { return Ok(()) };
            let Ok(index) = index.parse::<usize>() else { return Ok(()) };
            let Some(track) = self.upcoming.get(index).filter(|item| item.id == id) else { return Ok(()) };
            match action {
                "queue-play" => {
                    let track = track.clone();
                    self.upcoming.drain(..=index);
                    self.history.push(self.model.current.clone());
                    return self.local_play(track);
                }
                "queue-up" if index > 0 => self.upcoming.swap(index - 1, index),
                "queue-down" if index + 1 < self.upcoming.len() => self.upcoming.swap(index, index + 1),
                "queue-remove" => { self.upcoming.remove(index); }
                _ => return Ok(()),
            }
        }
        self.model.queue = self.upcoming.iter().cloned().collect();
        self.render(false, true, false);
        Ok(())
    }

    fn action(&mut self, action: &str, value: &str) -> spotify::Result<()> {
        match action {
            "art" => self.load_art(value),
            "import-folder" => self.import_folder(),
            "local-cover" => self.find_local_cover(value),
            "back" => self.go_back(),
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
            "play" => self.play(value)?,
            "toggle" => self.toggle()?,
            "next" => self.next(false, false)?,
            "previous" => self.next(true, false)?,
            "like" => self.like(value),
            "enqueue" => self.enqueue(value),
            "artist-play" => self.artist_action(value, true)?,
            "artist-enqueue" => self.artist_action(value, false)?,
            "queue-play" | "queue-up" | "queue-down" | "queue-remove" | "queue-clear"
                if !self.model.connected || self.file_playback =>
            {
                self.edit_queue(action, value)?;
            }
            "devices" => self.navigate("devices", ""),
            "transfer" if self.model.connected => {
                if self.file_playback {
                    self.status("Play a Spotify track before switching Spotify devices.");
                    return Ok(());
                }
                self.cancel_seek();
                let device = value.to_owned();
                let pending = self.pending_play.clone();
                let playing = self.model.playing;
                let volume = self.model.volume;
                if device == LOCAL_DEVICE && self.active_device.is_empty() {
                    self.status("Starting Zunify playback. If Spotify opens, authorize with the same account.");
                }
                let generation = self.generation;
                let current = self.network.generation.clone();
                self.api(None, false, false, move |session| {
                    let local = device == LOCAL_DEVICE;
                    let cancelled = || current.load(Ordering::Acquire) != generation;
                    let device = if local { session.start_local_player(volume, cancelled)? } else { device };
                    if cancelled() {
                        return Err("Spotify playback authorization cancelled.".into());
                    }
                    let started_playback = pending.is_some();
                    let (path, body) = device_selection_request(&device, pending, playing);
                    session.request(Http::Put, &path, Some(body))?;
                    Ok(Update::Device {
                        id: device,
                        local,
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
                    if self.model.connected && !self.file_playback {
                        self.volume_at = Some(Instant::now() + Duration::from_millis(180));
                    } else if let Some(audio) = &self.audio {
                        audio.sink.set_volume((value / 100.0) as f32);
                    }
                }
            }
            "shuffle" => {
                self.model.shuffle = !self.model.shuffle;
                if self.model.connected && !self.file_playback {
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
                if self.model.connected && !self.file_playback {
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
            let _ = self.network.art_requests.send((url.to_owned(), self.files.covers.get(url).cloned()));
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
                self.model.recent.clear();
                self.model.queue.clear();
                self.model.playing = false;
                self.position = 0.0;
                self.known.clear();
                self.remember_local_files();
                self.liked.clear();
                self.audio = None;
                self.file_playback = false;
                spectrum::set_listening(true);
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
            Update::LocalCover(id, bytes) => self.set_local_cover(&id, bytes),
            Update::Playback(data, external, notice) => {
                if self.file_playback { return; }
                if !data["device"].is_null() {
                    spectrum::set_listening(!external);
                }
                let track = item(&data["item"]);
                let playing = data["is_playing"].as_bool().unwrap_or(false);
                let changed_track = track
                    .as_ref()
                    .is_some_and(|item| item.id != self.model.current.id);
                let device = str_field(&data["device"], "id");
                if changed_track || (!device.is_empty() && device != self.active_device) {
                    self.cancel_seek();
                }
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
                let changed = track
                    .as_ref()
                    .is_some_and(|track| track != &self.model.current)
                    || playing != self.model.playing
                    || volume != self.model.volume
                    || shuffle != self.model.shuffle
                    || repeat != self.model.repeat;
                if !device.is_empty() {
                    self.active_device = device.to_owned();
                }
                if changed_track
                    || (self.seek_at.is_none()
                        && !self.seek_pending
                        && self
                            .seek_hold_until
                            .is_none_or(|until| Instant::now() >= until))
                {
                    let elapsed = if self.model.playing {
                        self.position_at.elapsed().as_secs_f64()
                    } else {
                        0.0
                    };
                    self.position = data["progress_ms"].as_f64().map_or(
                        (self.position + elapsed).min(self.model.current.duration),
                        |ms| ms / 1000.0,
                    );
                    self.position_at = Instant::now();
                }
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
                if let Some(notice) = notice {
                    if self.model.status != notice {
                        self.status(notice);
                    }
                } else if is_playback_notice(&self.model.status) {
                    self.status("");
                }
                if changed_track {
                    self.fetch_queue();
                }
            }
            Update::Queue(queue) => {
                if self.file_playback { return; }
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
            Update::Enqueued(items, error) => {
                if self.file_playback { return; }
                if !items.is_empty() {
                    self.remember(&items);
                    self.model.queue.extend(items.iter().cloned());
                    self.render(false, true, false);
                    self.fetch_queue();
                }
                if let Some(error) = error {
                    if items.is_empty() {
                        self.status(error);
                    } else {
                        self.status(format!("Added {} tracks to the queue. {error}", items.len()));
                    }
                }
            }
            Update::Liked(item, add) => self.apply_like(item, add),
            Update::StartedPlayback(id, local) => {
                if self.file_playback { return; }
                spectrum::set_listening(local);
                self.active_device = id;
                self.pending_play = None;
                self.status("");
                self.poll_playback();
                self.poll_again_at = Some(Instant::now() + Duration::from_secs(1));
                self.fetch_queue();
            }
            Update::PlaybackNeedsDevice(payload, error) => {
                if self.file_playback { return; }
                self.pending_play = Some(payload);
                self.status(error);
            }
            Update::Device {
                id,
                local,
                started_playback,
            } => {
                spectrum::set_listening(local);
                self.active_device.clone_from(&id);
                self.selected_device = if local { String::new() } else { id };
                if started_playback {
                    self.pending_play = None;
                    self.status("Playback sent to the selected device.");
                    self.fetch_queue();
                } else {
                    self.status("Playback device selected.");
                }
                self.poll_playback();
                self.poll_again_at = Some(Instant::now() + Duration::from_secs(1));
            }
            Update::Command { refresh_queue } => {
                if self.model.connected {
                    self.poll_playback();
                    self.poll_again_at = Some(Instant::now() + Duration::from_secs(1));
                    if refresh_queue {
                        self.fetch_queue();
                    }
                }
            }
            Update::Seek { target, result } => {
                self.seek_pending = false;
                if self.matches_seek(&target) && self.seek_at.is_none() {
                    match result {
                        Ok(()) => {
                            self.position = target.seconds;
                            self.position_at = Instant::now();
                            self.seek_hold_until =
                                Some(self.position_at + Duration::from_millis(900));
                        }
                        Err(error) => self.status(error),
                    }
                    self.poll_again_at = Some(Instant::now() + Duration::from_secs(1));
                }
            }
        }
    }

    fn clock(&mut self) {
        if let Some(result) = self.folder_result.as_ref().and_then(|receiver| receiver.try_recv().ok()) {
            self.finish_import(result);
        }
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
        if self.model.connected && !self.file_playback {
            if !self.poll_pending
                && (self.last_poll.elapsed() >= Duration::from_secs(5)
                    || self.poll_again_at.is_some_and(|at| at <= now))
            {
                self.poll_again_at = None;
                self.poll_playback();
            }
            if !self.seek_pending
                && self.seek_at.as_ref().is_some_and(|seek| seek.due <= now)
                && let Some(target) = self.seek_at.take()
                && self.matches_seek(&target)
            {
                self.seek_pending = true;
                let ms = (target.seconds * 1000.0).round() as u64;
                let suffix = if target.device.is_empty() {
                    String::new()
                } else {
                    format!("&{}", query(&[("device_id", target.device.as_str())]))
                };
                let path = format!("/me/player/seek?position_ms={ms}{suffix}");
                self.api(None, true, false, move |session| {
                    let result = session.request(Http::Put, &path, None).map(|_| ());
                    Ok(Update::Seek { target, result })
                });
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
            if self.seek_at.is_none() && !self.seek_pending {
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

fn enqueue_spotify(session: &mut Session, tracks: Vec<Item>, output: Option<&str>) -> Update {
    let mut added = Vec::new();
    for track in tracks {
        let mut params = vec![("uri", track.id.as_str())];
        if let Some(device) = output {
            params.push(("device_id", device));
        }
        let path = format!("/me/player/queue?{}", query(&params));
        if let Err(error) = session.request(Http::Post, &path, None) {
            return Update::Enqueued(added, Some(error));
        }
        added.push(track);
    }
    Update::Enqueued(added, None)
}

fn catalog_artist_tracks(session: &mut Session, artist: &Item) -> spotify::Result<Vec<Item>> {
    let term = format!("artist:\"{}\"", artist.title.replace('"', " "));
    let response = session.get(&format!(
        "/search?{}",
        query(&[("q", &term), ("type", "track"), ("limit", "10")])
    ))?;
    Ok(response["tracks"]["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|track| track["artists"].as_array().is_some_and(|artists| {
            artists.iter().any(|entry| str_field(entry, "uri") == artist.id)
        }))
        .filter_map(item)
        .collect())
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
                let tracks = catalog_artist_tracks(session, &target)?;
                let caption = if tracks.is_empty() {
                    "No playable tracks found.".into()
                } else {
                    format!("Tracks by {}", target.title)
                };
                return Ok(Update::View(tracks, caption));
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

fn available_devices(data: &Value) -> impl Iterator<Item = (&str, &Value)> {
    data["devices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|device| device["is_restricted"].as_bool() != Some(true))
        .filter_map(|device| {
            let id = device["id"].as_str().filter(|id| !id.is_empty())?;
            Some((id, device))
        })
}

fn device_name(device: &Value) -> &str {
    match str_field(device, "name") {
        "SpotiFe" | "Zunify" => "Zunify (browser)",
        name => name,
    }
}

fn playback_devices(data: &Value, local: Option<&str>) -> Vec<Item> {
    let active = |device: &Value| device["is_active"].as_bool() == Some(true);
    let local_active = available_devices(data).any(|(id, device)| Some(id) == local && active(device));
    let mut devices = vec![Item {
        id: LOCAL_DEVICE.into(),
        title: if local_active { "Zunify · Active" } else { "Zunify" }.into(),
        ..Item::default()
    }];
    devices.extend(available_devices(data).filter(|(id, _)| Some(*id) != local).map(|(id, device)| {
        let suffix = if active(device) { " · Active" } else { "" };
        Item {
            id: id.to_owned(),
            title: format!("{}{suffix}", device_name(device)),
            ..Item::default()
        }
    }));
    devices
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

fn playback_payload(target: &Item, items: &[Item], context: Option<&str>) -> Value {
    if matches!(target.kind.as_str(), "album" | "artist" | "playlist") {
        return json!({ "context_uri": target.id });
    }
    let mut tracks = items.iter().filter(|item| item.kind == "track").peekable();
    if target.kind == "track" {
        while tracks.peek().is_some_and(|item| item.id != target.id) {
            tracks.next();
        }
        if tracks.peek().is_none() {
            return json!({ "uris": [target.id] });
        }
        if let Some(context) = context
            .filter(|uri| uri.starts_with("spotify:album:") || uri.starts_with("spotify:playlist:"))
        {
            return json!({ "context_uri": context, "offset": { "uri": target.id } });
        }
    }
    json!({ "uris": tracks.take(100).map(|item| &item.id).collect::<Vec<_>>() })
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
    fn playing_a_song_keeps_the_rest_of_its_list_or_spotify_context() {
        let tracks: Vec<Item> = ["first", "selected", "next"]
            .map(|name| Item {
                id: format!("spotify:track:{name}"),
                kind: "track".into(),
                ..Item::default()
            })
            .into();
        assert_eq!(
            playback_payload(&tracks[1], &tracks, None),
            json!({ "uris": ["spotify:track:selected", "spotify:track:next"] })
        );
        assert_eq!(
            playback_payload(&tracks[1], &tracks, Some("spotify:playlist:music")),
            json!({ "context_uri": "spotify:playlist:music", "offset": { "uri": "spotify:track:selected" } })
        );
    }

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

    #[test]
    fn the_native_player_is_listed_once_even_when_spotify_reports_it() {
        let data = json!({"devices": [
            {"id": "native", "name": "Zunify", "is_active": true},
            {"id": "web", "name": "Zunify"},
            {"id": "restricted", "is_restricted": true}
        ]});
        let devices = playback_devices(&data, Some("native"));
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].id, LOCAL_DEVICE);
        assert_eq!(devices[0].title, "Zunify · Active");
        assert_eq!(devices[1].id, "web");
        assert_eq!(devices[1].title, "Zunify (browser)");
        assert_eq!(playback_devices(&json!({}), None)[0].id, LOCAL_DEVICE);
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
                state.audio = None;
            });
        })
        .detach();
}
