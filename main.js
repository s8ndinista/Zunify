import { FerroExports } from "./ferroui.js";
import { loadModule } from "./ferroui-loader.js";
import { PLAYER_NAME, Spotify, itemOf } from "./spotify.js";
import { AudioPath } from "./audio.js";
import { spotifyClientId } from "./spotify-config.js";
import { droppedMusicFiles, findCover, pickMusicFiles, readMusicFiles, readMusicFolder } from "./local-media.js";

import { loadFolders, saveFolders } from "./local-folder-store.js";

const spotify = new Spotify();
const audio = document.querySelector("audio");
const sound = new AudioPath(audio);
const settings = document.querySelector("#settings");
const settingsStatus = document.querySelector("#settings-status");
const folderSettings = document.querySelector("#music-folder-settings");
const folderList = document.querySelector("#music-folder-list");
const folderStatus = document.querySelector("#music-folder-status");
const addFolderButton = document.querySelector("#add-music-folder");
const playlistDialog = document.querySelector("#local-playlist");
const playlistForm = playlistDialog.querySelector("form");
const playlistSelect = playlistForm.querySelector("select");
const playlistName = playlistForm.querySelector("input");
const playlistRemove = playlistForm.querySelector("[data-remove]");
let playlistTarget = "";
const known = new Map();
const sections = [
  { id: "section:liked", title: "Liked songs", artist: "Saved tracks", kind: "section" },
  { id: "section:albums", title: "Albums", artist: "Saved albums", kind: "section" },
  { id: "section:artists", title: "Artists", artist: "Followed artists", kind: "section" },
];
const artRequests = new Set();
let runtime, nav = 0, searchDelay, seekDelay, volumeDelay, position = 0, updatedAt = 0;
let library = [], recent = [], liked = new Set(), order = [], queue = [], deviceRows = [], selected = "";
let spotifyAlbums = [];
let current = {}, page = "home", detail = "", query = "", title = "", caption = "", status = "";
let playing = false, shuffle = false, repeat = false, volume = 65, items = [];
let connectionVersion = 0;
let recentCommandAt = -Infinity, seekHoldUntil = 0, pendingSeek, seekRequest, seeking = false;
let playbackRevision = 0, playbackPoll, playbackRefresh;
let queueWrite = Promise.resolve();
let local = { tracks: [], albums: [], files: new Map(), covers: new Map() };
let localPlayback = false, localUrl, importing = false, importVersion = 0, lastCoverLookup = -Infinity;
let folderChoice, folderReady;
let folders = [];
let localQueue = [];
let spotifyPause, sourceRevision = 0;
let historyEntry = { index: 0, page: "home", detail: "", query: "" };
let backTarget;
const isLocal = id => id?.startsWith("local:");
const spotifyPlayback = () => spotify.connected && !localPlayback;
const read = (name, fallback) => {
  try { return JSON.parse(localStorage.getItem(name)) ?? fallback; } catch { return fallback; }
};
const save = (name, value) => {
  try { localStorage.setItem(name, JSON.stringify(value)); } catch { /* optional preferences */ }
};
const localLikes = new Set(read("zunify.local.likes", []));
const playlists = read("zunify.local.playlists", []);
const savedTracks = () => [...localLikes, ...liked].map(id => known.get(id)).filter(Boolean);
const remember = xs => xs.filter(Boolean).map(item => (known.set(item.id, item), item));
const decorated = item => ({ ...item, liked: (isLocal(item.id) ? localLikes : liked).has(item.id) });
const render = (body = true, player = true, reset = false) => {
  if (!runtime) return;
  runtime.updateModel(JSON.stringify(model()), body || (player && (page === "home" || page === "queue" || page === "local-queue")), player, reset);
  if (player) { tick(); pump(); }
};
const model = () => ({
  page, title, caption, detail, query, status, connected: spotify.connected, can_go_back: historyEntry.index > 0,
  collections: [
    ...(["local", "library"].includes(page) ? localCollections() : []),
    ...(page === "local" ? [] : library),
    ...(page === "library" ? spotifyAlbums : []),
  ].map(({ tracks, ...album }) => decorated(album)),
  items: items.map(decorated), queue: (page === "local-queue" || !spotifyPlayback() ? localQueue : queue).map(decorated),
  collection_art: page === "queue" || page === "local-queue"
    ? [...new Set([...known.values()].map(item => item.art).filter(Boolean))] : [],
  has_music_folders: folders.length > 0,
  liked_music: page === "home" ? savedTracks().slice(0, 6) : [],
  recent, current: decorated(current), playing, shuffle, repeat, volume,
});
const tick = () => {
  if (!runtime) return;
  const time = spotifyPlayback() ? pendingSeek?.seconds ?? spotifyPosition() : audio.currentTime;
  runtime.tick(Math.max(0, Math.min(current.duration || 0, time)));
};
const spotifyPosition = () => position + (playing && updatedAt ? (performance.now() - updatedAt) / 1000 : 0);
// While music plays, each frame hands the analyser the samples that are sounding right now.
const silence = new Float32Array(0);
let pumping = false;
const pump = () => {
  if (pumping || !playing || !runtime) return;
  pumping = true;
  requestAnimationFrame(function frame() {
    const heard = !playing ? null : !spotifyPlayback() ? sound.samples()
      : sound.capturing ? sound.samples(true, 100 / Math.max(10, volume)) : null;
    // Spotify is out of earshot until the listener shares the tab's audio.
    runtime.spectrum(heard ?? silence, sound.rate, !spotifyPlayback() || sound.capturing);
    if (playing) requestAnimationFrame(frame); else pumping = false;
  });
};
const announce = message => { document.querySelector("#announcement").textContent = message; };
const notice = message => { status = message; announce(message); render(true, false); };
const fail = error => notice(error?.message || "Something went wrong.");
const releaseLocalUrl = () => { if (localUrl) URL.revokeObjectURL(localUrl); localUrl = undefined; };
const stopLocalAudio = () => { audio.pause(); audio.removeAttribute("src"); audio.load(); releaseLocalUrl(); };
function rememberLocal() {
  remember([...local.tracks, ...local.albums]);
  remember(playlists.map(playlist => ({
    id: playlist.id, kind: "playlist", title: playlist.name, artist: "Local playlist",
    art: playlist.ids.map(id => known.get(id)?.art).find(Boolean) || "",
    tracks: playlist.ids.map(id => known.get(id)).filter(Boolean),
  })));
}
function openPlaylistDialog(target) {
  playlistTarget = target;
  const editing = playlists.find(playlist => playlist.id === target);
  playlistDialog.querySelector("h2").textContent = editing ? "Edit playlist" : target ? "Add to playlist" : "New playlist";
  playlistSelect.replaceChildren(new Option("New playlist", ""));
  if (target && !editing) {
    for (const playlist of playlists) playlistSelect.add(new Option(playlist.name, playlist.id));
    playlistSelect.value = playlists.find(playlist => playlist.id === detail)?.id ?? playlists[0]?.id ?? "";
  }
  playlistSelect.parentElement.hidden = !target || Boolean(editing) || !playlists.length;
  playlistName.value = editing?.name ?? "";
  updatePlaylistDialog();
  playlistDialog.showModal();
}
function updatePlaylistDialog() {
  const editing = playlists.find(playlist => playlist.id === playlistTarget);
  const selected = playlists.find(playlist => playlist.id === playlistSelect.value);
  playlistName.required = !selected;
  playlistName.parentElement.hidden = Boolean(selected);
  playlistRemove.hidden = !editing && !selected?.ids.includes(playlistTarget);
  playlistRemove.textContent = editing ? "Delete playlist" : "Remove track";
}
playlistSelect.addEventListener("change", updatePlaylistDialog);
async function finishPlaylistEdit() {
  save("zunify.local.playlists", playlists);
  rememberLocal();
  playlistDialog.close();
  await navigate(page, detail, false, false);
}
playlistForm.addEventListener("submit", async event => {
  event.preventDefault();
  if (event.submitter?.value === "cancel") return playlistDialog.close();
  const name = playlistName.value.trim();
  let playlist = playlists.find(list => list.id === playlistTarget);
  if (playlist) {
    if (!name) return;
    playlist.name = name;
  } else {
    playlist = playlists.find(list => list.id === playlistSelect.value);
    if (!playlist) {
      if (!name) return;
      playlist = { id: "local:playlist:" + crypto.randomUUID(), name, ids: [] };
      playlists.push(playlist);
    }
    if (playlistTarget && !playlist.ids.includes(playlistTarget)) playlist.ids.push(playlistTarget);
  }
  await finishPlaylistEdit();
  announce("Saved playlist " + playlist.name);
});
playlistRemove.addEventListener("click", async () => {
  const index = playlists.findIndex(list => list.id === playlistTarget);
  if (index >= 0) {
    if (!confirm("Delete this playlist?")) return;
    known.delete(playlists[index].id);
    playlists.splice(index, 1);
    await navigate("local", "", false, false);
  } else {
    const playlist = playlists.find(list => list.id === playlistSelect.value);
    if (playlist) playlist.ids = playlist.ids.filter(id => id !== playlistTarget);
  }
  await finishPlaylistEdit();
});
const localCollections = () => [...playlists.map(list => known.get(list.id)).filter(Boolean), ...local.albums];
const saveLocalQueue = () => save("zunify.local.queue", localQueue.map(track => track.id));
function drawFolderSettings() {
  addFolderButton.disabled = importing || Boolean(folderChoice);
  folderList.replaceChildren();
  for (const folder of folders) {
    const row = document.createElement("li");
    const name = document.createElement("strong");
    name.textContent = folder.name;
    const summary = document.createElement("p");
    const count = folder.catalog.tracks.length;
    summary.textContent = folder.problem || (folder.scan || folder.handle
      ? count + (count === 1 ? " track" : " tracks")
      : "Drop this folder again to reconnect playback and check for changes.");
    const actions = document.createElement("div");
    actions.className = "actions";
    if (folder.handle) {
      const rescan = document.createElement("button");
      rescan.type = "button";
      rescan.textContent = "Rescan";
      rescan.setAttribute("aria-label", "Rescan " + folder.name);
      rescan.disabled = importing;
      rescan.onclick = () => rescanFolders([folder], true).catch(folderError);
      actions.append(rescan);
    }
    const remove = document.createElement("button");
    remove.type = "button";
    remove.textContent = "Remove";
    remove.setAttribute("aria-label", "Remove " + folder.name);
    remove.disabled = importing;
    remove.onclick = () => removeFolder(folder).catch(folderError);
    actions.append(remove);
    row.append(name, summary, actions);
    folderList.append(row);
  }
  if (!folders.length) {
    const row = document.createElement("li");
    row.textContent = "No music folders added.";
    folderList.append(row);
  }
}

