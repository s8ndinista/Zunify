async function withFolders(mode, operation) {
  const db = await new Promise((resolve, reject) => {
    const request = indexedDB.open("zunify-local-music", 1);
    request.onupgradeneeded = () => request.result.createObjectStore("folders");
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
  try {
    return await new Promise((resolve, reject) => {
      const transaction = db.transaction("folders", mode);
      const request = operation(transaction.objectStore("folders"));
      transaction.oncomplete = () => resolve(request.result);
      transaction.onabort = () => reject(transaction.error);
    });
  } finally {
    db.close();
  }
}

export async function loadFolders() {
  const saved = await withFolders("readonly", store => store.get("library"));
  if (saved?.folders) return saved.folders;
  if (!saved?.names) return [];
  return saved.names.map((name, index) => {
    const id = crypto.randomUUID();
    const belongs = track => decodeURIComponent(track.id.slice("local:track:".length)).startsWith(name + "/");
    const tracks = (saved.catalog?.tracks ?? []).filter(belongs);
    const albums = (saved.catalog?.albums ?? []).map(album => ({
      ...album, id: "local:album:" + id + ":" + album.id.slice("local:album:".length),
      tracks: album.tracks.filter(belongs),
    })).filter(album => album.tracks.length);
    const metadata = new Map([...(saved.metadata ?? [])].filter(([path]) => path.startsWith(name + "/")));
    return { id, name, legacy: true, handle: saved.handles?.[index], catalog: { tracks, albums }, metadata };
  });
}

export const saveFolders = folders => withFolders("readwrite", store => store.put({ folders }, "library"));
