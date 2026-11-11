use crate::app::Item;
use lofty::{
    config::ParseOptions,
    file::{AudioFile, TaggedFileExt},
    picture::PictureType,
    probe::Probe,
    tag::{Accessor, ItemKey},
};
use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_COVER_BYTES: usize = 12 * 1024 * 1024;

#[derive(Clone)]
pub(crate) enum Cover {
    Embedded(PathBuf),
    Image(PathBuf),
    Downloaded(std::sync::Arc<[u8]>),
}

impl Cover {
    pub(crate) fn read(self) -> Option<Vec<u8>> {
        match self {
            Self::Downloaded(bytes) => Some(bytes.to_vec()),
            Self::Embedded(path) => {
                let tagged = Probe::open(path)
                    .ok()?
                    .options(ParseOptions::new().read_properties(false))
                    .read()
                    .ok()?;
                let picture = tagged
                    .tags()
                    .iter()
                    .flat_map(|tag| tag.pictures())
                    .filter(|picture| picture.data().len() <= MAX_COVER_BYTES)
                    .min_by_key(|picture| picture.pic_type() != PictureType::CoverFront)?;
                Some(picture.data().to_vec())
            }
            Self::Image(path) => {
                let mut bytes = Vec::new();
                fs::File::open(path)
                    .ok()?
                    .take((MAX_COVER_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)
                    .ok()?;
                (bytes.len() <= MAX_COVER_BYTES).then_some(bytes)
            }
        }
    }
}

pub(crate) fn find_cover(album: &Item) -> Result<Vec<u8>, String> {
    let name = |text: &str| text.replace('\\', "\\\\").replace('"', "\\\"");
    let query = format!(
        "releasegroup:\"{}\" AND artist:\"{}\"",
        name(&album.title),
        name(&album.artist)
    );
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(4)
        .timeout_global(Some(std::time::Duration::from_secs(8)))
        .user_agent("Zunify/0.1 (https://github.com/s8ndinista/Zunify)")
        .build()
        .into();
    let unavailable = "Cover art lookup is unavailable. Try again later.";
    let results: serde_json::Value = agent
        .get("https://musicbrainz.org/ws/2/release-group")
        .query("query", &query)
        .query("fmt", "json")
        .query("limit", "5")
        .call()
        .map_err(|_| unavailable)?
        .body_mut()
        .with_config()
        .limit(262_144)
        .read_json()
        .map_err(|_| unavailable)?;
    let equal = |left: &str, right: &str| left.trim().to_lowercase() == right.trim().to_lowercase();
    let id = results["release-groups"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|release| {
            release["title"]
                .as_str()
                .is_some_and(|title| equal(title, &album.title))
                && release["artist-credit"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|artist| {
                        artist["name"]
                            .as_str()
                            .is_some_and(|name| equal(name, &album.artist))
                    })
        })
        .and_then(|release| release["id"].as_str())
        .filter(|id| id.len() == 36 && id.bytes().all(|b| b == b'-' || b.is_ascii_hexdigit()))
        .ok_or("No matching album was found in MusicBrainz.")?;
    let mut response = agent
        .get(format!(
            "https://coverartarchive.org/release-group/{id}/front-250"
        ))
        .call()
        .map_err(|error| {
            if matches!(error, ureq::Error::StatusCode(404)) {
                "The Cover Art Archive has no cover for this album."
            } else {
                unavailable
            }
        })?;
    if !response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("image/"))
    {
        return Err("The archive did not return a cover image.".into());
    }
    response
        .body_mut()
        .with_config()
        .limit(4 * 1024 * 1024)
        .read_to_vec()
        .map_err(|_| unavailable.into())
}

#[derive(Default)]
pub(crate) struct Library {
    pub(crate) tracks: Vec<Item>,
    pub(crate) albums: Vec<Item>,
    pub(crate) album_tracks: HashMap<String, Vec<Item>>,
    pub(crate) files: HashMap<String, PathBuf>,
    pub(crate) covers: HashMap<String, Cover>,
    pub(crate) unreadable: usize,
}

struct Track {
    item: Item,
    path: PathBuf,
    group: (String, String, PathBuf),
    album_artist: String,
    disc: u32,
    number: u32,
    cover: Option<Cover>,
}

