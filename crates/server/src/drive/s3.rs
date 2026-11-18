//! An S3-compatible object store (docs/local.md#只在不存在时写入): a file is written where none is
//! with `If-None-Match: *`. Requests are signed as AWS Signature Version 4 has it, with HMAC-SHA256.

use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, KeyInit, Mac};
use nexttodo_core::model::Date;
use quick_xml::Reader;
use quick_xml::events::Event;
use sha2::{Digest, Sha256};
use ureq::http::{Request, Response};

use super::http::{self, encode, refused, transport};
use super::remote::{Created, Failed, Remote};

pub(crate) struct S3 {
    agent: ureq::Agent,
    /// `https://<host>`, where the bucket is.
    origin: String,
    host: String,
    /// The bucket in the path of an address: empty when it is in the host.
    bucket_path: String,
    /// The folder's objects' keys start with this.
    prefix: String,
    keys: Keys,
}

struct Keys {
    access: String,
    secret: String,
    region: String,
}

impl S3 {
    /// The folder `folder` in `bucket`, at the endpoint `endpoint`: as a host of its own,
    /// `<bucket>.<endpoint host>`, or with `path_style` in the path, as self-hosted stores such as
    /// MinIO have it.
    pub(crate) fn new(
        endpoint: &str,
        region: &str,
        bucket: &str,
        access: &str,
        secret: &str,
        path_style: bool,
        folder: &str,
    ) -> Result<S3, String> {
        let (scheme, host) = endpoint.trim_end_matches('/').split_once("://").ok_or("接口地址要以 https:// 或 http:// 开头")?;
        if host.is_empty() || host.contains('/') {
            return Err("接口地址只写到域名或端口为止".into());
        }
        let (host, bucket_path) =
            if path_style { (host.to_owned(), format!("/{}", encode(bucket))) } else { (format!("{bucket}.{host}"), String::new()) };
        let prefix = folder.split('/').filter(|p| !p.is_empty()).map(|p| format!("{p}/")).collect();
        Ok(S3 {
            agent: http::agent(),
            origin: format!("{scheme}://{host}"),
            host,
            bucket_path,
            prefix,
            keys: Keys { access: access.to_owned(), secret: secret.to_owned(), region: region.to_owned() },
        })
    }

    /// Sends a request for the object at `path` in the folder, or with a `query`, for the bucket.
    fn send(
        &self,
        method: &str,
        path: Option<&str>,
        query: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<Response<ureq::Body>, Failed> {
        let uri = match path {
            Some(path) => {
                format!("{}/{}", self.bucket_path, format!("{}{path}", self.prefix).split('/').map(encode).collect::<Vec<_>>().join("/"))
            }
            None if self.bucket_path.is_empty() => "/".into(),
            None => self.bucket_path.clone(),
        };
        let payload = hex(&Sha256::digest(body));
        let at = now();
        let mut signed: Vec<(&str, &str)> = vec![("host", &self.host), ("x-amz-content-sha256", &payload), ("x-amz-date", &at)];
        signed.extend_from_slice(headers);
        let authorization = authorization(&self.keys, method, &uri, query, &mut signed, &payload, &at);
        let address = if query.is_empty() { format!("{}{uri}", self.origin) } else { format!("{}{uri}?{query}", self.origin) };
        let mut req = Request::builder().method(method).uri(address).header("Authorization", authorization);
        for (name, value) in signed {
            req = req.header(name, value);
        }
        let req = req.body(body).map_err(|e| Failed::Unsent(format!("请求无效：{e}")))?;
        self.agent.run(req).map_err(transport)
    }
}

impl Remote for S3 {
    fn get(&self, path: &str) -> Result<Option<Vec<u8>>, Failed> {
        let res = self.send("GET", Some(path), "", &[], &[])?;
        match res.status().as_u16() {
            200 => http::body(res).map(Some),
            404 => Ok(None),
            status => Err(refused(status)),
        }
    }

