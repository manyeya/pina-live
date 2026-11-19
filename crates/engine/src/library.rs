//! The music library: the folders it watches, every track in them (its tags, analysis, cues,
//! loops, rating, plays), crates the DJ makes, and the history of what's been played. Kept as
//! one JSON file; waveforms beside it, a small file each.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    analyze::{self, Analysis, Tags, Wave},
    deck::HOT_CUES,
};

/// The audio files a library takes in.
pub const AUDIO: [&str; 10] = [
    "mp3", "wav", "wave", "aif", "aiff", "flac", "m4a", "mp4", "aac", "ogg",
];

/// Where Pina Live keeps its things: `PINA_LIVE_HOME` if it's set (tests use it), else the
/// system's place for an app's data.
pub fn data_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("PINA_LIVE_HOME") {
        return PathBuf::from(home);
    }
    #[cfg(target_os = "macos")]
    let base =
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"));
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(any(target_os = "macos", windows)))]
    let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share"));
    // No folders at all in a browser (and std's temp_dir panics there): a name that's never read.
    #[cfg(target_family = "wasm")]
    return base.unwrap_or_else(|| PathBuf::from("/")).join("Pina Live");
    #[cfg(not(target_family = "wasm"))]
    base.unwrap_or_else(std::env::temp_dir).join("Pina Live")
}

pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO.iter().any(|a| a.eq_ignore_ascii_case(e)))
}

/// A track's id: its path, hashed (FNV-1a).
pub fn id_of(path: &Path) -> u64 {
    path.to_string_lossy()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325, |h, b| {
            (h ^ b as u64).wrapping_mul(0x100_0000_01b3)
        })
}

pub fn now() -> u64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct HotCue {
    /// Seconds in.
    pub at: f64,
    #[serde(default)]
    pub name: String,
    pub color: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SavedLoop {
    pub start: f64,
    pub end: f64,
    #[serde(default)]
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Track {
    pub id: u64,
    pub path: PathBuf,
    pub size: u64,
    pub modified: u64,
    pub tags: Tags,
    pub seconds: f32,
    pub added: u64,
    pub analysis: Option<Analysis>,
    /// Why it couldn't be read, if it couldn't.
    pub error: Option<String>,
    /// The cue point, seconds.
    pub cue: f64,
    pub hot: [Option<HotCue>; HOT_CUES],
    pub loops: Vec<SavedLoop>,
    /// 0 to 5 stars.
    pub rating: u8,
    pub color: Option<u32>,
    pub comment: String,
    pub plays: u32,
    pub last_played: Option<u64>,
    /// The grid's been set by hand: analysis leaves it alone.
    pub grid_locked: bool,
}

impl Track {
    pub fn new(path: &Path) -> Track {
        let meta = std::fs::metadata(path).ok();
        let stem = path
            .file_stem()
            .map_or(String::new(), |s| s.to_string_lossy().into_owned());
        // Till its tags are read: "Artist - Title" as the file's named.
        // ("02 - Song" is a track number, not an artist.)
        let (artist, title) = match stem.split_once(" - ") {
            Some((a, t)) if !a.trim().chars().all(|c| c.is_ascii_digit()) => {
                (a.trim().to_string(), t.trim().to_string())
            }
            Some((_, t)) => (String::new(), t.trim().to_string()),
            None => (String::new(), stem.clone()),
        };
        Track {
            id: id_of(path),
            path: path.to_path_buf(),
            size: meta.as_ref().map_or(0, |m| m.len()),
            modified: modified(meta.as_ref()),
            tags: Tags {
                title,
                artist,
                ..Tags::default()
            },
            added: now(),
            ..Track::default()
        }
    }

    pub fn title(&self) -> &str {
        &self.tags.title
    }

    pub fn bpm(&self) -> Option<f64> {
        self.analysis.as_ref()?.grid.map(|g| g.bpm)
    }

    pub fn camelot(&self) -> &str {
        self.analysis.as_ref().map_or("", |a| a.camelot.as_str())
    }

    /// Needs (another) look: never analysed, analysed by an older Pina Live, or changed since.
    pub fn needs_analysis(&self) -> bool {
        self.error.is_none()
            && self
                .analysis
                .as_ref()
                .is_none_or(|a| a.version != analyze::VERSION)
    }

    /// What analysis found, taken in (a grid set by hand kept).
    pub fn take(&mut self, mut a: Analysis, tags: Tags) {
        if self.grid_locked
            && let Some(old) = &self.analysis
        {
            a.grid = old.grid;
        }
        let keep = |new: String, old: &str| if new.is_empty() { old.to_string() } else { new };
        self.tags = Tags {
            title: keep(tags.title, &self.tags.title),
            artist: keep(tags.artist, &self.tags.artist),
            album: keep(tags.album, &self.tags.album),
            genre: keep(tags.genre, &self.tags.genre),
            year: keep(tags.year, &self.tags.year),
        };
        self.seconds = a.seconds;
        self.analysis = Some(a);
        self.error = None;
    }

    pub fn matches(&self, q: &Query) -> bool {
        if let Some((lo, hi)) = q.bpm {
            match self.bpm() {
                Some(b) if (lo..=hi).contains(&b) => {}
                _ => return false,
            }
        }
        if let Some(k) = &q.key {
            let mine = self.camelot();
            let ok = match q.key_near {
                true => analyze::compatible(mine, k),
                false => mine.eq_ignore_ascii_case(k),
            };
            if !ok {
                return false;
            }
        }
        if self.rating < q.rating {
            return false;
        }
        let fields = [
            &self.tags.title,
            &self.tags.artist,
            &self.tags.album,
            &self.tags.genre,
            &self.comment,
        ];
        q.words.iter().all(|w| {
            fields.iter().any(|f| contains_folded(f, w))
                || self
                    .path
                    .file_name()
                    .is_some_and(|n| contains_folded(&n.to_string_lossy(), w))
        })
    }
}

fn modified(meta: Option<&std::fs::Metadata>) -> u64 {
    meta.and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

/// `hay` has `needle` (already lower case) in it, case aside.
fn contains_folded(hay: &str, needle: &str) -> bool {
    if hay.is_ascii() && needle.is_ascii() {
        let (h, n) = (hay.as_bytes(), needle.as_bytes());
        return n.is_empty() || h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n));
    }
    hay.to_lowercase().contains(needle)
}

/// A search: words each found somewhere in the track, and `bpm:120-128` (or `bpm:124`, within
/// a BPM), `key:8A` (or `key:~8A`, anything that mixes with it), `rating:4`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    pub words: Vec<String>,
    pub bpm: Option<(f64, f64)>,
    pub key: Option<String>,
    pub key_near: bool,
    pub rating: u8,
}

