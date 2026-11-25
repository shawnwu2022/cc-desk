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