function openFolderSettings() {
  if (settings.open) settings.close();
  folderStatus.textContent = "";
  drawFolderSettings();
  if (!folderSettings.open) folderSettings.showModal();
}

function folderError(error) {
  if (folderSettings.open) folderStatus.textContent = error.message;
  else fail(error);
}

async function persistFolderList() {
  const records = folders.map(({ scan, problem, ...record }) => record);
  try { await saveFolders(records); }
  catch (error) {
    if (error.name !== "DataCloneError") throw error;
    await saveFolders(records.map(({ handle, ...record }) => record));
  }
}

async function updateLocalCollection() {
  for (const item of [...local.tracks, ...local.albums]) known.delete(item.id);
  const next = { tracks: [], albums: [], files: new Map(), covers: new Map() };
  for (const folder of folders) {
    const view = folder.scan ?? folder.catalog;
    const albumByTrack = new Map();
    for (const album of view.albums) {
      for (const track of album.tracks) {
        track.album_id = album.id;
        albumByTrack.set(track.id, album.id);
      }
    }
    for (const track of view.tracks) {
      track.album_id = albumByTrack.get(track.id) ?? "";
      next.tracks.push(track);
    }
    for (const album of view.albums) next.albums.push(album);
    for (const [id, file] of folder.scan?.files ?? []) next.files.set(id, file);
    for (const [id, cover] of folder.scan?.covers ?? []) next.covers.set(id, cover);
  }
  local = next;
  rememberLocal();
  localQueue = read("zunify.local.queue", []).map(id => known.get(id)).filter(Boolean);
  order = order.map(track => known.get(track.id)).filter(Boolean);
  recent = recent.map(track => known.get(track.id)).filter(Boolean);
  if (localPlayback) {
    const track = known.get(current.id);
    if (track) current = track;
    else {
      sourceRevision++;
      stopLocalAudio();
      localPlayback = false;
      playing = false;
      current = {};
    }
  }
  status = "";
  const missingDetail = isLocal(detail) && !known.has(detail);
  await navigate(missingDetail ? "library" : page, missingDetail ? "" : detail, false, false);
  render(true, true);
}

