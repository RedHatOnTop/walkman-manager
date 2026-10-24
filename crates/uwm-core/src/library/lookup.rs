//! Finds an album's details in public music databases, MusicBrainz and the iTunes
//! store, and works out what would change in each of its files.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Track;
use super::tags::{Cover, TagEdit};
use crate::error::{Error, ErrorReport, Result, Why};

const USER_AGENT: &str = concat!("UltimateWalkmanManager/", env!("CARGO_PKG_VERSION"), " ( local build )");
const MUSICBRAINZ: &str = "https://musicbrainz.org/ws/2";
/// MusicBrainz asks for at most one request a second from one client.
const MUSICBRAINZ_GAP: Duration = Duration::from_millis(1100);
const RESULTS: usize = 8;
/// Results scoring below this share neither the title nor the artist with the query.
const MIN_SCORE: u8 = 20;
/// Largest cover image taken from a database.
const MAX_IMAGE: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    MusicBrainz,
    Itunes,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: String,
    pub source: Source,
    pub title: String,
    pub artist: String,
    pub year: Option<u16>,
    pub date: Option<String>,
    pub country: Option<String>,
    pub label: Option<String>,
    /// `CD`, `Digital Media`, `Single`, `EP` and the like.
    pub format: Option<String>,
    pub track_count: u32,
    /// How well it fits the album asked about, 0–100.
    pub score: u8,
    #[serde(skip)]
    pub thumb_url: Option<String>,
    #[serde(skip)]
    pub cover_url: Option<String>,
    #[serde(skip)]
    store: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseTrack {
    pub disc: u32,
    pub number: u32,
    pub title: String,
    pub artist: String,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Release {
    pub candidate: Candidate,
    pub genre: Option<String>,
    pub tracks: Vec<ReleaseTrack>,
}

impl Release {
    fn disc_count(&self) -> u32 {
        self.tracks.iter().map(|t| t.disc).max().unwrap_or(1)
    }

    fn disc_tracks(&self, disc: u32) -> u32 {
        self.tracks.iter().filter(|t| t.disc == disc).count() as u32
    }
}

/// What the user typed to look an album up.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Query {
    pub album: String,
    #[serde(default)]
    pub artist: Option<String>,
    /// Tracks the album has here, to rank releases of the same length first.
    #[serde(default)]
    pub tracks: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub candidates: Vec<Candidate>,
    /// A source that could not be reached, with its error, when the other answered.
    pub failed: Vec<(Source, ErrorReport)>,
}

pub struct Client {
    agent: ureq::Agent,
    last_musicbrainz: Mutex<Option<Instant>>,
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

fn network(e: ureq::Error) -> Error {
    Error::Network(match e {
        ureq::Error::StatusCode(503) => "the service is busy (503); try again in a moment".to_owned(),
        ureq::Error::StatusCode(code) => format!("the service answered {code}"),
        ureq::Error::HostNotFound => "no connection to the internet (host not found)".to_owned(),
        other => other.to_string(),
    })
}

impl Client {
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .user_agent(USER_AGENT)
            .https_only(true)
            .build();
        Client {
            agent: ureq::Agent::new_with_config(config),
            last_musicbrainz: Mutex::new(None),
        }
    }

    fn musicbrainz(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        // MusicBrainz answers 503 to a client over its rate, which a pause clears.
        let mut tries = 0;
        loop {
            tries += 1;
            {
                let mut last = self.last_musicbrainz.lock().unwrap();
                if let Some(t) = *last {
                    let since = t.elapsed();
                    if since < MUSICBRAINZ_GAP {
                        std::thread::sleep(MUSICBRAINZ_GAP - since);
                    }
                }
                *last = Some(Instant::now());
            }
            let mut req = self
                .agent
                .get(format!("{MUSICBRAINZ}{path}"))
                .header("Accept", "application/json")
                .query("fmt", "json");
            for (k, v) in query {
                req = req.query(*k, *v);
            }
            match req.call() {
                Ok(mut resp) => return resp.body_mut().read_json::<Value>().map_err(network),
                Err(ureq::Error::StatusCode(503)) if tries < 3 => {
                    std::thread::sleep(MUSICBRAINZ_GAP * tries);
                }
                Err(e) => return Err(network(e)),
            }
        }
    }

