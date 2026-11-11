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

export const loadFolder = () => withFolders("readonly", store => store.get("library"));
export const saveFolder = folder => withFolders("readwrite", store => store.put(folder, "library"));
