use std::collections::HashMap;
use std::fmt;
use std::io::{self, BufReader, Read};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{process, thread};

use nix::sys::signal::{self, Signal};
use nix::unistd;

use serde::de::{Deserializer, MapAccess, Visitor};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize};

struct Command<W: fmt::Write> {
    child: Arc<Mutex<process::Child>>,
    logs: Arc<Mutex<W>>,
}

impl<W: fmt::Write + Send + 'static> Command<W> {
    fn start(
        command: &[&str],
        working_directory: &str,
        environment_overrides: HashMap<String, String>,
        output: Arc<Mutex<W>>,
    ) -> io::Result<Self> {
        let mut environment = HashMap::<String, String>::new();
        for (key, value) in std::env::vars() {
            environment.insert(key.to_string(), value.to_string());
        }

        for (key, value) in environment_overrides {
            environment.insert(key, value);
        }

        let child = process::Command::new(command[0])
            .args(&command[1..])
            .current_dir(working_directory)
            .envs(environment)
            .stdin(process::Stdio::null())
            .stdout(process::Stdio::piped())
            .stderr(process::Stdio::piped())
            .spawn()?;
        let command = Self {
            child: Arc::new(Mutex::new(child)),
            logs: output,
        };

        let stdout_thread_output = command.logs.clone();
        let stdout_thread_child = command.child.clone();
        thread::spawn(move || {
            let stdout = match stdout_thread_child.lock().unwrap().stdout.take() {
                Some(stdout) => stdout,
                None => return,
            };
            let mut reader = BufReader::new(stdout);

            let mut chunk = [0u8; 16];
            while let Ok(bytes_read) = reader.read(&mut chunk) {
                if bytes_read == 0 {
                    break;
                }
                let chunk = &chunk[..bytes_read];

                let mut output = stdout_thread_output.lock().unwrap();
                let _ = write!(output, "{}", String::from_utf8_lossy(chunk));
            }
        });

        let stderr_thread_output = command.logs.clone();
        let stderr_thread_child = command.child.clone();
        thread::spawn(move || {
            let stderr = match stderr_thread_child.lock().unwrap().stderr.take() {
                Some(stderr) => stderr,
                None => return,
            };
            let mut reader = BufReader::new(stderr);

            let mut chunk = [0u8; 16];
            while let Ok(bytes_read) = reader.read(&mut chunk) {
                if bytes_read == 0 {
                    break;
                }
                let chunk = &chunk[..bytes_read];

                let mut output = stderr_thread_output.lock().unwrap();
                let _ = write!(output, "{}", String::from_utf8_lossy(chunk));
            }
        });

        Ok(command)
    }

    fn stop(&self) -> io::Result<()> {
        let mut child = self.child.lock().unwrap();
        let child_pid = unistd::Pid::from_raw(child.id() as i32);

        'kill_attempt: for _ in 0..5 {
            signal::kill(child_pid, Signal::SIGINT)?;

            let timeout = Duration::from_secs(30);
            let deadline = Instant::now() + timeout;
            while match child.try_wait() {
                Ok(None) => true,
                _ => break 'kill_attempt,
            } {
                if Instant::now() > deadline {
                    break;
                }
                thread::sleep(timeout / 15);
            }
        }

        child.kill()
    }

    fn wait(&self) -> io::Result<process::ExitStatus> {
        let mut child = self.child.lock().unwrap();
        child.wait()
    }
}

pub enum ServiceError {
    IOError(io::Error),
    ServiceNotRunning,
    ServiceAlreadyRunning,
}

impl fmt::Display for ServiceError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            Self::IOError(err) => err.fmt(fmt),
            Self::ServiceNotRunning => write!(fmt, "service not running"),
            Self::ServiceAlreadyRunning => write!(fmt, "service already running"),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub enum ServiceKind {
    Synchronous {
        command: Vec<String>,
    },
    Asynchronous {
        start_command: Vec<String>,
        stop_command: Vec<String>,
    },
}

pub struct Service {
    pub working_directory: String,
    pub environment: HashMap<String, String>,
    pub group: Option<String>,
    pub kind: ServiceKind,

    async_running: bool,
    child: Option<Command<String>>,
    logs: Arc<Mutex<String>>,
}

impl Serialize for Service {
    fn serialize<S: serde::ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("Service", 3)?;
        s.serialize_field("working_directory", &self.working_directory)?;
        s.serialize_field("environment", &self.environment)?;
        s.serialize_field("group", &self.group)?;
        s.serialize_field("kind", &self.kind)?;
        s.end()
    }
}

impl<'de> Deserialize<'de> for Service {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ServiceVisitor;

        impl<'de> Visitor<'de> for ServiceVisitor {
            type Value = Service;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct Service")
            }

