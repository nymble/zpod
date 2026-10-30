//! OPDS 1.2 catalog of /data/books (M6), for KOReader, Thorium, Marvin,
//! Moon+ Reader, Librera...
//!
//!   /opds            navigation feed: All books, Newest, By author
//!   /opds/all        acquisition feed, by title
//!   /opds/new        acquisition feed, newest first (by file time)
//!   /opds/authors    navigation feed, one entry per author
//!   /opds/author/<name>  acquisition feed for one author
//!   /books/<path>    the book file; /cover/<path> its EPUB cover
//!
//! Titles/authors come from EPUB metadata (OPF dc:title / dc:creator);
//! other formats use the file name ("Author - Title.pdf" is split).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::epub;

pub const BOOK_EXT: &[(&str, &str)] = &[
    ("epub", "application/epub+zip"),
    ("pdf", "application/pdf"),
    ("mobi", "application/x-mobipocket-ebook"),
    ("azw3", "application/vnd.amazon.ebook"),
    ("fb2", "application/x-fictionbook+xml"),
    ("cbz", "application/vnd.comicbook+zip"),
    ("cbr", "application/vnd.comicbook-rar"),
    ("djvu", "image/vnd.djvu"),
    ("txt", "text/plain; charset=utf-8"),
];

pub fn mime_of(p: &Path) -> Option<&'static str> {
    let e = p.extension()?.to_str()?.to_ascii_lowercase();
    BOOK_EXT.iter().find(|(x, _)| *x == e).map(|(_, m)| *m)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Book {
    /// path relative to the books dir ('/' separated)
    pub rel: String,
    pub title: String,
    pub author: String,
    pub language: Option<String>,
    pub mime: &'static str,
    pub mtime: i64,
    pub size: u64,
    pub cover: Option<String>,
}

/// "Author - Title" or plain "Title" from a file stem.
pub fn from_name(stem: &str) -> (String, String) {
    let s = stem.replace('_', " ");
    match s.split_once(" - ") {
        Some((a, t)) if !a.trim().is_empty() && !t.trim().is_empty() => (t.trim().to_string(), a.trim().to_string()),
        _ => (s.trim().to_string(), String::new()),
    }
}

type Cache = HashMap<PathBuf, (i64, u64, Book)>;

fn walk(root: &Path, dir: &Path, depth: u32, out: &mut Vec<(PathBuf, std::fs::Metadata)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let p = e.path();
        let Ok(m) = e.metadata() else { continue };
        if m.is_dir() && depth < 6 {
            walk(root, &p, depth + 1, out);
        } else if m.is_file() && mime_of(&p).is_some() && out.len() < 20_000 {
            out.push((p, m));
        }
    }
}

/// Scan the books dir (cached per file by mtime + size).
pub fn scan(root: &Path, cache: &mut Cache) -> Vec<Book> {
    let mut files = Vec::new();
    walk(root, root, 0, &mut files);
    let mut v = Vec::with_capacity(files.len());
    for (p, m) in files {
        let mtime = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
        if let Some((t, s, b)) = cache.get(&p) {
            if *t == mtime && *s == m.len() {
                v.push(b.clone());
                continue;
            }
        }
        let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
        let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let (mut title, mut author) = from_name(&stem);
        let mut language = None;
        let mut cover = None;
        let mime = mime_of(&p).unwrap_or("application/octet-stream");
        if mime == "application/epub+zip" {
            if let Some(md) = epub::read_meta(&p) {
                if let Some(t) = md.title {
                    title = t;
                }
                if let Some(a) = md.author {
                    author = a;
                }
                language = md.language;
                cover = md.cover.map(|(_, mt)| mt);
            }
        }
        let b = Book { rel, title, author, language, mime, mtime, size: m.len(), cover };
        cache.insert(p, (mtime, m.len(), b.clone()));
        v.push(b);
    }
    v.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    v
}

pub fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Percent-encode each path segment (keeps '/').
pub fn href_path(rel: &str) -> String {
    rel.split('/').map(|seg| {
        let mut o = String::new();
        for b in seg.bytes() {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                o.push(b as char);
            } else {
                o.push_str(&format!("%{b:02X}"));
            }
        }
        o
    }).collect::<Vec<_>>().join("/")
}

pub fn rfc3339(t: i64) -> String {
    let d = zpod_core::DateTime::from_unix(t);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", d.year, d.month, d.day, d.hour, d.minute, d.second)
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

const NAV: &str = "application/atom+xml;profile=opds-catalog;kind=navigation";
const ACQ: &str = "application/atom+xml;profile=opds-catalog;kind=acquisition";

fn head(id: &str, title: &str, self_href: &str, kind: &str, updated: i64, q: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<feed xmlns=\"http://www.w3.org/2005/Atom\" xmlns:dc=\"http://purl.org/dc/terms/\" xmlns:opds=\"http://opds-spec.org/2010/catalog\">\n\
<id>{id}</id>\n<title>{}</title>\n<updated>{}</updated>\n<author><name>ZPOD</name></author>\n\
<link rel=\"self\" href=\"{self_href}{q}\" type=\"{kind}\"/>\n<link rel=\"start\" href=\"/opds{q}\" type=\"{NAV}\"/>\n",
        xml(title),
        rfc3339(updated)
    )
}

