//! Connexions sortantes du cœur (Twitch, Spotify, TikTok, GitHub) : TCP, puis
//! TLS de Windows (SChannel, via native-tls) pour https:// et wss://.
//! Les certificats sont vérifiés par Windows (magasin du système) : aucune
//! exception, aucun certificat accepté sans vérification.

use batlay_core::net::client::{tcp_connect, Connector, Stream};
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::time::Duration;

pub struct TlsConnector {
    inner: native_tls::TlsConnector,
}

impl TlsConnector {
    pub fn new() -> Result<TlsConnector, String> {
        native_tls::TlsConnector::new().map(|inner| TlsConnector { inner }).map_err(|e| format!("TLS de Windows indisponible : {e}"))
    }
}

struct TlsStream(native_tls::TlsStream<TcpStream>);

impl Read for TlsStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for TlsStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl Stream for TlsStream {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.0.get_ref().set_read_timeout(timeout)
    }
    fn shutdown(&mut self) {
        let _ = self.0.shutdown();
        let _ = self.0.get_ref().shutdown(Shutdown::Both);
    }
}

impl Connector for TlsConnector {
    fn connect(&self, host: &str, port: u16, tls: bool, timeout: Duration) -> io::Result<Box<dyn Stream>> {
        let tcp = tcp_connect(host, port, timeout)?;
        if !tls {
            return Ok(Box::new(tcp));
        }
        // La poignée de main TLS ne doit pas bloquer plus longtemps que la connexion elle-même.
        tcp.set_read_timeout(Some(timeout))?;
        tcp.set_write_timeout(Some(timeout))?;
        let stream = self.inner.connect(host, tcp).map_err(|e| io::Error::other(format!("TLS avec {host} : {e}")))?;
        // Écritures : un délai large (un envoi ne doit jamais bloquer indéfiniment).
        stream.get_ref().set_write_timeout(Some(Duration::from_secs(30)))?;
        Ok(Box::new(TlsStream(stream)))
    }
}
