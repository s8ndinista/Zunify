const accounts = "https://accounts.spotify.com";
const api = "https://api.spotify.com/v1";
const key = "spotifast.spotify.session";
const pending = "spotifast.spotify.pending";
const scope = [
  "streaming", "user-read-email", "user-read-private", "user-library-read",
  "user-library-modify", "user-follow-read", "playlist-read-private", "playlist-read-collaborative",
  "user-read-playback-state", "user-modify-playback-state", "user-read-currently-playing",
].join(" ");
const encode = bytes => btoa(String.fromCharCode(...bytes)).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
const redirect = () => new URL("./", location.href).href;
const safeRead = name => {
  try { return JSON.parse(sessionStorage.getItem(name)); } catch { return null; }
};
const write = (name, value) => sessionStorage.setItem(name, JSON.stringify(value));

export class SpotifyError extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}
export class Spotify {
  session = safeRead(key);
  refreshPromise;
  player;
  ready;
  device = "";
  active = "";
  retryAfter = 0;
  generation = 0;
  error = () => {};
  state = () => {};
  devicesChanged = () => {};

  get connected() { return Boolean(this.session?.access_token); }
  get redirect() { return redirect(); }

  async signIn(clientId) {
    if (!/^[a-f0-9]{32}$/i.test(clientId)) throw new Error("Enter the 32-character Client ID from your Spotify dashboard.");
    if (!crypto.subtle) throw new Error("Spotify sign-in requires HTTPS or 127.0.0.1.");
    const verifier = encode(crypto.getRandomValues(new Uint8Array(64)));
    const state = encode(crypto.getRandomValues(new Uint8Array(32)));
    const challenge = encode(new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier))));
    write(pending, { clientId, state, verifier, time: Date.now() });
    location.assign(accounts + "/authorize?" + new URLSearchParams({
      client_id: clientId, response_type: "code", redirect_uri: redirect(), scope, state,
      code_challenge_method: "S256", code_challenge: challenge,
    }));
  }

  async completeSignIn() {
    const original = location.search;
    const params = new URLSearchParams(original);
    if (!params.has("code") && !params.has("error")) return;
    const request = safeRead(pending);
    sessionStorage.removeItem(pending);
    for (const key of ["code", "state", "error"]) params.delete(key);
    history.replaceState(null, "", location.pathname + (params.size ? "?" + params : "") + location.hash);
    const result = new URLSearchParams(original);
    if (!request || result.get("state") !== request.state || Date.now() - request.time > 600_000) {
      throw new Error("This Spotify sign-in has expired. Please start it again.");
    }
    if (result.has("error")) throw new Error("Spotify sign-in was cancelled.");
    await this.exchange({
      grant_type: "authorization_code", code: result.get("code"), redirect_uri: redirect(),
      client_id: request.clientId, code_verifier: request.verifier,
    });
  }

  async exchange(data) {
    const generation = this.generation;
    const response = await fetch(accounts + "/api/token", { method: "POST", body: new URLSearchParams(data), signal: AbortSignal.timeout(15_000) });
    if (!response.ok) throw new SpotifyError(response.status, "Spotify authorization failed. Check the Client ID and redirect URL, then sign in again.");
    const token = await response.json();
    if (generation !== this.generation) throw new Error("Spotify signed out.");
    this.session = {
      ...token, clientId: data.client_id, expires: Date.now() + token.expires_in * 1000,
      refresh_token: token.refresh_token ?? this.session?.refresh_token,
    };
    write(key, this.session);
  }

  async token(force = false) {
    if (!this.session) throw new Error("Connect a Spotify account first.");
    if (!force && this.session.expires > Date.now() + 30_000) return this.session.access_token;
    if (!this.session.refresh_token) throw new Error("Your session expired. Sign out and connect again.");
    const { clientId, refresh_token } = this.session;
    this.refreshPromise ??= this.exchange({ client_id: clientId, refresh_token, grant_type: "refresh_token" })
      .finally(() => { this.refreshPromise = undefined; });
    await this.refreshPromise;
    return this.session.access_token;
  }

  async request(path, method = "GET", body, retry = true) {
    if (Date.now() < this.retryAfter) throw new SpotifyError(429, "Spotify is limiting requests. Give it a moment before trying again.");
    const target = new URL(api + path);
    const headers = { Authorization: "Bearer " + await this.token() };
    if (body !== undefined) headers["Content-Type"] = "application/json";
    const r = await fetch(target, { method, headers, signal: AbortSignal.timeout(15_000), body: body === undefined ? undefined : JSON.stringify(body) });
    if (r.status === 401 && retry) {
      await this.token(true);
      return this.request(path, method, body, false);
    }
    if (!r.ok) {
      if (r.status === 429) this.retryAfter = Date.now() + (Number(r.headers.get("Retry-After")) || 10) * 1000;
      const detail = await r.json().catch(() => null);
      throw new SpotifyError(r.status, ({
        403: "Spotify denied this action. Check Premium, your app’s user access, and whether you own the playlist.",
        404: "Spotify couldn't find the item or an active device. Choose a device and try again.",
        429: "Spotify is limiting requests. Give it a moment before trying again.",
      })[r.status] || detail?.error?.message || "Spotify request failed (" + r.status + ").");
    }
    // Write endpoints acknowledge the command. Only reads return data to the app.
    if (method !== "GET" || r.status === 204) return null;
    const json = (await r.text()).trim();
    try {
      return json ? JSON.parse(json) : null;
    } catch {
      throw new SpotifyError(r.status, "Spotify returned an unreadable response.");
    }
  }

  async page(path, limit = 100, key) {
    const rows = [];
    for (let next = path; next && rows.length < limit;) {
      const data = await this.request(next);
      const result = key ? data?.[key] : data;
      rows.push(...(result?.items ?? []));
      const url = result?.next && new URL(result.next);
      next = url?.origin === "https://api.spotify.com" && url.pathname.startsWith("/v1/")
        ? url.pathname.slice(3) + url.search : null;
    }
    return rows.slice(0, limit);
  }

  enableBrowser() {
    if (!this.ready) {
      const generation = this.generation;
      this.ready = this.createPlayer(generation).catch(error => {
        if (generation === this.generation) {
          this.player?.disconnect();
          this.player = undefined;
          this.ready = undefined;
          this.device = "";
        }
        throw error;
      });
    }
    return this.ready;
  }

  async createPlayer(generation) {
    if (!window.Spotify?.Player) {
      await new Promise((resolve, reject) => {
        const script = document.createElement("script");
        const timeout = setTimeout(() => fail(), 12_000);
        const fail = () => {
          clearTimeout(timeout);
          delete window.onSpotifyWebPlaybackSDKReady;
          script.remove();
          reject(new Error("Could not load Spotify playback. Check your connection or content blocker."));
        };
        window.onSpotifyWebPlaybackSDKReady = () => {
          clearTimeout(timeout);
          delete window.onSpotifyWebPlaybackSDKReady;
          resolve();
        };
        script.src = "https://sdk.scdn.co/spotify-player.js";
        script.onerror = fail;
        document.head.append(script);
      });
    }
    if (generation !== this.generation) throw new Error("Spotify signed out.");
    const player = this.player = new window.Spotify.Player({
      name: "SpotiFe",
      getOAuthToken: callback => this.token().then(callback).catch(this.error),
      volume: 0.65,
    });
    await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => fail("Spotify playback timed out. Try again or choose another device."), 15_000);
      const fail = message => { clearTimeout(timeout); reject(new Error(message)); };
      player.addListener("ready", ({ device_id }) => {
        clearTimeout(timeout);
        if (generation !== this.generation) return;
        this.device = device_id;
        this.devicesChanged();
        resolve();
      });
      player.addListener("not_ready", () => {
        if (generation !== this.generation) return;
        this.device = "";
        this.devicesChanged();
      });
      for (const event of ["initialization_error", "authentication_error", "account_error", "playback_error"]) {
        player.addListener(event, ({ message }) => {
          this.error(new Error(message));
          fail(message);
        });
      }
      player.addListener("autoplay_failed", () => this.error(new Error("Playback was blocked. Press Play to start listening.")));
      player.addListener("player_state_changed", state => { if (state) this.state(state); });
      player.connect().then(ok => { if (!ok) fail("Spotify could not connect the browser player."); }, reject);
    });
    return player;
  }

  signOut() {
    this.generation++;
    this.player?.disconnect();
    this.ready = this.player = this.session = undefined;
    this.active = this.device = "";
    this.retryAfter = 0;
    sessionStorage.removeItem(key);
    sessionStorage.removeItem(pending);
  }
}

export function itemOf(raw, kind = raw?.type) {
  if (!raw?.uri || raw.is_local || raw.is_playable === false) return null;
  const type = kind || "track";
  return {
    id: raw.uri, kind: type, title: raw.name ?? "",
    artist: raw.artists?.map(a => a.name).join(", ") || raw.owner?.display_name || raw.publisher || (type === "artist" ? "Artist" : type === "playlist" ? "Playlist" : ""),
    album: raw.album?.name ?? "",
    art: (raw.album?.images ?? raw.images)?.find(im => im?.url)?.url ?? "",
    duration: (raw.duration_ms || 0) / 1000,
  };
}
