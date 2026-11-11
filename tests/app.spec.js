import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { test, expect } from "@playwright/test";

const button = (page, name) => page.getByRole("button", { name, exact: true });
const press = async (page, name, n = 0) => {
  const control = button(page, name).nth(n);
  await expect(control).toBeAttached();
  // FerroUI mirrors accessible controls into visually hidden DOM elements.
  // A click on the mirror uses the same invoke pattern as assistive software.
  await control.dispatchEvent("click");
  // Let FerroUI publish its updated accessibility tree before invoking another control.
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
};
// A strip of the transport fire that no control overlaps. It is static until music lights it.
const fire = page => page.screenshot({ clip: { x: 160, y: 880, width: 360, height: 20 } });
const lit = (page, dark) => async () => !(await fire(page)).equals(dark);
const boot = async page => {
  await page.goto("./", { waitUntil: "domcontentloaded" });
  await expect(page.locator("#splash")).toHaveCount(0, { timeout: 45_000 });
  await expect(page.locator("#out canvas").first()).toBeVisible();
  await expect(page).toHaveTitle("Zunify · Your music");
};

function taggedWave(title, album = "Folder album", cover) {
  const chunk = (name, bytes) => {
    const header = Buffer.alloc(8);
    header.write(name);
    header.writeUInt32LE(bytes.length, 4);
    return Buffer.concat([header, bytes, Buffer.alloc(bytes.length % 2)]);
  };
  const field = (name, value) => chunk(name, Buffer.from(value + "\0"));
  const format = Buffer.alloc(16);
  format.writeUInt16LE(1, 0);
  format.writeUInt16LE(1, 2);
  format.writeUInt32LE(8000, 4);
  format.writeUInt32LE(16000, 8);
  format.writeUInt16LE(2, 12);
  format.writeUInt16LE(16, 14);
  const embedded = [];
  if (cover) {
    const picture = Buffer.concat([Buffer.from([0]), Buffer.from("image/jpeg\0"), Buffer.from([3, 0]), cover]);
    const frame = Buffer.alloc(10);
    frame.write("APIC");
    frame.writeUInt32BE(picture.length, 4);
    const length = frame.length + picture.length;
    const size = Buffer.from([length >>> 21, length >>> 14 & 127, length >>> 7 & 127, length & 127]);
    const tag = Buffer.concat([Buffer.from("ID3"), Buffer.from([3, 0, 0]), size, frame, picture]);
    embedded.push(chunk("ID3 ", tag));
  }
  return chunk("RIFF", Buffer.concat([
    Buffer.from("WAVE"), chunk("fmt ", format), chunk("data", Buffer.alloc(32_000)),
    chunk("LIST", Buffer.concat([Buffer.from("INFO"), field("INAM", title), field("IART", "Folder artist"), field("IPRD", album)])),
    ...embedded,
  ]));
}

