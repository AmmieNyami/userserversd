use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;

use nix::unistd;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

fn read_from_stream<S: Read, T: DeserializeOwned>(stream: &mut S) -> io::Result<Option<T>> {
    let mut bytes = Vec::<u8>::new();
    BufReader::new(stream).read_until(255, &mut bytes)?;
    if bytes.len() == 0 {
        return Ok(None);
    }
    bytes.pop();

    let data = serde_json::from_slice::<T>(bytes.as_slice())?;

    Ok(Some(data))
}

fn write_to_stream<S: Write, T: Serialize>(data: &T, stream: &mut S) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(data)?;
    bytes.push(255);
    stream.write_all(&bytes)?;
    stream.flush()
}

#[allow(dead_code)]
pub mod command;
#[allow(dead_code)]
pub mod response;

pub fn get_socket_path() -> io::Result<String> {
    let mut base_path = "";
    for path in vec!["/run", "/var/run", "/tmp"] {
        if Path::new(path).exists() {
            base_path = path;
            break;
        }
    }
    if base_path == "" {
        return Err(io::Error::from(io::ErrorKind::NotFound));
    }

    let user_path = format!("{base_path}/user/{}", unistd::getuid().as_raw());
    match fs::create_dir_all(&user_path) {
        Ok(_) => Ok(format!("{user_path}/userserversd.sock")),
        Err(_) => Ok(format!("{base_path}/userserversd.sock")),
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum ServiceKind {
    Synchronous {
        command: Vec<String>,
    },
    Asynchronous {
        start_command: Vec<String>,
        stop_command: Vec<String>,
    },
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Service {
    pub working_directory: String,
    pub environment: HashMap<String, String>,
    pub group: Option<String>,
    pub kind: ServiceKind,
}
