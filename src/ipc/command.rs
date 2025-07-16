use std::collections::HashMap;
use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub enum Command {
    AddSynchronousService {
        name: String,
        working_directory: String,
        environment: HashMap<String, String>,
        group: Option<String>,

        command: Vec<String>,
    },
    AddAsynchronousService {
        name: String,
        working_directory: String,
        environment: HashMap<String, String>,
        group: Option<String>,

        start_command: Vec<String>,
        stop_command: Vec<String>,
    },
    RemoveService {
        name: String,
    },

    StartService {
        name: String,
    },
    StopService {
        name: String,
    },
    RestartService {
        name: String,
    },

    GetServiceStatus {
        name: String,
    },
    ListServices,
}

impl Command {
    pub fn read_from_stream<T: Read>(stream: &mut T) -> io::Result<Option<Command>> {
        super::read_from_stream(stream)
    }

    pub fn write_to_stream<T: Write>(&self, stream: &mut T) -> io::Result<()> {
        super::write_to_stream(self, stream)
    }
}
