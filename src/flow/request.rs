use crate::http::HeaderMap;
use std::io::{Result, Write};

fn write_headers<W: Write>(writer: &mut W, map: &HeaderMap) -> Result<()> {
    for (key, value) in map.iter() {
        writer.write_all(key.as_str().as_bytes())?;
        writer.write_all(b": ")?;
        writer.write_all(value.as_bytes())?;
        writer.write_all(b"\r\n")?;
    }
    Ok(())
}

fn write_host_port<W: Write>(writer: &mut W, host: &str, port: u16) -> Result<()> {
    writer.write_all(host.as_bytes())?;
    writer.write_all(b":")?;
    write!(writer, "{}", port)?;
    Ok(())
}

pub fn write<W: Write>(writer: &mut W, host: &str, port: u16, headers: &HeaderMap) -> Result<()> {
    writer.write_all(b"CONNECT ")?;
    write_host_port(writer, host, port)?;
    writer.write_all(b" HTTP/1.1\r\n")?;

    writer.write_all(b"Host: ")?;
    write_host_port(writer, host, port)?;
    writer.write_all(b"\r\n")?;

    write_headers(writer, headers)?;

    writer.write_all(b"\r\n")?;
    Ok(())
}
