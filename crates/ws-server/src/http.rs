//! Minimal HTTP/1.1 request parser and response writer, plus RFC 6455 helpers.
//!
//! Only the subset needed by WorldSim is implemented:
//! - Request line + headers + `Content-Length` bodies (no chunked, no trailers).
//! - `Keep-Alive` is ignored — each REST connection handles one request then
//!   closes.  WebSocket and SSE connections are upgraded to long-lived streams.
//! - WebSocket framing covers what browsers actually send: single-frame masked
//!   text, ping/pong, close.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

// ---------- request ----------

#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query_raw: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>, // names lower-cased
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str)->Option<&str>{
        let low=name.to_ascii_lowercase();
        self.headers.iter().find(|(k,_)| k==&low).map(|(_,v)| v.as_str())
    }
    pub fn query_param(&self, key: &str)->Option<&str>{
        self.query.iter().find(|(k,_)| k==key).map(|(_,v)| v.as_str())
    }
}

// ---------- response ----------

pub struct Response {
    pub status: u16,
    pub reason: &'static str,
    pub headers: Vec<(String,String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16)->Self{
        let reason=match status{
            101=>"Switching Protocols",
            200=>"OK", 201=>"Created", 204=>"No Content",
            302=>"Found",
            400=>"Bad Request", 404=>"Not Found", 405=>"Method Not Allowed",
            409=>"Conflict", 413=>"Payload Too Large", 500=>"Internal Server Error",
            _=>"OK",
        };
        Self{ status, reason, headers: Vec::new(), body: Vec::new() }
    }
    pub fn json_body(mut self, json: &ws_core::json::Json)->Self{
        let s=json.dump();
        self.body=s.into_bytes();
        self.headers.push(("Content-Type".into(),"application/json; charset=utf-8".into()));
        self
    }
    pub fn text_body(mut self, text: impl Into<Vec<u8>>, ct: &str)->Self{
        self.body=text.into();
        self.headers.push(("Content-Type".into(), ct.into()));
        self
    }
    pub fn bytes_body(mut self, bytes: Vec<u8>, ct: &str)->Self{
        self.body=bytes;
        self.headers.push(("Content-Type".into(), ct.into()));
        self
    }
    pub fn header(mut self, k: impl Into<String>, v: impl Into<String>)->Self{
        self.headers.push((k.into(), v.into()));
        self
    }
    pub fn with_cors(mut self)->Self{
        self.headers.push(("Access-Control-Allow-Origin".into(), "*".into()));
        self.headers.push(("Access-Control-Allow-Methods".into(), "GET, POST, PUT, DELETE, OPTIONS".into()));
        self.headers.push(("Access-Control-Allow-Headers".into(), "Content-Type, Authorization".into()));
        self
    }
    pub fn redirect(to: &str)->Self{
        let mut r=Self::new(302);
        r.headers.push(("Location".into(), to.into()));
        r.with_cors()
    }
}

// ---------- wire formatting ----------

fn status_line(r: &Response)->String{
    format!("HTTP/1.1 {} {}\r\n", r.status, r.reason)
}

pub fn write_response(stream: &mut TcpStream, mut resp: Response) -> std::io::Result<()>{
    // Always close REST connections for simplicity.
    if !resp.headers.iter().any(|(k,_)| k.eq_ignore_ascii_case("Connection")) {
        resp.headers.push(("Connection".into(), "close".into()));
    }
    if !resp.headers.iter().any(|(k,_)| k.eq_ignore_ascii_case("Access-Control-Allow-Origin")) {
        resp.headers.push(("Access-Control-Allow-Origin".into(), "*".into()));
    }
    resp.headers.push(("Content-Length".into(), resp.body.len().to_string()));
    let mut buf=Vec::with_capacity(256+resp.body.len());
    buf.extend_from_slice(status_line(&resp).as_bytes());
    for (k,v) in &resp.headers { buf.extend_from_slice(k.as_bytes()); buf.extend_from_slice(b": "); buf.extend_from_slice(v.as_bytes()); buf.extend_from_slice(b"\r\n"); }
    buf.extend_from_slice(b"\r\n");
    buf.extend_from_slice(&resp.body);
    stream.write_all(&buf)?;
    stream.flush()?;
    Ok(())
}

