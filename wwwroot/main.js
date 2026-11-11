import { FerroExports } from "./ferroui.js";
import { loadModule } from "./ferroui-loader.js";
import { mixes, tracks } from "./demo.js";
import { Spotify, itemOf } from "./spotify.js";
import { spotifyClientId } from "./spotify-config.js";

const spotify = new Spotify();
const audio = document.querySelector("audio");
const settings = document.querySelector("#settings");
const settingsStatus = document.querySelector("#settings-status");
const known = new Map();
const sections = [
  { id: "section:liked", title: "Liked songs", artist: "Saved tracks", kind: "section" },
  { id: "section:albums", title: "Albums", artist: "Saved albums", kind: "section" },
  { id: "section:artists", title: "Artists", artist: "Followed artists", kind: "section" },
];
const artRequests = new Set();
let runtime, nav = 0, searchDelay, seekDelay, volumeDelay, position = 0, updatedAt = 0;
let library = [], liked = new Set(), order = [], queue = [], deviceRows = [], selected = "";
let current = {}, page = "home", detail = "", query = "", title = "", caption = "", status = "";
let playing = false, shuffle = false, repeat = false, volume = 65, items = [];
let connectionVersion = 0;
let recentCommandAt = -Infinity, skipProgressUntil = 0;
const read = (name, fallback) => {
  try { return JSON.parse(localStorage.getItem(name)) ?? fallback; } catch { return fallback; }
};
const save = (name, value) => {
  try { localStorage.setItem(name, JSON.stringify(value)); } catch { /* optional preferences */ }
};
const remember = xs => xs.filter(Boolean).map(item => (known.set(item.id, item), item));
const decorated = item => ({ ...item, liked: liked.has(item.id) });
const render = (body = true, player = true, reset = false) => {
  if (!runtime) return;
  runtime.updateModel(JSON.stringify(model()), body || (player && page === "queue"), player, reset);
  if (player) tick();
};
const model = () => ({
  page, title, caption, detail, query, status, connected: spotify.connected,
  collections: library.map(decorated), items: items.map(decorated), queue: queue.map(decorated),
  current: decorated(current), playing, shuffle, repeat, volume,
});
const tick = () => {
  if (!runtime || performance.now() < skipProgressUntil) return;
  const time = spotify.connected ? position + (playing && updatedAt ? (performance.now() - updatedAt) / 1000 : 0) : audio.currentTime;
  runtime.tick(Math.max(0, Math.min(current.duration || 0, time)));
};
const announce = message => { document.querySelector("#announcement").textContent = message; };
const notice = message => { status = message; announce(message); render(true, false); };
const fail = error => notice(error?.message || "Something went wrong.");
const stopDemo = () => { audio.pause(); audio.removeAttribute("src"); audio.load(); };
function demo() {
  remember([...mixes, ...tracks]);
  library = mixes;
  liked = new Set(read("spotifast.demo.liked", [tracks[0].id, tracks[2].id]));
  volume = read("spotifast.volume", 65);
  audio.volume = volume / 100;
  current = tracks[0];
  order = tracks;
  queue = tracks.slice(1);
  playing = shuffle = repeat = false;
  position = 0;
  items = tracks;
}
async function loadLibrary() {
  const version = connectionVersion;
  const [playlists, saved] = await Promise.allSettled([
    spotify.page("/me/playlists?limit=50"),
    spotify.page("/me/tracks?limit=50"),
  ]);
  if (version !== connectionVersion) return;
  if (playlists.status === "fulfilled") library = remember(playlists.value.map(x => itemOf(x, "playlist")));
  if (saved.status === "fulfilled") {
    const savedItems = remember(saved.value.map(x => itemOf(x.track ?? x.item)));
    liked = new Set(savedItems.map(x => x.id));
  }
  const failure = [playlists, saved].find(x => x.status === "rejected");
  if (failure) fail(failure.reason);
}
async function navigate(next, id = "", preserveStatus = false) {
  const request = ++nav;
  page = next;
  detail = id;
  const target = known.get(id);
  title = ({ home: "Home", search: "Browse", library: "Your library", liked: "Liked songs",
    detail: target?.title || "Collection", devices: "Playback devices", queue: "Queue" })[next];
  caption = "";
  if (!preserveStatus) status = "";
  const set = (data, text = "") => {
    if (request !== nav) return;
    items = data; caption = text; render(true, false);
  };
  const saved = () => [...liked].map(id => known.get(id)).filter(Boolean);
  items = next === "home" ? (spotify.connected ? saved().slice(0, 12) : tracks) :
    next === "library" ? [...remember(spotify.connected ? sections : sections.slice(0, 1)), ...library] :
    next === "liked" ? saved() : next === "search" && !query && !spotify.connected ? mixes : [];
  if (next === "search") caption = query ? "Looking for music…" : "Search for a song, album, artist, or playlist.";
  if (next === "detail") caption = target?.artist || "";
  if (next === "devices") caption = spotify.connected
    ? "Play here or send music to another Spotify Connect device."
    : "Demo audio plays on this device. Connect Spotify to see your Spotify Connect devices.";
  if (next === "liked" && !items.length) caption = "Songs you save appear here.";
  if (next === "library" && !items.length) caption = "Your Spotify playlists appear here.";
  render(true, next === "queue", true);
  try {
    if (next === "search" && query) {
      if (!spotify.connected) {
        const terms = query.toLowerCase().split(/\s+/);
        const found = [...tracks, ...mixes].filter(t => terms.every(q => (t.title + " " + t.artist + " " + (t.album || "")).toLowerCase().includes(q)));
        set(found, found.length ? "Results in the demo library" : "No results in the demo library.");
      } else {
        const result = await spotify.request("/search?" + new URLSearchParams({ q: query, type: "track,album,artist,playlist", limit: "10" }));
        if (request !== nav) return;
        const found = remember(["tracks", "albums", "artists", "playlists"].flatMap(type => (result?.[type]?.items || []).map(x => itemOf(x))));
        set(found, found.length ? "Spotify search results" : "No results found.");
      }
    } else if (next === "detail" && target) {
      if (id === "section:albums" || id === "section:artists") {
        const albums = id.endsWith("albums");
        const result = await (albums ? spotify.page("/me/albums?limit=50")
          : spotify.page("/me/following?type=artist&limit=50", 100, "artists"));
        if (request !== nav) return;
        const found = remember(result.map(x => itemOf(albums ? x.album : x)));
        set(found, found.length ? target.artist : "Nothing saved here yet.");
      } else if (!spotify.connected) {
        set(target.tracks ?? [], target.artist);
      } else {
        const [, kind, spotifyId] = id.split(":");
        const paths = { album: `/albums/${spotifyId}/tracks?limit=50`, playlist: `/playlists/${spotifyId}/items?limit=50` };
        if (kind === "artist") {
          const data = await spotify.request(`/search?${new URLSearchParams({ q: 'artist:"' + target.title + '"', type: "track", limit: "10" })}`);
          if (request === nav) set(remember(data.tracks.items.map(x => itemOf(x))), "Tracks by " + target.title);
        } else if (paths[kind]) {
          const data = await spotify.page(paths[kind]);
          if (request !== nav) return;
          const songs = remember(data.map(x => itemOf(x.item ?? x.track ?? x)).map(x => x && ({
            ...x, art: x.art || target.art, album: x.album || target.title,
          })));
          set(songs, songs.length ? target.artist : "No playable tracks in this collection.");
        }
      }
    } else if (next === "devices") {
      await getDevices();
      set(deviceRows, caption);
    }
  } catch (error) {
    if (request === nav) fail(error);
  }
}