    fn itunes(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        let mut req = self.agent.get(format!("https://itunes.apple.com{path}"));
        for (k, v) in query {
            req = req.query(*k, *v);
        }
        let mut resp = req.call().map_err(network)?;
        resp.body_mut()
            .with_config()
            .limit(4 * 1024 * 1024)
            .read_json::<Value>()
            .map_err(network)
    }

    /// Asks both databases at once. Fails only when neither answers.
    pub fn search(&self, q: &Query) -> Result<SearchResult> {
        let album = q.album.trim();
        if album.is_empty() {
            return Err(Error::unsupported(
                Why::EmptyQuery,
                "type the album's name to search for",
            ));
        }
        let artist = q.artist.as_deref().map(str::trim).filter(|a| !a.is_empty());
        let store = storefront(album, artist);
        let limit = RESULTS.to_string();
        let mb_search = |artist: Option<&str>| {
            let lucene = musicbrainz_query(album, artist);
            self.musicbrainz("/release", &[("query", &lucene), ("limit", &limit)])
                .map(|v| parse_musicbrainz_search(&v))
        };
        let it_search = |artist: Option<&str>| {
            let term = match artist {
                Some(a) => format!("{a} {album}"),
                None => album.to_owned(),
            };
            self.itunes(
                "/search",
                &[
                    ("term", &term),
                    ("media", "music"),
                    ("entity", "album"),
                    ("limit", &limit),
                    ("country", store),
                ],
            )
            .map(|v| parse_itunes_search(&v, store))
        };
        // A soundtrack or compilation is credited to "Various Artists", not to the artist
        // of the track at hand, so a search that names the artist and finds little is
        // made again without it.
        let (mb, it) = std::thread::scope(|s| {
            let mb = s.spawn(|| {
                let mut found = mb_search(artist)?;
                if artist.is_some() && found.len() < 3 {
                    for c in mb_search(None)? {
                        if !found.iter().any(|f| f.id == c.id) {
                            found.push(c);
                        }
                    }
                }
                Ok::<_, Error>(found)
            });
            let it = s.spawn(|| match it_search(artist) {
                Ok(found) if found.is_empty() && artist.is_some() => it_search(None),
                other => other,
            });
            (mb.join().unwrap(), it.join().unwrap())
        });
        let mut candidates = Vec::new();
        let mut failed = Vec::new();
        for (source, result) in [(Source::MusicBrainz, mb), (Source::Itunes, it)] {
            match result {
                Ok(found) => candidates.extend(found),
                Err(e) => failed.push((source, ErrorReport::from(&e))),
            }
        }
        if candidates.is_empty() && failed.len() == 2 {
            return Err(Error::Network(failed.remove(0).1.message));
        }
        for c in &mut candidates {
            c.score = score(c, album, artist, q.tracks);
        }
        candidates.retain(|c| c.score >= MIN_SCORE);
        candidates.sort_by(|a, b| b.score.cmp(&a.score).then(a.source.cmp_rank(&b.source)));
        Ok(SearchResult { candidates, failed })
    }

    pub fn release(&self, c: &Candidate) -> Result<Release> {
        match c.source {
            Source::MusicBrainz => {
                let id = c.id.strip_prefix("mb:").unwrap_or(&c.id);
                let v = self.musicbrainz(
                    &format!("/release/{id}"),
                    &[("inc", "recordings+artist-credits+genres+labels+release-groups")],
                )?;
                Ok(parse_musicbrainz_release(&v, c.clone()))
            }
            Source::Itunes => {
                let id = c.id.rsplit(':').next().unwrap_or(&c.id);
                let store = c.store.as_deref().unwrap_or("US");
                let v = self.itunes(
                    "/lookup",
                    &[("id", id), ("entity", "song"), ("country", store), ("limit", "200")],
                )?;
                Ok(parse_itunes_release(&v, c.clone()))
            }
        }
    }

