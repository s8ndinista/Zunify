import { createHash } from "node:crypto";
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
const boot = async (page, path = "/") => {
  await page.goto(path, { waitUntil: "domcontentloaded" });
  await expect(page.locator("#splash")).toHaveCount(0, { timeout: 45_000 });
  await expect(page.locator("#out canvas").first()).toBeVisible();
  await expect(page).toHaveTitle("SpotiFe · Your music");
};
test("plays a bundled mix and supports queueing, liking and mobile layout", async ({ page }) => {
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await boot(page);
  await expect(button(page, "Play mix")).toBeAttached();
  await page.mouse.click(320, 270);
  await expect.poll(() => page.locator("audio").evaluate(audio => audio.currentTime)).toBeGreaterThan(0.05);
  await press(page, "Pause");
  await expect.poll(() => page.locator("audio").evaluate(audio => audio.paused)).toBe(true);
  await expect(button(page, "Play")).toBeAttached();
  await page.mouse.click(737, 842);
  await expect.poll(() => page.locator("audio").evaluate(audio => audio.getAttribute("src"))).toMatch(/sample-5/);
  await press(page, "Like current track");
  await press(page, "Liked songs");
  await expect(button(page, "Play Low Orbit").first()).toBeAttached();
  await press(page, "Add to queue");
  await press(page, "Queue");
  await expect(button(page, "Play Midnight Signal").first()).toBeAttached();
  await page.setViewportSize({ width: 390, height: 780 });
  await expect(button(page, "Spotify settings")).toBeAttached({ timeout: 15_000 });
  await expect(button(page, "Your library")).toHaveCount(1);
  await press(page, "Your library");
  await expect(button(page, "Deep Focus").first()).toBeAttached();
  expect(errors).toEqual([]);
});

test("search accepts keyboard input and opens matching collections", async ({ page }) => {
  await boot(page);
  await page.mouse.click(714, 33);
  await page.keyboard.type("weightless");
  await expect(page.getByText("Results in the demo library")).toBeAttached();
  await expect(button(page, "Weightless")).toHaveCount(2);
  await expect(button(page, "Play Blue Hour").first()).toBeAttached();
  await press(page, "Weightless", 1);
  await expect(button(page, "Play all")).toBeAttached();
  await press(page, "Play all");
  await expect.poll(() => page.locator("audio").evaluate(audio => audio.getAttribute("src"))).toMatch(/sample-3/);
});

test("offline install caches the app and demo audio", async ({ page, context }) => {
  await boot(page);
  await page.evaluate(() => navigator.serviceWorker.ready);
  await expect.poll(() => page.evaluate(() => Boolean(navigator.serviceWorker.controller))).toBe(true);
  const manifest = await page.evaluate(() => fetch("./manifest.webmanifest").then(r => r.json()));
  expect(manifest.display).toBe("standalone");
  await context.setOffline(true);
  try {
    await boot(page);
    await expect(button(page, "Play mix")).toBeAttached();
    await page.mouse.click(320, 270);
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
      const sitePath = mode === "shared" ? "/Zunify/" : "/";
      const redirectUri = new URL(sitePath, baseURL).href;
      if (mode === "shared") {
        await page.route(`${redirectUri}**`, async route => {
          const requestUrl = new URL(route.request().url());
          requestUrl.pathname = requestUrl.pathname.slice("/Zunify".length);
          const response = await route.fetch({ url: requestUrl.href });
          await route.fulfill({ response });
        });
      }
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
          listeners={}; addListener(n,cb){this.listeners[n]=cb;}
          connect(){queueMicrotask(()=>this.listeners.ready({device_id:"web-spotife"}));return Promise.resolve(true);}
          disconnect(){} activateElement(){return Promise.resolve();}
        }}; window.onSpotifyWebPlaybackSDKReady();`,
      }));
      await page.addInitScript(raw => {
        const fetchReal = window.fetch.bind(window);
        window.spotifyRequests = [];
        window.fetch = async (input, options = {}) => {
          const url = new URL(input, location.href);
          if (url.origin !== "https://api.spotify.com") return fetchReal(input, options);
          window.spotifyRequests.push({
            path: url.pathname, params: Object.fromEntries(url.searchParams), method: options.method,
            authorization: new Headers(options.headers).get("authorization"),
            body: options.body ? JSON.parse(options.body) : null,
          });
          if (url.pathname === "/v1/me/tracks") return Response.json({ items: [{ track: raw }] });
          if (url.pathname === "/v1/me/playlists") return Response.json({ items: [] });
          return new Response(null, { status: 204 });
        };
      }, raw);
      await boot(page, sitePath);
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
      await expect(button(page, "Play Test Song")).toBeAttached();
      await press(page, "Play Test Song");
      const request = path => page.evaluate(path => window.spotifyRequests.find(r => r.path === path), path);
      await expect.poll(() => request("/v1/me/player/play")).toMatchObject({
        params: { device_id: "web-spotife" }, body: { uris: ["spotify:track:test"] },
        authorization: "Bearer test-token",
      });
      await press(page, "Like current track");
      await expect.poll(() => request("/v1/me/library")).toMatchObject({
        method: "DELETE", params: { uris: "spotify:track:test" },
      });
      await press(page, "Account");
      await button(page, "Sign out").click();
      await expect(button(page, "Play mix")).toBeAttached();
    });
  }
});
