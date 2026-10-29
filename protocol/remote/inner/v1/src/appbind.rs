//! Candidate bootstrap DTOs only: parsing is not endpoint/session authorization.
use serde::{Deserialize, Serialize};
pub const MAX_JSON: usize = 16384;
pub const MAX_ORIGIN: usize = 2048;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodecError {
    Length,
    Depth,
    Json,
    Canonical,
    Field,
    Unsupported,
    Mismatch,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct CanonicalOrigin(String);
impl CanonicalOrigin {
    pub fn trusted(value: &str) -> Result<Self, CodecError> {
        if value.len() > MAX_ORIGIN
            || value
                .bytes()
                .any(|b| !(0x20..=0x7e).contains(&b) || b"@%\\\"".contains(&b))
        {
            return Err(CodecError::Field);
        }
        let url = url::Url::parse(value).map_err(|_| CodecError::Field)?;
        if url.scheme() != "wss"
            || url.host().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !url
                .path()
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/._~-".contains(&b))
        {
            return Err(CodecError::Field);
        }
        let serialized = url.to_string();
        if serialized.len() > MAX_ORIGIN {
            return Err(CodecError::Field);
        }
        Ok(Self(serialized))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for CanonicalOrigin {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        let o = Self::trusted(&s).map_err(|_| serde::de::Error::custom("origin"))?;
        if o.0 != s {
            return Err(serde::de::Error::custom("canonical origin"));
        }
        Ok(o)
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Mac(String);
impl std::fmt::Debug for Mac {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Mac(<redacted>)")
    }
}
impl Mac {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes.iter().map(|b| format!("{b:02x}")).collect())
    }
    pub fn bytes(&self) -> Result<[u8; 32], CodecError> {
        let mut b = [0; 32];
        if !hex32(&self.0) {
            return Err(CodecError::Field);
        }
        for (i, v) in b.iter_mut().enumerate() {
            *v =
                u8::from_str_radix(&self.0[i * 2..i * 2 + 2], 16).map_err(|_| CodecError::Field)?;
        }
        Ok(b)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Context {
    pub host_endpoint_id: String,
    pub client_endpoint_id: String,
    pub host_key_epoch: String,
    pub client_key_epoch: String,
    pub activation: String,
    pub incarnation: String,
    pub policy_epoch: String,
    pub issuer_epoch: String,
    pub origin: CanonicalOrigin,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestedRole {
    ManagementOnly,
    Observer,
    Controller,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostOffer {
    pub context: Context,
    pub host_nonce: String,
    pub profile: u16,
    pub major: u16,
    #[serde(deserialize_with = "bounded_vec")]
    pub minors: Vec<u16>,
    pub minimum_security: String,
    #[serde(deserialize_with = "bounded_vec")]
    pub required_capabilities: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientOffer {
    pub context: Context,
    pub host_nonce: String,
    pub client_nonce: String,
    pub profile: u16,
    pub major: u16,
    #[serde(deserialize_with = "bounded_vec")]
    pub minors: Vec<u16>,
    pub minimum_security: String,
    #[serde(deserialize_with = "bounded_vec")]
    pub required_capabilities: Vec<String>,
    pub requested_role: RequestedRole,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostSelection {
    pub profile: u16,
    pub major: u16,
    pub minor: u16,
    pub security_version: String,
    pub requested_role: RequestedRole,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostConfirm {
    pub selection: HostSelection,
    pub mac: Mac,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientConfirm {
    pub mac: Mac,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "message", content = "body", deny_unknown_fields)]
pub enum Message {
    HostOffer(HostOffer),
    ClientOffer(ClientOffer),
    HostConfirm(HostConfirm),
    ClientConfirm(ClientConfirm),
}
fn bounded_vec<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Bounded<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Bounded<T> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("at most eight entries")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut a: A,
        ) -> Result<Self::Value, A::Error> {
            let mut values = Vec::new();
            while let Some(v) = a.next_element()? {
                if values.len() == 8 {
                    return Err(serde::de::Error::custom("array limit"));
                }
                values.push(v);
            }
            Ok(values)
        }
    }
    d.deserialize_seq(Bounded(std::marker::PhantomData))
}
fn hex32(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn decimal(value: &str) -> Result<u64, CodecError> {
    if value.is_empty()
        || value.len() > 20
        || !value.bytes().all(|b| b.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err(CodecError::Field);
    }
    value.parse().map_err(|_| CodecError::Field)
}
fn context(c: &Context) -> Result<(), CodecError> {
    if !hex32(&c.host_endpoint_id) || !hex32(&c.client_endpoint_id) || !hex32(&c.activation) {
        return Err(CodecError::Field);
    }
    for n in [
        &c.host_key_epoch,
        &c.client_key_epoch,
        &c.incarnation,
        &c.policy_epoch,
        &c.issuer_epoch,
    ] {
        decimal(n)?;
    }
    if CanonicalOrigin::trusted(c.origin.as_str())?.as_str() != c.origin.as_str() {
        return Err(CodecError::Field);
    }
    Ok(())
}
fn offers(minors: &[u16], floor: &str, caps: &[String]) -> Result<(), CodecError> {
    decimal(floor)?;
    if minors.len() > 8
        || minors.windows(2).any(|w| w[0] >= w[1])
        || caps.len() > 8
        || caps.windows(2).any(|w| w[0] >= w[1])
        || caps.iter().any(|s| {
            s.is_empty()
                || s.len() > 32
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
    {
        return Err(CodecError::Field);
    }
    Ok(())
}
fn host(h: &HostOffer) -> Result<(), CodecError> {
    context(&h.context)?;
    if !hex32(&h.host_nonce) {
        return Err(CodecError::Field);
    }
    offers(&h.minors, &h.minimum_security, &h.required_capabilities)
}
fn client(c: &ClientOffer) -> Result<(), CodecError> {
    context(&c.context)?;
    if !hex32(&c.host_nonce) || !hex32(&c.client_nonce) {
        return Err(CodecError::Field);
    }
    offers(&c.minors, &c.minimum_security, &c.required_capabilities)
}
fn selection(s: &HostSelection) -> Result<(), CodecError> {
    decimal(&s.security_version)?;
    Ok(())
}
fn valid(m: &Message) -> Result<(), CodecError> {
    match m {
        Message::HostOffer(h) => host(h),
        Message::ClientOffer(c) => client(c),
        Message::HostConfirm(h) => {
            selection(&h.selection)?;
            if !hex32(&h.mac.0) {
                return Err(CodecError::Field);
            }
            Ok(())
        }
        Message::ClientConfirm(c) => {
            if hex32(&c.mac.0) {
                Ok(())
            } else {
                Err(CodecError::Field)
            }
        }
    }
}
fn depth(bytes: &[u8]) -> Result<(), CodecError> {
    let (mut in_string, mut escaped, mut depth) = (false, false, 0usize);
    for b in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if *b == b'\\' {
                escaped = true;
            } else if *b == b'"' {
                in_string = false;
            }
        } else {
            match *b {
                b'"' => in_string = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > 16 {
                        return Err(CodecError::Depth);
                    }
                }
                b'}' | b']' => {
                    depth = depth.checked_sub(1).ok_or(CodecError::Json)?;
                }
                _ => {}
            }
        }
    }
    Ok(())
}
fn sorted(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<_> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let mut out = serde_json::Map::new();
            for (k, v) in entries {
                out.insert(k, sorted(v));
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(a) => {
            serde_json::Value::Array(a.into_iter().map(sorted).collect())
        }
        v => v,
    }
}
fn canonical<T: Serialize>(value: &T) -> Result<Vec<u8>, CodecError> {
    let value = serde_json::to_value(value).map_err(|_| CodecError::Json)?;
    let bytes = serde_json::to_vec(&sorted(value)).map_err(|_| CodecError::Json)?;
    if bytes.len() > MAX_JSON {
        return Err(CodecError::Length);
    }
    Ok(bytes)
}
pub fn decode(bytes: &[u8]) -> Result<Message, CodecError> {
    if bytes.is_empty() || bytes.len() > MAX_JSON {
        return Err(CodecError::Length);
    }
    depth(bytes)?;
    // Direct typed deserialization: duplicate fields must not be collapsed through Value.
    let message: Message = serde_json::from_slice(bytes).map_err(|_| CodecError::Json)?;
    valid(&message)?;
    if encode(&message)? != bytes {
        return Err(CodecError::Canonical);
    }
    Ok(message)
}
pub fn encode(message: &Message) -> Result<Vec<u8>, CodecError> {
    valid(message)?;
    canonical(message)
}
pub fn encode_selection(s: &HostSelection) -> Result<Vec<u8>, CodecError> {
    selection(s)?;
    canonical(s)
}
pub fn transcript(
    h: &HostOffer,
    c: &ClientOffer,
    s: &HostSelection,
) -> Result<Vec<u8>, CodecError> {
    host(h)?;
    client(c)?;
    selection(s)?;
    let parts = [
        encode(&Message::HostOffer(h.clone()))?,
        encode(&Message::ClientOffer(c.clone()))?,
        encode_selection(s)?,
    ];
    let mut bytes = b"CC-DESK-APPBIND-v1\0".to_vec();
    for part in parts {
        bytes.extend_from_slice(&(part.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&part);
    }
    Ok(bytes)
}
pub fn negotiate(h: &HostOffer, c: &ClientOffer) -> Result<HostSelection, CodecError> {
    host(h)?;
    client(c)?;
    if h.context != c.context || h.host_nonce != c.host_nonce || h.host_nonce == c.client_nonce {
        return Err(CodecError::Mismatch);
    }
    if h.profile != 1
        || c.profile != 1
        || h.major != 1
        || c.major != 1
        || c.requested_role != RequestedRole::ManagementOnly
        || decimal(&h.minimum_security)? > 1
        || decimal(&c.minimum_security)? > 1
        || !h.minors.contains(&0)
        || !c.minors.contains(&0)
        || h.required_capabilities.as_slice() != ["canonical-json"]
        || c.required_capabilities.as_slice() != ["canonical-json"]
        || h.required_capabilities
            .iter()
            .chain(c.required_capabilities.iter())
            .any(|s| s != "canonical-json")
    {
        return Err(CodecError::Unsupported);
    }
    Ok(HostSelection {
        profile: 1,
        major: 1,
        minor: 0,
        security_version: "1".into(),
        requested_role: RequestedRole::ManagementOnly,
    })
}