async function scanFolder(folder, entries) {
  const scan = await readMusicFiles(entries, ++importVersion, folder);
  const withoutCover = item => ({ ...item, art: "" });
  folder.scan = scan;
  folder.metadata = scan.metadataCache;
  folder.catalog = {
    tracks: scan.tracks.map(withoutCover),
    albums: scan.albums.map(album => ({ ...withoutCover(album), tracks: album.tracks.map(withoutCover) })),
  };
  folder.problem = "";
}

async function rescanFolders(list, prompt = false) {
  if (folderChoice || importing) return;
  importing = true;
  drawFolderSettings();
  let changed = false;
  try {
    for (const folder of list) {
      if (!folder.handle) continue;
      try {
        const permission = prompt ? await folder.handle.requestPermission({ mode: "read" })
          : await folder.handle.queryPermission({ mode: "read" });
        if (permission !== "granted") {
          folder.problem = "Folder access expired. Rescan to grant access again.";
          continue;
        }
        const { entries } = await readMusicFolder(folder.handle);
        await scanFolder(folder, entries);
        changed = true;
      } catch {
        folder.problem = "Could not read this folder. Drop it again or choose it with the picker.";
      }
    }
    if (changed) {
      await updateLocalCollection();
      await persistFolderList();
    }
  } finally {
    importing = false;
    drawFolderSettings();
  }
}

async function restoreFolders() {
  folders = await loadFolders().catch(() => []);
  if (folders.length) await updateLocalCollection();
  drawFolderSettings();
}

async function matchingFolder(selection) {
  const named = folders.filter(folder => folder.name === selection.name);
  if (selection.handle) {
    for (const folder of named) {
      if (folder.handle && await selection.handle.isSameEntry(folder.handle).catch(() => false)) return folder;
    }
  }
  const unconnected = named.filter(folder => !folder.handle && !folder.scan);
  if (unconnected.length === 1) return unconnected[0];
  if (!selection.handle) {
    return named.find(folder => !folder.handle && selection.entries.some(({ path, file }) => {
      const previous = folder.metadata.get(path);
      return previous?.size === file.size && previous.modified === file.lastModified;
    }));
  }
}

async function chooseMusicFolder() {
  if (folderChoice || importing) return;
  const choice = folderChoice = new AbortController();
  drawFolderSettings();
  try {
    const selection = await pickMusicFiles(choice.signal);
    if (!choice.signal.aborted && selection) await addMusicFolders(Promise.resolve(selection));
  } catch (error) {
    if (!choice.signal.aborted) throw error;
  } finally {
    if (folderChoice === choice) folderChoice = undefined;
    drawFolderSettings();
  }
}

async function addMusicFolders(pendingSelection) {
  if (importing) return;
  importing = true;
  drawFolderSettings();
  folderStatus.textContent = "Scanning music folders…";
  if (!folderSettings.open) notice("Scanning music folders…");
  try {
    const selections = await pendingSelection;
    await folderReady;
    for (const selection of selections) {
      const existing = await matchingFolder(selection);
      const folder = existing ?? { id: crypto.randomUUID(), name: selection.name };
      await scanFolder(folder, selection.entries);
      folder.handle = selection.handle ?? folder.handle;
      if (!existing) folders.push(folder);
    }
    await updateLocalCollection();
    await persistFolderList();
    folderStatus.textContent = "";
  } finally {
    importing = false;
    drawFolderSettings();
    if (status === "Scanning music folders…") { status = ""; render(true, false); }
  }
}

async function removeFolder(folder) {
  if (importing || folderChoice) return;
  importing = true;
  drawFolderSettings();
  try {
    const before = folders;
    folders = folders.filter(item => item !== folder);
    try { await persistFolderList(); }
    catch (error) { folders = before; throw error; }
    const removed = new Set(folder.catalog.tracks.map(track => track.id));
    for (const id of removed) localLikes.delete(id);
    for (const playlist of playlists) playlist.ids = playlist.ids.filter(id => !removed.has(id));
    save("zunify.local.likes", [...localLikes]);
    save("zunify.local.playlists", playlists);
    save("zunify.local.queue", read("zunify.local.queue", []).filter(id => !removed.has(id)));
    await updateLocalCollection();
    folderStatus.textContent = "Removed " + folder.name + " from Collection.";
  } finally {
    importing = false;
    drawFolderSettings();
  }
}

addFolderButton.onclick = () => chooseMusicFolder().catch(folderError);
document.querySelector("#open-music-folders").onclick = openFolderSettings;

function installFolderDrop(target, isAllowed) {
  const accepts = event => runtime && isAllowed() && !playlistDialog.open
    && Array.from(event.dataTransfer?.types ?? []).includes("Files");
  target.addEventListener("dragover", event => {
    if (!accepts(event)) return;
    event.preventDefault();
    event.stopPropagation();
    event.dataTransfer.dropEffect = "copy";
    target.classList.add("music-dragging");
  }, true);
  target.addEventListener("dragleave", event => {
    if (!target.contains(event.relatedTarget)) target.classList.remove("music-dragging");
  }, true);
  target.addEventListener("drop", event => {
    if (!accepts(event)) return;
    event.preventDefault();
    event.stopPropagation();
    target.classList.remove("music-dragging");
    if (importing) return;
    folderChoice?.abort();
    folderChoice = undefined;
    if (page === "home") navigate("local").catch(fail);
    addMusicFolders(droppedMusicFiles(event.dataTransfer)).catch(folderError);
  }, true);
}
installFolderDrop(document.querySelector("#out"), () =>
  ["home", "local", "library"].includes(page) && !settings.open && !folderSettings.open);
installFolderDrop(folderSettings, () => folderSettings.open);