    fn create(&self, path: &str, body: &[u8]) -> Result<Created, Failed> {
        let res = self.send("PUT", Some(path), "", &[("if-none-match", "*")], body)?;
        match res.status().as_u16() {
            200 => Ok(Created::Written),
            412 => Ok(Created::Exists),
            // Another write of it is under way: this one may go in yet, or not.
            409 => Err(Failed::Unknown("网盘正在写入同一个文件".into())),
            501 => Err(Failed::Unsent("这个网盘不支持只在不存在时写入".into())),
            status => Err(refused(status)),
        }
    }

    fn delete(&self, path: &str) -> Result<(), Failed> {
        let res = self.send("DELETE", Some(path), "", &[], &[])?;
        match res.status().as_u16() {
            200 | 204 | 404 => Ok(()),
            status => Err(refused(status)),
        }
    }

    fn list(&self, dir: &str) -> Result<Vec<String>, Failed> {
        let prefix = format!("{}{}", self.prefix, http::folder(dir));
        let (mut names, mut next) = (Vec::new(), None);
        loop {
            // In order, as the signature has it.
            let from = next.map(|token: String| format!("continuation-token={}&", encode(&token))).unwrap_or_default();
            let query = format!("{from}delimiter=%2F&list-type=2&prefix={}", encode(&prefix));
            let res = self.send("GET", None, &query, &[], &[])?;
            let page = match res.status().as_u16() {
                200 => listed(&http::body(res)?, &prefix)?,
                status => return Err(refused(status)),
            };
            names.extend(page.names);
            next = page.next;
            if next.is_none() {
                return Ok(names);
            }
        }
    }