impl Query {
    pub fn parse(s: &str) -> Query {
        let mut q = Query::default();
        for part in s.split_whitespace() {
            let lower = part.to_lowercase();
            if let Some(v) = lower.strip_prefix("bpm:") {
                let range = match v.split_once('-') {
                    Some((a, b)) => a.parse().ok().zip(b.parse().ok()),
                    None => v.parse::<f64>().ok().map(|b| (b - 1.0, b + 1.0)),
                };
                if let Some(r) = range {
                    q.bpm = Some(r);
                    continue;
                }
            } else if let Some(v) = lower.strip_prefix("key:") {
                q.key_near = v.starts_with('~');
                q.key = Some(v.trim_start_matches('~').to_uppercase());
                continue;
            } else if let Some(v) = lower.strip_prefix("rating:") {
                if let Ok(r) = v.trim_end_matches('+').parse() {
                    q.rating = r;
                    continue;
                }
            }
            q.words.push(lower);
        }
        q
    }

    pub fn is_empty(&self) -> bool {
        *self == Query::default()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Crate {
    pub name: String,
    pub tracks: Vec<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Played {
    pub track: u64,
    /// When it started playing out, seconds since 1970.
    pub at: u64,
    pub deck: u8,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Library {
    pub folders: Vec<PathBuf>,
    /// Files in the folders that aren't music (a video with no sound): left out of every scan.
    pub ignored: Vec<PathBuf>,
    pub tracks: Vec<Track>,
    pub crates: Vec<Crate>,
    pub history: Vec<Played>,
}

impl Library {
    pub fn file() -> PathBuf {
        data_dir().join("library.json")
    }

    /// The library kept on this computer (an empty one the first time).
    pub fn load() -> Library {
        std::fs::read(Library::file())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Kept: written beside and moved over, so a crash mid-write never loses it.
    pub fn save(&self) -> Result<(), String> {
        let file = Library::file();
        let dir = file.parent().ok_or("no folder")?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let tmp = file.with_extension("json.tmp");
        let bytes = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &file).map_err(|e| e.to_string())
    }

    pub fn get(&self, id: u64) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    /// A folder taken in (if it's new), and what's in it.
    pub fn add_folder(&mut self, dir: &Path) -> Vec<u64> {
        if !self.folders.iter().any(|f| f == dir) {
            self.folders.push(dir.to_path_buf());
        }
        self.scan()
    }

    /// The audio files in the folders not yet in the library, taken in; the files changed since
    /// they were analysed, marked to be looked at again. Their ids.
    pub fn scan(&mut self) -> Vec<u64> {
        let known: HashSet<u64> = self.tracks.iter().map(|t| t.id).collect();
        let mut found = Vec::new();
        for dir in self.folders.clone() {
            walk(&dir, &mut found);
        }
        let ignored: HashSet<&PathBuf> = self.ignored.iter().collect();
        found.retain(|p| !ignored.contains(p));
        let mut new = Vec::new();
        for path in found {
            let id = id_of(&path);
            if !known.contains(&id) {
                self.tracks.push(Track::new(&path));
                new.push(id);
            } else if let Some(t) = self.get_mut(id) {
                let meta = std::fs::metadata(&path).ok();
                let m = modified(meta.as_ref());
                if m != t.modified {
                    (t.modified, t.analysis, t.error) = (m, None, None);
                    new.push(id);
                }
            }
        }
        new
    }

    /// A single file taken in (dropped on a deck from anywhere).
    pub fn add_file(&mut self, path: &Path) -> u64 {
        let id = id_of(path);
        if self.get(id).is_none() {
            self.tracks.push(Track::new(path));
        }
        id
    }

    /// A track that isn't music taken out, and left out of every scan after.
    pub fn ignore(&mut self, id: u64) {
        if let Some(i) = self.tracks.iter().position(|t| t.id == id) {
            let t = self.tracks.remove(i);
            self.ignored.push(t.path);
            for c in &mut self.crates {
                c.tracks.retain(|x| *x != id);
            }
        }
    }

    pub fn played(&mut self, id: u64, deck: u8) {
        let at = now();
        if let Some(t) = self.get_mut(id) {
            t.plays += 1;
            t.last_played = Some(at);
        }
        self.history.push(Played {
            track: id,
            at,
            deck,
        });
    }

    /// The indices of the tracks that match, in the library's order.
    pub fn search(&self, q: &Query) -> Vec<usize> {
        (0..self.tracks.len())
            .filter(|&i| self.tracks[i].matches(q))
            .collect()
    }
}

/// Audio files under `dir`, not into hidden folders.
fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if e.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&p, out),
            Ok(_) if is_audio(&p) => out.push(p),
            _ => {}
        }
    }
}

