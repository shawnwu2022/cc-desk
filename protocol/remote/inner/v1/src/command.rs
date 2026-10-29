use crate::{input, projection};
pub enum Command {
    Snapshot(u64),
    Window(u64),
    Disconnect(u64),
    Input(input::Input),
    Ack(Box<projection::Ack>),
}
#[derive(Debug)]
pub struct Error;
pub fn decode(bytes: &[u8]) -> Result<Command, Error> {
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct Tag {
        message: String,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        message: String,
        profile: u16,
        request: String,
    }
    if bytes.len() > input::MAX_PAYLOAD {
        return Err(Error);
    }
    let tag: Tag = serde_json::from_slice(bytes).map_err(|_| Error)?;
    match tag.message.as_str() {
        "InputChunk" => input::decode(bytes).map(Command::Input).map_err(|_| Error),
        "ProjectionAck" => projection::decode_ack(bytes, 1)
            .map(|ack| Command::Ack(Box::new(ack)))
            .map_err(|_| Error),
        "Snapshot" | "Window" | "Disconnect" => {
            let r: Request = serde_json::from_slice(bytes).map_err(|_| Error)?;
            let n = r.request.parse::<u64>().map_err(|_| Error)?;
            if r.profile != 1 || n == 0 || n.to_string() != r.request {
                return Err(Error);
            }
            match r.message.as_str() {
                "Snapshot" => Ok(Command::Snapshot(n)),
                "Window" => Ok(Command::Window(n)),
                "Disconnect" => Ok(Command::Disconnect(n)),
                _ => Err(Error),
            }
        }
        _ => Err(Error),
    }
}
