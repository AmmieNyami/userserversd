use std::collections::HashMap;
use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum ResponseStatus {
    Ok,
    ServiceAlreadyExists,
    ServiceDoesNotExist,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum ResponseKind {
    None,
    ServiceStatus {
        service: super::Service,
        running: bool,
        logs: String,
    },
    ServiceList {
        services: HashMap<String, super::Service>,
    },
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Response {
    pub status: ResponseStatus,
    pub kind: ResponseKind,
}

impl Response {
    pub fn read_from_stream<T: Read>(stream: &mut T) -> io::Result<Option<Response>> {
        super::read_from_stream(stream)
    }

    pub fn write_to_stream<T: Write>(&self, stream: &mut T) -> io::Result<()> {
        super::write_to_stream(self, stream)
    }
}
