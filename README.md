# Zunify

A native desktop and browser port of [Spotifast](https://github.com/crmne/spotifast), styled after the [Zune 4 desktop player](https://blogs.windows.com/windowsexperience/2009/09/15/new-zune-software-update-for-zune-4-0-optimized-for-windows-7/). Quickplay brings together pins, new music, and listening history. Collection has artist, playlist, and song panes. Now Playing puts the current cover and queue over a tinted wall of album art.

FerroUI renders the interface with virtualized lists, pointer Reveal, compositor keyframes, and SkSL/WGSL effects. A full-width fire effect animates beneath the transport during playback. Sixty-four Fourier spectrum bands of the music that is playing shape the glow, with bass at the center and higher frequencies toward the edges. The app analyzes the audio live wherever it can hear it, and moves at random where it has no way to listen. See [Spectrum](#spectrum). The app opens to a demo library with original covers and six playable samples. You can try it without a Spotify account.

## Run the native desktop app

```sh
git submodule update --init --recursive
cargo run --locked
```

FerroUI opens a native window and the demo plays through the system audio output. On macOS, run `scripts/bundle-macos.sh` to build a local [Zunify.app](dist-desktop/Zunify.app) bundle that opens from Finder. On Linux, install the ALSA development headers required by the audio backend.

## Run in the browser

The [hosted browser build](https://s8ndinista.github.io/Zunify/) is published from the `gh-pages` branch. To publish a new build, replace that branch’s site files with the contents of `dist/`, including `.nojekyll`.

With Git, Node.js 22+, Python 3.10+, and Rustup installed:

```sh
git submodule update --init --recursive
vendor/FerroUI/scripts/browser/setup.sh "$PWD/.tools"
npm run build
npm run preview
```

Open [http://127.0.0.1:4173/](http://127.0.0.1:4173/). The setup script provisions the Emscripten, Rust, and wasm-bindgen versions required by the FerroUI pin. If you already have those versions, source that toolchain's environment before running the build. The local preview only binds to the loopback address. To host it for others, serve `dist/` over HTTPS with `.wasm` served as `application/wasm`. A subdirectory works; no server-side Spotify credentials or proxy are needed.

You can also install the browser build from Chrome or Edge’s app menu. It caches the demo for offline use.

## Configure Spotify for listeners

The app publisher registers one Spotify developer app. Listeners click **Connect Spotify**, sign in on Spotify, and approve access. They use their regular Spotify accounts.

1. In Spotify’s developer dashboard, create an app and enable **Web API**. Enable **Web Playback SDK** for browser playback. The registered app name appears on the consent screen. [Spotify app registration](https://developer.spotify.com/documentation/web-api/concepts/apps)
2. Add each redirect URL to the same Spotify app and use the same Client ID for both builds. The native callback is `http://127.0.0.1:4176/spotify/callback`. The local browser preview uses `http://127.0.0.1:4173/`. The hosted callback is `https://s8ndinista.github.io/Zunify/`. Register the exact URLs, including the repository path and trailing slash for the hosted build. [Spotify redirect URI rules](https://developer.spotify.com/documentation/web-api/concepts/redirect_uri)
3. The native build includes the shared public Client ID. Supply `SPOTIFY_CLIENT_ID` to use a different registration or configure the browser build. Zunify uses OAuth with PKCE and keeps tokens in the running session. Keep the Client Secret out of the build. [Spotify PKCE flow](https://developer.spotify.com/documentation/web-api/tutorials/code-pkce-flow)

```sh
export SPOTIFY_CLIENT_ID="YOUR_PUBLIC_CLIENT_ID"
npm run build
scripts/bundle-macos.sh
```

To configure an existing browser build, run `npm run configure:browser` with `SPOTIFY_CLIENT_ID` set, then serve the updated `dist/`. The native app also accepts the environment variable at launch. Set `SPOTIFY_CLIENT_ID` to an empty string at launch to use the native manual setup form.

Spotify Development Mode supports up to five allowlisted accounts on a standard app. The publisher adds testers in the dashboard. Public access requires Spotify’s approval for Extended Quota Mode. Spotify Premium is required for playback. [Spotify quota modes](https://developer.spotify.com/documentation/web-api/concepts/quota-modes) · [Spotify Web Playback SDK](https://developer.spotify.com/documentation/web-playback-sdk)

The native app opens the default browser for authorization and receives the callback locally. It controls playback through Spotify Connect. To listen on the same computer, open the official Spotify desktop app and select it under **Devices**. The browser build can play directly through the Web Playback SDK.

## Spectrum

The fire follows the music when the app can hear it. Spotify’s streams are protected, so only Spotify’s own players can decode them. For Spotify the app listens to audio that is already playing.

- **Demo.** The app plays the demo itself and analyzes the samples on their way to the output, ahead of the volume control.
- **Desktop, Spotify.** While you are signed in, the app records what the computer is playing and analyzes it. Play in the official Spotify app on the same computer. The recording stays in memory and is never stored or sent. macOS 14.6 or later asks once for permission to record system audio. Windows needs no permission. Linux needs a PulseAudio or PipeWire monitor source. The Windows and Linux paths are untested.
- **Browser, Spotify.** Open **Spotify settings** and choose **Show the spectrum for Spotify**, then share the tab with its audio when the browser asks. This needs a Chromium desktop browser, and the browser asks again on each visit.

When the music goes quiet, the flames fall away. When the app has no way to listen, the fire moves at random and keeps no time with the music. That covers a computer whose output cannot be recorded and a browser tab whose audio is not shared. Music on another Spotify Connect device is silence to the app, so the fire stays dark for it.

## Verify

```sh
npm ci
npx playwright install chromium
npm test
```

The tests run against the built `dist/`. They cover demo playback and its live spectrum, library and queue interaction, search, mobile layout, offline caching, and the Spotify sign-in and player request flow against mocked Spotify endpoints. Live Spotify playback requires the account and app configuration above. `cargo test` covers the analyzer, and `cargo test -- --ignored` plays a bundled sample through the audio output to check the desktop tap.

FerroUI is pinned as a Git submodule tracking `main`. See [NOTICE.md](NOTICE.md) for the upstream revisions.
