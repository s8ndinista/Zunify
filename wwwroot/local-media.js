import { parseBlob } from "music-metadata";

const MAX_COVER = 12 * 1024 * 1024;
const extensions = /\.(mp3|flac|wav|ogg|oga|m4a|mp4)$/i;
const names = ["cover.jpg", "cover.jpeg", "cover.png", "folder.jpg", "folder.jpeg", "folder.png",
  "front.jpg", "front.jpeg", "front.png"];
const relevant = name => extensions.test(name) || names.includes(name.toLowerCase());
const folderRequired = () => new Error("Zunify did not receive a folder. Drag the folder itself from your file manager onto Local music.");
const clean = value => typeof value === "string" ? value.trim() : "";
const parent = path => path.slice(0, Math.max(path.lastIndexOf("/"), 0));
const front = metadata => {
  const pictures = metadata?.common.picture?.filter(cover => cover.data.length <= MAX_COVER) || [];
  return pictures.find(cover => cover.type === "Cover (front)") || pictures[0];
};
const metadataFor = file => parseBlob(file).catch(() => null);

async function directoryFiles(directory, prefix = directory.name + "/") {
  const files = [];
  const folders = [[directory, prefix]];
  while (folders.length) {
    const [folder, path] = folders.pop();
    for await (const handle of folder.values()) {
      const relative = path + handle.name;
      if (handle.kind === "directory") folders.push([handle, relative + "/"]);
      else if (relevant(handle.name)) {
        try { files.push({ file: await handle.getFile(), path: relative }); } catch { /* File disappeared during import. */ }
      }
    }
  }
  return files;
}

async function entryFiles(roots) {
  const files = [];
  const pending = roots.map(root => [root, root.name]);
  while (pending.length) {
    const [entry, path] = pending.pop();
    if (entry.isDirectory) {
      const reader = entry.createReader();
      while (true) {
        // Some browsers return only one batch per call.
        const children = await new Promise((resolve, reject) => reader.readEntries(resolve, reject));
        if (!children.length) break;
        for (const child of children) pending.push([child, path + "/" + child.name]);
      }
    } else if (entry.isFile && relevant(entry.name)) {
      try {
        const file = await new Promise((resolve, reject) => entry.file(resolve, reject));
        files.push({ file, path });
      } catch { /* A file can disappear during import. */ }
    }
  }
  return files;
}

export async function droppedMusicFiles(transfer) {
  // Read the directory entries while the browser is dispatching the drop event.
  const items = Array.from(transfer.items ?? []).filter(item => item.kind === "file");
  const handles = items.map(async item => {
    try { return await item.getAsFileSystemHandle?.(); }
    catch { return null; }
  });
  const directories = items.map(item =>
    (item.getAsEntry ?? item.webkitGetAsEntry)?.call(item)).filter(entry => entry?.isDirectory);
  const nativeDirectories = (await Promise.all(handles)).filter(handle => handle?.kind === "directory");
  if (nativeDirectories.length && nativeDirectories.length >= directories.length) {
    return readMusicFolders(nativeDirectories);
  }
  if (!directories.length) throw folderRequired();
  return { entries: await entryFiles(directories), names: directories.map(entry => entry.name) };
}

export async function readMusicFolders(handles) {
  const entries = (await Promise.all(handles.map(handle => directoryFiles(handle)))).flat();
  return { entries, handles, names: handles.map(handle => handle.name) };
}

export async function pickMusicFiles(signal) {
  if (typeof window.showDirectoryPicker === "function") {
    let directory;
    try {
      directory = await window.showDirectoryPicker({ id: "zunify-music", mode: "read" });
    } catch (error) {
      if (signal.aborted || error.name === "AbortError") return null;
      if (error.name !== "SecurityError" && error.name !== "NotSupportedError") throw error;
    }
    if (signal.aborted) return null;
    if (directory) return readMusicFolders([directory]);
  }
  const input = document.createElement("input");
  input.type = "file";
  if (!("webkitdirectory" in input)) {
    throw folderRequired();
  }
  if (navigator.userActivation?.isActive === false) {
    throw new Error("The browser blocked the picker. Drag the folder itself onto Local music, or click Choose music folder again.");
  }
  input.setAttribute("webkitdirectory", "");
  input.multiple = true;
  return new Promise((resolve, reject) => {
    input.hidden = true;
    document.body.append(input);
    const close = () => {
      input.remove();
      signal.removeEventListener("abort", cancel);
    };
    const done = files => { close(); resolve(files); };
    const cancel = () => done(null);
    signal.addEventListener("abort", cancel, { once: true });
    input.addEventListener("cancel", cancel, { once: true });
    input.addEventListener("change", () => {
      const directories = Array.from(input.webkitEntries ?? []).filter(entry => entry.isDirectory);
      const files = Array.from(input.files).filter(file => file.webkitRelativePath);
      if (directories.length) {
        done(entryFiles(directories).then(entries => ({ entries, names: directories.map(entry => entry.name) })));
      } else if (files.length) {
        const entries = files.map(file => ({ file, path: file.webkitRelativePath }));
        done({ entries, names: [...new Set(entries.map(({ path }) => path.split("/")[0]))] });
      }
      else { close(); reject(folderRequired()); }
    }, { once: true });
    try { input.click(); }
    catch (error) { close(); reject(error); }
  });
}

