const response = await fetch(new URL("./demo-library.json", import.meta.url));
if (!response.ok) throw new Error("Could not load the demo library.");
const library = await response.json();
export const tracks = library.tracks;
const byId = new Map(tracks.map(track => [track.id, track]));
export const mixes = library.mixes.map(mix => ({
  ...mix,
  tracks: mix.tracks.map(id => byId.get(id)).filter(Boolean),
}));