    /// Downloads a cover image from one of the databases' image hosts.
    pub fn image(&self, url: &str) -> Result<Vec<u8>> {
        let host = url
            .strip_prefix("https://")
            .and_then(|r| r.split('/').next())
            .unwrap_or("");
        let allowed = host == "coverartarchive.org" || host.ends_with(".mzstatic.com");
        if !allowed {
            return Err(
                Error::unsupported(Why::NotCoverAddress, format!("not a cover image address: {url}"))
                    .with_detail(url.to_string()),
            );
        }
        // archive.org, where Cover Art Archive images live, answers 5xx now and then.
        let mut tries = 0;
        loop {
            tries += 1;
            match self.agent.get(url).call() {
                Ok(mut resp) => {
                    return resp
                        .body_mut()
                        .with_config()
                        .limit(MAX_IMAGE)
                        .read_to_vec()
                        .map_err(network);
                }
                Err(ureq::Error::StatusCode(code)) if code >= 500 && tries < 3 => {
                    std::thread::sleep(Duration::from_millis(700 * tries));
                }
                Err(e) => return Err(network(e)),
            }
        }
    }
}

/// A downloaded cover as something to embed: a JPEG or PNG whose header reads, with
/// its pixel size.
pub fn cover_from(bytes: Vec<u8>) -> Option<(Cover, u32, u32)> {
    let mime = if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else {
        return None;
    };
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    Some((Cover { data: bytes, mime }, w, h))
}

impl Source {
    fn cmp_rank(&self, other: &Source) -> std::cmp::Ordering {
        let rank = |s: &Source| match s {
            Source::MusicBrainz => 0,
            Source::Itunes => 1,
        };
        rank(self).cmp(&rank(other))
    }
}

fn has_kana(s: &str) -> bool {
    s.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c))
}

/// The iTunes store to search. The Korean store answers no searches through this API
/// (none for "IU" either, 2026-10-16); the US store finds Korean releases from Korean
/// text, under their English names.
fn storefront(album: &str, artist: Option<&str>) -> &'static str {
    if has_kana(album) || artist.is_some_and(has_kana) {
        "JP"
    } else {
        "US"
    }
}

fn lucene_phrase(s: &str) -> String {
    let escaped: String = s
        .chars()
        .flat_map(|c| match c {
            '"' | '\\' => vec!['\\', c],
            _ => vec![c],
        })
        .collect();
    format!("\"{escaped}\"")
}

fn musicbrainz_query(album: &str, artist: Option<&str>) -> String {
    match artist {
        Some(a) => format!("release:{} AND artist:{}", lucene_phrase(album), lucene_phrase(a)),
        None => format!("release:{}", lucene_phrase(album)),
    }
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)?.as_str().map(str::trim).filter(|s| !s.is_empty())
}

fn year_of(date: Option<&str>) -> Option<u16> {
    date?.get(..4)?.parse().ok().filter(|y| *y > 0)
}

fn credit(v: &Value) -> String {
    v.get("artist-credit")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .map(|p| {
                    format!(
                        "{}{}",
                        str_of(p, "name").unwrap_or(""),
                        p["joinphrase"].as_str().unwrap_or("")
                    )
                })
                .collect::<String>()
        })
        .unwrap_or_default()
}

pub fn parse_musicbrainz_search(v: &Value) -> Vec<Candidate> {
    let Some(releases) = v.get("releases").and_then(Value::as_array) else {
        return Vec::new();
    };
    releases
        .iter()
        .filter_map(|r| {
            let id = str_of(r, "id")?;
            let date = str_of(r, "date");
            let formats: Vec<&str> = r["media"]
                .as_array()
                .map(|m| m.iter().filter_map(|x| str_of(x, "format")).collect())
                .unwrap_or_default();
            let format = match formats.as_slice() {
                [] => None,
                [one] => Some((*one).to_owned()),
                [first, ..] if formats.iter().all(|f| f == first) => Some(format!("{}×{first}", formats.len())),
                _ => Some(formats.join(" + ")),
            };
            Some(Candidate {
                id: format!("mb:{id}"),
                source: Source::MusicBrainz,
                title: str_of(r, "title")?.to_owned(),
                artist: credit(r),
                year: year_of(date),
                date: date.map(str::to_owned),
                country: str_of(r, "country").map(str::to_owned),
                label: r["label-info"]
                    .as_array()
                    .and_then(|l| l.iter().find_map(|x| str_of(&x["label"], "name")))
                    .map(str::to_owned),
                format,
                track_count: r["track-count"].as_u64().unwrap_or(0) as u32,
                score: 0,
                thumb_url: Some(format!("https://coverartarchive.org/release/{id}/front-250")),
                cover_url: Some(format!("https://coverartarchive.org/release/{id}/front-500")),
                store: None,
            })
        })
        .collect()
}