test("imports a music folder, virtualizes its album tracks, and goes back", async ({ page }, info) => {
  const folder = info.outputPath("test music");
  const album = join(folder, "Album");
  await mkdir(album, { recursive: true });
  const secondAlbum = join(folder, "Missing cover");
  await mkdir(secondAlbum);
  await writeFile(join(secondAlbum, "01.wav"), taggedWave("Song without cover", "Missing cover"));
  const embeddedAlbum = join(folder, "Embedded cover");
  await mkdir(embeddedAlbum);
  const embeddedCover = await readFile("wwwroot/art/mix-3.jpg");
  await writeFile(join(embeddedAlbum, "01.wav"), taggedWave("Song with cover", "Embedded cover", embeddedCover));
  await Promise.all(Array.from({ length: 80 }, (_, index) => {
    const number = String(index + 1).padStart(2, "0");
    return writeFile(join(album, number + ".wav"), taggedWave("Tagged track " + number));
  }));
  await writeFile(join(album, "cover.jpg"), await readFile("wwwroot/art/mix-1.jpg"));
  await page.addInitScript(() => { window.showDirectoryPicker = undefined; });
  await boot(page);
  await press(page, "Your library");
  await press(page, "Local music");
  const picker = await button(page, "Choose music folder").boundingBox();
  const chooser = page.waitForEvent("filechooser");
  await page.mouse.click(picker.x + picker.width / 2, picker.y + picker.height / 2);
  await (await chooser).setFiles(folder);
  await expect(button(page, "Folder album")).toBeAttached();
  await press(page, "Folder album");
  const header = button(page, "Play all");
  await expect(header).toBeAttached();
  await expect(button(page, "Find cover art")).toHaveCount(0);
  await expect(button(page, "Play Tagged track 01")).toBeAttached();
  expect(await page.getByRole("button", { name: /^Play Tagged track/ }).count()).toBeLessThan(80);
  const before = await header.boundingBox();
  await page.mouse.move(600, 550);
  await page.mouse.wheel(0, 1600);
  await expect(button(page, "Play Tagged track 01")).toHaveCount(0);
  expect((await header.boundingBox()).y).toBe(before.y);
  await press(page, "Play all");
  await expect.poll(() => page.locator("audio").evaluate(audio => audio.currentTime)).toBeGreaterThan(0);
  await expect(page.locator("audio")).toHaveAttribute("src", /^blob:/);
  await press(page, "Back");
  await expect(button(page, "Choose music folder")).toBeAttached();
  await page.goForward();
  await expect(button(page, "Play all")).toBeAttached();
  await page.evaluate(() => {
    const dispatch = globalThis.spotifyFerroAction;
    globalThis.ferroBackRequests = 0;
    globalThis.spotifyFerroAction = (action, data) => {
      if (action === "back") globalThis.ferroBackRequests++;
      return dispatch(action, data);
    };
  });
  await page.goBack();
  await expect(button(page, "Choose music folder")).toBeAttached();
  expect(await page.evaluate(() => globalThis.ferroBackRequests)).toBe(1);
  await page.route("https://musicbrainz.org/ws/2/release-group?*", route => {
    expect(new URL(route.request().url()).searchParams.get("query")).toContain('releasegroup:"Missing cover"');
    return route.fulfill({ headers: { "access-control-allow-origin": "*" }, json: {
      "release-groups": [{ id: "9162580e-5df4-32de-80cc-f45a8d8a9b1d", title: "Missing cover",
        "artist-credit": [{ name: "Folder artist" }] }],
    } });
  });
  await page.route("https://coverartarchive.org/release-group/**", route => route.fulfill({
    headers: { "access-control-allow-origin": "*" }, path: "wwwroot/art/mix-2.jpg", contentType: "image/jpeg",
  }));
  await press(page, "Missing cover");
  await press(page, "Find cover art");
  await expect(button(page, "Find cover art")).toHaveCount(0);
  await press(page, "Back");
  await expect(button(page, "Choose music folder")).toBeAttached();
  await press(page, "Embedded cover");
  await expect(button(page, "Play Song with cover")).toBeAttached();
  await expect(button(page, "Find cover art")).toHaveCount(0);
  await press(page, "Back");
  await expect(button(page, "Choose music folder")).toBeAttached();
  await press(page, "Back");
  await expect(button(page, "Local music")).toBeAttached();
});
test("plays a bundled mix and supports queueing, liking and mobile layout", async ({ page }) => {
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await boot(page);
  await expect(button(page, "Play mix")).toBeAttached();
  await page.mouse.move(412, 350);
  const dark = await fire(page);
  await page.mouse.click(412, 350);
  await expect.poll(() => page.locator("audio").evaluate(audio => audio.currentTime)).toBeGreaterThan(0.05);
  // The spectrum is analyzed from the audio as it plays, ahead of the volume control.
  await expect.poll(lit(page, dark)).toBe(true);
  await press(page, "Pause");
  await expect.poll(() => page.locator("audio").evaluate(audio => audio.paused)).toBe(true);
  await expect(button(page, "Play")).toBeAttached();
  await expect.poll(lit(page, dark)).toBe(false);
  await press(page, "Next track");
  await expect.poll(() => page.locator("audio").evaluate(audio => audio.getAttribute("src"))).toMatch(/sample-5/);
  await press(page, "Like current track");
  await press(page, "Your library");
  await press(page, "Liked songs");
  await expect(button(page, "Play Low Orbit").first()).toBeAttached();
  await press(page, "Add to queue");
  await press(page, "Now playing");
  await expect(page.getByText("Up next", { exact: true })).toBeAttached();
  await expect(button(page, "Play Midnight Signal").first()).toBeAttached();
  await press(page, "Move Midnight Signal up in queue");
  await press(page, "Move Midnight Signal up in queue");
  await press(page, "Next track");
  await expect.poll(() => page.locator("audio").evaluate(audio => audio.getAttribute("src"))).toMatch(/sample-1/);
  await press(page, "Remove Blue Hour from queue");
  await expect(button(page, "Play Blue Hour")).toHaveCount(0);
  await press(page, "Clear queue");
  await expect(page.getByText("Your queue is empty.")).toBeAttached();
  await press(page, "Your library");
  const artistQueue = button(page, "Add Glass Meridian to queue");
  await expect(artistQueue).toBeAttached();
  const artistAction = await artistQueue.boundingBox();
  await page.mouse.move(artistAction.x + artistAction.width / 2, artistAction.y + artistAction.height / 2);
  await page.mouse.click(artistAction.x + artistAction.width / 2, artistAction.y + artistAction.height / 2);
  await expect(page.locator("#announcement")).toHaveText("Added Glass Meridian to queue");
  await press(page, "Add Deep Focus to queue");
  await press(page, "Now playing");
  await expect(button(page, "Play Low Orbit").first()).toBeAttached();
  await expect(button(page, "Play A Place Between")).toBeAttached();
  await expect(button(page, "Play Soft Horizons")).toBeAttached();
  await page.setViewportSize({ width: 390, height: 780 });
  await expect(button(page, "Spotify settings")).toBeAttached({ timeout: 15_000 });
  await expect(button(page, "Your library")).toHaveCount(1);
  await press(page, "Your library");
  await expect(button(page, "Deep Focus").first()).toBeAttached();
  expect(errors).toEqual([]);
});

