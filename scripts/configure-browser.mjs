import { writeFile } from "node:fs/promises";

const clientId = process.env.SPOTIFY_CLIENT_ID?.trim() ?? "";
if (clientId && !/^[a-f0-9]{32}$/i.test(clientId)) {
  throw new Error("SPOTIFY_CLIENT_ID must be the 32-character Client ID from your Spotify developer app.");
}

await writeFile(
  new URL("../dist/spotify-config.js", import.meta.url),
  `export const spotifyClientId = ${JSON.stringify(clientId)};\n`,
);
