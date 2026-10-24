use std::fmt;
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use rustls_platform_verifier::BuilderVerifierExt;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::rustls::pki_types::ServerName;

const MAIL_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_REPLY_LINE: u64 = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mail {
    pub to: String,
    pub subject: String,
    pub text: String,
}

pub type Sending<'a> = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

pub trait Mailer: Send + Sync {
    fn send<'a>(&'a self, mail: &'a Mail) -> Sending<'a>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmtpSecurity {
    Tls,
    StartTls,
    Plain,
}

#[derive(Clone)]
pub struct SmtpSettings {
    pub host: String,
    pub port: u16,
    pub security: SmtpSecurity,
    pub user: Option<String>,
    pub password: Option<String>,
    pub from: String,
}

impl fmt::Debug for SmtpSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SmtpSettings")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("security", &self.security)
            .field("user", &self.user)
            .field("from", &self.from)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
pub enum MailSettings {
    Log,
    Smtp(SmtpSettings),
}

#[derive(Default)]
pub struct LogMailer {
    sent: Mutex<Vec<Mail>>,
}

impl LogMailer {
    pub fn sent(&self) -> Vec<Mail> {
        self.sent
            .lock()
            .expect("the mail log lock is never poisoned")
            .clone()
    }
}

impl Mailer for LogMailer {
    fn send<'a>(&'a self, mail: &'a Mail) -> Sending<'a> {
        eprintln!(
            "kelpie-hub: mail to {} (not sent, no SMTP configured): {}\n{}",
            mail.to, mail.subject, mail.text
        );
        self.sent
            .lock()
            .expect("the mail log lock is never poisoned")
            .push(mail.clone());
        Box::pin(async { Ok(()) })
    }
}

pub fn plausible_address(text: &str) -> bool {
    let Some((local, domain)) = text.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && text.len() <= 254
        && text
            .chars()
            .all(|char| char.is_ascii_graphic() && !matches!(char, '<' | '>' | '"' | ',' | ';'))
        && !domain.contains('@')
}

pub struct SmtpMailer {
    settings: SmtpSettings,
    helo: String,
    tls: TlsConnector,
}

impl SmtpMailer {
    pub fn new(settings: SmtpSettings, helo: &str) -> Result<SmtpMailer, String> {
        if !plausible_address(&settings.from) {
            return Err(format!(
                "{} is not a mail address to send from",
                settings.from
            ));
        }
        let loopback = settings.host == "localhost"
            || settings
                .host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback());
        if settings.security == SmtpSecurity::Plain && !loopback {
            return Err(format!(
                "plain SMTP sends the password in the clear; use tls or starttls for {}",
                settings.host
            ));
        }
        if settings.user.is_some() != settings.password.is_some() {
            return Err(
                "an SMTP user needs KELPIE_HUB_SMTP_PASSWORD, and a password needs --smtp-user"
                    .to_string(),
            );
        }
        let provider = Arc::new(tokio_rustls::rustls::crypto::aws_lc_rs::default_provider());
        let config = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .and_then(|builder| builder.with_platform_verifier())
            .map_err(|error| format!("the SMTP TLS client does not build: {error}"))?
            .with_no_client_auth();
        Ok(SmtpMailer {
            settings,
            helo: helo.to_string(),
            tls: TlsConnector::from(Arc::new(config)),
        })
    }

    async fn deliver(&self, mail: &Mail) -> Result<(), String> {
        let settings = &self.settings;
        let tcp = TcpStream::connect((settings.host.as_str(), settings.port))
            .await
            .map_err(|error| format!("{}:{}: {error}", settings.host, settings.port))?;
        let name = || {
            ServerName::try_from(settings.host.clone())
                .map_err(|error| format!("{}: {error}", settings.host))
        };
        match settings.security {
            SmtpSecurity::Tls => {
                let stream = self
                    .tls
                    .connect(name()?, tcp)
                    .await
                    .map_err(|error| format!("TLS with {}: {error}", settings.host))?;
                let mut session = Session::new(stream);
                session.expect_reply(2).await?;
                session.hello(&self.helo).await?;
                session.transact(settings, &self.helo, mail).await
            }
            SmtpSecurity::StartTls => {
                let mut plain = Session::new(tcp);
                plain.expect_reply(2).await?;
                plain.hello(&self.helo).await?;
                plain.command("STARTTLS", 2).await?;
                let stream = self
                    .tls
                    .connect(name()?, plain.into_inner())
                    .await
                    .map_err(|error| format!("STARTTLS with {}: {error}", settings.host))?;
                let mut session = Session::new(stream);
                session.hello(&self.helo).await?;
                session.transact(settings, &self.helo, mail).await
            }
            SmtpSecurity::Plain => {
                let mut session = Session::new(tcp);
                session.expect_reply(2).await?;
                session.hello(&self.helo).await?;
                session.transact(settings, &self.helo, mail).await
            }
        }
    }
}