test("search accepts keyboard input and opens matching collections", async ({ page }) => {
  await boot(page);
  await page.mouse.click(755, 38);
  await page.keyboard.type("weightless");
  await expect(page.getByText("Results in the demo library")).toBeAttached();
  await expect(button(page, "Weightless")).toHaveCount(1);
  await expect(button(page, "Play Blue Hour").first()).toBeAttached();
  await press(page, "Weightless");
  await expect(button(page, "Play all")).toBeAttached();
  await press(page, "Play all");
  await expect.poll(() => page.locator("audio").evaluate(audio => audio.getAttribute("src"))).toMatch(/sample-3/);
  await press(page, "Play A Place Between");
  await press(page, "Like current track");
  await press(page, "Quickplay");
  await expect(page.getByText("liked songs", { exact: true })).toBeAttached();
  await expect(page.getByText("history", { exact: true })).toBeAttached();
  await expect(button(page, "Play A Place Between").first()).toBeAttached();
  await boot(page);
  await expect(button(page, "Play A Place Between").first()).toBeAttached();

});

test("offline install caches the app and demo audio", async ({ page, context }) => {
  await boot(page);
  await page.evaluate(() => navigator.serviceWorker.ready);
  await expect.poll(() => page.evaluate(() => Boolean(navigator.serviceWorker.controller))).toBe(true);
  const manifest = await page.evaluate(() => fetch("./manifest.webmanifest").then(r => r.json()));
  expect(manifest.name).toBe("Zunify");
  expect(manifest.display).toBe("standalone");
  await context.setOffline(true);
  try {
    await boot(page);
    await expect(button(page, "Play mix")).toBeAttached();
    await page.mouse.click(412, 350);
    await expect.poll(() => page.locator("audio").evaluate(audio => audio.currentTime)).toBeGreaterThan(0.05);
  } finally {
    await context.setOffline(false);
  }
});