    /// Object stores have no folders.
    fn mkdir(&self, _: &str) -> Result<(), Failed> {
        Ok(())
    }
}

/// A page of a listing.
#[derive(Debug, PartialEq)]
struct Listed {
    names: Vec<String>,
    /// Where the next page starts, when there is one.
    next: Option<String>,
}

/// The names of what is under `prefix`, from a ListObjectsV2 answer: its objects' keys, and the
/// common prefixes, as folders.
fn listed(xml: &[u8], prefix: &str) -> Result<Listed, Failed> {
    let unreadable = |e: quick_xml::Error| Failed::Unknown(format!("网盘的回复无法读取：{e}"));
    let mut reader = Reader::from_reader(xml);
    let (mut buf, mut path, mut text) = (Vec::new(), Vec::<String>::new(), String::new());
    let (mut found, mut next) = (Vec::new(), None);
    loop {
        match reader.read_event_into(&mut buf).map_err(unreadable)? {
            Event::Start(e) => {
                path.push(e.local_name().into_inner().to_owned());
                text.clear();
            }
            Event::Text(t) => text.push_str(&t.xml10_content()),
            Event::GeneralRef(r) => text.push_str(&http::referred(&r)?),
            Event::End(_) => {
                match path.as_slice() {
                    [.., a, b] if (a == "Contents" && b == "Key") || (a == "CommonPrefixes" && b == "Prefix") => {
                        if let Some(name) = text.strip_prefix(prefix).map(|n| n.trim_end_matches('/')).filter(|n| !n.is_empty()) {
                            found.push(name.to_owned());
                        }
                    }
                    [.., a] if a == "NextContinuationToken" => next = Some(text.clone()),
                    _ => {}
                }
                path.pop();
                text.clear();
            }
            Event::Eof => return Ok(Listed { names: found, next }),
            _ => {}
        }
        buf.clear();
    }
}

/// The `Authorization` header for a request: `headers`, by lowercase names, are what is signed, and
/// are sorted.
fn authorization(k: &Keys, method: &str, uri: &str, query: &str, headers: &mut [(&str, &str)], payload: &str, at: &str) -> String {
    headers.sort_unstable_by_key(|(name, _)| *name);
    let canonical: String = headers.iter().map(|(name, value)| format!("{name}:{}\n", value.trim())).collect();
    let names = headers.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(";");
    let request = format!("{method}\n{uri}\n{query}\n{canonical}\n{names}\n{payload}");
    let day = &at[..8];
    let scope = format!("{day}/{}/s3/aws4_request", k.region);
    let to_sign = format!("AWS4-HMAC-SHA256\n{at}\n{scope}\n{}", hex(&Sha256::digest(request)));
    let key = ["s3", "aws4_request"]
        .iter()
        .fold(mac(mac(format!("AWS4{}", k.secret).as_bytes(), day).as_slice(), &k.region), |key, part| mac(&key, part));
    format!("AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={names}, Signature={}", k.access, hex(&mac(&key, &to_sign)))
}

fn mac(key: &[u8], data: &str) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes keys of any length");
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Now, as `x-amz-date` has it: `20130524T000000Z`.
fn now() -> String {
    let ms =
        i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).expect("after 1970").as_millis()).expect("before the year 292 million");
    let secs = ms.div_euclid(1000).rem_euclid(86_400);
    format!("{}T{:02}{:02}{:02}Z", Date::utc(ms).as_str().replace('-', ""), secs / 3600, secs / 60 % 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The example in AWS's documentation of Signature Version 4 for S3, "GET Object".
    #[test]
    fn signs_as_aws_does() {
        let k = Keys {
            access: "AKIAIOSFODNN7EXAMPLE".into(),
            secret: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
            region: "us-east-1".into(),
        };
        let empty = hex(&Sha256::digest(b""));
        let mut headers = [
            ("host", "examplebucket.s3.amazonaws.com"),
            ("range", "bytes=0-9"),
            ("x-amz-content-sha256", empty.as_str()),
            ("x-amz-date", "20130524T000000Z"),
        ];
        let signed = authorization(&k, "GET", "/test.txt", "", &mut headers, &empty, "20130524T000000Z");
        assert!(signed.ends_with("Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"), "{signed}");
    }

    #[test]
    fn lists_keys_and_folders_under_the_prefix() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
            <ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
              <Name>b</Name><Prefix>NextTodo/</Prefix><KeyCount>3</KeyCount>
              <Contents><Key>NextTodo/nexttodo.json</Key><Size>40</Size></Contents>
              <CommonPrefixes><Prefix>NextTodo/0193a1b2-0000-7000-8000-000000000000/</Prefix></CommonPrefixes>
            </ListBucketResult>"#;
        let names = ["nexttodo.json", "0193a1b2-0000-7000-8000-000000000000"].map(str::to_owned).to_vec();
        assert_eq!(listed(xml, "NextTodo/").unwrap(), Listed { names, next: None });
        let more = br#"<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken>1ueGcxLPRx1Tr/XYExHnhbYLgveDs2J/wm36Hy4vbOwM=</NextContinuationToken>
            <Contents><Key>NextTodo/tmp/a</Key></Contents></ListBucketResult>"#;
        let next = Some("1ueGcxLPRx1Tr/XYExHnhbYLgveDs2J/wm36Hy4vbOwM=".to_owned());
        assert_eq!(listed(more, "NextTodo/tmp/").unwrap(), Listed { names: vec!["a".into()], next });
        // XML writes some characters as references, in the folder's name as in the others.
        let escaped = br#"<ListBucketResult><Contents><Key>Work&amp;Home/a&#x26;b&lt;c</Key></Contents>
            <CommonPrefixes><Prefix>Work&amp;Home/0193a1b2-0000-7000-8000-000000000000/</Prefix></CommonPrefixes></ListBucketResult>"#;
        let names = ["a&b<c", "0193a1b2-0000-7000-8000-000000000000"].map(str::to_owned).to_vec();
        assert_eq!(listed(escaped, "Work&Home/").unwrap(), Listed { names, next: None });
    }
}