/// Where a track's waveform is kept.
pub fn wave_file(id: u64) -> PathBuf {
    data_dir().join("waves").join(format!("{id:016x}.plw"))
}

pub fn save_wave(id: u64, w: &Wave) -> Result<(), String> {
    let f = wave_file(id);
    // A browser has no folders: kept in memory, as its tracks are.
    if cfg!(target_family = "wasm") {
        crate::memfs::put(&f, w.to_bytes());
        return Ok(());
    }
    if let Some(d) = f.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(f, w.to_bytes()).map_err(|e| e.to_string())
}

pub fn load_wave(id: u64) -> Option<Wave> {
    let f = wave_file(id);
    if let Some(bytes) = crate::memfs::get(&f) {
        return Wave::from_bytes(&bytes);
    }
    Wave::from_bytes(&std::fs::read(f).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Grid;

    fn track(title: &str, bpm: f64, key: &str) -> Track {
        Track {
            tags: Tags {
                title: title.into(),
                artist: "Someone".into(),
                genre: "Amapiano".into(),
                ..Tags::default()
            },
            analysis: Some(Analysis {
                version: analyze::VERSION,
                seconds: 300.0,
                grid: Some(Grid::new(bpm, 0.0)),
                steady: 1.0,
                key: String::new(),
                camelot: key.into(),
                lufs: -9.0,
                peak_db: 0.0,
                phrases: vec![],
                genre: None,
            }),
            ..Track::default()
        }
    }

    #[test]
    fn queries() {
        let lib = Library {
            tracks: vec![
                track("Night Drive", 112.0, "8A"),
                track("Sunrise", 124.0, "9A"),
                track("Ndiyakuthanda", 126.0, "3B"),
            ],
            ..Library::default()
        };
        let s = |q: &str| lib.search(&Query::parse(q));
        assert_eq!(s(""), vec![0, 1, 2]);
        assert_eq!(s("night"), vec![0]);
        assert_eq!(s("AMAPIANO someone"), vec![0, 1, 2]);
        assert_eq!(s("bpm:120-130"), vec![1, 2]);
        assert_eq!(s("bpm:124"), vec![1]);
        assert_eq!(s("key:8a"), vec![0]);
        assert_eq!(s("key:~8A"), vec![0, 1]);
        assert_eq!(s("bpm:120-130 key:~8A"), vec![1]);
        assert_eq!(s("ndiya"), vec![2]);
    }

    /// A folder scanned: its audio taken in once, hidden folders and other files left out.
    #[test]
    fn scans_folders() {
        let dir = std::env::temp_dir().join(format!("pina-live-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        for f in [
            "a.mp3",
            "sub/Artist - Song.flac",
            ".hidden/c.wav",
            "notes.txt",
            "02 - Track.wav",
        ] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        let mut lib = Library::default();
        assert_eq!(lib.add_folder(&dir).len(), 3);
        assert_eq!(lib.scan().len(), 0);
        // One that turns out not to be music stays out.
        let a = lib.tracks.iter().find(|t| t.title() == "a").unwrap().id;
        lib.ignore(a);
        assert_eq!(lib.scan().len(), 0);
        assert!(lib.get(a).is_none());
        let song = lib.tracks.iter().find(|t| t.title() == "Song").unwrap();
        assert_eq!(song.tags.artist, "Artist");
        let numbered = lib.tracks.iter().find(|t| t.title() == "Track").unwrap();
        assert_eq!(numbered.tags.artist, "");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