// ---------- reading one request (blocking, with headers+body) ----------

/// Read exactly one HTTP request from `stream`.  Returns `Ok(None)` on clean
/// EOF.  The stream's read timeout should already be set by the caller.
pub fn read_one_request(stream: &mut TcpStream)->std::io::Result<Option<Request>>{
    let mut buf: Vec<u8>=Vec::with_capacity(4096);
    let mut tmp=[0u8; 4096];
    // Read until header terminator
    let header_end: usize;
    loop{
        let n=stream.read(&mut tmp)?;
        if n==0 {
            if buf.is_empty() { return Ok(None); }
            else { return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof,"eof during headers")); }
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos)=find_header_end(&buf) { header_end=pos; break; }
        if buf.len()> 64*1024 { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,"header too large")); }
    }

    // Parse header slice
    let header_bytes=&buf[..header_end];
    let header_str=String::from_utf8_lossy(header_bytes);
    let mut lines=header_str.lines();
    let req_line=lines.next().unwrap_or("");
    let mut parts=req_line.split_whitespace();
    let method=parts.next().unwrap_or("").to_string();
    let target=parts.next().unwrap_or("/").to_string();
    if method.is_empty() { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,"missing method")); }

    // Split target into path + query
    let (raw_path, raw_query)=match target.find('?'){
        Some(i)=> (&target[..i], &target[i+1..]),
        None=> (target.as_str(), ""),
    };
    let path=percent_decode(raw_path);
    let query=parse_query(raw_query);

    // Headers
    let mut headers: Vec<(String,String)>=Vec::new();
    for line in lines{
        if line.is_empty() { break; }
        if let Some(col)=line.find(':'){
            let name=line[..col].trim().to_ascii_lowercase();
            let value=line[col+1..].trim().to_string();
            headers.push((name, value));
        }
    }
    let content_length: usize = headers.iter().find(|(k,_)| k=="content-length").and_then(|(_,v)| v.parse().ok()).unwrap_or(0);
    if content_length> 32*1024*1024 { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,"payload too large")); }

    // Body: already buffered tail + maybe need more
    let header_len=header_end+4;
    let already = if buf.len()>header_len { buf.len()-header_len } else { 0 };
    let mut body: Vec<u8>=Vec::with_capacity(content_length);
    if already>0 { body.extend_from_slice(&buf[header_len..]); }
    while body.len()<content_length{
        let need=content_length-body.len();
        let cap=tmp.len();
        let n=stream.read(&mut tmp[..need.min(cap)])?;
        if n==0 { return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof,"eof during body")); }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(content_length);

    let req=Request{ method, path, query_raw: raw_query.to_string(), query, headers, body };
    Ok(Some(req))
}

fn find_header_end(buf: &[u8])->Option<usize>{
    buf.windows(4).position(|w| w==b"\r\n\r\n")
}

fn parse_query(raw: &str)->Vec<(String,String)>{
    let mut out=Vec::new();
    if raw.is_empty() { return out; }
    for pair in raw.split('&'){
        if pair.is_empty() { continue; }
        let (k,v)=match pair.find('='){ Some(i)=> (&pair[..i], &pair[i+1..]), None=> (pair,"") };
        out.push((percent_decode(k), percent_decode(v)));
    }
    out
}

pub fn percent_decode(s: &str)->String{
    let mut out=String::with_capacity(s.len());
    let bytes=s.as_bytes();
    let mut i=0usize;
    while i<bytes.len(){
        if bytes[i]==b'%' && i+2 < bytes.len(){
            if let (Some(hi), Some(lo))=(hex_val(bytes[i+1]), hex_val(bytes[i+2])){
                out.push((hi<<4 | lo) as char);
                i+=3; continue;
            }
        }
        // Keep '+' as '+' (do NOT turn into space — breaks floats like 1e+3).
        out.push(bytes[i] as char);
        i+=1;
    }
    out
}
fn hex_val(c: u8)->Option<u8>{
    match c{
        b'0'..=b'9'=>Some(c-b'0'),
        b'a'..=b'f'=>Some(c-b'a'+10),
        b'A'..=b'F'=>Some(c-b'A'+10),
        _=>None,
    }
}