async function getDevices() {
  if (!spotify.connected) { deviceRows = [{ id: "demo", title: "This device" }]; return; }
  deviceRows = [{ id: "browser", title: "Play in this app" }];
  try {
    const data = await spotify.request("/me/player/devices");
    deviceRows.push(...(data?.devices || []).filter(x => x.id && !x.is_restricted && x.id !== spotify.device)
      .map(x => ({ id: x.id, title: x.name + (x.is_active ? " · Active" : "") })));
  } catch (error) {
    if (page === "devices") { items = deviceRows; render(true, false); }
    throw error;
  }
}
async function targetDevice() {
  if (selected && selected !== "browser") return selected;
  if (!spotify.device) {
    announce("Starting Spotify playback on this device.");
    await spotify.enableBrowser();
  }
  if (!spotify.device) throw new Error("This device is unavailable. Choose another playback device or try again.");
  return spotify.device;
}
const deviceQuery = id => "?device_id=" + encodeURIComponent(id);
async function transfer(id) {
  if (!spotify.connected) { notice("Demo audio plays on this device."); return; }
  const previous = selected;
  selected = id;
  let output;
  try {
    if (id === "browser" && spotify.player) await spotify.player.activateElement();
    output = await targetDevice();
    if (output === spotify.device) await spotify.player.activateElement();
    await spotify.request("/me/player", "PUT", { device_ids: [output], play: playing });
  } catch (error) {
    selected = previous;
    throw error;
  }
  spotify.active = output;
  await navigate("devices");
  notice("Playback device selected.");
}
function demoSource(track, upcoming) {
  current = track;
  if (upcoming) { order = upcoming; queue = upcoming.slice(upcoming.indexOf(track) + 1); }
  audio.src = track.audio;
  playing = true;
  render(false, true);
  return audio.play().catch(error => {
    playing = false;
    render(false, true);
    throw error;
  });
}
async function play(id) {
  let target = known.get(id || detail);
  if (!target && !id) target = items.find(x => x.kind === "track");
  if (!target) return;
  status = "";
  if (!spotify.connected) {
    if (target.tracks) return demoSource(target.tracks[0], target.tracks);
    const queued = queue.findIndex(x => x.id === target.id);
    if (page === "queue" && queued >= 0) {
      queue = queue.slice(queued + 1);
      return demoSource(target);
    }
    const list = items.filter(x => x.kind === "track");
    return demoSource(target, list.includes(target) ? list : tracks);
  }
  recentCommandAt = performance.now();
  if ((!selected || selected === "browser") && spotify.player) await spotify.player.activateElement();
  const output = await targetDevice();
  if (output === spotify.device) await spotify.player.activateElement();
  const payload = target.kind === "track" ? { uris: [target.id] }
    : target.kind === "album" || target.kind === "playlist" ? { context_uri: target.id }
    : { uris: items.filter(x => x.kind === "track").map(x => x.id).slice(0, 50) };
  if (payload.uris?.length === 0) return;
  await spotify.request("/me/player/play" + deviceQuery(output), "PUT", payload);
  spotify.active = output;
  if (target.kind === "track") current = target;
  playing = true; position = 0; updatedAt = performance.now();
  render(false, true);
  syncQueue();
}
async function control(name, method = "PUT", extra = "") {
  const output = spotify.active || await targetDevice();
  recentCommandAt = performance.now();
  await spotify.request("/me/player/" + name + deviceQuery(output) + extra, method);
}
async function toggle() {
  if (!spotify.connected) {
    if (!audio.getAttribute("src")) return demoSource(current.id ? current : tracks[0]);
    if (audio.paused) await audio.play(); else audio.pause();
    return;
  }
  if (!current.id && items.length) return play(items.find(x => x.kind === "track")?.id || library[0]?.id);
  if (!playing && spotify.active && spotify.active === spotify.device) await spotify.player?.activateElement();
  await control(playing ? "pause" : "play");
  playing = !playing; updatedAt = performance.now(); render(false, true);
}
async function nextTrack(previous = false, ended = false) {
  if (spotify.connected) {
    await control(previous ? "previous" : "next", "POST");
    return syncQueue();
  }
  if (previous) {
    if (audio.currentTime > 3) { audio.currentTime = 0; return; }
    const index = order.indexOf(current);
    return demoSource(order[Math.max(0, index - 1)] || tracks[0], order);
  }
  if (!queue.length && !repeat) {
    if (ended) { playing = false; render(false, true); }
    return;
  }
  if (!queue.length) queue = order.slice();
  const index = shuffle ? Math.floor(Math.random() * queue.length) : 0;
  const [target] = queue.splice(index, 1);
  if (target) return demoSource(target);
}
async function like(id) {
  if (!id) return;
  const remove = liked.has(id);
  if (spotify.connected) await spotify.request("/me/library?" + new URLSearchParams({ uris: id }), remove ? "DELETE" : "PUT");
  if (remove) liked.delete(id); else liked.add(id);
  if (!spotify.connected) save("spotifast.demo.liked", [...liked]);
  if (page === "liked" || (page === "home" && spotify.connected)) {
    items = [...liked].map(i => known.get(i)).filter(Boolean).slice(0, page === "home" ? 12 : 100);
    caption = items.length ? "" : "Songs you save appear here.";
  }
  render(true, true);
  announce(remove ? "Removed from Liked songs" : "Saved to Liked songs");
}
async function enqueue(id) {
  const item = known.get(id);
  if (!item) return;
  if (spotify.connected) {
    const output = spotify.active || await targetDevice();
    await spotify.request("/me/player/queue" + deviceQuery(output) + "&uri=" + encodeURIComponent(id), "POST");
  }
  queue.push(item);
  render(false, true);
  announce("Added " + item.title + " to queue");
}
async function syncQueue() {
  if (!spotify.connected) return;
  const version = connectionVersion;
  try {
    const response = await spotify.request("/me/player/queue");
    if (version !== connectionVersion) return;
    const next = remember((response?.queue || []).map(x => itemOf(x))).slice(0, 20);
    if (next.map(x => x.id).join() !== queue.map(x => x.id).join()) {
      queue = next; render(false, true);
    }
  } catch { /* Some Spotify app modes do not expose the remote queue. */ }
}
function setPlaying(track, isPlaying, seconds, output = spotify.device, settings = {}) {
  if (!spotify.connected || !track) return;
  const changedTrack = current.id !== track.id;
  const changed = changedTrack || playing !== isPlaying || volume !== (settings.volume ?? volume) ||
    shuffle !== (settings.shuffle ?? shuffle) || repeat !== (settings.repeat ?? repeat);
  spotify.active = output || "";
  current = track; playing = isPlaying; position = seconds; updatedAt = performance.now();
  if (settings.volume !== undefined) volume = settings.volume;
  if (settings.shuffle !== undefined) shuffle = settings.shuffle;
  if (settings.repeat !== undefined) repeat = settings.repeat;
  if (changed) render(false, true);
  if (changedTrack) syncQueue();
}
spotify.error = fail;
spotify.devicesChanged = () => { if (page === "devices") navigate("devices"); };
spotify.state = state => {
  if (selected && selected !== "browser" && selected !== spotify.device) return;
  const track = remember([itemOf(state.track_window.current_track)])[0];
  setPlaying(track, !state.paused, state.position / 1000);
};
async function syncPlayback() {
  if (!spotify.connected || performance.now() - recentCommandAt < 3500) return;
  const version = connectionVersion;
  try {
    const state = await spotify.request("/me/player");
    if (!state?.item || version !== connectionVersion) return;
    if (state.device?.id === spotify.device && spotify.player) return;
    const track = remember([itemOf(state.item)])[0];
    setPlaying(track, state.is_playing, (state.progress_ms || 0) / 1000, state.device?.id, {
      shuffle: state.shuffle_state, repeat: state.repeat_state !== "off", volume: state.device?.volume_percent ?? volume,
    });
  } catch { /* A remote device can disappear between polls. */ }
}