/// `q` = "" or "?token=..." appended to every link (readers that cannot
/// send headers keep working on Wi-Fi with ?token=).
pub fn navigation(books: &[Book], q: &str) -> String {
    let upd = books.iter().map(|b| b.mtime).max().unwrap_or(0);
    let mut s = head("urn:zpod:opds:root", "ZPOD books", "/opds", NAV, upd, q);
    let mut authors: Vec<&str> = books.iter().map(|b| b.author.as_str()).filter(|a| !a.is_empty()).collect();
    authors.sort_unstable();
    authors.dedup();
    for (id, title, href, kind, content) in [
        ("all", "All books", "/opds/all", ACQ, format!("{} books, by title", books.len())),
        ("new", "Newest", "/opds/new", ACQ, "Most recently added first".to_string()),
        ("authors", "By author", "/opds/authors", NAV, format!("{} authors", authors.len())),
    ] {
        s += &format!(
            "<entry><title>{title}</title><id>urn:zpod:opds:{id}</id><updated>{}</updated><content type=\"text\">{}</content><link rel=\"subsection\" href=\"{href}{q}\" type=\"{kind}\"/></entry>\n",
            rfc3339(upd),
            xml(&content)
        );
    }
    s + "</feed>\n"
}

pub fn authors(books: &[Book], q: &str) -> String {
    let upd = books.iter().map(|b| b.mtime).max().unwrap_or(0);
    let mut s = head("urn:zpod:opds:authors", "By author", "/opds/authors", NAV, upd, q);
    let mut m: Vec<(String, usize)> = Vec::new();
    for b in books {
        let a = if b.author.is_empty() { "Unknown author" } else { &b.author };
        match m.iter_mut().find(|(x, _)| x == a) {
            Some(e) => e.1 += 1,
            None => m.push((a.to_string(), 1)),
        }
    }
    m.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    for (a, n) in m {
        s += &format!(
            "<entry><title>{}</title><id>urn:zpod:author:{:016x}</id><updated>{}</updated><content type=\"text\">{n} book{}</content><link rel=\"subsection\" href=\"/opds/author/{}{q}\" type=\"{ACQ}\"/></entry>\n",
            xml(&a),
            fnv(&a),
            rfc3339(upd),
            if n == 1 { "" } else { "s" },
            href_path(&a)
        );
    }
    s + "</feed>\n"
}

pub fn acquisition(books: &[Book], id: &str, title: &str, self_href: &str, q: &str) -> String {
    let upd = books.iter().map(|b| b.mtime).max().unwrap_or(0);
    let mut s = head(&format!("urn:zpod:opds:{id}"), title, self_href, ACQ, upd, q);
    for b in books {
        let h = href_path(&b.rel);
        s += &format!("<entry><title>{}</title><id>urn:zpod:book:{:016x}</id><updated>{}</updated>", xml(&b.title), fnv(&b.rel), rfc3339(b.mtime));
        if !b.author.is_empty() {
            s += &format!("<author><name>{}</name></author>", xml(&b.author));
        }
        if let Some(l) = &b.language {
            s += &format!("<dc:language>{}</dc:language>", xml(l));
        }
        s += &format!("<dc:format>{}</dc:format>", xml(b.mime));
        s += &format!("<link rel=\"http://opds-spec.org/acquisition\" href=\"/books/{h}{q}\" type=\"{}\" length=\"{}\"/>", xml(b.mime), b.size);
        if let Some(mt) = &b.cover {
            s += &format!("<link rel=\"http://opds-spec.org/image\" href=\"/cover/{h}{q}\" type=\"{}\"/>", xml(mt));
            s += &format!("<link rel=\"http://opds-spec.org/image/thumbnail\" href=\"/cover/{h}{q}\" type=\"{}\"/>", xml(mt));
        }
        s += "</entry>\n";
    }
    s + "</feed>\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_paths() {
        assert_eq!(from_name("Jane Austen - Emma"), ("Emma".into(), "Jane Austen".into()));
        assert_eq!(from_name("Some_Book"), ("Some Book".into(), String::new()));
        assert_eq!(href_path("Sci Fi/Dune & co.epub"), "Sci%20Fi/Dune%20%26%20co.epub");
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(mime_of(Path::new("a/B.EPUB")), Some("application/epub+zip"));
        assert_eq!(mime_of(Path::new("a.exe")), None);
    }

    #[test]
    fn feeds_from_a_books_dir() {
        let d = std::env::temp_dir().join(format!("zopds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("Classics")).unwrap();
        crate::epub::tests::make_epub(&d.join("Classics/md.epub"), "Moby Dick", "Herman Melville");
        std::fs::write(d.join("Jane Austen - Emma.pdf"), b"%PDF-1.4").unwrap();
        std::fs::write(d.join(".hidden.epub"), b"x").unwrap();
        std::fs::write(d.join("notes.docx"), b"x").unwrap();
        let mut cache = HashMap::new();
        let books = scan(&d, &mut cache);
        assert_eq!(books.len(), 2);
        assert_eq!(books[0].title, "Emma");
        assert_eq!(books[1].author, "Herman Melville");
        assert_eq!(books[1].cover.as_deref(), Some("image/jpeg"));
        assert_eq!(cache.len(), 2);
        let acq = acquisition(&books, "all", "All books", "/opds/all", "");
        assert!(acq.contains("<title>Moby Dick</title>") && acq.contains("href=\"/books/Classics/md.epub\" type=\"application/epub+zip\""));
        assert!(acq.contains("rel=\"http://opds-spec.org/image\" href=\"/cover/Classics/md.epub\""));
        assert!(acq.contains("kind=acquisition"));
        let nav = navigation(&books, "?token=abc");
        assert!(nav.contains("href=\"/opds/all?token=abc\"") && nav.contains("2 books"));
        let au = authors(&books, "");
        assert!(au.contains("/opds/author/Herman%20Melville") && au.contains("Jane Austen"));
        assert_eq!(acq.matches("<entry>").count(), 2);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