fn most_counted_genre(v: &Value) -> Option<String> {
    let list = v.get("genres")?.as_array()?;
    list.iter()
        .filter_map(|g| Some((g["count"].as_u64().unwrap_or(0), str_of(g, "name")?)))
        .max_by_key(|(n, _)| *n)
        .map(|(_, name)| title_case(name))
}

fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().chain(c).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn parse_musicbrainz_release(v: &Value, candidate: Candidate) -> Release {
    let mut tracks = Vec::new();
    for (i, medium) in v["media"].as_array().into_iter().flatten().enumerate() {
        let disc = medium["position"].as_u64().map_or(i as u32 + 1, |p| p as u32);
        for (j, t) in medium["tracks"].as_array().into_iter().flatten().enumerate() {
            let number = t["position"].as_u64().map_or(j as u32 + 1, |p| p as u32);
            let title = str_of(t, "title")
                .or_else(|| str_of(&t["recording"], "title"))
                .unwrap_or_default();
            let mut artist = credit(t);
            if artist.is_empty() {
                artist = candidate.artist.clone();
            }
            tracks.push(ReleaseTrack {
                disc,
                number,
                title: title.to_owned(),
                artist,
                duration_ms: t["length"].as_u64().or_else(|| t["recording"]["length"].as_u64()),
            });
        }
    }
    let genre = most_counted_genre(v).or_else(|| most_counted_genre(&v["release-group"]));
    Release {
        candidate,
        genre,
        tracks,
    }
}

/// iTunes names singles and EPs `Title - Single`; the suffix is the format, not the title.
fn split_itunes_title(name: &str) -> (String, Option<String>) {
    for suffix in [" - Single", " - EP"] {
        if let Some(t) = name.strip_suffix(suffix) {
            return (t.to_owned(), Some(suffix[3..].to_owned()));
        }
    }
    (name.to_owned(), None)
}

fn artwork(url: &str, size: u32) -> String {
    url.replace("100x100bb", &format!("{size}x{size}bb"))
}

pub fn parse_itunes_search(v: &Value, store: &str) -> Vec<Candidate> {
    let Some(results) = v.get("results").and_then(Value::as_array) else {
        return Vec::new();
    };
    results
        .iter()
        .filter(|r| r["wrapperType"].as_str() == Some("collection"))
        .filter_map(|r| {
            let id = r["collectionId"].as_u64()?;
            let (title, format) = split_itunes_title(str_of(r, "collectionName")?);
            let date = str_of(r, "releaseDate").map(|d| d.get(..10).unwrap_or(d).to_owned());
            let art = str_of(r, "artworkUrl100");
            Some(Candidate {
                id: format!("itunes:{store}:{id}"),
                source: Source::Itunes,
                title,
                artist: str_of(r, "artistName").unwrap_or_default().to_owned(),
                year: year_of(date.as_deref()),
                date,
                country: Some(store.to_owned()),
                label: str_of(r, "copyright").map(|c| c.trim_start_matches(['℗', '©', ' ']).to_owned()),
                format,
                track_count: r["trackCount"].as_u64().unwrap_or(0) as u32,
                score: 0,
                thumb_url: art.map(|u| artwork(u, 250)),
                cover_url: art.map(|u| artwork(u, 600)),
                store: Some(store.to_owned()),
            })
        })
        .collect()
}

pub fn parse_itunes_release(v: &Value, candidate: Candidate) -> Release {
    let results = v["results"].as_array().map(Vec::as_slice).unwrap_or_default();
    let genre = results
        .iter()
        .find(|r| r["wrapperType"].as_str() == Some("collection"))
        .and_then(|r| str_of(r, "primaryGenreName"))
        .map(str::to_owned);
    let mut tracks: Vec<ReleaseTrack> = results
        .iter()
        .filter(|r| r["wrapperType"].as_str() == Some("track"))
        .filter_map(|r| {
            Some(ReleaseTrack {
                disc: r["discNumber"].as_u64().unwrap_or(1) as u32,
                number: r["trackNumber"].as_u64()? as u32,
                title: str_of(r, "trackName")?.to_owned(),
                artist: str_of(r, "artistName").unwrap_or(&candidate.artist).to_owned(),
                duration_ms: r["trackTimeMillis"].as_u64(),
            })
        })
        .collect();
    tracks.sort_by_key(|t| (t.disc, t.number));
    Release {
        candidate,
        genre,
        tracks,
    }
}