            fn visit_map<V>(self, mut map: V) -> Result<Service, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut working_directory = None;
                let mut environment = None;
                let mut kind = None;
                let mut group = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        "working_directory" => {
                            if working_directory.is_some() {
                                return Err(serde::de::Error::duplicate_field("working_directory"));
                            }
                            working_directory = Some(map.next_value()?);
                        }
                        "environment" => {
                            if environment.is_some() {
                                return Err(serde::de::Error::duplicate_field("environment"));
                            }
                            environment = Some(map.next_value()?);
                        }
                        "group" => {
                            if group.is_some() {
                                return Err(serde::de::Error::duplicate_field("group"));
                            }
                            group = Some(map.next_value()?);
                        }
                        "kind" => {
                            if kind.is_some() {
                                return Err(serde::de::Error::duplicate_field("kind"));
                            }
                            kind = Some(map.next_value()?);
                        }
                        field => {
                            return Err(serde::de::Error::unknown_field(
                                field,
                                &["working_directory", "environment", "group", "kind"],
                            ));
                        }
                    }
                }

                let working_directory = working_directory
                    .ok_or_else(|| serde::de::Error::missing_field("working_directory"))?;
                let environment =
                    environment.ok_or_else(|| serde::de::Error::missing_field("environment"))?;
                let group = group.ok_or_else(|| serde::de::Error::missing_field("group"))?;
                let kind = kind.ok_or_else(|| serde::de::Error::missing_field("kind"))?;

                Ok(Service::new(working_directory, environment, group, kind))
            }
        }

        deserializer.deserialize_struct(
            "Service",
            &["working_directory", "environment", "kind", "group"],
            ServiceVisitor,
        )
    }
}

impl Service {
    pub fn new(
        working_directory: String,
        environment: HashMap<String, String>,
        group: Option<String>,
        kind: ServiceKind,
    ) -> Self {
        Self {
            working_directory,
            environment,
            group,
            kind,

            async_running: false,
            child: None,
            logs: Arc::new(Mutex::new(String::new())),
        }
    }

    fn start_synchronous(&mut self, command: Vec<String>) -> Result<(), ServiceError> {
        self.child = Some(
            match Command::start(
                command
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<&str>>()
                    .as_slice(),
                &self.working_directory,
                self.environment.clone(),
                self.logs.clone(),
            ) {
                Ok(command) => command,
                Err(err) => return Err(ServiceError::IOError(err)),
            },
        );
        Ok(())
    }

    fn start_asynchronous(&mut self, start_command: Vec<String>) -> Result<(), ServiceError> {
        let command = match Command::start(
            start_command
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<&str>>()
                .as_slice(),
            &self.working_directory,
            self.environment.clone(),
            self.logs.clone(),
        ) {
            Ok(command) => command,
            Err(err) => return Err(ServiceError::IOError(err)),
        };
        if let Err(err) = command.wait() {
            return Err(ServiceError::IOError(err));
        }

        self.async_running = true;
        Ok(())
    }

    pub fn start(&mut self) -> Result<(), ServiceError> {
        if self.is_running() {
            return Err(ServiceError::ServiceAlreadyRunning);
        }

        match &self.kind {
            ServiceKind::Synchronous { command } => self.start_synchronous(command.clone())?,
            ServiceKind::Asynchronous { start_command, .. } => {
                self.start_asynchronous(start_command.clone())?;
            }
        }

        Ok(())
    }

    fn stop_synchronous(&mut self) -> Result<(), ServiceError> {
        let child = match &self.child {
            Some(child) => child,
            None => return Err(ServiceError::ServiceNotRunning),
        };
        if let Err(err) = child.stop() {
            return Err(ServiceError::IOError(err));
        }
        self.child = None;
        Ok(())
    }

    fn stop_asynchronous(&mut self, stop_command: Vec<String>) -> Result<(), ServiceError> {
        let command = match Command::start(
            stop_command
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<&str>>()
                .as_slice(),
            &self.working_directory,
            self.environment.clone(),
            self.logs.clone(),
        ) {
            Ok(command) => command,
            Err(err) => return Err(ServiceError::IOError(err)),
        };
        if let Err(err) = command.wait() {
            return Err(ServiceError::IOError(err));
        }

        self.async_running = false;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), ServiceError> {
        if !self.is_running() {
            return Err(ServiceError::ServiceNotRunning);
        }

        match &self.kind {
            ServiceKind::Synchronous { .. } => self.stop_synchronous()?,
            ServiceKind::Asynchronous { stop_command, .. } => {
                self.stop_asynchronous(stop_command.clone())?;
            }
        }

        Ok(())
    }

    pub fn restart(&mut self) -> Result<(), ServiceError> {
        self.stop()?;
        self.start()
    }

    pub fn is_running(&self) -> bool {
        match self.kind {
            ServiceKind::Synchronous { .. } => self.child.is_some(),
            ServiceKind::Asynchronous { .. } => self.async_running,
        }
    }

    pub fn get_logs(&self) -> String {
        self.logs.clone().lock().unwrap().clone()
    }
}