const clientIdInput = document.querySelector("#client-id");
let authorizing = false;

function showSettings(message = "") {
  settingsStatus.textContent = message;
  document.querySelector("#redirect-uri").value = spotify.redirect;
  document.querySelector("#signin").hidden = spotify.connected;
  document.querySelector("#connected").hidden = !spotify.connected;
  document.querySelector("#developer-setup").hidden = Boolean(spotifyClientId);
  clientIdInput.disabled = Boolean(spotifyClientId);
  if (!spotifyClientId) clientIdInput.value = read("spotifast.client-id", "");
  if (!settings.open) settings.showModal();
}

async function startSignIn(clientId) {
  if (authorizing) return;
  authorizing = true;
  try {
    await spotify.signIn(clientId);
    if (!spotifyClientId) save("spotifast.client-id", clientId);
  } catch (error) {
    authorizing = false;
    if (settings.open) settingsStatus.textContent = error.message;
    else showSettings(error.message);
  }
}

addEventListener("pageshow", event => {
  if (event.persisted) authorizing = false;
});

function openSettings() {
  if (!spotify.connected && spotifyClientId) return startSignIn(spotifyClientId);
  showSettings();
}

document.querySelector("#signin").addEventListener("submit", event => {
  event.preventDefault();
  return startSignIn(spotifyClientId || clientIdInput.value.trim());
});
document.querySelector("#disconnect").onclick = () => {
  connectionVersion++;
  spotify.signOut(); selected = ""; known.clear(); demo();
  settings.close(); navigate("home"); render(true, true);
};
document.querySelector("#enable-playback").onclick = async () => {
  settingsStatus.textContent = "Starting playback…";
  try {
    await transfer("browser");
    settings.close();
  } catch (error) { settingsStatus.textContent = error.message; }
};
audio.addEventListener("play", () => { if (!spotify.connected && !playing) { playing = true; render(false, true); } });
audio.addEventListener("pause", () => { if (!spotify.connected && playing) { playing = false; render(false, true); } });
audio.addEventListener("ended", () => nextTrack(false, true).catch(fail));
audio.addEventListener("error", () => { if (audio.getAttribute("src")) fail(new Error("Could not load the demo audio.")); });