/// Lower case letters and digits only, so punctuation and spacing do not count.
fn norm(s: &str) -> Vec<char> {
    s.chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// Dice coefficient over character pairs, 0.0–1.0. Works the same for Hangul and kana,
/// which a word-based measure would not.
pub fn similarity(a: &str, b: &str) -> f32 {
    let (a, b) = (norm(a), norm(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    if a.len() < 2 || b.len() < 2 {
        return 0.0;
    }
    let mut pairs: HashMap<(char, char), i32> = HashMap::new();
    for w in a.windows(2) {
        *pairs.entry((w[0], w[1])).or_default() += 1;
    }
    let mut common = 0;
    for w in b.windows(2) {
        if let Some(n) = pairs.get_mut(&(w[0], w[1]))
            && *n > 0
        {
            *n -= 1;
            common += 1;
        }
    }
    2.0 * common as f32 / (a.len() + b.len() - 2) as f32
}

/// Drops `(feat. …)`, `[Remastered]` and the like, which one source has and another not.
fn core_title(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    if norm(&out).is_empty() { s.to_owned() } else { out }
}

fn title_similarity(a: &str, b: &str) -> f32 {
    similarity(a, b).max(similarity(&core_title(a), &core_title(b)))
}

fn score(c: &Candidate, album: &str, artist: Option<&str>, tracks: usize) -> u8 {
    let title = title_similarity(&c.title, album);
    let who = artist.map_or(0.6, |a| similarity(&c.artist, a));
    let count = match (tracks, c.track_count as usize) {
        (0, _) | (_, 0) => 0.5,
        (n, m) if n == m => 1.0,
        (n, m) if n < m => 0.6,
        _ => 0.0,
    };
    (title * 60.0 + who * 30.0 + count * 10.0).round().clamp(0.0, 100.0) as u8
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Field {
    Title,
    Artist,
    Album,
    AlbumArtist,
    Genre,
    Year,
    Track,
    Disc,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub field: Field,
    pub from: Option<String>,
    pub to: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackProposal {
    pub track_id: String,
    pub name: String,
    /// The release track this file was matched to.
    pub matched: Option<ReleaseTrack>,
    pub changes: Vec<Change>,
    /// Its tags are stored in a legacy encoding and will be written back as Unicode.
    pub repairs: bool,
    pub writable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub candidate: Candidate,
    pub album: Vec<Change>,
    pub tracks: Vec<TrackProposal>,
    /// Release tracks no file was matched to.
    pub unmatched: Vec<ReleaseTrack>,
}

/// Pairs each file with a release track: the same disc and number count for a lot,
/// a similar title and length for more. A pair has to clear a bar, so a single from a
/// compilation is not forced onto the album's first track.
pub fn match_tracks(files: &[&Track], release: &Release) -> Vec<Option<usize>> {
    let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
    for (i, f) in files.iter().enumerate() {
        for (j, r) in release.tracks.iter().enumerate() {
            let title = f.title.as_deref().map_or(0.0, |t| title_similarity(t, &r.title));
            let length = match r.duration_ms {
                Some(d) if f.duration_ms > 0 => {
                    let off = f.duration_ms.abs_diff(d) as f32 / 1000.0;
                    (1.0 - off / 8.0).max(0.0)
                }
                _ => 0.0,
            };
            let same_number = f.track == Some(r.number) && f.disc.unwrap_or(1) == r.disc;
            let s = title * 0.6 + length * 0.3 + if same_number { 0.25 } else { 0.0 };
            if s >= 0.5 {
                pairs.push((s, i, j));
            }
        }
    }
    pairs.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut out = vec![None; files.len()];
    let mut taken = vec![false; release.tracks.len()];
    for (_, i, j) in pairs {
        if out[i].is_none() && !taken[j] {
            out[i] = Some(j);
            taken[j] = true;
        }
    }
    out
}

fn most_common<'a>(values: impl Iterator<Item = Option<&'a str>>) -> Option<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for v in values.flatten() {
        *counts.entry(v).or_default() += 1;
    }
    counts.into_iter().max_by_key(|(_, n)| *n).map(|(v, _)| v.to_owned())
}

fn change(field: Field, from: Option<String>, to: Option<String>) -> Option<Change> {
    let to = to.filter(|t| !t.trim().is_empty())?;
    (from.as_deref() != Some(to.as_str())).then_some(Change { field, from, to })
}

/// What looking the album up as `release` would change. Album fields are offered when
/// any file differs, since they are written to every file.
pub fn propose(files: &[&Track], release: &Release, writable: impl Fn(&Track) -> bool) -> Proposal {
    let c = &release.candidate;
    let album_changes = [
        (
            Field::Album,
            files.iter().map(|t| t.album.as_deref()).collect::<Vec<_>>(),
            Some(c.title.clone()),
        ),
        (
            Field::AlbumArtist,
            files.iter().map(|t| t.album_artist.as_deref()).collect(),
            Some(c.artist.clone()),
        ),
        (
            Field::Genre,
            files.iter().map(|t| t.genre.as_deref()).collect(),
            release.genre.clone(),
        ),
    ];
    let mut album: Vec<Change> = album_changes
        .into_iter()
        .filter_map(|(field, now, to)| {
            let differs = to.as_ref().is_some_and(|t| now.iter().any(|v| *v != Some(t.as_str())));
            if !differs {
                return None;
            }
            Some(Change {
                field,
                from: most_common(now.into_iter()),
                to: to?,
            })
        })
        .collect();
    if let Some(y) = c.year
        && files.iter().any(|t| t.year != Some(y))
    {
        let from = most_common(
            files
                .iter()
                .map(|t| t.year.map(|v| v.to_string()))
                .collect::<Vec<_>>()
                .iter()
                .map(Option::as_deref),
        );
        album.push(Change {
            field: Field::Year,
            from,
            to: y.to_string(),
        });
    }

    let matches = match_tracks(files, release);
    let mut used = vec![false; release.tracks.len()];
    let tracks = files
        .iter()
        .zip(&matches)
        .map(|(t, m)| {
            let matched = m.map(|j| {
                used[j] = true;
                release.tracks[j].clone()
            });
            let changes = matched
                .as_ref()
                .map(|r| {
                    [
                        change(Field::Title, t.title.clone(), Some(r.title.clone())),
                        change(Field::Artist, t.artist.clone(), Some(r.artist.clone())),
                        change(Field::Track, t.track.map(|n| n.to_string()), Some(r.number.to_string())),
                        change(
                            Field::Disc,
                            t.disc.map(|n| n.to_string()),
                            (release.disc_count() > 1 || t.disc.is_some_and(|d| d != r.disc))
                                .then(|| r.disc.to_string()),
                        ),
                    ]
                    .into_iter()
                    .flatten()
                    .collect()
                })
                .unwrap_or_default();
            TrackProposal {
                track_id: t.id.clone(),
                name: t.rel.rsplit('/').next().unwrap_or(&t.rel).to_owned(),
                matched,
                changes,
                repairs: t.legacy_text,
                writable: writable(t),
            }
        })
        .collect();
    let unmatched = release
        .tracks
        .iter()
        .zip(&used)
        .filter(|(_, u)| !**u)
        .map(|(r, _)| r.clone())
        .collect();
    Proposal {
        candidate: c.clone(),
        album,
        tracks,
        unmatched,
    }
}

/// The edit for one file: the album fields chosen, and its own fields when it is one of
/// `tracks`.
pub fn edit_for(p: &Proposal, release: &Release, track: &TrackProposal, album: &[Field], with_track: bool) -> TagEdit {
    let mut edit = TagEdit::default();
    let mut set = |c: &Change| match c.field {
        Field::Title => edit.title = Some(c.to.clone()),
        Field::Artist => edit.artist = Some(c.to.clone()),
        Field::Album => edit.album = Some(c.to.clone()),
        Field::AlbumArtist => edit.album_artist = Some(c.to.clone()),
        Field::Genre => edit.genre = Some(c.to.clone()),
        Field::Year => edit.year = c.to.parse().ok(),
        Field::Track => edit.track = c.to.parse().ok(),
        Field::Disc => edit.disc = c.to.parse().ok(),
    };
    for c in p.album.iter().filter(|c| album.contains(&c.field)) {
        set(c);
    }
    if with_track {
        for c in &track.changes {
            set(c);
        }
        if let Some(r) = &track.matched
            && edit.track.is_some()
        {
            edit.track_total = Some(release.disc_tracks(r.disc));
            if release.disc_count() > 1 {
                edit.disc = Some(r.disc);
                edit.disc_total = Some(release.disc_count());
            }
        }
    }
    edit
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tests::track;
    use serde_json::json;

    fn mb_search() -> Value {
        json!({"releases": [{
            "id": "6b1c0b5a-0000-4000-8000-000000000001",
            "score": 100,
            "title": "Saint o'clock",
            "status": "Official",
            "date": "2010-01-28",
            "country": "KR",
            "track-count": 3,
            "artist-credit": [{"name": "2AM", "joinphrase": ""}],
            "label-info": [{"label": {"name": "JYP Entertainment"}}],
            "media": [{"format": "CD", "track-count": 3}]
        }]})
    }

    fn mb_release(c: Candidate) -> Release {
        let v = json!({
            "id": "6b1c0b5a-0000-4000-8000-000000000001",
            "genres": [{"name": "k-pop", "count": 3}, {"name": "ballad", "count": 1}],
            "media": [{"position": 1, "tracks": [
                {"position": 1, "title": "Intro", "length": 61000, "artist-credit": [{"name": "2AM", "joinphrase": ""}]},
                {"position": 2, "title": "죽어도 못 보내", "length": 236000, "artist-credit": [{"name": "2AM", "joinphrase": ""}]},
                {"position": 3, "title": "Like Tears", "length": 214000,
                 "artist-credit": [{"name": "2AM", "joinphrase": " feat. "}, {"name": "Yubin", "joinphrase": ""}]}
            ]}]
        });
        parse_musicbrainz_release(&v, c)
    }

    #[test]
    fn reads_musicbrainz() {
        let found = parse_musicbrainz_search(&mb_search());
        assert_eq!(found.len(), 1);
        let c = &found[0];
        assert_eq!(c.id, "mb:6b1c0b5a-0000-4000-8000-000000000001");
        assert_eq!(
            (c.title.as_str(), c.artist.as_str(), c.year),
            ("Saint o'clock", "2AM", Some(2010))
        );
        assert_eq!(c.label.as_deref(), Some("JYP Entertainment"));
        assert_eq!(c.format.as_deref(), Some("CD"));
        let r = mb_release(c.clone());
        assert_eq!(r.genre.as_deref(), Some("K-pop"));
        assert_eq!(r.tracks.len(), 3);
        assert_eq!(r.tracks[2].artist, "2AM feat. Yubin");
        assert_eq!(r.tracks[1].duration_ms, Some(236_000));
        assert_eq!(
            musicbrainz_query("Saint \"o'clock\"", Some("2AM")),
            r#"release:"Saint \"o'clock\"" AND artist:"2AM""#
        );
    }

    #[test]
    fn reads_itunes() {
        let v = json!({"resultCount": 2, "results": [
            {"wrapperType": "collection", "collectionId": 1234, "collectionName": "이노래 - Single",
             "artistName": "2AM", "releaseDate": "2008-07-10T07:00:00Z", "trackCount": 2,
             "copyright": "℗ 2008 JYP", "primaryGenreName": "K-Pop",
             "artworkUrl100": "https://is1-ssl.mzstatic.com/image/thumb/x/100x100bb.jpg"},
            {"wrapperType": "artist", "artistName": "2AM"}
        ]});
        let found = parse_itunes_search(&v, "KR");
        assert_eq!(found.len(), 1);
        let c = &found[0];
        assert_eq!(c.id, "itunes:KR:1234");
        assert_eq!((c.title.as_str(), c.format.as_deref()), ("이노래", Some("Single")));
        assert_eq!(c.year, Some(2008));
        assert_eq!(c.label.as_deref(), Some("2008 JYP"));
        assert_eq!(
            c.cover_url.as_deref(),
            Some("https://is1-ssl.mzstatic.com/image/thumb/x/600x600bb.jpg")
        );
        let tracks = json!({"results": [
            {"wrapperType": "collection", "primaryGenreName": "K-Pop"},
            {"wrapperType": "track", "trackName": "이 노래 (Inst.)", "artistName": "2AM", "discNumber": 1, "trackNumber": 2, "trackTimeMillis": 240000},
            {"wrapperType": "track", "trackName": "이 노래", "artistName": "2AM", "discNumber": 1, "trackNumber": 1, "trackTimeMillis": 241000}
        ]});
        let r = parse_itunes_release(&tracks, c.clone());
        assert_eq!(r.genre.as_deref(), Some("K-Pop"));
        assert_eq!(r.tracks[0].title, "이 노래");
        assert_eq!(r.tracks[1].number, 2);
    }

    #[test]
    fn similarity_handles_hangul_and_noise() {
        assert_eq!(similarity("이 노래", "이노래"), 1.0);
        assert!(similarity("죽어도 못 보내", "죽어도못보내") > 0.99);
        assert!(similarity("Bohemian Rhapsody", "Bohemian Rhapsody (Remastered 2011)") > 0.6);
        assert_eq!(
            title_similarity("Bohemian Rhapsody", "Bohemian Rhapsody (Remastered 2011)"),
            1.0
        );
        assert!(similarity("Intro", "Like Tears") < 0.2);
        assert_eq!(storefront("マリーゴールド", None), "JP");
        assert_eq!(storefront("이노래", Some("2AM")), "US");
    }

    #[test]
    fn proposes_changes_for_matched_files() {
        let c = parse_musicbrainz_search(&mb_search()).remove(0);
        let release = mb_release(c);
        let mut a = track("2AM/Saint/02.mp3", Some("Saint oclock"), Some("2AM"), None);
        a.title = Some("죽어도못보내".into());
        a.track = Some(2);
        a.duration_ms = 235_500;
        a.legacy_text = true;
        let mut b = track("2AM/Saint/xx.mp3", Some("Saint oclock"), Some("2AM"), None);
        b.title = Some("Like Tears".into());
        b.duration_ms = 214_400;
        let mut stray = track("2AM/Saint/bonus.mp3", Some("Saint oclock"), Some("2AM"), None);
        stray.title = Some("Something Else Entirely".into());
        stray.duration_ms = 100_000;
        let files = [&a, &b, &stray];
        let p = propose(&files, &release, |_| true);

        let fields: Vec<Field> = p.album.iter().map(|c| c.field).collect();
        assert_eq!(fields, [Field::Album, Field::AlbumArtist, Field::Genre, Field::Year]);
        assert_eq!(p.album[0].from.as_deref(), Some("Saint oclock"));
        assert_eq!(p.album[0].to, "Saint o'clock");

        assert_eq!(p.tracks[0].matched.as_ref().map(|r| r.number), Some(2));
        assert_eq!(p.tracks[0].changes[0].to, "죽어도 못 보내");
        assert!(p.tracks[0].repairs);
        assert_eq!(p.tracks[1].matched.as_ref().map(|r| r.number), Some(3));
        let artist = p.tracks[1].changes.iter().find(|c| c.field == Field::Artist).unwrap();
        assert_eq!(artist.to, "2AM feat. Yubin");
        assert!(p.tracks[2].matched.is_none());
        assert_eq!(p.unmatched.len(), 1);
        assert_eq!(p.unmatched[0].title, "Intro");

        let e = edit_for(&p, &release, &p.tracks[0], &[Field::Album, Field::Year], true);
        assert_eq!(e.album.as_deref(), Some("Saint o'clock"));
        assert_eq!(e.year, Some(2010));
        assert_eq!(e.genre, None);
        assert_eq!(e.title.as_deref(), Some("죽어도 못 보내"));
        assert_eq!((e.track, e.track_total, e.disc), (None, None, None));
        let only_album = edit_for(&p, &release, &p.tracks[1], &[Field::Album], false);
        assert_eq!(only_album.title, None);
        assert_eq!(only_album.album.as_deref(), Some("Saint o'clock"));
    }
}
