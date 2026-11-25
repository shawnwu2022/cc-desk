package cn.ccdesk.android;

/** JNI ABI for observer handles minted only by the retained native owner. */
final class NativeDesktopBackend {
    private NativeDesktopBackend() {}

    static boolean loadLibrary() {
        try {
            System.loadLibrary("cc_desk_android_native");
            return true;
        } catch (SecurityException | LinkageError ignored) {
            return false;
        }
    }

    static native int nativeObserverState(long handle);
    static native byte[][] nativeTakeObserverSnapshot(long handle);
    static native void nativeCancelObserver(long handle);
}