async function artwork(url) {
  if (artRequests.has(url)) return;
  const target = new URL(url, location.href);
  if (target.origin !== location.origin && target.protocol !== "https:") return;
  artRequests.add(url);
  try {
    const response = await fetch(target);
    if (!response.ok) throw new Error();
    const bytes = await response.arrayBuffer();
    runtime?.setImage(url, new Uint8Array(bytes));
  } catch { /* Keep the visual placeholder for unavailable art. */ }
  finally { artRequests.delete(url); }
}
async function dispatch(action, data) {
  switch (action) {
    case "art": return artwork(data);
    case "navigate":
      clearTimeout(searchDelay);
      if (data === "search") runtime?.focusSearch?.();
      return navigate(data);
    case "query":
      const value = data.trim();
      if (value === query) return;
      query = value;
      clearTimeout(searchDelay);
      nav++;
      searchDelay = setTimeout(() => navigate(query ? "search" : "home"), 260);
      return;
    case "open":
      clearTimeout(searchDelay);
      return data === "section:liked" ? navigate("liked") : navigate("detail", data);
    case "spotify-link": {
      const match = /^spotify:(track|album|artist|playlist|episode):([a-zA-Z0-9]+)$/.exec(data);
      if (match) window.open("https://open.spotify.com/" + match[1] + "/" + match[2], "_blank", "noopener,noreferrer");
      return;
    }
    case "settings": return openSettings();
    case "play": return play(data);
    case "toggle": return toggle();
    case "next": return nextTrack();
    case "previous": return nextTrack(true);
    case "like": return like(data);
    case "enqueue": return enqueue(data);
    case "devices": return navigate("devices");
    case "transfer": return transfer(data);
    case "seek": {
      const value = Number(data);
      if (!spotify.connected) { if (audio.getAttribute("src")) audio.currentTime = value; }
      else {
        const version = connectionVersion;
        skipProgressUntil = performance.now() + 700;
        clearTimeout(seekDelay);
        seekDelay = setTimeout(() => {
          if (version !== connectionVersion) return;
          control("seek", "PUT", "&position_ms=" + Math.round(value * 1000))
            .then(() => { position = value; updatedAt = performance.now(); }, fail);
        }, 200);
      }
      return;
    }
    case "volume": {
      volume = Math.round(Number(data));
      if (!spotify.connected) { audio.volume = volume / 100; save("spotifast.volume", volume); }
      else {
        const version = connectionVersion;
        clearTimeout(volumeDelay);
        volumeDelay = setTimeout(() => {
          if (version !== connectionVersion) return;
          if (spotify.active === spotify.device && spotify.player) spotify.player.setVolume(volume / 100).catch(fail);
          else control("volume", "PUT", "&volume_percent=" + volume).catch(fail);
        }, 180);
      }
      return;
    }
    case "shuffle":
    case "repeat":
      if (action === "shuffle") {
        if (spotify.connected) await control("shuffle", "PUT", "&state=" + !shuffle);
        shuffle = !shuffle;
      } else {
        if (spotify.connected) await control("repeat", "PUT", "&state=" + (repeat ? "off" : "context"));
        repeat = !repeat;
      }
      return render(false, true);
  }
}
globalThis.spotifyFerroAction = (action, data) => {
  // Defer JS-to-Wasm callbacks until the routed FerroUI event has returned.
  queueMicrotask(() => dispatch(action, data).catch(fail));
};
if ("mediaSession" in navigator) {
  for (const [key, action] of Object.entries({ play: "toggle", pause: "toggle", nexttrack: "next", previoustrack: "previous" })) {
    try { navigator.mediaSession.setActionHandler(key, () => {
      if ((key === "play" && playing) || (key === "pause" && !playing)) return;
      dispatch(action, "").catch(fail);
    }); } catch { /* Browser may not support each media action. */ }
  }
}
document.addEventListener("keydown", e => {
  if ((e.metaKey || e.ctrlKey) && e.code === "KeyK" && !settings.open) {
    e.preventDefault();
    runtime?.focusSearch?.();
    navigate("search");
    return;
  }
  if (e.code !== "Space" || e.repeat || settings.open || ["TEXTAREA","INPUT","BUTTON"].includes(e.target?.tagName)) return;
  if (e.target?.closest?.("[role=button],[role=slider]")) return;
  e.preventDefault(); toggle().catch(fail);
});
demo();
const auth = spotify.completeSignIn().catch(error => { status = error.message; announce(status); });
try {
  const loaded = await loadModule({ script: "./spotify-ferro.js", element: document.querySelector("#out") });
  if (!loaded) throw new Error("Could not start FerroUI.");
  await auth;
  if (spotify.connected) {
    stopDemo(); known.clear(); library = []; items = []; liked.clear(); order = []; queue = []; current = {};
  }
  runtime = await loaded.createRuntime();
  FerroExports.attach(runtime);
  runtime.runMain(JSON.stringify(model()), innerWidth, matchMedia("(prefers-reduced-motion: reduce)").matches);
  document.querySelector("#splash").remove();
  let size;
  addEventListener("resize", () => { cancelAnimationFrame(size); size = requestAnimationFrame(() => runtime.resize(innerWidth)); });
  setInterval(tick, 250);
  if (spotify.connected) {
    const version = connectionVersion;
    loadLibrary().then(() => {
      if (version === connectionVersion && ["home", "library", "liked"].includes(page)) {
        return navigate(page, "", true);
      }
    }).catch(fail);
    syncPlayback();
    spotify.enableBrowser().catch(() => {});
    setInterval(syncPlayback, 7000);
  }
  if ("serviceWorker" in navigator) navigator.serviceWorker.register("./sw.js").catch(() => {});
} catch (error) {
  document.querySelector("#loading-message").textContent = error.message + " Reload the page to try again.";
  document.querySelector("#splash").classList.add("failed");
  console.error(error);
}
