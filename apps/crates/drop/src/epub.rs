//! Just enough ZIP + EPUB to read a book's title, author and cover:
//! the central directory from the end of the file, then only the entries we
//! need (container.xml, the OPF, the cover image). Never loads the book.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const MAX_ENTRY: u64 = 8 << 20;

struct Entry {
    name: String,
    method: u16,
    csize: u64,
    usize_: u64,
    local: u64,
}

fn u16le(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

fn central_dir(f: &mut File) -> Option<Vec<Entry>> {
    let len = f.seek(SeekFrom::End(0)).ok()?;
    let tail = len.min(66_000);
    f.seek(SeekFrom::Start(len - tail)).ok()?;
    let mut t = vec![0u8; tail as usize];
    f.read_exact(&mut t).ok()?;
    let e = (0..t.len().saturating_sub(21)).rev().find(|&i| t[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])?;
    let n = u16le(&t, e + 10) as usize;
    let size = u32le(&t, e + 12) as u64;
    let off = u32le(&t, e + 16) as u64;
    if size > 16 << 20 || off + size > len {
        return None;
    }
    f.seek(SeekFrom::Start(off)).ok()?;
    let mut cd = vec![0u8; size as usize];
    f.read_exact(&mut cd).ok()?;
    let mut v = Vec::with_capacity(n);
    let mut i = 0;
    while i + 46 <= cd.len() && cd[i..i + 4] == [0x50, 0x4b, 0x01, 0x02] {
        let (nl, xl, cl) = (u16le(&cd, i + 28) as usize, u16le(&cd, i + 30) as usize, u16le(&cd, i + 32) as usize);
        if i + 46 + nl > cd.len() {
            break;
        }
        v.push(Entry {
            name: String::from_utf8_lossy(&cd[i + 46..i + 46 + nl]).into_owned(),
            method: u16le(&cd, i + 10),
            csize: u32le(&cd, i + 20) as u64,
            usize_: u32le(&cd, i + 24) as u64,
            local: u32le(&cd, i + 42) as u64,
        });
        i += 46 + nl + xl + cl;
    }
    Some(v)
}

fn read_entry(f: &mut File, e: &Entry) -> Option<Vec<u8>> {
    if e.usize_ > MAX_ENTRY || e.csize > MAX_ENTRY {
        return None;
    }
    f.seek(SeekFrom::Start(e.local)).ok()?;
    let mut h = [0u8; 30];
    f.read_exact(&mut h).ok()?;
    if h[..4] != [0x50, 0x4b, 0x03, 0x04] {
        return None;
    }
    let skip = u16le(&h, 26) as i64 + u16le(&h, 28) as i64;
    f.seek(SeekFrom::Current(skip)).ok()?;
    let mut c = vec![0u8; e.csize as usize];
    f.read_exact(&mut c).ok()?;
    match e.method {
        0 => Some(c),
        8 => {
            let mut out = Vec::with_capacity(e.usize_ as usize);
            flate2::read::DeflateDecoder::new(&c[..]).take(MAX_ENTRY).read_to_end(&mut out).ok()?;
            Some(out)
        }
        _ => None,
    }
}

pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(j) = rest.find(';').filter(|j| *j < 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..j];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if ent.starts_with("#x") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[j + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Text of the first element whose name ends with `tag` (e.g. "title"
/// matches <dc:title> and <title>), with nested markup stripped.
fn elem_text(xml: &str, tag: &str) -> Option<String> {
    let mut from = 0;
    while let Some(p) = xml[from..].find('<') {
        let s = from + p + 1;
        let end = xml[s..].find('>')? + s;
        let head = &xml[s..end];
        from = end + 1;
        let name = head.split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        if name.starts_with('/') || name.starts_with('?') || name.starts_with('!') || head.ends_with('/') {
            continue;
        }
        if name == tag || name.ends_with(&format!(":{tag}")) {
            let close = xml[from..].find(&format!("</{name}"))? + from;
            let raw = &xml[from..close];
            let mut t = String::new();
            let mut intag = false;
            for c in raw.chars() {
                match c {
                    '<' => intag = true,
                    '>' => intag = false,
                    _ if !intag => t.push(c),
                    _ => {}
                }
            }
            let t = unescape(t.split_whitespace().collect::<Vec<_>>().join(" ").as_str());
            return (!t.is_empty()).then_some(t);
        }
    }
    None
}

fn attr(tag: &str, name: &str) -> Option<String> {
    for q in ['"', '\''] {
        let pat = format!(" {name}={q}");
        if let Some(i) = tag.find(&pat) {
            let s = i + pat.len();
            let e = tag[s..].find(q)? + s;
            return Some(unescape(&tag[s..e]));
        }
    }
    None
}

/// All start tags named `name` (or `*:name`).
fn tags<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let mut v = Vec::new();
    let mut from = 0;
    while let Some(p) = xml[from..].find('<') {
        let s = from + p;
        let Some(e) = xml[s..].find('>') else { break };
        let t = &xml[s..s + e + 1];
        let n = t[1..].split(|c: char| c.is_whitespace() || c == '/' || c == '>').next().unwrap_or("");
        if n == name || n.ends_with(&format!(":{name}")) {
            v.push(t);
        }
        from = s + e + 1;
    }
    v
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta {
    pub title: Option<String>,
    pub author: Option<String>,
    pub language: Option<String>,
    /// zip entry name of the cover image + its media type
    pub cover: Option<(String, String)>,
}

pub fn parse_opf(opf: &str, opf_dir: &str) -> Meta {
    let mut m = Meta { title: elem_text(opf, "title"), author: elem_text(opf, "creator"), language: elem_text(opf, "language"), cover: None };
    let items = tags(opf, "item");
    let cover_id = tags(opf, "meta").into_iter().find(|t| attr(t, "name").as_deref() == Some("cover")).and_then(|t| attr(t, "content"));
    let pick = items
        .iter()
        .find(|t| attr(t, "properties").is_some_and(|p| p.split_whitespace().any(|x| x == "cover-image")))
        .or_else(|| cover_id.as_ref().and_then(|id| items.iter().find(|t| attr(t, "id").as_deref() == Some(id))));
    if let Some(t) = pick {
        if let (Some(href), Some(mt)) = (attr(t, "href"), attr(t, "media-type")) {
            if mt.starts_with("image/") {
                let href = crate::pct_decode(&href).unwrap_or(href);
                m.cover = Some((if opf_dir.is_empty() { href } else { format!("{opf_dir}/{href}") }, mt));
            }
        }
    }
    m
}

fn opf_of(f: &mut File, cd: &[Entry]) -> Option<(String, String)> {
    let c = cd.iter().find(|e| e.name == "META-INF/container.xml")?;
    let x = String::from_utf8(read_entry(f, c)?).ok()?;
    let path = tags(&x, "rootfile").into_iter().find_map(|t| attr(t, "full-path"))?;
    let e = cd.iter().find(|e| e.name == path)?;
    let opf = String::from_utf8_lossy(&read_entry(f, e)?).into_owned();
    let dir = path.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
    Some((opf, dir))
}

pub fn read_meta(p: &Path) -> Option<Meta> {
    let mut f = File::open(p).ok()?;
    let cd = central_dir(&mut f)?;
    let (opf, dir) = opf_of(&mut f, &cd)?;
    Some(parse_opf(&opf, &dir))
}

/// Cover image bytes and media type.
pub fn read_cover(p: &Path) -> Option<(Vec<u8>, String)> {
    let mut f = File::open(p).ok()?;
    let cd = central_dir(&mut f)?;
    let (opf, dir) = opf_of(&mut f, &cd)?;
    let (name, mt) = parse_opf(&opf, &dir).cover?;
    let e = cd.iter().find(|e| e.name == name)?;
    Some((read_entry(&mut f, e)?, mt))
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A minimal stored + deflated EPUB written by hand (no zip crate).
    pub fn make_epub(path: &Path, title: &str, author: &str) {
        use std::io::Write;
        let container = br#"<?xml version="1.0"?><container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#.to_vec();
        let opf = format!(
            r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{title}</dc:title><dc:creator opf:role="aut">{author}</dc:creator><dc:language>en</dc:language><meta name="cover" content="c1"/></metadata><manifest><item id="c1" href="images/cover%20art.jpg" media-type="image/jpeg"/></manifest></package>"#
        );
        let files: Vec<(&str, Vec<u8>, bool)> = vec![
            ("mimetype", b"application/epub+zip".to_vec(), false),
            ("META-INF/container.xml", container, true),
            ("OEBPS/content.opf", opf.into_bytes(), true),
            ("OEBPS/images/cover art.jpg", b"\xff\xd8\xff\xe0JPEGDATA".to_vec(), false),
        ];
        let mut out = Vec::new();
        let mut cd = Vec::new();
        for (name, data, deflate) in &files {
            let comp = if *deflate {
                let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(data).unwrap();
                e.finish().unwrap()
            } else {
                data.clone()
            };
            let off = out.len() as u32;
            let m: u16 = if *deflate { 8 } else { 0 };
            out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04, 20, 0, 0, 0]);
            out.extend_from_slice(&m.to_le_bytes());
            out.extend_from_slice(&[0; 8]); // time, date, crc (unchecked)
            out.extend_from_slice(&(comp.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&comp);
            cd.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02, 20, 0, 20, 0, 0, 0]);
            cd.extend_from_slice(&m.to_le_bytes());
            cd.extend_from_slice(&[0; 8]);
            cd.extend_from_slice(&(comp.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&[0; 12]);
            cd.extend_from_slice(&off.to_le_bytes());
            cd.extend_from_slice(name.as_bytes());
        }
        let cd_off = out.len() as u32;
        out.extend_from_slice(&cd);
        out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0]);
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        std::fs::write(path, out).unwrap();
    }

    #[test]
    fn epub_metadata_and_cover() {
        let d = std::env::temp_dir().join(format!("zepub-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("b.epub");
        make_epub(&p, "Moby-Dick &amp; Other Tales", "Herman Melville");
        let m = read_meta(&p).unwrap();
        assert_eq!(m.title.as_deref(), Some("Moby-Dick & Other Tales"));
        assert_eq!(m.author.as_deref(), Some("Herman Melville"));
        assert_eq!(m.language.as_deref(), Some("en"));
        assert_eq!(m.cover, Some(("OEBPS/images/cover art.jpg".into(), "image/jpeg".into())));
        let (img, mt) = read_cover(&p).unwrap();
        assert_eq!(mt, "image/jpeg");
        assert!(img.starts_with(b"\xff\xd8"));
        std::fs::write(d.join("bad.epub"), b"not a zip").unwrap();
        assert_eq!(read_meta(&d.join("bad.epub")), None);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn xml_bits() {
        assert_eq!(unescape("a &amp; b &#233; &#x41; &bogus; &"), "a & b é A &bogus; &");
        let opf = r#"<metadata><dc:title id="t">  The   <i>Title</i> </dc:title><dc:creator/><dc:creator>X</dc:creator></metadata><manifest><item properties="nav cover-image" href="c.png" id="x" media-type="image/png"/></manifest>"#;
        let m = parse_opf(opf, "");
        assert_eq!(m.title.as_deref(), Some("The Title"));
        assert_eq!(m.author.as_deref(), Some("X"), "self-closing creator skipped");
        assert_eq!(m.cover, Some(("c.png".into(), "image/png".into())));
    }
}
