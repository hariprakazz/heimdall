use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Read,
    Write,
    Stat,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Spawn { parent: i32, child: i32 },
    Exec { pid: i32, path: String, argv: Vec<String> },
    Access { pid: i32, kind: Access, path: String, ok: bool },
    Exit { pid: i32, code: i32 },
}

pub fn encode(ev: &Event) -> Result<String> {
    Ok(serde_json::to_string(ev)?)
}

pub fn decode(line: &str) -> Result<Event> {
    Ok(serde_json::from_str(line)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_survives_a_round_trip() {
        let ev = Event::Access {
            pid: 42,
            kind: Access::Read,
            path: "/work/src/main.c".to_string(),
            ok: true,
        };
        let line = encode(&ev).unwrap();
        assert_eq!(decode(&line).unwrap(), ev);
    }

    #[test]
    fn exec_keeps_its_argv() {
        let ev = Event::Exec {
            pid: 7,
            path: "/usr/bin/gcc".to_string(),
            argv: vec!["gcc".to_string(), "-c".to_string(), "a.c".to_string()],
        };
        let line = encode(&ev).unwrap();
        assert_eq!(decode(&line).unwrap(), ev);
    }
}