async function findLocalCover(id) {
  const album = known.get(id);
  if (!album || album.art || !local.albums.includes(album)) return;
  if (!album.artist) return notice("Add an album artist tag to look up this cover.");
  if (performance.now() - lastCoverLookup < 1000) return;
  lastCoverLookup = performance.now();
  notice("Looking up cover art in MusicBrainz…");
  const requestedPage = nav;
  const blob = await findCover(album);
  if (!local.albums.includes(album)) return;
  album.art = "local-cover:" + id;
  local.covers.set(album.art, async () => blob);
  for (const track of album.tracks) track.art = album.art;
  if (nav === requestedPage) status = "";
  render(true, true);
}
function resetPlayback() {
  rememberLocal();
  library = []; recent = []; liked = new Set(); order = []; queue = [];
  spotifyAlbums = [];
  volume = read("spotifast.volume", 65);
  sound.volume = volume / 100;
  current = {};
  playing = shuffle = repeat = false;
  position = 0;
  items = [];
}
function updateSpotifyAlbums() {
  const albums = new Map();
  for (const id of liked) {
    const track = known.get(id);
    if (track?.album && track.album_id && !albums.has(track.album_id)) {
      albums.set(track.album_id, {
        id: track.album_id, kind: "album", title: track.album, artist: track.artist, art: track.art,
      });
    }
  }
  spotifyAlbums = remember([...albums.values()]);
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
    updateSpotifyAlbums();
  }
  const failure = [playlists, saved].find(x => x.status === "rejected");
  if (failure) fail(failure.reason);
}
async function catalogArtistTracks(artist) {
  const params = new URLSearchParams({
    q: 'artist:"' + artist.title.replaceAll('"', " ") + '"', type: "track", limit: "10",
  });
  const data = await spotify.request("/search?" + params);
  return (data?.tracks?.items || []).filter(raw => raw?.artists?.some(entry => entry?.uri === artist.id))
    .map(raw => itemOf(raw, "track")).filter(Boolean);
}
async function navigate(next, id = "", preserveStatus = false, record = true) {
  const push = record && (next !== page || id !== detail);
  historyEntry = { index: historyEntry.index + Number(push), page: next, detail: id, query };
  if (push) history.pushState({ zunify: historyEntry }, "");
  else history.replaceState({ zunify: historyEntry }, "");
  const request = ++nav;
  page = next;
  detail = id;
  const target = known.get(id);
  title = ({ home: "Quickplay", search: "Browse", library: "Collection", local: "Local music", liked: "Liked songs",
    detail: target?.title || "Collection", devices: "Playback devices", queue: "Queue", "local-queue": "Local queue" })[next];
  caption = "";
  if (!preserveStatus) status = "";
  const set = (data, text = "") => {
    if (request !== nav) return;
    items = data; caption = text; render(true, false);
  };
  items = next === "home" ? savedTracks().slice(0, 12) :
    next === "local" ? local.tracks :
    next === "library" ? [...remember(sections), ...local.tracks, ...savedTracks().filter(item => !isLocal(item.id))] :
    next === "liked" ? savedTracks() : [];
  if (next === "search") caption = query ? "Looking for music…" : "Search for a song, album, artist, or playlist.";
  if (next === "detail") caption = target?.artist || "";
  if (next === "devices") caption = spotify.connected
    ? "Play here or send music to another Spotify Connect device."
    : "Local music plays on this device. Connect Spotify to see your Spotify Connect devices.";
  if (next === "liked" && !items.length) caption = "Songs you save appear here.";
  if (next === "library" && !items.length) caption = "Your Spotify playlists appear here.";
  render(true, (next === "queue" || next === "local-queue"), true);
  try {
    if (next === "search" && query) {
      const terms = query.toLowerCase().split(/\s+/);
      const found = [...local.tracks, ...local.albums, ...playlists.map(list => known.get(list.id))]
        .filter(t => terms.every(q => (t.title + " " + t.artist + " " + (t.album || "")).toLowerCase().includes(q)));
      set(found, found.length ? "Local music" : "No results found.");
      if (spotify.connected) {
        const result = await spotify.request("/search?" + new URLSearchParams({ q: query, type: "track,album,artist,playlist", limit: "10" }));
        if (request !== nav) return;
        const remote = remember(["tracks", "albums", "artists", "playlists"].flatMap(type => (result?.[type]?.items || []).map(x => itemOf(x))));
        set([...found, ...remote], found.length || remote.length ? "Local music and Spotify" : "No results found.");
      }
    } else if (next === "detail" && target) {
      if (isLocal(id)) {
        set(target.tracks ?? [], target.artist);
      } else if (id === "section:albums" || id === "section:artists") {
        const albums = id.endsWith("albums");
        const result = await (albums ? spotify.page("/me/albums?limit=50")
          : spotify.page("/me/following?type=artist&limit=50", 100, "artists"));
        if (request !== nav) return;
        const found = remember(result.map(x => itemOf(albums ? x.album : x, albums ? "album" : "artist")));
        set(found, found.length ? target.artist : "Nothing saved here yet.");
      } else if (!spotify.connected) {
        set(target.tracks ?? [], target.artist);
      } else {
        const [, kind, spotifyId] = id.split(":");
        const paths = { album: `/albums/${spotifyId}/tracks?limit=50`, playlist: `/playlists/${spotifyId}/items?limit=50` };
        if (kind === "artist") {
          const songs = await catalogArtistTracks(target);
          if (request === nav) set(remember(songs), songs.length ? "Tracks by " + target.title : "No playable tracks found.");
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

function restoreHistory(entry) {
  clearTimeout(searchDelay);
  historyEntry = entry;
  query = entry.query;
  runtime.setSearchText(query);
  if (isLocal(entry.detail) && !known.has(entry.detail)) return navigate("local", "", false, false);
  return navigate(entry.page, entry.detail, false, false);
}

// FerroUI's sentinel helper only handles Back. Actual route entries preserve browser Forward.
addEventListener("popstate", event => {
  const entry = event.state?.zunify;
  if (!runtime || !entry) return;
  if (entry.index < historyEntry.index) {
    backTarget = entry;
    if (FerroExports.NavigationHelper?.OnBackRequested?.()) return;
    backTarget = undefined;
  }
  restoreHistory(entry).catch(fail);
});

async function getDevices() {
  if (!spotify.connected) { deviceRows = [{ id: "local-device", title: "This device" }]; return; }
  deviceRows = [{ id: "browser", title: PLAYER_NAME }];
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
  if (!spotify.connected) { notice("Local music plays on this device."); return; }
  const previous = selected;
  const seekInFlight = cancelSeek();
  playbackRevision++;
  selected = id;
  let output;
  try {
    if (id === "browser" && spotify.player) await spotify.player.activateElement();
    output = await targetDevice();
    if (output === spotify.device) await spotify.player.activateElement();
    await seekInFlight;
    await spotify.request("/me/player", "PUT", { device_ids: [output], play: playing });
  } catch (error) {
    selected = previous;
    throw error;
  }
  spotify.active = output;
  refreshPlayback();
  if (page === "devices") await navigate("devices");
  notice("Playback device selected.");
}
function recordRecent(track) {
  if (track.id && recent[0]?.id !== track.id) {
    recent = [track, ...recent.filter(item => item.id !== track.id)].slice(0, 6);
  }
}
async function playLocal(track, upcoming) {
  const source = ++sourceRevision;
  let file = local.files.get(track.id);
  if (!file) {
    try {
      const [, , folderId, encodedPath] = track.id.split(":");
      const [root, ...path] = decodeURIComponent(encodedPath ?? folderId).split("/");
      let directory = folders.find(folder => encodedPath ? folder.id === folderId : folder.legacy && folder.name === root)?.handle;
      const filename = path.pop();
      for (const segment of path) directory = await directory.getDirectoryHandle(segment);
      file = await (await directory.getFileHandle(filename)).getFile();
    } catch { /* The folder may need permission, or the file may have moved. */ }
  }
  if (source !== sourceRevision || !known.has(track.id)) return false;
  if (!file) return notice("Reconnect the music folder to play this track.");
  if (spotifyPlayback()) {
    cancelSeek(); playbackRevision++;
    if (spotify.active) spotifyPause = control("pause").catch(() => {});
    sound.volume = volume / 100;
  }
  localPlayback = true;
  current = track;
  recordRecent(track);
  if (upcoming) {
    order = upcoming;
    const upcomingTracks = upcoming.slice(upcoming.findIndex(item => item.id === track.id) + 1);
    const queued = new Set(localQueue.map(item => item.id));
    localQueue.push(...upcomingTracks.filter(item => !queued.has(item.id)));
    saveLocalQueue();
  }
  releaseLocalUrl();
  localUrl = URL.createObjectURL(file);
  audio.src = localUrl;
  playing = true;
  render(false, true);
  sound.wake();
  try {
    await audio.play();
    return source === sourceRevision;
  } catch (error) {
    if (source !== sourceRevision) return false;
    playing = false;
    render(false, true);
    throw error;
  }
}
function playbackPayload(target) {
  if (["album", "artist", "playlist"].includes(target.kind)) return { context_uri: target.id };
  const list = (page === "queue" ? queue : items).filter(item => item.id.startsWith("spotify:track:"));
  if (target.kind !== "track") return { uris: list.slice(0, 100).map(item => item.id) };
  const index = list.findIndex(item => item.id === target.id);
  if (index < 0) return { uris: [target.id] };
  if (page === "detail" && /^spotify:(album|playlist):/.test(detail)) {
    return { context_uri: detail, offset: { uri: target.id } };
  }
  return { uris: list.slice(index, index + 100).map(item => item.id) };
}
async function play(id) {
  let target = known.get(id || detail);
  if (!target && !id) target = items.find(x => x.kind === "track");
  if (!target) return;
  status = "";
  if (isLocal(target.id)) {
    if (target.kind !== "track") {
      if (target.tracks?.length) return playLocal(target.tracks[0], target.tracks);
      return notice("Add some tracks to this playlist first.");
    }
    const context = items.filter(item => item.kind === "track" && isLocal(item.id));
    return playLocal(target, context.some(item => item.id === target.id) ? context : local.tracks);
  }
  if (!spotify.connected) return;
  return startSpotifyPlayback(target, playbackPayload(target));
}
async function startSpotifyPlayback(target, payload) {
  if (payload.uris?.length === 0) return;
  const source = ++sourceRevision;
  if (localPlayback) {
    stopLocalAudio(); localPlayback = false; current = {}; queue = []; playing = false;
    render(false, true);
  }
  const seekInFlight = cancelSeek();
  const version = connectionVersion;
  resumeCapture();
  if ((!selected || selected === "browser") && spotify.player) await spotify.player.activateElement();
  const output = await targetDevice();
  if (output === spotify.device) await spotify.player.activateElement();
  await seekInFlight;
  await spotifyPause;
  if (version !== connectionVersion || source !== sourceRevision) return;
  recentCommandAt = performance.now();
  const revision = ++playbackRevision;
  await spotify.request("/me/player/play" + deviceQuery(output), "PUT", payload);
  if (version !== connectionVersion || source !== sourceRevision) return;
  refreshPlayback();
  if (revision !== playbackRevision) return;
  spotify.active = output;
  if (target.kind === "track") {
    current = target;
    recordRecent(target);
  }
  playing = true; position = 0; updatedAt = performance.now();
  render(false, true);
  syncQueue();
}
async function control(name, method = "PUT", extra = "", output = spotify.active, body) {
  output ||= await targetDevice();
  recentCommandAt = performance.now();
  await spotify.request("/me/player/" + name + deviceQuery(output) + extra, method, body);
}
async function toggle() {
  if (!spotifyPlayback()) {
    if (!audio.getAttribute("src")) {
      const track = localQueue[0] || items.find(item => item.kind === "track") || local.tracks[0];
      if (!track) return notice("Import a music folder or connect Spotify to play music.");
      if (localQueue[0] === track) return editQueue("queue-play", "0:" + track.id);
      return play(track.id);
    }
    if (audio.paused) { sound.wake(); await audio.play(); } else audio.pause();
    return;
  }
  resumeCapture();
  if (!current.id) {
    const first = items.find(x => x.kind === "track") || library[0];
    if (first) return play(first.id);
    return notice("Choose a track or playlist to start playback.");
  }
  const nextPlaying = !playing;
  const version = connectionVersion;
  const track = current.id;
  const player = spotify.player;
  if (nextPlaying && (!selected || selected === "browser")) await player?.activateElement();
  const output = nextPlaying ? await targetDevice() : spotify.active;
  if (nextPlaying && !player && output === spotify.device) await spotify.player.activateElement();
  if (version !== connectionVersion) return;
  const restart = nextPlaying && output !== spotify.active;
  const body = restart ? { uris: [track], position_ms: Math.round(spotifyPosition() * 1000) } : undefined;
  playbackRevision++;
  await control(nextPlaying ? "play" : "pause", "PUT", "", output, body);
  if (version !== connectionVersion) return;
  if (restart) spotify.active = output;
  refreshPlayback();
  // The SDK may already have reported the new state while the request was in flight.
  if (version !== connectionVersion || track !== current.id || playing === nextPlaying) return;
  const now = performance.now();
  if (!nextPlaying && updatedAt) {
    position = Math.min(current.duration || 0, position + (now - updatedAt) / 1000);
  }
  playing = nextPlaying;
  updatedAt = now;
  render(false, true);
}
async function nextTrack(previous = false, ended = false) {
  if (spotifyPlayback()) {
    await cancelSeek();
    playbackRevision++;
    await control(previous ? "previous" : "next", "POST");
    refreshPlayback();
    return syncQueue();
  }
  if (previous) {
    if (audio.currentTime > 3) { audio.currentTime = 0; return; }
    const index = order.indexOf(current);
    const track = order[Math.max(0, index - 1)] || current;
    if (track.id) return playLocal(track);
    return;
  }
  if (!localQueue.length && repeat) localQueue = order.slice();
  if (!localQueue.length) {
    if (ended) { playing = false; render(false, true); }
    return;
  }
  const index = shuffle ? Math.floor(Math.random() * localQueue.length) : 0;
  const target = localQueue[index];
  const activeQueue = localQueue;
  if (target && await playLocal(target) && localQueue === activeQueue && localQueue[index] === target) {
    localQueue.splice(index, 1);
    saveLocalQueue();
    render(false, true);
  }
}
async function like(id) {
  if (!id) return;
  const favorites = isLocal(id) ? localLikes : liked;
  const remove = favorites.has(id);
  if (spotify.connected && !isLocal(id)) await spotify.request("/me/library?" + new URLSearchParams({ uris: id }), remove ? "DELETE" : "PUT");
  if (remove) favorites.delete(id); else favorites.add(id);
  if (isLocal(id)) save("zunify.local.likes", [...localLikes]);
  else updateSpotifyAlbums();
  if (page === "liked" || ["home", "library"].includes(page)) {
    await navigate(page, "", false, false);
  }
  render(true, true);
  announce(remove ? "Removed from Liked songs" : "Saved to Liked songs");
}
async function enqueue(id) {
  const target = known.get(id);
  if (!target) return;
  return enqueueTracks(target);
}
function artistAction(name, playNow) {
  const songs = items.filter(item => item.kind === "track" && item.artist === name);
  if (!songs.length) return;
  const localArtist = isLocal(songs[0].id);
  const sourceTracks = songs.filter(song => isLocal(song.id) === localArtist);
  if (!playNow) return enqueueTracks({ title: name }, sourceTracks);
  status = "";
  if (localArtist) return playLocal(sourceTracks[0], sourceTracks);
  if (spotify.connected) return startSpotifyPlayback(sourceTracks[0], { uris: sourceTracks.slice(0, 100).map(item => item.id) });
}
async function spotifyQueueTracks(target) {
  if (target.kind === "artist") return catalogArtistTracks(target);
  if (target.kind === "album") {
    const match = /^spotify:album:([a-zA-Z0-9]+)$/.exec(target.id);
    if (!match) return [];
    const rows = await spotify.page("/albums/" + match[1] + "/tracks?limit=50", Infinity);
    return rows.map(raw => itemOf(raw, "track")).filter(Boolean)
      .map(track => ({ ...track, art: target.art, album: target.title }));
  }
  return target.kind === "track" ? [target] : [];
}
async function enqueueTracks(target, listedTracks) {
  const localTarget = isLocal(target.id) || isLocal(listedTracks?.[0]?.id);
  if (localTarget) {
    const tracks = listedTracks || target.tracks || [target];
    if (!tracks.length) return notice("Add some tracks to this playlist first.");
    localQueue.push(...tracks);
    saveLocalQueue();
    render(false, true);
    announce("Added " + target.title + " to queue");
    return;
  }
  if (spotify.connected && localPlayback) return notice("Play a Spotify track to use its queue.");
  if (!spotify.connected) return;
  const version = connectionVersion;
  const write = queueWrite.then(async () => {
    if (version !== connectionVersion) return;
    const tracks = listedTracks ?? await spotifyQueueTracks(target);
    if (!tracks.length) throw new Error("There are no playable tracks to queue.");
    if (version !== connectionVersion) return;
    const output = spotify.active || await targetDevice();
    const added = [];
    try {
      for (const track of tracks) {
        if (version !== connectionVersion) return;
        await spotify.request("/me/player/queue" + deviceQuery(output) + "&uri=" + encodeURIComponent(track.id), "POST");
        added.push(track);
      }
    } catch (error) {
      if (version !== connectionVersion) return;
      if (!added.length) throw error;
      notice("Added " + added.length + " tracks to the queue. " + error.message);
      return;
    } finally {
      if (version === connectionVersion && added.length) {
        queue.push(...remember(added));
        render(false, true);
      }
    }
    announce("Added " + target.title + " to queue");
  });
  queueWrite = write.catch(() => {});
  return write;
}
async function editQueue(action, entry) {
  if (spotifyPlayback() && page !== "local-queue") return;
  if (action === "queue-clear") {
    localQueue = [];
    order = [];
  } else {
    const delimiter = entry.indexOf(":");
    if (delimiter < 1) return;
    const index = Number(entry.slice(0, delimiter));
    const track = localQueue[index];
    if (!track || track.id !== entry.slice(delimiter + 1)) return;
    if (action === "queue-play") {
      const activeQueue = localQueue;
      if (await playLocal(track) && localQueue === activeQueue && localQueue[index] === track) {
        localQueue.splice(0, index + 1);
        saveLocalQueue();
        render(false, true);
      }
      return;
    }
    if (action === "queue-remove") localQueue.splice(index, 1);
    else {
      const neighbor = index + (action === "queue-up" ? -1 : 1);
      if (!localQueue[neighbor]) return;
      [localQueue[index], localQueue[neighbor]] = [localQueue[neighbor], localQueue[index]];
    }
  }
  saveLocalQueue();
  render(false, true);
}
async function syncQueue() {
  if (!spotifyPlayback()) return;
  const version = connectionVersion;
  try {
    const response = await spotify.request("/me/player/queue");
    if (version !== connectionVersion || localPlayback) return;
    if (!Array.isArray(response?.queue)) return;
    const next = remember(response.queue.map(x => itemOf(x))).slice(0, 20);
    if (next.map(x => x.id).join() !== queue.map(x => x.id).join()) {
      queue = next; render(false, true);
    }
  } catch { /* Some Spotify app modes do not expose the remote queue. */ }
}
function setPlaying(track, isPlaying, seconds, output = spotify.device, settings = {}) {
  if (!spotifyPlayback() || !track) return;
  const changedTrack = current.id !== track.id;
  const changed = changedTrack || track.title !== current.title || track.artist !== current.artist ||
    track.album !== current.album ||
    track.art !== current.art || track.duration !== current.duration ||
    playing !== isPlaying || volume !== (settings.volume ?? volume) ||
    shuffle !== (settings.shuffle ?? shuffle) || repeat !== (settings.repeat ?? repeat);
  if (changedTrack || (output && output !== spotify.active)) cancelSeek();
  spotify.active = output || "";
  if (isPlaying) recordRecent(track);
  current = track; playing = isPlaying;
  if (!pendingSeek && performance.now() >= seekHoldUntil) {
    position = Number.isFinite(seconds) ? seconds : changedTrack ? 0 : spotifyPosition();
    updatedAt = performance.now();
  }
  if (settings.volume !== undefined) volume = settings.volume;
  if (settings.shuffle !== undefined) shuffle = settings.shuffle;
  if (settings.repeat !== undefined) repeat = settings.repeat;
  if (changed) render(false, true);
  if (changedTrack) syncQueue();
}
spotify.error = fail;
spotify.devicesChanged = () => { if (page === "devices") navigate("devices"); };
spotify.state = state => {
  if (localPlayback) return;
  if (selected && selected !== "browser" && selected !== spotify.device) return;
  if (!selected && spotify.active && spotify.active !== spotify.device) return;
  const track = itemOf(state.track_window?.current_track);
  if (!track) return;
  const knownTrack = known.get(track.id);
  track.duration = state.duration > 0 ? state.duration / 1000 : track.duration || knownTrack?.duration || 0;
  if (knownTrack) {
    track.title ||= knownTrack.title;
    track.artist ||= knownTrack.artist;
    track.album ||= knownTrack.album;
    track.art ||= knownTrack.art;
  }
  remember([track]);
  playbackRevision++;
  setPlaying(track, !state.paused, state.position / 1000);
};
function refreshPlayback() {
  clearTimeout(playbackRefresh);
  playbackRefresh = setTimeout(syncPlayback, 900);
}
async function syncPlayback() {
  if (!spotifyPlayback() || playbackPoll) return;
  const version = connectionVersion;
  const revision = playbackRevision;
  const poll = (async () => {
    const useLocal = (!selected || selected === "browser") && spotify.device
      && (!spotify.active || spotify.active === spotify.device);
    if (useLocal && spotify.player?.getCurrentState) {
      const local = await spotify.player.getCurrentState().catch(() => null);
      if (local?.track_window?.current_track) return { local };
    }
    return { remote: await spotify.request("/me/player") };
  })();
  playbackPoll = poll;
  try {
    const { local, remote: state } = await poll;
    if (version !== connectionVersion || revision !== playbackRevision) return;
    if (local) {
      spotify.state(local);
      return;
    }
    if (!state?.item) {
      if (playing && current.id && performance.now() - recentCommandAt > 3500
          && (!spotify.device || spotify.active !== spotify.device || spotifyPosition() >= current.duration)) {
        setPlaying(current, false, Math.min(spotifyPosition(), current.duration), spotify.active);
      }
      return;
    }
    if (selected && selected !== "browser" && state.device?.id !== selected) return;
    if (selected === "browser" && state.device?.id !== spotify.device) return;
    const track = remember([itemOf(state.item)])[0];
    setPlaying(track, state.is_playing, (state.progress_ms || 0) / 1000, state.device?.id, {
      shuffle: state.shuffle_state, repeat: state.repeat_state !== "off", volume: state.device?.volume_percent ?? volume,
    });
  } catch { /* A remote device can disappear between polls. */ }
  finally { if (playbackPoll === poll) playbackPoll = undefined; }
}
function cancelSeek() {
  clearTimeout(seekDelay);
  pendingSeek = undefined;
  seekHoldUntil = 0;
  return seekRequest?.catch(() => {});
}
function queueSeek(value) {
  if (!current.id || current.duration <= 0) return;
  const seconds = Math.min(Math.max(value, 0), Math.max(0, current.duration - 0.001));
  pendingSeek = { seconds, version: connectionVersion, track: current.id, device: spotify.active };
  clearTimeout(seekDelay);
  seekDelay = setTimeout(sendSeek, 160);
}
async function sendSeek() {
  if (seeking || !pendingSeek) return;
  const seek = pendingSeek;
  const valid = () => seek.version === connectionVersion && seek.track === current.id;
  seeking = true;
  try {
    const output = seek.device || await targetDevice();
    if (!valid() || pendingSeek !== seek || (spotify.active && output !== spotify.active)) return;
    playbackRevision++;
    const ms = Math.round(seek.seconds * 1000);
    seekRequest = output === spotify.device && spotify.player?.seek
      ? spotify.player.seek(ms)
      : spotify.request("/me/player/seek" + deviceQuery(output) + "&position_ms=" + ms, "PUT");
    await seekRequest;
    if (!valid() || pendingSeek !== seek) return;
    position = seek.seconds;
    updatedAt = performance.now();
    seekHoldUntil = updatedAt + 900;
    refreshPlayback();
  } catch (error) {
    if (valid() && pendingSeek === seek) fail(error);
  } finally {
    seekRequest = undefined;
    seeking = false;
    if (pendingSeek === seek) pendingSeek = undefined;
    else if (pendingSeek) {
      clearTimeout(seekDelay);
      seekDelay = setTimeout(sendSeek, 160);
    }
    tick();
  }
}

// Spotify's protected stream never passes through the page, so its spectrum needs the listener to
// share this tab's audio. The choice is remembered; the browser still asks once per visit.
const spectrumToggle = document.querySelector("#spectrum-toggle");
const showSpectrumSetting = () => {
  document.querySelector("#spectrum").hidden = !sound.canCapture;
  spectrumToggle.textContent = sound.capturing ? "Stop sharing tab audio" : "Show the spectrum for Spotify";
};
sound.changed = showSpectrumSetting;
async function capture() {
  try {
    await sound.capture();
    save("spotife.spectrum", true);
  } catch (error) {
    if (error.name === "NotAllowedError") save("spotife.spectrum", false);
    throw error;
  } finally { showSpectrumSetting(); }
}
let captureResumed = false;
function resumeCapture() {
  if (captureResumed || sound.capturing || !sound.canCapture || (selected && selected !== "browser")) return;
  if (!read("spotife.spectrum", false)) return;
  captureResumed = true;
  capture().catch(() => {});
}
spectrumToggle.onclick = async () => {
  if (sound.capturing) {
    sound.release();
    save("spotife.spectrum", false);
    settingsStatus.textContent = "";
    return;
  }
  try {
    await capture();
    settingsStatus.textContent = "The spectrum now follows the music playing in this tab.";
  } catch (error) {
    settingsStatus.textContent = error.name === "NotAllowedError" ? "Tab audio was not shared." : error.message;
  }
};

const clientIdInput = document.querySelector("#client-id");
let authorizing = false;

function showSettings(message = "") {
  settingsStatus.textContent = message;
  document.querySelector("#redirect-uri").value = spotify.redirect;
  document.querySelector("#signin").hidden = spotify.connected;
  document.querySelector("#connected").hidden = !spotify.connected;
  showSpectrumSetting();
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
  authorizing = false;
  clearTimeout(searchDelay);
  clearTimeout(volumeDelay);
  cancelSeek();
  clearTimeout(playbackRefresh);
  playbackPoll = undefined;
  deviceRows = [];
  spotify.signOut();
  stopLocalAudio(); localPlayback = false; sourceRevision++;
  sound.release(); selected = ""; known.clear(); resetPlayback();
  query = ""; runtime?.setSearchText("");
  settings.close(); navigate("home", "", false, false); render(true, true);
  announce("Signed out of Spotify in Zunify.");
};
document.querySelector("#enable-playback").onclick = async () => {
  settingsStatus.textContent = "Starting playback…";
  try {
    await transfer("browser");
    settings.close();
  } catch (error) { settingsStatus.textContent = error.message; }
};
audio.addEventListener("play", () => { if (!spotifyPlayback() && !audio.paused && !playing) { playing = true; render(false, true); } });
audio.addEventListener("pause", () => { if (!spotifyPlayback() && audio.paused && playing) { playing = false; render(false, true); } });
audio.addEventListener("loadedmetadata", () => {
  if (localPlayback && !current.duration && Number.isFinite(audio.duration)) {
    current.duration = audio.duration;
    render(true, true);
  }
});
audio.addEventListener("seeked", tick);
audio.addEventListener("ended", () => nextTrack(false, true).catch(fail));
audio.addEventListener("error", () => { if (audio.getAttribute("src")) fail(new Error("This browser could not decode the local track.")); });

async function artwork(url) {
  if (artRequests.has(url)) return;
  if (url.startsWith("local-cover:")) {
    artRequests.add(url);
    try {
      const blob = await local.covers.get(url)?.();
      runtime?.setImage(url, blob ? new Uint8Array(await blob.arrayBuffer()) : new Uint8Array());
    } catch { runtime?.setImage(url, new Uint8Array()); }
    finally { artRequests.delete(url); }
    return;
  }
  const target = new URL(url, location.href);
  if (target.origin !== location.origin && target.protocol !== "https:") {
    runtime?.setImage(url, new Uint8Array());
    return;
  }
  artRequests.add(url);
  try {
    const response = await fetch(target);
    if (!response.ok) throw new Error();
    const bytes = await response.arrayBuffer();
    runtime?.setImage(url, new Uint8Array(bytes));
  } catch {
    runtime?.setImage(url, new Uint8Array());
  }
  finally { artRequests.delete(url); }
}
async function dispatch(action, data) {
  switch (action) {
    case "art": return artwork(data);
    case "import-folder": return chooseMusicFolder();
    case "music-folders": return openFolderSettings();
    case "local-playlist": return openPlaylistDialog(data);
    case "local-cover": return findLocalCover(data);
    case "back": {
      if (backTarget) {
        const entry = backTarget;
        backTarget = undefined;
        return restoreHistory(entry);
      }
      if (historyEntry.index > 0) history.back();
      return;
    }
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
    case "artist-play": return artistAction(data, true);
    case "artist-enqueue": return artistAction(data, false);
    case "queue-play":
    case "queue-up":
    case "queue-down":
    case "queue-remove":
    case "queue-clear": return editQueue(action, data);
    case "devices": return navigate("devices");
    case "transfer": return localPlayback ? notice("Play a Spotify track before switching Spotify devices.") : transfer(data);
    case "seek": {
      const value = Number(data);
      if (!Number.isFinite(value)) return;
      if (!spotifyPlayback()) { if (audio.getAttribute("src")) audio.currentTime = value; }
      else queueSeek(value);
      return;
    }
    case "volume": {
      volume = Math.round(Number(data));
      if (!spotifyPlayback()) { sound.volume = volume / 100; save("spotifast.volume", volume); }
      else {
        const version = connectionVersion;
        clearTimeout(volumeDelay);
        volumeDelay = setTimeout(() => {
          if (version !== connectionVersion || localPlayback) return;
          if (spotify.active === spotify.device && spotify.player) spotify.player.setVolume(volume / 100).catch(fail);
          else control("volume", "PUT", "&volume_percent=" + volume).catch(fail);
        }, 180);
      }
      return;
    }
    case "shuffle":
    case "repeat":
      if (action === "shuffle") {
        if (spotifyPlayback()) await control("shuffle", "PUT", "&state=" + !shuffle);
        shuffle = !shuffle;
      } else {
        if (spotifyPlayback()) await control("repeat", "PUT", "&state=" + (repeat ? "off" : "context"));
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
  if (e.code !== "Space" || e.repeat || settings.open || folderSettings.open || playlistDialog.open || ["TEXTAREA","INPUT","BUTTON"].includes(e.target?.tagName)) return;
  if (e.target?.closest?.("[role=button],[role=slider]")) return;
  e.preventDefault(); toggle().catch(fail);
});
resetPlayback();
const auth = spotify.completeSignIn().catch(error => { status = error.message; announce(status); });
try {
  const loaded = await loadModule({ script: "./spotify-ferro.js", element: document.querySelector("#out") });
  if (!loaded) throw new Error("Could not start FerroUI.");
  const [instance] = await Promise.all([loaded.createRuntime(), auth]);
  if (spotify.connected) {
    stopLocalAudio(); known.clear(); library = []; items = []; liked.clear(); order = []; queue = []; current = {};
    recent = [];
  }
  runtime = instance;
  FerroExports.attach(runtime);
  const previousEntry = history.state?.zunify;
  if (previousEntry) {
    historyEntry.index = previousEntry.index;
    if (previousEntry.page === "local" || isLocal(previousEntry.detail)) {
      page = historyEntry.page = "local";
      title = "Local music";
    }
  }
  history.replaceState({ zunify: historyEntry }, "");
  runtime.runMain(JSON.stringify(model()), innerWidth, matchMedia("(prefers-reduced-motion: reduce)").matches);
  runtime.installBackNavigation();
  const clearReveal = () => runtime.clearReveal?.();
  document.querySelector("#out").addEventListener("pointerleave", clearReveal);
  addEventListener("blur", clearReveal);
  document.querySelector("#splash").remove();
  let size;
  addEventListener("resize", () => { cancelAnimationFrame(size); size = requestAnimationFrame(() => runtime.resize(innerWidth)); });
  setInterval(tick, 250);
  folderReady = restoreFolders();
  folderReady.then(() => rescanFolders(folders.filter(folder => folder.handle))).catch(folderError);
  if (spotify.connected) {
    const version = connectionVersion;
    loadLibrary().then(() => {
      if (version === connectionVersion && ["home", "library", "liked"].includes(page)) {
        return navigate(page, "", true);
      }
    }).catch(fail);
    syncPlayback();
    spotify.enableBrowser().catch(() => {});
    setInterval(syncPlayback, 5000);
  }
} catch (error) {
  document.querySelector("#loading-message").textContent = error.message + " Reload the page to try again.";
  document.querySelector("#splash").classList.add("failed");
  console.error(error);
}