test.describe("mocked Spotify integration", () => {
  test.use({ serviceWorkers: "block" });
  for (const mode of ["shared", "manual"]) {
    test(`Spotify signs in with PKCE using ${mode} configuration`, async ({ page, baseURL }) => {
      const clientId = mode === "shared" ? "a".repeat(32) : "b".repeat(32);
      const redirectUri = baseURL;
      await page.route("**/spotify-config.js", route => route.fulfill({
        contentType: "application/javascript",
        body: `export const spotifyClientId = ${JSON.stringify(mode === "shared" ? clientId : "")};`,
      }));
      let challenge;
      const raw = {
        type: "track", uri: "spotify:track:test", name: "Test Song", duration_ms: 100000,
        artists: [{ name: "Test Artist" }], album: { name: "Test Album", images: [] },
      };
      await page.route("https://accounts.spotify.com/authorize?**", route => {
        const params = new URL(route.request().url()).searchParams;
        challenge = params.get("code_challenge");
        expect(params.get("client_id")).toBe(clientId);
        expect(params.get("code_challenge_method")).toBe("S256");
        expect(params.get("redirect_uri")).toBe(redirectUri);
        return route.fulfill({ status: 302, headers: { location: `${redirectUri}?code=example&state=${params.get("state")}` } });
      });
      await page.route("https://accounts.spotify.com/api/token", route => {
        const params = new URLSearchParams(route.request().postData());
        expect(createHash("sha256").update(params.get("code_verifier")).digest("base64url")).toBe(challenge);
        expect(params.get("client_id")).toBe(clientId);
        expect(params.get("redirect_uri")).toBe(redirectUri);
        expect(params.has("client_secret")).toBe(false);
        return route.fulfill({ headers: { "access-control-allow-origin": "*" },
          json: { access_token: "test-token", refresh_token: "test-refresh", expires_in: 3600 } });
      });
      await page.route("https://sdk.scdn.co/spotify-player.js", route => route.fulfill({
        contentType: "application/javascript",
        body: `window.Spotify={Player:class {
          constructor({name}){this.name=name;}
          listeners={}; addListener(n,cb){this.listeners[n]=cb;}
          connect(){window.mockSpotifyPlayer=this;queueMicrotask(()=>this.listeners.ready({device_id:"web-zunify"}));return Promise.resolve(true);}
          disconnect(){}
          getCurrentState(){return Promise.resolve(window.mockPlaybackState ?? null);}
          seek(ms){window.mockSeekCalls.push(ms);return Promise.resolve();}
          // Stands in for the protected stream: a tone the page's own audio graph never sees.
          activateElement(){
            if(!this.tone){this.tone=new AudioContext();const o=this.tone.createOscillator();
              o.frequency.value=220;const g=this.tone.createGain();g.gain.value=0.08;o.connect(g).connect(this.tone.destination);o.start();}
            return this.tone.resume();
          }
        }}; window.onSpotifyWebPlaybackSDKReady();`,
      }));
      await page.addInitScript(({ raw, largeLibrary }) => {
        const fetchReal = window.fetch.bind(window);
        const saved = [{ track: raw }];
        const playlists = [];
        if (largeLibrary) {
          for (let index = 1; index <= 76; index++) {
            saved.push({ track: { ...raw, uri: "spotify:track:library" + index, name: "Library song " + index,
              artists: [{ name: "Library artist " + String(index).padStart(2, "0") }],
            } });
          }
          for (let index = 1; index <= 48; index++) {
            playlists.push({ uri: "spotify:playlist:library" + index, name: "Library playlist " + index });
          }
        }
        window.spotifyRequests = [];
        window.mockSeekCalls = [];
        window.fetch = async (input, options = {}) => {
          const url = new URL(input, location.href);
          if (url.origin !== "https://api.spotify.com") return fetchReal(input, options);
          window.spotifyRequests.push({
            path: url.pathname, params: Object.fromEntries(url.searchParams), method: options.method,
            authorization: new Headers(options.headers).get("authorization"),
            body: options.body ? JSON.parse(options.body) : null,
          });
          if (url.pathname === "/v1/me/tracks") return Response.json({ items: saved });
          if (url.pathname === "/v1/me/playlists") return Response.json({ items: playlists });
          if (url.pathname === "/v1/me/albums") return Response.json({ items: [
            { album: { type: "album", uri: "spotify:album:test", name: "Test Album", artists: raw.artists } },
          ] });
          if (url.pathname === "/v1/me/following") return Response.json({ artists: { items: [
            { type: "artist", uri: "spotify:artist:test", name: "Catalog Artist" },
          ] } });
          if (url.pathname === "/v1/search") return Response.json({ tracks: { items: [
            { ...raw, uri: "spotify:track:unrelated", artists: [{ name: "Catalog Artist", uri: "spotify:artist:other" }] },
            { ...raw, uri: "spotify:track:byartist", artists: [{ name: "Catalog Artist", uri: "spotify:artist:test" }] },
          ] } });
          if (url.pathname === "/v1/albums/test/tracks") return Response.json({ items: [
            { ...raw, uri: "spotify:track:album1", name: "Album track one" },
            { ...raw, uri: "spotify:track:album2", name: "Album track two" },
          ] });
          if (url.pathname === "/v1/me/player/devices") return Response.json({ devices: [
            { id: "horizon", name: "Horizon" }, { id: "web-zunify", name: "Zunify" },
          ] });
          if (largeLibrary && url.pathname === "/v1/me/player/next") {
            window.mockPlaybackState = {
              track_window: { current_track: saved[2].track }, paused: false, position: 0,
            };
          }
          if (url.pathname === "/v1/me/player/pause") {
            if (largeLibrary) {
              window.mockSpotifyPlayer.listeners.player_state_changed({
                track_window: { current_track: raw }, paused: true, position: 1200,
              });
            }
            return new Response("spotifyPlaybackAcknowledgement", {
              headers: { "Content-Type": "text/plain" },
            });
          }
          return new Response(null, { status: 204 });
        };
      }, { raw, largeLibrary: mode === "shared" });
      await boot(page);
      if (mode === "shared") {
        const connect = button(page, "Connect Spotify");
        await expect(connect).toBeAttached();
        await connect.dispatchEvent("click");
      } else {
        await press(page, "Connect Spotify");
        await page.getByLabel("Client ID").fill(clientId);
        await button(page, "Sign in with Spotify").click();
      }
      await expect(button(page, "Account")).toBeAttached({ timeout: 45_000 });
      await expect.poll(() => page.evaluate(() => window.spotifyRequests.map(r => r.path))).toContain("/v1/me/tracks");
      if (mode === "shared") await press(page, "Your library");
      await expect(button(page, "Play Test Song")).toBeAttached();
      await page.mouse.move(720, 450);
      const dark = await fire(page);
      await press(page, "Play Test Song");
      const request = path => page.evaluate(path => window.spotifyRequests.find(r => r.path === path), path);
      const initialUris = ["spotify:track:test"];
      if (mode === "shared") {
        initialUris.push(...Array.from({ length: 76 }, (_, index) => "spotify:track:library" + (index + 1)));
      }
      await expect.poll(() => request("/v1/me/player/play")).toMatchObject({
        params: { device_id: "web-zunify" }, body: { uris: initialUris },
        authorization: "Bearer test-token",
      });
      expect(await page.evaluate(() => window.mockSpotifyPlayer.name)).toBe("Zunify");
      await expect(button(page, "Refresh devices")).toHaveCount(0);
      await press(page, "Pause");
      await expect.poll(() => request("/v1/me/player/pause")).toMatchObject({
        method: "PUT", params: { device_id: "web-zunify" },
      });
      await expect(button(page, "Play")).toBeAttached();
      await expect(page.locator("#announcement")).toHaveText("");
      await press(page, "Play");
      await expect.poll(() => page.evaluate(() => window.spotifyRequests
        .filter(r => r.path === "/v1/me/player/play").map(r => r.body))).toEqual([
        { uris: initialUris }, null,
      ]);
      await expect(button(page, "Pause")).toBeAttached();
      await press(page, "Like current track");
      await expect.poll(() => request("/v1/me/library")).toMatchObject({
        method: "DELETE", params: { uris: "spotify:track:test" },
      });
      if (mode === "manual") {
        // The page cannot hear Spotify's audio, so the fire settles into its own glow.
        await page.mouse.move(720, 450);
        await expect.poll(lit(page, dark)).toBe(true);
        // Sharing the tab's audio lets the spectrum follow the music instead.
        await press(page, "Account");
        await page.locator("#spectrum-toggle").click();
        await expect(page.locator("#settings-status")).toHaveText(/spectrum now follows/);
        await expect(page.locator("#spectrum-toggle")).toHaveText("Stop sharing tab audio");
        await page.locator("#settings .close").click();
        await page.locator("#spectrum-toggle").evaluate(button => button.click());
        await expect(page.locator("#spectrum-toggle")).toHaveText("Show the spectrum for Spotify");
        await press(page, "Account");
        await button(page, "Play on this device").click();
        await expect(page.locator("#settings")).not.toBeVisible();
        await expect(button(page, "Refresh devices")).toHaveCount(0);
        await press(page, "Select playback device");
        await expect(button(page, "Zunify")).toHaveCount(1);
        await expect(button(page, "Horizon")).toBeAttached();
        await press(page, "Your library");
        await press(page, "Albums");
        await press(page, "Add Test Album to queue");
        await expect.poll(() => page.evaluate(() => window.spotifyRequests
          .filter(r => r.method === "POST" && r.path === "/v1/me/player/queue")
          .map(r => r.params))).toEqual([
          { device_id: "web-zunify", uri: "spotify:track:album1" },
          { device_id: "web-zunify", uri: "spotify:track:album2" },
        ]);
        await press(page, "Play Test Album");
        await expect.poll(() => page.evaluate(() => window.spotifyRequests
          .find(r => r.body?.context_uri === "spotify:album:test")?.path)).toBe("/v1/me/player/play");
        await press(page, "Your library");
        await press(page, "Artists");
        await press(page, "Add Catalog Artist to queue");
        await expect.poll(() => page.evaluate(() => window.spotifyRequests
          .filter(r => r.method === "POST" && r.path === "/v1/me/player/queue")
          .map(r => r.params.uri))).toEqual([
          "spotify:track:album1", "spotify:track:album2", "spotify:track:byartist",
        ]);
        await press(page, "Play Catalog Artist");
        await expect.poll(() => page.evaluate(() => window.spotifyRequests
          .find(r => r.body?.context_uri === "spotify:artist:test")?.path)).toBe("/v1/me/player/play");
      }
      if (mode === "shared") {
        const songs = page.getByRole("region", { name: "SONGS", exact: true });
        const songBar = songs.getByRole("scrollbar");
        const duration = songs.getByText("1:40", { exact: true }).first();
        await expect.poll(async () => {
          const [bar, time] = await Promise.all([songBar.boundingBox(), duration.boundingBox()]);
          return bar && time ? bar.x - (time.x + time.width) : 0;
        }).toBeGreaterThanOrEqual(6);

        const panes = [
          { x: 110, first: "Library artist 01", last: "Library artist 76" },
          { x: 500, first: "Library playlist 1", last: "Library playlist 48" },
          { x: 1150, first: "Play Library song 1", last: "Play Library song 76" },
        ];
        for (const { first, last } of panes) {
          await expect(button(page, first)).toBeAttached();
          await expect(button(page, last)).toHaveCount(0);
        }
        for (const [index, { x, last }] of panes.entries()) {
          if (index === 2) {
            const thumb = songBar.locator('[data-ferroui-name="VerticalThumb"]');
            await expect(thumb).toBeAttached();
            const [track, handle] = await Promise.all([songBar.boundingBox(), thumb.boundingBox()]);
            const centerX = handle.x + handle.width / 2;
            await page.mouse.move(centerX, handle.y + handle.height / 2);
            await page.mouse.down();
            await page.mouse.move(centerX, track.y + track.height - 1, { steps: 8 });
            await page.mouse.up();
          } else {
            await page.mouse.move(x, 450);
            await page.mouse.wheel(0, 6500);
          }
          await expect(button(page, last)).toBeAttached();
          for (const [otherIndex, other] of panes.entries()) {
            if (otherIndex > index) {
              await expect(button(page, other.first)).toBeAttached();
              await expect(button(page, other.last)).toHaveCount(0);
            } else if (otherIndex < index) {
              await expect(button(page, other.last)).toBeAttached();
            }
          }
        }
        for (const heading of ["ARTISTS", "PLAYLISTS", "SONGS"]) {
          await expect(page.getByText(heading, { exact: true })).toBeAttached();
        }

        await press(page, "Library artist 76");
        await expect(button(page, "Play Library song 76")).toBeAttached();
        await expect(button(page, "Play Library song 75")).toHaveCount(0);
        await expect(button(page, "Library artist 76")).toBeAttached();
        await expect(button(page, "Library playlist 48")).toBeAttached();

        await page.setViewportSize({ width: 390, height: 780 });
        await expect(button(page, "Show Playlists")).toBeAttached();
        await expect(button(page, "Library playlist 1")).toBeAttached();
        await expect(button(page, "Play Library song 76")).toHaveCount(0);
        await page.mouse.move(150, 430);
        await page.mouse.wheel(0, 6500);
        await expect(button(page, "Library playlist 48")).toBeAttached();
        await press(page, "Show Songs");
        await expect(button(page, "Play Library song 76")).toBeAttached();
        await expect(button(page, "Library playlist 48")).toHaveCount(0);
        await press(page, "Show Playlists");
        await expect(button(page, "Library playlist 48")).toBeAttached();

        await page.evaluate(() => {
          for (const position of [5, 16, 37]) window.spotifyFerroAction("seek", String(position));
        });
        await expect.poll(() => page.evaluate(() => window.mockSeekCalls)).toEqual([37000]);

        await page.evaluate(() => {
          window.mockSpotifyPlayer.listeners.player_state_changed({
            track_window: { current_track: {
              type: "track", uri: "spotify:track:library1", name: "Library song 1",
            } }, paused: false, position: 0, duration: 100000,
          });
        });
        await expect(button(page, "Open Now Playing")).toContainText("Library song 1");
        await page.evaluate(() => window.spotifyFerroAction("seek", "18"));
        await expect.poll(() => page.evaluate(() => window.mockSeekCalls)).toEqual([37000, 18000]);
        await press(page, "Next track");
        // The SDK does not emit an event for this command. Its state query still refreshes the footer.
        await expect(button(page, "Open Now Playing")).toContainText("Library song 2");
      }
      await press(page, mode === "shared" ? "Spotify settings" : "Account");
      await button(page, "Sign out").click();
      await expect(button(page, "Play mix")).toBeAttached();
    });
  }
});
