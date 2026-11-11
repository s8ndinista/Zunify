import { parseBlob } from "music-metadata";

const MAX_COVER = 12 * 1024 * 1024;
const extensions = /\.(mp3|flac|wav|ogg|oga|m4a|mp4)$/i;
const names = ["cover.jpg", "cover.jpeg", "cover.png", "folder.jpg", "folder.jpeg", "folder.png",
  "front.jpg", "front.jpeg", "front.png"];
const clean = value => typeof value === "string" ? value.trim() : "";
const parent = path => path.slice(0, Math.max(path.lastIndexOf("/"), 0));
const front = metadata => {
  const pictures = metadata?.common.picture?.filter(cover => cover.data.length <= MAX_COVER) || [];
  return pictures.find(cover => cover.type === "Cover (front)") || pictures[0];
};
const metadataFor = file => parseBlob(file).catch(() => null);

async function directoryFiles(directory, prefix = "") {
  const files = [];
  const folders = [[directory, prefix]];
  while (folders.length) {
    const [folder, path] = folders.pop();
    for await (const handle of folder.values()) {
      const relative = path + handle.name;
      if (handle.kind === "directory") folders.push([handle, relative + "/"]);
      else if (extensions.test(handle.name) || names.includes(handle.name.toLowerCase())) {
        try { files.push({ file: await handle.getFile(), path: relative }); } catch { /* File disappeared during import. */ }
      }
    }
  }
  return files;
}

export async function pickMusicFiles() {
  if (typeof window.showDirectoryPicker === "function") {
    let directory;
    try {
      directory = await window.showDirectoryPicker({ id: "zunify-music", mode: "read" });
    } catch (error) {
      if (error.name === "AbortError") return null;
      if (error.name !== "SecurityError" && error.name !== "NotSupportedError") throw error;
    }
    if (directory) return directoryFiles(directory);
  }
  return new Promise(resolve => {
    const input = document.createElement("input");
    input.type = "file";
    input.webkitdirectory = true;
    input.multiple = true;
    input.hidden = true;
    document.body.append(input);
    const done = files => { input.remove(); resolve(files); };
    input.addEventListener("cancel", () => done(null), { once: true });
    input.addEventListener("change", () => done([...input.files].map(file => ({
      file, path: file.webkitRelativePath || file.name,
    }))), { once: true });
    input.click();
  });
}

export async function readMusicFiles(entries, version) {
  const files = new Map(), covers = new Map(), albums = [], tracks = [];
  const folderCovers = new Map();
  const audio = [];
  for (const entry of entries) {
    const { path, file } = entry;
    if (extensions.test(file.name)) audio.push(entry);
    else {
      const rank = names.indexOf(file.name.toLowerCase());
      const folder = parent(path);
      if (rank >= 0 && file.size <= MAX_COVER && rank < (folderCovers.get(folder)?.rank ?? Infinity)) {
        folderCovers.set(folder, { file, rank });
      }
    }
  }
  audio.sort((a, b) => a.path.localeCompare(b.path));
  const parsed = new Array(audio.length);
  let index = 0;
  await Promise.all(Array.from({ length: Math.min(audio.length, 4) }, async () => {
    while (index < audio.length) {
      const at = index++;
      const { file, path } = audio[at];
      const metadata = await metadataFor(file);
      const tag = metadata?.common ?? {};
      const album = clean(tag.album);
      const albumArtist = clean(tag.albumartist);
      const folder = parent(path);
      const group = JSON.stringify([album.toLowerCase(), albumArtist.toLowerCase(), albumArtist ? "" : folder]);
      const id = "local:track:" + version + ":" + at;
      const embedded = Boolean(front(metadata));
      parsed[at] = {
        file, group, albumArtist, disc: tag.disk?.no || 0, number: tag.track?.no || Infinity,
        embedded, folderCover: folderCovers.get(folder)?.file,
        track: { id, kind: "track", album, artist: clean(tag.artist), art: "",
          title: clean(tag.title) || file.name.replace(extensions, ""), duration: metadata?.format.duration || 0 },
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
      albums.push({ id: "local:album:" + version + ":" + groupIndex, kind: "album", title: track.album, art,
        artist: albumArtist || (group.every(row => row.track.artist === track.artist) ? track.artist : ""),
        tracks: group.map(row => row.track) });
    }
    groupIndex++;
  }
  return { tracks, albums, files, covers };
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
