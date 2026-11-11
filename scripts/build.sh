#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if ! command -v emcc >/dev/null && [ -f .tools/env.sh ]; then
  source .tools/env.sh
fi
command -v emcc >/dev/null && command -v wasm-bindgen >/dev/null || {
  echo 'Run vendor/FerroUI/scripts/browser/setup.sh "$PWD/.tools" first.' >&2; exit 1;
}
export EMCC_CFLAGS="${EMCC_CFLAGS:-} -s ERROR_ON_UNDEFINED_SYMBOLS=0"
cargo build --locked --target wasm32-unknown-emscripten --profile browser
browser=vendor/FerroUI/src/Browser/FerroUI.Browser
(cd "$browser/webapp" && npm ci --ignore-scripts && npm run build)
out="${CARGO_TARGET_DIR:-target}/wasm32-unknown-emscripten/browser"
rm -rf dist/snippets
mkdir -p dist
cp -R wwwroot/. dist/
npm exec --offline -- esbuild wwwroot/local-media.js --bundle --platform=browser --format=esm --minify --outfile=dist/local-media.js
cp "$browser"/dist/{ferroui.js,storage.js,ferroui-sw.js} dist/
cp vendor/FerroUI/scripts/browser/threads/ferroui-loader.js dist/
cp "$out/spotify-ferro.js" "$out/spotify_ferro.wasm" dist/
cp -R "$out/deps/snippets" dist/
npm run configure:browser
touch dist/.nojekyll
