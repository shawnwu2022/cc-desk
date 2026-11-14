use crate::{BrowserProfile, BrowserResumeGrantTrust, ClientGrantTrust, GrantError, P256Spki};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use serde::Deserialize;
use std::time::Duration;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Enrollment {
    origin: String,
    rp_id: String,
    credential_id: String,
    credential_spki: String,
    backup_eligible: bool,
    allow_backed_up: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Wire {
    issuer_spki: String,
    principal: String,
    #[serde(default, deserialize_with = "present")]
    scope: Option<cc_desk_remote_record::grant::SessionScope>,
    #[serde(default, deserialize_with = "present")]
    scope_policy: Option<ScopePolicy>,
    role: cc_desk_remote_record::grant::DataRole,
    budget_ms: u64,
    profile: Enrollment,
    #[serde(default, deserialize_with = "peer_present")]
    peer_install: Option<PeerInstall>,
}
#[derive(Deserialize)]
enum ScopePolicy {
    #[serde(rename = "authenticated-host-run")]
    AuthenticatedHostRun,
}
fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PeerInstall {
    budget_ms: u64,
}
fn peer_present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<PeerInstall>, D::Error> {
    PeerInstall::deserialize(d).map(Some)
}
fn decode(value: &str, max: usize) -> Result<Vec<u8>, GrantError> {
    if value.len() > max.div_ceil(3) * 4 {
        return Err(GrantError::Bounds);
    }
    let b = B64.decode(value).map_err(|_| GrantError::Encoding)?;
    if b.len() > max || B64.encode(&b) != value {
        return Err(GrantError::Encoding);
    }
    Ok(b)
}
pub(super) enum Trust {
    Exact(ClientGrantTrust),
    AuthenticatedHostRun(BrowserResumeGrantTrust),
}
pub(super) struct Configuration {
    pub trust: Trust,
    pub profile: BrowserProfile,
    pub issuer: P256Spki,
    pub budget: Duration,
    pub peer_budget: Option<Duration>,
}
impl Configuration {
    pub fn parse(bytes: &[u8]) -> Result<Self, GrantError> {
        if bytes.is_empty() || bytes.len() > 8192 {
            return Err(GrantError::Bounds);
        }
        let w: Wire = serde_json::from_slice(bytes).map_err(|_| GrantError::Encoding)?;
        let peer_budget = match w.peer_install {
            Some(p) if p.budget_ms > 0 && p.budget_ms <= 10000 => {
                Some(Duration::from_millis(p.budget_ms))
            }
            Some(_) => return Err(GrantError::Policy),
            None => None,
        };
        if w.budget_ms == 0
            || w.budget_ms > 30000
            || w.principal.len() != 64
            || !w
                .principal
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || w.principal.bytes().all(|b| b == b'0')
        {
            return Err(GrantError::Policy);
        }
        match (&w.scope, &w.scope_policy) {
            (Some(scope), None) => {
                cc_desk_remote_record::grant::encode_scope(scope)
                    .map_err(|_| GrantError::Policy)?;
            }
            (None, Some(ScopePolicy::AuthenticatedHostRun)) => {}
            _ => return Err(GrantError::Policy),
        }
        let principal: Vec<_> = (0..64)
            .step_by(2)
            .map(|i| u8::from_str_radix(&w.principal[i..i + 2], 16).map_err(|_| GrantError::Policy))
            .collect::<Result<_, _>>()?;
        let issuer = P256Spki::new(&decode(&w.issuer_spki, 91)?).map_err(|_| GrantError::Policy)?;
        let key = P256Spki::new(&decode(&w.profile.credential_spki, 91)?)
            .map_err(|_| GrantError::Policy)?;
        let profile = BrowserProfile::enrolled(
            &w.profile.origin,
            &w.profile.rp_id,
            decode(&w.profile.credential_id, 1024)?,
            key,
            w.profile.backup_eligible,
            w.profile.allow_backed_up,
        )?;
        let principal = principal.try_into().map_err(|_| GrantError::Policy)?;
        let trust = match w.scope {
            Some(scope) => Trust::Exact(ClientGrantTrust {
                issuer: issuer.clone(),
                principal,
                scope,
                role: w.role,
            }),
            None => Trust::AuthenticatedHostRun(BrowserResumeGrantTrust {
                issuer: issuer.clone(),
                principal,
                role: w.role,
            }),
        };
        Ok(Self {
            trust,
            profile,
            issuer,
            budget: Duration::from_millis(w.budget_ms),
            peer_budget,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn exact() -> serde_json::Value {
        serde_json::json!({
            "issuerSpki": B64.encode(crate::grant_tests::admin().0),
            "principal": "04".repeat(32),
            "scope": crate::grant_tests::scope(), "role": "Controller", "budgetMs": 1000,
            "profile": {"origin": "https://desk.invalid", "rpId": "desk.invalid", "credentialId": B64.encode([7; 32]), "credentialSpki": B64.encode(crate::grant_tests::admin().0), "backupEligible": false, "allowBackedUp": false},
            "peerInstall": { "budgetMs": 1000 }
        })
    }
    fn parse(value: &serde_json::Value) -> Result<Configuration, GrantError> {
        Configuration::parse(&serde_json::to_vec(value).unwrap())
    }
    #[test]
    fn browser_resume_wire_requires_explicit_exclusive_scope_policy_005() {
        let exact = exact();
        let parsed = parse(&exact).unwrap();
        assert_eq!(parsed.budget, Duration::from_secs(1));
        assert_eq!(parsed.peer_budget, Some(Duration::from_secs(1)));
        assert_eq!(parsed.issuer.0, crate::grant_tests::admin().0);
        let _ = parsed.profile;
        assert!(matches!(parsed.trust, Trust::Exact(t) if t.scope == crate::grant_tests::scope()));
        let mut resume = exact.clone();
        resume.as_object_mut().unwrap().remove("scope");
        resume["scopePolicy"] = "authenticated-host-run".into();
        assert!(
            matches!(parse(&resume).unwrap().trust, Trust::AuthenticatedHostRun(t) if t.principal == [4;32])
        );
        for case in 0..8 {
            let mut invalid = resume.clone();
            match case {
                0 => {
                    invalid.as_object_mut().unwrap().remove("scopePolicy");
                }
                1 => invalid["scope"] = exact["scope"].clone(),
                2 => invalid["scope"] = serde_json::Value::Null,
                3 => invalid["scopePolicy"] = serde_json::Value::Null,
                4 => invalid["scopePolicy"] = "wildcard".into(),
                5 => invalid["scopePolicy"] = "".into(),
                6 => {
                    invalid = exact.clone();
                    invalid["scopePolicy"] = serde_json::Value::Null;
                }
                _ => {
                    invalid = exact.clone();
                    invalid["scope"] = serde_json::Value::Null;
                }
            }
            assert!(parse(&invalid).is_err(), "case {case}");
        }
    }
}