// ---------- WebSocket helpers ----------

const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

pub fn ws_accept_for(key: &str)->String{
    let mut h=ws_core::sha1::Sha1::new();
    h.update(key.as_bytes());
    h.update(WS_GUID.as_bytes());
    let digest=h.finish();
    ws_core::base64::encode(&digest)
}

/// Write a server→client text frame (unmasked, FIN=1, opcode 1).
pub fn ws_write_text(stream: &mut TcpStream, text: &str)->std::io::Result<()>{
    ws_write_frame(stream, 0x1, text.as_bytes())
}

pub fn ws_write_frame(stream: &mut TcpStream, opcode: u8, payload: &[u8])->std::io::Result<()>{
    let mut hdr=Vec::with_capacity(10+payload.len());
    hdr.push(0x80 | (opcode & 0x0F));
    if payload.len() < 126 {
        hdr.push(payload.len() as u8);
    } else if payload.len() < 65536 {
        hdr.push(126);
        hdr.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        hdr.push(127);
        hdr.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    hdr.extend_from_slice(payload);
    stream.write_all(&hdr)?;
    stream.flush()?;
    Ok(())
}

/// Read one client→server frame (masked as required by the spec).  Returns
/// `Ok(None)` if the read would block (timeout).  Handles 126/127 extended
/// lengths.  Reassembles only single-frame messages (viewer never fragments).
pub fn ws_read_frame(stream: &mut TcpStream)->std::io::Result<Option<(u8, Vec<u8>)>>{
    let mut hdr=[0u8;2];
    // Use a short read with timeout semantics: if timeout, return None.
    match stream.read_exact(&mut hdr){
        Ok(())=>{},
        Err(e) if e.kind()==std::io::ErrorKind::WouldBlock || e.kind()==std::io::ErrorKind::TimedOut => return Ok(None),
        Err(e) if e.kind()==std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e)=> return Err(e),
    }
    let fin=(hdr[0]&0x80)!=0;
    let opcode=hdr[0]&0x0F;
    let masked=(hdr[1]&0x80)!=0;
    let mut len=(hdr[1]&0x7F) as u64;
    if len==126{
        let mut ext=[0u8;2]; stream.read_exact(&mut ext)?; len=u16::from_be_bytes(ext) as u64;
    } else if len==127{
        let mut ext=[0u8;8]; stream.read_exact(&mut ext)?; len=u64::from_be_bytes(ext);
    }
    if len> 8*1024*1024 { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,"ws frame too large")); }
    let mask_key = if masked { let mut k=[0u8;4]; stream.read_exact(&mut k)?; Some(k) } else { None };
    let mut payload=vec![0u8; len as usize];
    if len>0 { stream.read_exact(&mut payload)?; }
    if let Some(key)=mask_key{
        for (i,b) in payload.iter_mut().enumerate(){ *b ^= key[i%4]; }
    }
    if !fin {
        // Continuation not supported in v0.1 — drain and error.
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,"fragmented ws frames not supported"));
    }
    Ok(Some((opcode, payload)))
}

pub fn is_websocket_upgrade(req: &Request)->bool{
    let is_upgrade=req.header("upgrade").map(|v| v.to_ascii_lowercase().contains("websocket")).unwrap_or(false);
    let has_key=req.headers.iter().any(|(k,_)| k=="sec-websocket-key");
    is_upgrade && has_key
        && req.header("connection").map(|v| v.to_ascii_lowercase().contains("upgrade")).unwrap_or(false)
}

pub fn header_lookup<'a>(headers: &'a [(String,String)], name: &str)->Option<&'a str>{
    let low=name.to_ascii_lowercase();
    headers.iter().find(|(k,_)| k==&low).map(|(_,v)| v.as_str())
}