fn track(path: PathBuf, id: String, folder_covers: &HashMap<PathBuf, PathBuf>) -> Track {
    let tagged = Probe::open(&path).ok().and_then(|probe| probe.read().ok());
    let tags = tagged
        .as_ref()
        .into_iter()
        .flat_map(|file| file.primary_tag().into_iter().chain(file.tags()));
    let title = tags
        .clone()
        .find_map(|tag| tag.title().filter(|s| !s.trim().is_empty()))
        .map(|s| s.into_owned())
        .unwrap_or_else(|| {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
    let artist = tags
        .clone()
        .find_map(|tag| tag.artist().filter(|s| !s.trim().is_empty()))
        .map(|s| s.into_owned())
        .unwrap_or_default();
    let album = tags
        .clone()
        .find_map(|tag| tag.album().filter(|s| !s.trim().is_empty()))
        .map(|s| s.into_owned())
        .unwrap_or_default();
    let album_artist = tags
        .clone()
        .find_map(|tag| {
            tag.get_string(ItemKey::AlbumArtist)
                .filter(|s| !s.trim().is_empty())
        })
        .unwrap_or_default()
        .to_owned();
    let disc = tags.clone().find_map(|tag| tag.disk()).unwrap_or(0);
    let number = tags.clone().find_map(|tag| tag.track()).unwrap_or(0);
    let embedded = tags
        .flat_map(|tag| tag.pictures())
        .any(|picture| picture.data().len() <= MAX_COVER_BYTES);
    let parent = path.parent().unwrap_or(Path::new(""));
    let group = (
        album.trim().to_lowercase(),
        album_artist.trim().to_lowercase(),
        if album_artist.is_empty() {
            parent.to_owned()
        } else {
            PathBuf::new()
        },
    );
    let cover = if embedded {
        Some(Cover::Embedded(path.clone()))
    } else {
        folder_covers.get(parent).cloned().map(Cover::Image)
    };
    Track {
        item: Item {
            id,
            title,
            artist,
            album,
            kind: "track".into(),
            duration: tagged
                .as_ref()
                .map_or(0.0, |file| file.properties().duration().as_secs_f64()),
            ..Item::default()
        },
        path,
        group,
        album_artist,
        disc,
        number,
        cover,
    }
}

pub(crate) fn scan(root: &Path, version: u64) -> Result<Library, String> {
    let mut folders = vec![root.to_owned()];
    let mut paths = Vec::new();
    let mut folder_covers = HashMap::new();
    let mut library = Library::default();
    while let Some(folder) = folders.pop() {
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(_) if folder == root => return Err("Could not read this music folder.".into()),
            Err(_) => {
                library.unreadable += 1;
                continue;
            }
        };
        let mut cover = None;
        for entry in entries {
            let Ok(entry) = entry else {
                library.unreadable += 1;
                continue;
            };
            let Ok(kind) = entry.file_type() else {
                library.unreadable += 1;
                continue;
            };
            if kind.is_dir() {
                folders.push(entry.path());
            } else if kind.is_file() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if ["mp3", "flac", "wav", "ogg", "oga", "m4a", "mp4"]
                    .contains(&name.rsplit('.').next().unwrap_or_default())
                {
                    paths.push(path);
                } else if let Some(rank) = [
                    "cover.jpg",
                    "cover.jpeg",
                    "cover.png",
                    "folder.jpg",
                    "folder.jpeg",
                    "folder.png",
                    "front.jpg",
                    "front.jpeg",
                    "front.png",
                ]
                .iter()
                .position(|value| *value == name)
                    && cover.as_ref().is_none_or(|(best, _)| rank < *best)
                {
                    cover = Some((rank, path));
                }
            }
        }
        if let Some((_, cover)) = cover {
            folder_covers.insert(folder, cover);
        }
    }

    paths.sort();
    let mut tracks: Vec<_> = paths
        .into_iter()
        .enumerate()
        .map(|(index, path)| {
            track(
                path,
                format!("local:track:{version}:{index}"),
                &folder_covers,
            )
        })
        .collect();
    tracks.sort_by(|a, b| {
        (&a.group, a.disc, a.number == 0, a.number, &a.item.title).cmp(&(
            &b.group,
            b.disc,
            b.number == 0,
            b.number,
            &b.item.title,
        ))
    });

    for (index, group) in tracks.chunk_by_mut(|a, b| a.group == b.group).enumerate() {
        let art = format!("local-cover:{version}:{index}");
        let cover = group
            .iter()
            .filter_map(|track| track.cover.as_ref())
            .min_by_key(|cover| !matches!(cover, Cover::Embedded(_)))
            .cloned();
        if let Some(cover) = cover {
            library.covers.insert(art.clone(), cover);
            for track in group.iter_mut() {
                track.item.art.clone_from(&art);
            }
        }
        let first = &group[0];
        if !first.item.album.is_empty() {
            let album = Item {
                id: format!("local:album:{version}:{index}"),
                kind: "album".into(),
                title: first.item.album.clone(),
                art: first.item.art.clone(),
                artist: if first.album_artist.is_empty() {
                    group
                        .iter()
                        .all(|track| track.item.artist == first.item.artist)
                        .then(|| first.item.artist.clone())
                        .unwrap_or_default()
                } else {
                    first.album_artist.clone()
                },
                ..Item::default()
            };
            library.album_tracks.insert(
                album.id.clone(),
                group.iter().map(|track| track.item.clone()).collect(),
            );
            library.albums.push(album);
        }
    }
    for track in tracks {
        library.files.insert(track.item.id.clone(), track.path);
        library.tracks.push(track.item);
    }
    Ok(library)
}
