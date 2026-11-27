use crate::{cancel, status};
use jni::{
    JNIEnv,
    objects::JClass,
    sys::{jint, jlong},
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
};

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeObserverState(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    token: jlong,
) -> jint {
    catch_unwind(|| status(token) as jint).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeCancelObserver(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    token: jlong,
) {
    let _ = catch_unwind(|| cancel(token));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeKeystoreBridge_nativeCompleteApproval(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    request: jlong,
    raw: jni::objects::JByteArray<'_>,
) {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if raw.is_null() || env.get_array_length(&raw).ok() != Some(64) {
            return None;
        }
        env.convert_byte_array(raw).ok()?.try_into().ok()
    }))
    .ok()
    .flatten();
    let _ = catch_unwind(|| crate::keystore::complete(request, result));
}

fn exact_bytes<const N: usize>(
    env: &JNIEnv<'_>,
    raw: &jni::objects::JByteArray<'_>,
) -> Option<[u8; N]> {
    if raw.is_null() || env.get_array_length(raw).ok()? as usize != N {
        return None;
    }
    env.convert_byte_array(raw).ok()?.try_into().ok()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeBeginPairing(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    facade: jni::objects::JObject<'_>,
    local_id: jni::objects::JByteArray<'_>,
    origin: jni::objects::JString<'_>,
    code: jni::objects::JByteArray<'_>,
    host: jni::objects::JByteArray<'_>,
    endpoint: jni::objects::JByteArray<'_>,
    relay: jni::objects::JByteArray<'_>,
    approval: jni::objects::JByteArray<'_>,
) -> jlong {
    catch_unwind(AssertUnwindSafe(|| {
        use crate::{android_pairing, enrollment::HostCommitment};
        use cc_desk_remote_record::appbind::CanonicalOrigin;
        if facade.is_null() || origin.is_null() || host.is_null() {
            return None;
        }
        let local = exact_bytes(&env, &local_id)?;
        let raw_code = zeroize::Zeroizing::new(exact_bytes::<16>(&env, &code)?);
        let code = zeroize::Zeroizing::new(std::str::from_utf8(&*raw_code).ok()?.to_owned());
        let host = match env.get_array_length(&host).ok()? {
            32 => HostCommitment::QrEndpointId(exact_bytes(&env, &host)?),
            20 => HostCommitment::Manual(exact_bytes(&env, &host)?),
            _ => return None,
        };
        let origin_raw: String = env.get_string(&origin).ok()?.into();
        let origin = CanonicalOrigin::trusted(&origin_raw).ok()?;
        if origin.as_str() != origin_raw {
            return None;
        }
        let args = android_pairing::Begin {
            origin,
            host,
            code,
            local,
            endpoint: exact_bytes(&env, &endpoint)?,
            relay: exact_bytes(&env, &relay)?,
            approval: exact_bytes(&env, &approval)?,
        };
        android_pairing::begin(
            env.get_java_vm().ok()?,
            env.new_global_ref(facade).ok()?,
            args,
        )
    }))
    .ok()
    .flatten()
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativePairingState(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
) -> jint {
    catch_unwind(|| crate::android_pairing::state(id)).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativePairingHostId(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
) -> jni::sys::jstring {
    catch_unwind(AssertUnwindSafe(|| {
        let id = crate::android_pairing::host_id(id)?;
        Some(env.new_string(id).ok()?.into_raw())
    }))
    .ok()
    .flatten()
    .unwrap_or_else(ptr::null_mut)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeCancelPairing(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
) {
    let _ = catch_unwind(|| crate::android_pairing::cancel(id));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeStartPairedObserver(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
) -> jlong {
    catch_unwind(AssertUnwindSafe(|| {
        crate::android_pairing::start_view(&env, id).map(crate::ObserverHandle::token)
    }))
    .ok()
    .flatten()
    .unwrap_or(0)
}

/// The Java caller runs this on its Keystore/disk worker, never on the main
/// Activity thread. Only the public reverified desktop identity is returned.
#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeReadPairedHostId(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    facade: jni::objects::JObject<'_>,
    local_id: jni::objects::JByteArray<'_>,
    origin: jni::objects::JString<'_>,
    endpoint: jni::objects::JByteArray<'_>,
    relay: jni::objects::JByteArray<'_>,
    approval: jni::objects::JByteArray<'_>,
) -> jni::sys::jstring {
    let result = catch_unwind(AssertUnwindSafe(|| {
        use cc_desk_remote_record::appbind::CanonicalOrigin;
        use jni::objects::{JByteArray, JObject, JValue};
        use std::time::{SystemTime, UNIX_EPOCH};
        if facade.is_null() || origin.is_null() {
            return None;
        }
        let local = exact_bytes::<16>(&env, &local_id)?;
        if local == [0; 16] {
            return None;
        }
        let endpoint = exact_bytes(&env, &endpoint)?;
        let relay = exact_bytes(&env, &relay)?;
        let approval = exact_bytes(&env, &approval)?;
        let origin_text: String = env.get_string(&origin).ok()?.into();
        let origin = CanonicalOrigin::trusted(&origin_text).ok()?;
        if origin.as_str() != origin_text {
            return None;
        }
        let local_hex: String = local.iter().map(|x| format!("{x:02x}")).collect();
        let arg = JObject::from(env.new_string(local_hex).ok()?);
        let raw = env
            .call_method(
                &facade,
                "readPairingRecord",
                "(Ljava/lang/String;)[B",
                &[JValue::Object(&arg)],
            )
            .ok()?
            .l()
            .ok()?;
        if raw.is_null() {
            return None;
        }
        let raw = JByteArray::from(raw);
        let n = env.get_array_length(&raw).ok()?;
        if !(1..=9216).contains(&n) {
            return None;
        }
        let record = zeroize::Zeroizing::new(env.convert_byte_array(raw).ok()?);
        let now = u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_millis(),
        )
        .ok()?;
        let host = crate::enrollment::restore_stored_desktop(
            &record, origin, endpoint, relay, approval, now,
        )
        .ok()?;
        let hex: String = host.iter().map(|x| format!("{x:02x}")).collect();
        Some(env.new_string(hex).ok()?.into_raw())
    }))
    .ok()
    .flatten();
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    result.unwrap_or_else(ptr::null_mut)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeTakeObserverText(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    token: jlong,
) -> jni::sys::jstring {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let parts = crate::take_authenticated_snapshot(token)?;
        let text = match crate::observer_text(&parts) {
            Some(text) => text,
            None => {
                crate::cancel(token);
                return None;
            }
        };
        if crate::status(token) != crate::ObserverStatus::Authenticated {
            return None;
        }
        Some(env.new_string(text).ok()?.into_raw())
    }))
    .ok()
    .flatten();
    result.unwrap_or_else(ptr::null_mut)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeAndroidAbiVersion(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
) -> jint {
    3
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeLoadShares(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    facade: jni::objects::JObject<'_>,
    local_id: jni::objects::JByteArray<'_>,
    origin: jni::objects::JString<'_>,
    endpoint: jni::objects::JByteArray<'_>,
    relay: jni::objects::JByteArray<'_>,
    approval: jni::objects::JByteArray<'_>,
) -> jlong {
    load_share_list(
        env, facade, local_id, origin, endpoint, relay, approval, false, false,
    )
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeLoadArchivedShares(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    facade: jni::objects::JObject<'_>,
    local_id: jni::objects::JByteArray<'_>,
    origin: jni::objects::JString<'_>,
    endpoint: jni::objects::JByteArray<'_>,
    relay: jni::objects::JByteArray<'_>,
    approval: jni::objects::JByteArray<'_>,
) -> jlong {
    load_share_list(
        env, facade, local_id, origin, endpoint, relay, approval, true, false,
    )
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeLoadManagementList(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    facade: jni::objects::JObject<'_>,
    local_id: jni::objects::JByteArray<'_>,
    origin: jni::objects::JString<'_>,
    endpoint: jni::objects::JByteArray<'_>,
    relay: jni::objects::JByteArray<'_>,
    approval: jni::objects::JByteArray<'_>,
    archived: jni::sys::jboolean,
) -> jlong {
    load_share_list(
        env,
        facade,
        local_id,
        origin,
        endpoint,
        relay,
        approval,
        archived == jni::sys::JNI_TRUE,
        true,
    )
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeManagementRows(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
) -> jni::sys::jstring {
    catch_unwind(AssertUnwindSafe(|| {
        let text = crate::android_shares::management_rows(id)?;
        Some(env.new_string(text).ok()?.into_raw())
    }))
    .ok()
    .flatten()
    .unwrap_or_else(ptr::null_mut)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeSubmitManagement(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
    reference: jni::objects::JByteArray<'_>,
    action: jni::objects::JByteArray<'_>,
) -> jni::sys::jboolean {
    let allowed = catch_unwind(AssertUnwindSafe(|| {
        use cc_desk_remote_security::{
            session_management::ManagementAction, session_management_channel::ManagementRequest,
        };
        let reference = exact_bytes(&env, &reference)?;
        if reference == [0; 32] || !(1..=2048).contains(&env.get_array_length(&action).ok()?) {
            return None;
        }
        let action = ManagementAction::decode(&env.convert_byte_array(&action).ok()?).ok()?;
        crate::android_shares::submit_management(id, ManagementRequest { reference, action })
    }))
    .ok()
    .flatten()
    .is_some();
    if allowed {
        jni::sys::JNI_TRUE
    } else {
        jni::sys::JNI_FALSE
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeManagementProjection(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
) -> jni::sys::jstring {
    catch_unwind(AssertUnwindSafe(|| {
        let text = crate::android_shares::management_projection(id)?;
        Some(env.new_string(text).ok()?.into_raw())
    }))
    .ok()
    .flatten()
    .unwrap_or_else(ptr::null_mut)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeManagementOutcome(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
) -> jint {
    catch_unwind(|| crate::android_shares::management_result(id))
        .ok()
        .flatten()
        .unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
fn load_share_list(
    mut env: JNIEnv<'_>,
    facade: jni::objects::JObject<'_>,
    local_id: jni::objects::JByteArray<'_>,
    origin: jni::objects::JString<'_>,
    endpoint: jni::objects::JByteArray<'_>,
    relay: jni::objects::JByteArray<'_>,
    approval: jni::objects::JByteArray<'_>,
    archived: bool,
    management: bool,
) -> jlong {
    catch_unwind(AssertUnwindSafe(|| {
        use cc_desk_remote_record::appbind::CanonicalOrigin;
        use cc_desk_remote_security::session_catalogue::ListMode;
        if facade.is_null() || origin.is_null() {
            return None;
        }
        let text: String = env.get_string(&origin).ok()?.into();
        let origin = CanonicalOrigin::trusted(&text).ok()?;
        if origin.as_str() != text {
            return None;
        }
        let input = crate::android_shares::Inputs {
            origin,
            management,
            mode: if archived {
                ListMode::Archived
            } else {
                ListMode::Sessions
            },
            local: exact_bytes(&env, &local_id)?,
            endpoint: exact_bytes(&env, &endpoint)?,
            relay: exact_bytes(&env, &relay)?,
            approval: exact_bytes(&env, &approval)?,
        };
        crate::android_shares::begin(
            env.get_java_vm().ok()?,
            env.new_global_ref(facade).ok()?,
            input,
        )
    }))
    .ok()
    .flatten()
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeShareState(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
) -> jint {
    catch_unwind(|| crate::android_shares::state(id)).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeShareOffers(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
) -> jni::sys::jstring {
    catch_unwind(AssertUnwindSafe(|| {
        let text = crate::android_shares::offers(id)?;
        Some(env.new_string(text).ok()?.into_raw())
    }))
    .ok()
    .flatten()
    .unwrap_or_else(ptr::null_mut)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeCatalogueRows(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
) -> jni::sys::jstring {
    catch_unwind(AssertUnwindSafe(|| {
        let text = crate::android_shares::catalogue_rows(id)?;
        Some(env.new_string(text).ok()?.into_raw())
    }))
    .ok()
    .flatten()
    .unwrap_or_else(ptr::null_mut)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeChooseShare(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
    offer: jni::objects::JString<'_>,
) -> jni::sys::jboolean {
    catch_unwind(AssertUnwindSafe(|| {
        if offer.is_null() {
            return None;
        }
        let text: String = env.get_string(&offer).ok()?.into();
        crate::android_shares::choose(id, &text)
    }))
    .ok()
    .flatten()
    .map_or(0, |_| 1)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeShareObserverHandle(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
) -> jlong {
    catch_unwind(|| crate::android_shares::viewer(id))
        .ok()
        .flatten()
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeCancelShares(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
) {
    let _ = catch_unwind(|| crate::android_shares::stop(id));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeViewerRole(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    token: jlong,
) -> jint {
    catch_unwind(|| crate::viewer_role(token)).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeTakeControllerFrame(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    token: jlong,
) -> jni::sys::jstring {
    catch_unwind(AssertUnwindSafe(|| {
        if crate::viewer_role(token) != 2 {
            return None;
        }
        let text = crate::control_owner(token)?.take_frame()?;
        Some(env.new_string(text).ok()?.into_raw())
    }))
    .ok()
    .flatten()
    .unwrap_or_else(ptr::null_mut)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeConfirmControllerFrame(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    token: jlong,
    id: jlong,
) -> jni::sys::jboolean {
    u8::from(
        catch_unwind(|| {
            crate::viewer_role(token) == 2
                && crate::control_owner(token).is_some_and(|c| c.confirm(id))
        })
        .unwrap_or(false),
    )
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeQueueControllerText(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    token: jlong,
    text: jni::objects::JString<'_>,
) -> jlong {
    catch_unwind(AssertUnwindSafe(|| {
        if crate::viewer_role(token) != 2 || text.is_null() {
            return None;
        }
        if !(1..=2048).contains(
            &env.call_method(&text, "length", "()I", &[])
                .ok()?
                .i()
                .ok()?,
        ) {
            return None;
        }
        let string: String = env.get_string(&text).ok()?.into();
        crate::control_owner(token)?.enqueue(crate::controller::text(string)?)
    }))
    .ok()
    .flatten()
    .unwrap_or(0)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeQueueControllerKey(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    token: jlong,
    key: jint,
) -> jlong {
    catch_unwind(|| {
        if crate::viewer_role(token) != 2 {
            return None;
        }
        crate::control_owner(token)?.enqueue(crate::controller::key(key)?)
    })
    .ok()
    .flatten()
    .unwrap_or(0)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeControllerInputOutcome(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    token: jlong,
    id: jlong,
) -> jint {
    catch_unwind(|| crate::control_owner(token).map_or(0, |c| c.outcome(id))).unwrap_or(0)
}
