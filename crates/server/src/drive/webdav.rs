//! A WebDAV drive, such as Jianguoyun's (docs/local/drive.md#只在不存在时写入). Some servers, Jianguoyun's
//! among them, overwrite whatever a PUT's conditions say, so a file is written where none is by
//! uploading it to `tmp/` and moving it in place with `Overwrite: F`.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use quick_xml::Reader;
use quick_xml::events::Event;
use ureq::http::{Request, Response};
use uuid::Uuid;

use super::http::{self, encode, refused, transport};
use super::remote::{Created, Failed, Remote};

pub(crate) struct WebDav {
    agent: ureq::Agent,
    /// The folder's address, ending in `/`.
    root: String,
    authorization: String,
}

impl WebDav {
    /// The folder `folder`, its parts separated by `/`, on the server at `url`.
    pub(crate) fn new(url: &str, user: &str, password: &str, folder: &str) -> WebDav {
        let mut root = url.trim_end_matches('/').to_owned();
        for part in folder.split('/').filter(|p| !p.is_empty()) {
            root.push('/');
            root.push_str(&encode(part));
        }
        root.push('/');
        WebDav { agent: http::agent(), root, authorization: format!("Basic {}", BASE64.encode(format!("{user}:{password}"))) }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.root)
    }

    fn send(&self, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Result<Response<ureq::Body>, Failed> {
        let mut req = Request::builder().method(method).uri(self.url(path)).header("Authorization", &self.authorization);
        for (name, value) in headers {
            req = req.header(*name, *value);
        }
        let req = req.body(body).map_err(|e| Failed::Unsent(format!("请求无效：{e}")))?;
        self.agent.run(req).map_err(transport)
    }
}

impl Remote for WebDav {
    fn get(&self, path: &str) -> Result<Option<Vec<u8>>, Failed> {
        let res = self.send("GET", path, &[], &[])?;
        match res.status().as_u16() {
            200 => http::body(res).map(Some),
            404 => Ok(None),
            status => Err(refused(status)),
        }
    }

    fn create(&self, path: &str, body: &[u8]) -> Result<Created, Failed> {
        let tmp = format!("tmp/{}", Uuid::now_v7());
        // Nothing is in place until the move: a failure here leaves at most a file in `tmp/`.
        let put = self.send("PUT", &tmp, &[], body).map_err(|f| Failed::Unsent(f.message().to_owned()))?;
        match put.status().as_u16() {
            200 | 201 | 204 => {}
            status => return Err(Failed::Unsent(refused(status).message().to_owned())),
        }
        let res = self.send("MOVE", &tmp, &[("Destination", &self.url(path)), ("Overwrite", "F")], &[])?;
        match res.status().as_u16() {
            201 | 204 => Ok(Created::Written),
            // 412 as WebDAV has it, 409 from Jianguoyun. The folders it goes in are made beforehand, so
            // 409 can't mean that one of them is missing.
            409 | 412 => {
                // Left for the clearing of `tmp/` when this fails.
                let _ = self.delete(&tmp);
                Ok(Created::Exists)
            }
            status => Err(refused(status)),
        }
    }

    fn delete(&self, path: &str) -> Result<(), Failed> {
        let res = self.send("DELETE", path, &[], &[])?;
        match res.status().as_u16() {
            200 | 204 | 404 => Ok(()),
            status => Err(refused(status)),
        }
    }

    fn list(&self, dir: &str) -> Result<Vec<String>, Failed> {
        const ASK: &[u8] = br#"<?xml version="1.0" encoding="utf-8"?><propfind xmlns="DAV:"><prop><resourcetype/></prop></propfind>"#;
        let res = self.send("PROPFIND", &http::folder(dir), &[("Depth", "1"), ("Content-Type", "application/xml")], ASK)?;
        match res.status().as_u16() {
            207 => listed(&http::body(res)?),
            404 => Ok(Vec::new()),
            status => Err(refused(status)),
        }
    }

    fn mkdir(&self, dir: &str) -> Result<(), Failed> {
        let res = self.send("MKCOL", &http::folder(dir), &[], &[])?;
        match res.status().as_u16() {
            // 405: it is there already.
            200 | 201 | 405 => Ok(()),
            status => Err(refused(status)),
        }
    }
}

/// The names of what is in a folder, from a PROPFIND's answer: the folder itself is in it too, as
/// the one the others are in.
fn listed(xml: &[u8]) -> Result<Vec<String>, Failed> {
    let hrefs = hrefs(xml)?;
    let Some(own) = hrefs.iter().min_by_key(|h| h.len()) else { return Ok(Vec::new()) };
    Ok(hrefs.iter().filter(|h| *h != own).filter_map(|h| h.trim_end_matches('/').rsplit('/').next()).map(str::to_owned).collect())
}

fn hrefs(xml: &[u8]) -> Result<Vec<String>, Failed> {
    let unreadable = |e: quick_xml::Error| Failed::Unknown(format!("网盘的回复无法读取：{e}"));
    let mut reader = Reader::from_reader(xml);
    let (mut buf, mut found, mut href) = (Vec::new(), Vec::new(), None::<String>);
    loop {
        match reader.read_event_into(&mut buf).map_err(unreadable)? {
            Event::Start(e) => href = (e.local_name().into_inner() == "href").then(String::new),
            Event::Text(t) => {
                if let Some(h) = &mut href {
                    h.push_str(&t.xml10_content());
                }
            }
            Event::GeneralRef(r) => {
                if let Some(h) = &mut href {
                    h.push_str(&http::referred(&r)?);
                }
            }
            Event::End(_) => found.extend(href.take().map(|h| h.trim().to_owned())),
            Event::Eof => return Ok(found),
            _ => {}
        }
        buf.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_what_is_in_a_folder_not_the_folder() {
        let xml = br#"<?xml version="1.0" encoding="utf-8"?>
            <d:multistatus xmlns:d="DAV:">
              <d:response><d:href>/dav/%E5%90%8C%E6%AD%A5/g/snapshots/</d:href></d:response>
              <d:response><d:href>/dav/%E5%90%8C%E6%AD%A5/g/snapshots/200.db.gz</d:href></d:response>
              <d:response><d:href>/dav/%E5%90%8C%E6%AD%A5/g/snapshots/400.db.gz</d:href></d:response>
            </d:multistatus>"#;
        assert_eq!(listed(xml).unwrap(), ["200.db.gz", "400.db.gz"]);
        // A reference in XML text, as for `&` where the server does not percent-encode it.
        let xml = br#"<d:multistatus xmlns:d="DAV:"><d:response><d:href>/dav/Work&amp;Home/</d:href></d:response>
            <d:response><d:href>/dav/Work&amp;Home/a&#38;b</d:href></d:response></d:multistatus>"#;
        assert_eq!(listed(xml).unwrap(), ["a&b"]);
        assert_eq!(
            WebDav::new("https://dav.example.com/dav/", "u", "p", "同步").url("g/"),
            "https://dav.example.com/dav/%E5%90%8C%E6%AD%A5/g/"
        );
    }
}