impl Mailer for SmtpMailer {
    fn send<'a>(&'a self, mail: &'a Mail) -> Sending<'a> {
        Box::pin(async move {
            tokio::time::timeout(MAIL_TIMEOUT, self.deliver(mail))
                .await
                .map_err(|_| "the SMTP server did not answer in time".to_string())?
        })
    }
}

struct Session<S> {
    stream: BufReader<S>,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Session<S> {
    fn new(stream: S) -> Session<S> {
        Session {
            stream: BufReader::new(stream),
        }
    }

    fn into_inner(self) -> S {
        self.stream.into_inner()
    }

    async fn reply(&mut self) -> Result<(u16, Vec<String>), String> {
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            let read = (&mut self.stream)
                .take(MAX_REPLY_LINE)
                .read_line(&mut line)
                .await
                .map_err(|error| format!("reading from the SMTP server: {error}"))?;
            if read == 0 {
                return Err("the SMTP server closed the connection".to_string());
            }
            let code = line
                .get(..3)
                .and_then(|digits| digits.parse::<u16>().ok())
                .ok_or_else(|| format!("the SMTP server said {:?}", line.trim_end()))?;
            let more = line.as_bytes().get(3) == Some(&b'-');
            lines.push(line.get(4..).unwrap_or_default().trim_end().to_string());
            if !more {
                return Ok((code, lines));
            }
        }
    }

    async fn expect_reply(&mut self, class: u16) -> Result<Vec<String>, String> {
        let (code, lines) = self.reply().await?;
        if code / 100 != class {
            return Err(format!(
                "the SMTP server answered {code} {}",
                lines.join(" ")
            ));
        }
        Ok(lines)
    }

    async fn write(&mut self, text: &str) -> Result<(), String> {
        let stream = self.stream.get_mut();
        stream
            .write_all(text.as_bytes())
            .await
            .map_err(|error| format!("writing to the SMTP server: {error}"))?;
        stream
            .flush()
            .await
            .map_err(|error| format!("writing to the SMTP server: {error}"))
    }

    async fn command(&mut self, line: &str, class: u16) -> Result<Vec<String>, String> {
        self.write(&format!("{line}\r\n")).await?;
        self.expect_reply(class).await
    }

    async fn hello(&mut self, helo: &str) -> Result<(), String> {
        self.command(&format!("EHLO {helo}"), 2).await.map(drop)
    }

    async fn transact(
        &mut self,
        settings: &SmtpSettings,
        helo: &str,
        mail: &Mail,
    ) -> Result<(), String> {
        if let (Some(user), Some(password)) = (&settings.user, &settings.password) {
            let credentials = STANDARD.encode(format!("\0{user}\0{password}"));
            self.command(&format!("AUTH PLAIN {credentials}"), 2)
                .await?;
        }
        self.command(&format!("MAIL FROM:<{}>", settings.from), 2)
            .await?;
        self.command(&format!("RCPT TO:<{}>", mail.to), 2).await?;
        self.command("DATA", 3).await?;
        self.write(&message(&settings.from, helo, mail, crate::now()))
            .await?;
        self.expect_reply(2).await?;
        let _ = self.command("QUIT", 2).await;
        Ok(())
    }
}

fn message(from: &str, helo: &str, mail: &Mail, at: u64) -> String {
    let id = URL_SAFE_NO_PAD.encode(crate::auth::random::<12>());
    let mut text = format!(
        "From: {from}\r\nTo: {}\r\nSubject: {}\r\nDate: {}\r\nMessage-ID: <{id}@{helo}>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n",
        mail.to,
        mail.subject,
        date(at)
    );
    for line in mail.text.lines() {
        if line.starts_with('.') {
            text.push('.');
        }
        text.push_str(line);
        text.push_str("\r\n");
    }
    text.push_str(".\r\n");
    text
}

fn date(at: u64) -> String {
    let days = at / 86_400;
    let seconds = at % 86_400;
    let weekday = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][(days % 7) as usize];
    let shifted = days as i64 + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    let month_name = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ][(month - 1) as usize];
    format!(
        "{weekday}, {day} {month_name} {year} {:02}:{:02}:{:02} +0000",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_reads_as_rfc_5322() {
        assert_eq!(date(0), "Thu, 1 Jan 1970 00:00:00 +0000");
        assert_eq!(date(1_792_800_000), "Sat, 24 Oct 2026 00:00:00 +0000");
    }

    #[test]
    fn a_body_line_that_starts_with_a_dot_is_stuffed() {
        let mail = Mail {
            to: "carl@example.com".to_string(),
            subject: "s".to_string(),
            text: "one\n.two".to_string(),
        };
        assert!(
            message("hub@example.com", "hub.example.com", &mail, 0)
                .ends_with("one\r\n..two\r\n.\r\n")
        );
    }

    #[test]
    fn only_plain_addresses_pass() {
        assert!(plausible_address("carl@example.com"));
        for refused in [
            "carl",
            "carl@localhost",
            "a b@example.com",
            "carl@example.com\r\nBcc: x@y.z",
            "<a>@b.c",
            "a@b@c.d",
        ] {
            assert!(!plausible_address(refused), "{refused:?}");
        }
    }
}
