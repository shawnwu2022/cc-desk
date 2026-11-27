//! One-shot authority for typed session management. Read-only catalogue and
//! terminal leases cannot be exchanged for this authority.
use ring::{
    hmac,
    rand::{SecureRandom, SystemRandom},
};
use std::time::{Duration, Instant};

const TOKEN_DOMAIN: &[u8] = b"cc-desk.session-management.one-shot.v1\0";
const GRANT_LIFETIME: Duration = Duration::from_secs(30);
const MAX_ACTION_BYTES: usize = 2048;
const MAX_TITLE_BYTES: usize = 256;

/// All fields are from the desktop's current validated state, never from
/// the phone. The membership digest covers the exact confirmed peer receipt,
/// the document digest includes its generation, and the session digest
/// covers the original physical provider record and its known state.
/// The run binding (when present) covers the original admitted PTY generation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CurrentManagementScope {
    pub peer: [u8; 32],
    pub membership: [u8; 32],
    pub document: [u8; 32],
    pub project: [u8; 32],
    pub source: [u8; 32],
    pub profile_revision: u64,
    pub session: [u8; 32],
    pub run: Option<[u8; 32]>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum ManagementAction {
    Rename { title: String },
    Archive,
    Restore,
    Reconnect,
    Close,
}

// The deserializer for an internally tagged unit variant may ignore extra
// fields. Decode strictly shaped structs instead, including for unit actions.
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum WireAction {
    Rename(WireRename),
    Other(WireOther),
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRename {
    action: RenameKind,
    title: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RenameKind {
    Rename,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WireOther {
    action: OtherKind,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum OtherKind {
    Archive,
    Restore,
    Reconnect,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManagementDenied;

pub struct ManagementGrant<T> {
    key: hmac::Key,
    scope: CurrentManagementScope,
    action: ManagementAction,
    end: Instant,
    target: Option<T>,
}

impl ManagementAction {
    pub fn decode(bytes: &[u8]) -> Result<Self, ManagementDenied> {
        if bytes.len() > MAX_ACTION_BYTES
            || bytes.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{')
        {
            return Err(ManagementDenied);
        }
        let action = match serde_json::from_slice(bytes).map_err(|_| ManagementDenied)? {
            WireAction::Rename(WireRename {
                action: RenameKind::Rename,
                title,
            }) => Self::Rename { title },
            WireAction::Other(other) => match other.action {
                OtherKind::Archive => Self::Archive,
                OtherKind::Restore => Self::Restore,
                OtherKind::Reconnect => Self::Reconnect,
                OtherKind::Close => Self::Close,
            },
        };
        action.validate()?;
        Ok(action)
    }

    fn validate(&self) -> Result<(), ManagementDenied> {
        if let Self::Rename { title } = self {
            if title.len() > MAX_TITLE_BYTES
                || title.trim().is_empty()
                || title.chars().any(char::is_control)
            {
                return Err(ManagementDenied);
            }
        }
        Ok(())
    }

    fn requires_run(&self) -> bool {
        matches!(self, Self::Reconnect | Self::Close)
    }
}

impl<T> ManagementGrant<T> {
    /// Issue only after an independent management grant and a verified
    /// exact desktop-owned target. Never use a catalogue row as evidence.
    pub fn issue(
        scope: CurrentManagementScope,
        action: ManagementAction,
        target: T,
    ) -> Result<Self, ManagementDenied> {
        Self::issue_at(scope, action, target, Instant::now())
    }

    fn issue_at(
        scope: CurrentManagementScope,
        action: ManagementAction,
        target: T,
        now: Instant,
    ) -> Result<Self, ManagementDenied> {
        action.validate()?;
        if action.requires_run() && scope.run.is_none() {
            return Err(ManagementDenied);
        }
        let mut seed = [0; 32];
        SystemRandom::new()
            .fill(&mut seed)
            .map_err(|_| ManagementDenied)?;
        Ok(Self {
            key: hmac::Key::new(hmac::HMAC_SHA256, &seed),
            scope,
            action,
            end: now.checked_add(GRANT_LIFETIME).ok_or(ManagementDenied)?,
            target: Some(target),
        })
    }

    pub fn token(&self) -> [u8; 32] {
        hmac::sign(&self.key, TOKEN_DOMAIN)
            .as_ref()
            .try_into()
            .expect("SHA-256 HMAC has exactly 32 bytes")
    }

    /// Call at dispatch with the freshly validated scope, under the same
    /// owner guard used by dispatch. None includes a revoked membership.
    /// Consume before invoking the operation: even an uncertain outcome
    /// must not permit replay.
    pub fn redeem(
        &mut self,
        current: Option<&CurrentManagementScope>,
        token: &[u8; 32],
        action: &ManagementAction,
    ) -> Result<T, ManagementDenied> {
        self.redeem_at(Instant::now(), current, token, action)
    }

    fn redeem_at(
        &mut self,
        now: Instant,
        current: Option<&CurrentManagementScope>,
        token: &[u8; 32],
        action: &ManagementAction,
    ) -> Result<T, ManagementDenied> {
        if now >= self.end {
            self.target.take();
            return Err(ManagementDenied);
        }
        hmac::verify(&self.key, TOKEN_DOMAIN, token).map_err(|_| ManagementDenied)?;
        let target = self.target.take().ok_or(ManagementDenied)?;
        if current != Some(&self.scope) || action != &self.action {
            return Err(ManagementDenied);
        }
        Ok(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> CurrentManagementScope {
        CurrentManagementScope {
            peer: [1; 32],
            membership: [2; 32],
            document: [3; 32],
            project: [4; 32],
            source: [5; 32],
            profile_revision: 7,
            session: [6; 32],
            run: Some([7; 32]),
        }
    }

    fn arm(now: Instant) -> ManagementGrant<&'static str> {
        ManagementGrant::issue_at(context(), ManagementAction::Archive, "owned", now).unwrap()
    }

    #[test]
    fn management_grant_redeems_exact_intent_only_once() {
        let now = Instant::now();
        let mut grant = arm(now);
        let token = grant.token();
        assert_eq!(
            grant.redeem_at(now, Some(&context()), &token, &ManagementAction::Archive),
            Ok("owned")
        );
        assert_eq!(
            grant.redeem_at(now, Some(&context()), &token, &ManagementAction::Archive),
            Err(ManagementDenied)
        );
    }

    #[test]
    fn management_grant_requires_fresh_exact_authority_and_operation() {
        let now = Instant::now();
        for changed in [
            CurrentManagementScope {
                peer: [9; 32],
                ..context()
            },
            CurrentManagementScope {
                membership: [9; 32],
                ..context()
            },
            CurrentManagementScope {
                document: [9; 32],
                ..context()
            },
            CurrentManagementScope {
                project: [9; 32],
                ..context()
            },
            CurrentManagementScope {
                source: [9; 32],
                ..context()
            },
            CurrentManagementScope {
                profile_revision: 8,
                ..context()
            },
            CurrentManagementScope {
                session: [9; 32],
                ..context()
            },
            CurrentManagementScope {
                run: Some([9; 32]),
                ..context()
            },
        ] {
            let mut grant = arm(now);
            let token = grant.token();
            assert_eq!(
                grant.redeem_at(now, Some(&changed), &token, &ManagementAction::Archive),
                Err(ManagementDenied)
            );
        }
        let mut grant = arm(now);
        let token = grant.token();
        assert_eq!(
            grant.redeem_at(now, Some(&context()), &[0; 32], &ManagementAction::Archive),
            Err(ManagementDenied)
        );
        assert_eq!(
            grant.redeem_at(now, Some(&context()), &token, &ManagementAction::Restore),
            Err(ManagementDenied)
        );
        assert_eq!(
            grant.redeem_at(now, Some(&context()), &token, &ManagementAction::Archive),
            Err(ManagementDenied)
        );
        let mut grant = arm(now);
        let token = grant.token();
        assert_eq!(
            grant.redeem_at(
                now + Duration::from_secs(30),
                Some(&context()),
                &token,
                &ManagementAction::Archive
            ),
            Err(ManagementDenied)
        );
        assert_eq!(
            grant.redeem_at(now, Some(&context()), &token, &ManagementAction::Archive),
            Err(ManagementDenied)
        );
    }

    #[test]
    fn management_grant_revocation_and_missing_run_fail_closed() {
        let now = Instant::now();
        let mut grant = arm(now);
        let token = grant.token();
        assert_eq!(
            grant.redeem_at(now, None, &token, &ManagementAction::Archive),
            Err(ManagementDenied)
        );
        assert_eq!(
            grant.redeem_at(now, Some(&context()), &token, &ManagementAction::Archive),
            Err(ManagementDenied)
        );
        for action in [ManagementAction::Close, ManagementAction::Reconnect] {
            assert!(
                ManagementGrant::issue_at(
                    CurrentManagementScope {
                        run: None,
                        ..context()
                    },
                    action,
                    "owned",
                    now
                )
                .is_err()
            );
        }
    }

    #[test]
    fn management_action_wire_allows_only_typed_bounded_intents() {
        assert_eq!(
            ManagementAction::decode(br#"{"action":"rename","title":"New title"}"#),
            Ok(ManagementAction::Rename {
                title: "New title".into()
            })
        );
        for (wire, expected) in [
            (
                br#"{"action":"archive"}"#.as_slice(),
                ManagementAction::Archive,
            ),
            (
                br#"{"action":"restore"}"#.as_slice(),
                ManagementAction::Restore,
            ),
            (
                br#"{"action":"reconnect"}"#.as_slice(),
                ManagementAction::Reconnect,
            ),
            (br#"{"action":"close"}"#.as_slice(), ManagementAction::Close),
        ] {
            assert_eq!(ManagementAction::decode(wire), Ok(expected));
        }
        for wire in [
            br#"{"action":"delete"}"#.as_slice(),
            br#"["close"]"#.as_slice(),
            br#"["rename","unexpected"]"#.as_slice(),
            br#"{"action":"close","pid":100}"#.as_slice(),
            br#"{"action":"close","title":null}"#.as_slice(),
            br#"{"action":"close","action":"archive"}"#.as_slice(),
            br#"{"action":"rename","title":"ok","project":"unknown"}"#.as_slice(),
            br#"{"action":"rename","title":"bad\nvalue"}"#.as_slice(),
            br#"{"action":"rename","title":"  "}"#.as_slice(),
            br#"{"action":"restore","path":"unexpected"}"#.as_slice(),
            br#"{"action":"reconnect","args":["unexpected"]}"#.as_slice(),
        ] {
            assert_eq!(ManagementAction::decode(wire), Err(ManagementDenied));
        }
        let oversized = format!(r#"{{"action":"rename","title":"{}"}}"#, "界".repeat(90));
        assert_eq!(
            ManagementAction::decode(oversized.as_bytes()),
            Err(ManagementDenied)
        );
    }
}