export async function readMusicFiles(entries, version, previous = new Map()) {
  const files = new Map(), covers = new Map(), albums = [], tracks = [];
  const metadataCache = new Map();
  const folderCovers = new Map();
  const audioFiles = new Map();
  for (const entry of entries) {
    const { path, file } = entry;
    if (extensions.test(file.name)) audioFiles.set(path, entry);
    else {
      const rank = names.indexOf(file.name.toLowerCase());
      const folder = parent(path);
      if (rank >= 0 && file.size <= MAX_COVER && rank < (folderCovers.get(folder)?.rank ?? Infinity)) {
        folderCovers.set(folder, { file, rank });
      }
    }
  }
  const audio = [...audioFiles.values()].sort((a, b) => a.path.localeCompare(b.path));
  const parsed = new Array(audio.length);
  let index = 0;
  await Promise.all(Array.from({ length: Math.min(audio.length, 4) }, async () => {
    while (index < audio.length) {
      const at = index++;
      const { file, path } = audio[at];
      let saved = previous.get(path);
      if (saved?.size !== file.size || saved.modified !== file.lastModified) {
        const metadata = await metadataFor(file);
        const tag = metadata?.common ?? {};
        saved = {
          size: file.size, modified: file.lastModified, album: clean(tag.album),
          albumArtist: clean(tag.albumartist), artist: clean(tag.artist),
          disc: tag.disk?.no || 0, number: tag.track?.no || Infinity,
          embedded: Boolean(front(metadata)), duration: metadata?.format.duration || 0,
          title: clean(tag.title) || file.name.replace(extensions, ""),
        };
      }
      metadataCache.set(path, saved);
      const { album, albumArtist, artist, disc, number, embedded, duration, title } = saved;
      const folder = parent(path);
      const group = JSON.stringify([album.toLowerCase(), albumArtist.toLowerCase(), albumArtist ? "" : folder]);
      const id = "local:track:" + encodeURIComponent(path);
      parsed[at] = {
        file, group, albumArtist, disc, number,
        embedded, folderCover: folderCovers.get(folder)?.file,
        track: { id, kind: "track", album, artist, title, duration, art: "" },
      };
    }
  }));
  const groups = new Map();
  for (const row of parsed) {
    if (!groups.has(row.group)) groups.set(row.group, []);
    groups.get(row.group).push(row);
  }
  let groupIndex = 0;
  for (const group of [...groups.values()].sort(([a], [b]) => a.track.album.localeCompare(b.track.album))) {
    group.sort((a, b) => a.disc - b.disc || a.number - b.number || a.track.title.localeCompare(b.track.title));
    const embedded = group.find(row => row.embedded);
    const folderCover = group.find(row => row.folderCover)?.folderCover;
    const art = embedded || folderCover ? "local-cover:" + version + ":" + groupIndex : "";
    if (embedded) {
      covers.set(art, async () => {
        const picture = front(await metadataFor(embedded.file));
        return picture ? new Blob([picture.data], { type: picture.format }) : folderCover;
      });
    } else if (folderCover) covers.set(art, async () => folderCover);
    for (const { track, file } of group) {
      track.art = art;
      files.set(track.id, file);
      tracks.push(track);
    }
    const { track, albumArtist } = group[0];
    if (track.album) {
      albums.push({ id: "local:album:" + encodeURIComponent(group[0].group), kind: "album", title: track.album, art,
        artist: albumArtist || (group.every(row => row.track.artist === track.artist) ? track.artist : ""),
        tracks: group.map(row => row.track) });
    }
    groupIndex++;
  }
  return { tracks, albums, files, covers, metadataCache };
}

export async function findCover(album) {
  const quote = value => value.replace(/[\\"]/g, "\\$&");
  const query = 'releasegroup:"' + quote(album.title) + '" AND artist:"' + quote(album.artist) + '"';
  const params = "query=" + encodeURIComponent(query) + "&fmt=json&limit=5";
  const options = { credentials: "omit", referrerPolicy: "no-referrer", signal: AbortSignal.timeout(8000) };
  const search = await fetch("https://musicbrainz.org/ws/2/release-group?" + params, options);
  if (!search.ok) throw new Error("Cover art lookup is unavailable. Try again later.");
  const data = await search.json();
  const same = (a, b) => clean(a).toLowerCase() === clean(b).toLowerCase();
  const match = data["release-groups"]?.find(group => same(group.title, album.title) &&
    group["artist-credit"]?.some(artist => same(artist.name, album.artist)));
  if (!/^[0-9a-f-]{36}$/i.test(match?.id ?? "")) throw new Error("No matching album was found in MusicBrainz.");
  const cover = await fetch("https://coverartarchive.org/release-group/" + match.id + "/front-250", {
    ...options, signal: AbortSignal.timeout(8000),
  });
  if (cover.status === 404) throw new Error("The Cover Art Archive has no cover for this album.");
  if (!cover.ok || !cover.headers.get("content-type")?.startsWith("image/")) {
    throw new Error("The archive did not return a cover image.");
  }
  const blob = await cover.blob();
  if (blob.size > 4 * 1024 * 1024) throw new Error("The cover image is too large.");
  return blob;
}
