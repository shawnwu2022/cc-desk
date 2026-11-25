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
    1
}
