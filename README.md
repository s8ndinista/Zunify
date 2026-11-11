# Zunify

A native desktop and browser port of [Spotifast](https://github.com/crmne/spotifast), styled after the [Zune 4 desktop player](https://blogs.windows.com/windowsexperience/2009/09/15/new-zune-software-update-for-zune-4-0-optimized-for-windows-7/). Quickplay brings together liked songs, new music, and listening history. Collection has artist, playlist, and song panes. Now Playing puts the current cover and queue over a tinted wall of album art.

The browser and desktop share a Rust interface built with FerroUI’s fluent declarative API. It uses virtualized lists, pointer Reveal, compositor keyframes, and SkSL/WGSL effects. A full-width fire effect animates beneath the transport during playback. Sixty-four Fourier spectrum bands of the music that is playing shape the glow, with bass at the center and higher frequencies toward the edges. The app analyzes the audio live wherever it can hear it, and moves at random where it has no way to listen. See [Spectrum](#spectrum). The app opens to a demo library with original covers and six playable samples. You can try it without a Spotify account.

## Run the native desktop app

```sh
git submodule update --init --recursive
cargo run --locked
```

FerroUI opens a native window and the demo plays through the system audio output. On macOS, run `scripts/bundle-macos.sh` to build a size-optimized, stripped release of [Zunify.app](dist-desktop/Zunify.app) that opens from Finder. Pass `dev` to package a development build. On Linux, install the ALSA development headers required by the audio backend.

## Run in the browser

The [hosted browser build](https://s8ndinista.github.io/Zunify/) is published from the `gh-pages` branch. To publish a new build, replace that branch’s site files with the contents of `dist/`, including `.nojekyll`.

With Git, Node.js 22+, Python 3.10+, and Rustup installed:

```sh
git submodule update --init --recursive
vendor/FerroUI/scripts/browser/setup.sh "$PWD/.tools"
npm ci
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

The native app opens the default browser when you connect your Spotify library. The first time you press **Play** in a signed-in desktop session, Spotify may open a second authorization page for the built-in [librespot](https://github.com/librespot-org/librespot) player. Use the same Spotify account for both. Once you approve access, you can close the browser tab; Zunify plays through the computer’s audio output. The browser build uses Spotify’s Web Playback SDK. Open **Playback devices** to choose a different output.

Spotify may authorize the built-in player and still deny it the key needed to play audio. This is a [known librespot account compatibility issue](https://github.com/librespot-org/librespot/issues/1649). If Zunify reports that Spotify denied the audio key, choose another Spotify Connect output or use the browser build.

## Local music

Open **Collection → Local music → Choose music folder**, then select the folder itself. In the browser you can also drag the folder from Finder or your file manager onto the Local music page. Use drag and drop if the picker accepts only files. Zunify scans all subfolders and reads tags and embedded covers. If there is no embedded cover, it checks for common image names such as `cover.jpg` and `folder.png`. MP3, FLAC, WAV, Ogg Vorbis and M4A are supported by the desktop player. Browser playback depends on the browser’s codecs. If the browser asks to upload the folder, it is asking for permission to read the files; Zunify imports them locally. Choose the folder again in a new session.

Use **Find cover art** on an album without a cover to send its title and artist to MusicBrainz and look up a cover in the [Cover Art Archive](https://musicbrainz.org/doc/Cover_Art_Archive). The cover is kept in the current session. The selected audio files and their tags are read on your device, and Zunify leaves the files as they are.

## Spectrum

The fire follows the music when Zunify has access to the audio samples.

- **Demo.** The app plays the demo itself and analyzes the samples on their way to the output, ahead of the volume control.
- **Desktop, Spotify.** Zunify analyzes its own playback on the way to the audio output. Playback sent to another device uses the ambient animation.
- **Browser, Spotify.** Open **Spotify settings** and choose **Show the spectrum for Spotify**, then share the tab with its audio when the browser asks. This needs a Chromium desktop browser, and the browser asks again on each visit.

When the music goes quiet, the flames fall away. Playback on another Spotify Connect device or in a browser tab with unshared audio uses an ambient animation.

## Verify

```sh
npm ci
npx playwright install chromium
npm test
```

The tests run against the built `dist/`. They cover demo playback and its live spectrum, library and queue interaction, search, mobile layout, offline caching, and the Spotify sign-in and player request flow against mocked Spotify endpoints. Live Spotify playback requires the account and app configuration above. `cargo test` covers the analyzer, and `cargo test -- --ignored` plays a bundled sample through the audio output to check the desktop tap.

FerroUI is pinned as a Git submodule tracking `main`. See [NOTICE.md](NOTICE.md) for the upstream revisions.
