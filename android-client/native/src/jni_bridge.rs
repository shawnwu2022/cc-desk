use crate::{cancel, status, take_authenticated_snapshot};
use jni::{
    JNIEnv,
    objects::JClass,
    sys::{jint, jlong, jobjectArray},
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
pub extern "system" fn Java_cn_ccdesk_android_NativeDesktopBackend_nativeTakeObserverSnapshot(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    token: jlong,
) -> jobjectArray {
    let result = catch_unwind(AssertUnwindSafe(|| -> jni::errors::Result<jobjectArray> {
        let Some(parts) = take_authenticated_snapshot(token) else {
            return Ok(ptr::null_mut());
        };
        let array =
            env.new_object_array(parts.len() as i32, "[B", jni::objects::JObject::null())?;
        for (index, part) in parts.iter().enumerate() {
            let bytes = env.byte_array_from_slice(part)?;
            env.set_object_array_element(&array, index as i32, &bytes)?;
            env.delete_local_ref(bytes)?;
        }
        Ok(array.into_raw())
    }));
    match result {
        Ok(Ok(value)) => value,
        _ => {
            let _ = catch_unwind(|| cancel(token));
            ptr::null_mut()
        }
    }
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
