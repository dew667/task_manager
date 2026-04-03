#![allow(dead_code)]

#[cfg(feature = "server")]
mod inner {
    use dioxus::prelude::ServerFnError;
    use std::fmt;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf};
    use tokio::net::TcpStream;
    use tokio::sync::{Mutex, MutexGuard, OnceCell};
    use tracing::{debug, warn};

    // ── Error ──────────────────────────────────────────────────────────

    #[derive(Debug)]
    pub enum RespError {
        Io(std::io::Error),
        Protocol(String),
        Redis(String),
        Config(String),
    }

    impl fmt::Display for RespError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                RespError::Io(e) => write!(f, "IO error: {e}"),
                RespError::Protocol(msg) => write!(f, "Protocol error: {msg}"),
                RespError::Redis(msg) => write!(f, "Redis error: {msg}"),
                RespError::Config(msg) => write!(f, "Config error: {msg}"),
            }
        }
    }

    impl std::error::Error for RespError {}

    impl From<std::io::Error> for RespError {
        fn from(e: std::io::Error) -> Self {
            RespError::Io(e)
        }
    }

    impl From<RespError> for ServerFnError {
        fn from(e: RespError) -> Self {
            ServerFnError::new(e.to_string())
        }
    }

    // ── Config ─────────────────────────────────────────────────────────

    #[derive(serde::Deserialize, Debug, Clone)]
    pub struct RedisConfig {
        #[serde(default = "default_host")]
        pub host: String,
        #[serde(default = "default_port")]
        pub port: u16,
        pub password: Option<String>,
        #[serde(default)]
        pub db: u8,
    }

    fn default_host() -> String {
        "127.0.0.1".to_string()
    }
    fn default_port() -> u16 {
        6379
    }

    impl Default for RedisConfig {
        fn default() -> Self {
            Self {
                host: default_host(),
                port: default_port(),
                password: None,
                db: 0,
            }
        }
    }

    impl RedisConfig {
        pub fn load() -> Result<Self, RespError> {
            match std::fs::read_to_string("redis.json") {
                Ok(content) => serde_json::from_str(&content)
                    .map_err(|e| RespError::Config(format!("Failed to parse redis.json: {e}"))),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    warn!("redis.json not found, using default config (127.0.0.1:6379)");
                    Ok(Self::default())
                }
                Err(e) => Err(RespError::Config(format!("Failed to read redis.json: {e}"))),
            }
        }
    }

    // ── RESP Value ─────────────────────────────────────────────────────

    #[derive(Debug, Clone)]
    pub enum RespValue {
        SimpleString(String),
        Error(String),
        Integer(i64),
        BulkString(Option<Vec<u8>>),
        Array(Option<Vec<RespValue>>),
    }

    impl RespValue {
        pub fn as_string(&self) -> Option<String> {
            match self {
                RespValue::SimpleString(s) => Some(s.clone()),
                RespValue::BulkString(Some(data)) => String::from_utf8(data.clone()).ok(),
                _ => None,
            }
        }

        pub fn as_integer(&self) -> Option<i64> {
            match self {
                RespValue::Integer(n) => Some(*n),
                _ => None,
            }
        }

        pub fn as_array(&self) -> Option<&Vec<RespValue>> {
            match self {
                RespValue::Array(Some(arr)) => Some(arr),
                _ => None,
            }
        }

        pub fn is_null(&self) -> bool {
            matches!(
                self,
                RespValue::BulkString(None) | RespValue::Array(None)
            )
        }

        pub fn encode(&self) -> Vec<u8> {
            let mut buf = Vec::new();
            self.encode_into(&mut buf);
            buf
        }

        fn encode_into(&self, buf: &mut Vec<u8>) {
            match self {
                RespValue::SimpleString(s) => {
                    buf.push(b'+');
                    buf.extend_from_slice(s.as_bytes());
                    buf.extend_from_slice(b"\r\n");
                }
                RespValue::Error(s) => {
                    buf.push(b'-');
                    buf.extend_from_slice(s.as_bytes());
                    buf.extend_from_slice(b"\r\n");
                }
                RespValue::Integer(n) => {
                    buf.push(b':');
                    buf.extend_from_slice(n.to_string().as_bytes());
                    buf.extend_from_slice(b"\r\n");
                }
                RespValue::BulkString(None) => {
                    buf.extend_from_slice(b"$-1\r\n");
                }
                RespValue::BulkString(Some(data)) => {
                    buf.push(b'$');
                    buf.extend_from_slice(data.len().to_string().as_bytes());
                    buf.extend_from_slice(b"\r\n");
                    buf.extend_from_slice(data);
                    buf.extend_from_slice(b"\r\n");
                }
                RespValue::Array(None) => {
                    buf.extend_from_slice(b"*-1\r\n");
                }
                RespValue::Array(Some(items)) => {
                    buf.push(b'*');
                    buf.extend_from_slice(items.len().to_string().as_bytes());
                    buf.extend_from_slice(b"\r\n");
                    for item in items {
                        item.encode_into(buf);
                    }
                }
            }
        }

        pub fn decode<R: AsyncBufReadExt + Unpin + Send>(
            reader: &mut R,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<RespValue, RespError>> + Send + '_>> {
            Box::pin(async move {
                let mut line = String::new();
                let n = reader.read_line(&mut line).await?;
                if n == 0 {
                    return Err(RespError::Protocol("Connection closed".to_string()));
                }
                let line = line.trim_end_matches("\r\n").trim_end_matches('\n');

                if line.is_empty() {
                    return Err(RespError::Protocol("Empty response line".to_string()));
                }

                let prefix = line.as_bytes()[0];
                let payload = &line[1..];

                match prefix {
                    b'+' => Ok(RespValue::SimpleString(payload.to_string())),
                    b'-' => Ok(RespValue::Error(payload.to_string())),
                    b':' => {
                        let n: i64 = payload
                            .parse()
                            .map_err(|_| RespError::Protocol(format!("Invalid integer: {payload}")))?;
                        Ok(RespValue::Integer(n))
                    }
                    b'$' => {
                        let len: i64 = payload
                            .parse()
                            .map_err(|_| RespError::Protocol(format!("Invalid bulk length: {payload}")))?;
                        if len < 0 {
                            return Ok(RespValue::BulkString(None));
                        }
                        let len = len as usize;
                        let mut data = vec![0u8; len];
                        reader.read_exact(&mut data).await?;
                        // consume trailing \r\n
                        let mut crlf = [0u8; 2];
                        reader.read_exact(&mut crlf).await?;
                        Ok(RespValue::BulkString(Some(data)))
                    }
                    b'*' => {
                        let count: i64 = payload
                            .parse()
                            .map_err(|_| RespError::Protocol(format!("Invalid array count: {payload}")))?;
                        if count < 0 {
                            return Ok(RespValue::Array(None));
                        }
                        let count = count as usize;
                        let mut items = Vec::with_capacity(count);
                        for _ in 0..count {
                            items.push(RespValue::decode(reader).await?);
                        }
                        Ok(RespValue::Array(Some(items)))
                    }
                    _ => Err(RespError::Protocol(format!(
                        "Unknown RESP prefix byte: 0x{prefix:02X}"
                    ))),
                }
            })
        }
    }

    /// Build a RESP command directly from string arguments (Array of BulkStrings)
    fn build_command(args: &[&str]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(b'*');
        buf.extend_from_slice(args.len().to_string().as_bytes());
        buf.extend_from_slice(b"\r\n");
        for arg in args {
            buf.push(b'$');
            buf.extend_from_slice(arg.len().to_string().as_bytes());
            buf.extend_from_slice(b"\r\n");
            buf.extend_from_slice(arg.as_bytes());
            buf.extend_from_slice(b"\r\n");
        }
        buf
    }

    // ── Redis Client ───────────────────────────────────────────────────

    pub struct RedisClient {
        reader: BufReader<ReadHalf<TcpStream>>,
        writer: WriteHalf<TcpStream>,
        config: RedisConfig,
    }

    impl RedisClient {
        pub async fn connect(config: &RedisConfig) -> Result<Self, RespError> {
            let addr = format!("{}:{}", config.host, config.port);
            debug!("Connecting to Redis at {addr}");
            let stream = TcpStream::connect(&addr).await?;
            let (read_half, write_half) = tokio::io::split(stream);
            let mut client = Self {
                reader: BufReader::new(read_half),
                writer: write_half,
                config: config.clone(),
            };

            // AUTH if password is set
            if let Some(ref password) = config.password {
                let reply = client.send_command(&["AUTH", password]).await?;
                if let RespValue::Error(e) = &reply {
                    return Err(RespError::Redis(format!("AUTH failed: {e}")));
                }
                debug!("Redis AUTH successful");
            }

            // SELECT database if not 0
            if config.db != 0 {
                let db_str = config.db.to_string();
                let reply = client.send_command(&["SELECT", &db_str]).await?;
                if let RespValue::Error(e) = &reply {
                    return Err(RespError::Redis(format!("SELECT failed: {e}")));
                }
                debug!("Redis SELECT db {} successful", config.db);
            }

            debug!("Redis connection established");
            Ok(client)
        }

        async fn reconnect(&mut self) -> Result<(), RespError> {
            warn!("Attempting Redis reconnection...");
            let addr = format!("{}:{}", self.config.host, self.config.port);
            let stream = TcpStream::connect(&addr).await?;
            let (read_half, write_half) = tokio::io::split(stream);
            self.reader = BufReader::new(read_half);
            self.writer = write_half;

            if let Some(password) = self.config.password.clone() {
                let reply = self.send_command_raw(&["AUTH", &password]).await?;
                if let RespValue::Error(e) = &reply {
                    return Err(RespError::Redis(format!("AUTH failed on reconnect: {e}")));
                }
            }
            if self.config.db != 0 {
                let db_str = self.config.db.to_string();
                let reply = self.send_command_raw(&["SELECT", &db_str]).await?;
                if let RespValue::Error(e) = &reply {
                    return Err(RespError::Redis(format!("SELECT failed on reconnect: {e}")));
                }
            }
            debug!("Redis reconnection successful");
            Ok(())
        }

        /// Send command without reconnect logic (used internally during connect/reconnect)
        async fn send_command_raw(&mut self, args: &[&str]) -> Result<RespValue, RespError> {
            let cmd = build_command(args);
            self.writer.write_all(&cmd).await?;
            self.writer.flush().await?;
            let reply = RespValue::decode(&mut self.reader).await?;
            if let RespValue::Error(ref e) = reply {
                return Err(RespError::Redis(e.clone()));
            }
            Ok(reply)
        }

        /// Send a command with auto-reconnect on IO error
        pub async fn send_command(&mut self, args: &[&str]) -> Result<RespValue, RespError> {
            match self.send_command_raw(args).await {
                Ok(reply) => Ok(reply),
                Err(RespError::Io(_)) => {
                    // Attempt reconnect once
                    self.reconnect().await?;
                    self.send_command_raw(args).await
                }
                Err(e) => Err(e),
            }
        }

        // ── High-level commands ────────────────────────────────────────

        pub async fn ping(&mut self) -> Result<(), RespError> {
            let reply = self.send_command(&["PING"]).await?;
            match reply {
                RespValue::SimpleString(s) if s == "PONG" => Ok(()),
                _ => Err(RespError::Protocol(format!(
                    "Unexpected PING reply: {reply:?}"
                ))),
            }
        }

        pub async fn set(&mut self, key: &str, value: &str) -> Result<(), RespError> {
            self.send_command(&["SET", key, value]).await?;
            Ok(())
        }

        pub async fn get(&mut self, key: &str) -> Result<Option<String>, RespError> {
            let reply = self.send_command(&["GET", key]).await?;
            Ok(reply.as_string())
        }

        pub async fn del(&mut self, key: &str) -> Result<i64, RespError> {
            let reply = self.send_command(&["DEL", key]).await?;
            Ok(reply.as_integer().unwrap_or(0))
        }

        pub async fn setex(
            &mut self,
            key: &str,
            seconds: u64,
            value: &str,
        ) -> Result<(), RespError> {
            let secs = seconds.to_string();
            self.send_command(&["SETEX", key, &secs, value]).await?;
            Ok(())
        }

        pub async fn expire(&mut self, key: &str, seconds: u64) -> Result<bool, RespError> {
            let secs = seconds.to_string();
            let reply = self.send_command(&["EXPIRE", key, &secs]).await?;
            Ok(reply.as_integer().unwrap_or(0) == 1)
        }

        pub async fn incr(&mut self, key: &str) -> Result<i64, RespError> {
            let reply = self.send_command(&["INCR", key]).await?;
            Ok(reply.as_integer().unwrap_or(0))
        }

        pub async fn incrby(&mut self, key: &str, increment: i64) -> Result<i64, RespError> {
            let inc = increment.to_string();
            let reply = self.send_command(&["INCRBY", key, &inc]).await?;
            Ok(reply.as_integer().unwrap_or(0))
        }

        pub async fn lpush(&mut self, key: &str, value: &str) -> Result<i64, RespError> {
            let reply = self.send_command(&["LPUSH", key, value]).await?;
            Ok(reply.as_integer().unwrap_or(0))
        }

        pub async fn lrange(
            &mut self,
            key: &str,
            start: i64,
            stop: i64,
        ) -> Result<Vec<String>, RespError> {
            let start_s = start.to_string();
            let stop_s = stop.to_string();
            let reply = self.send_command(&["LRANGE", key, &start_s, &stop_s]).await?;
            match reply.as_array() {
                Some(items) => Ok(items.iter().filter_map(|v| v.as_string()).collect()),
                None => Ok(Vec::new()),
            }
        }
    }

    // ── Global accessor ────────────────────────────────────────────────

    static REDIS: OnceCell<Mutex<RedisClient>> = OnceCell::const_new();

    pub async fn get_redis() -> Result<MutexGuard<'static, RedisClient>, RespError> {
        let mutex = REDIS
            .get_or_try_init(|| async {
                let config = RedisConfig::load()?;
                let client = RedisClient::connect(&config).await?;
                Ok::<_, RespError>(Mutex::new(client))
            })
            .await?;
        Ok(mutex.lock().await)
    }
}

#[cfg(feature = "server")]
pub use inner::*;
