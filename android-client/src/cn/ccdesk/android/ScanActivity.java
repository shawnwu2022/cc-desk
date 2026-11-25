package cn.ccdesk.android;

import android.Manifest;
import android.app.Activity;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.hardware.Camera;
import android.os.Bundle;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.WindowManager;
import android.widget.Toast;
import com.google.zxing.BinaryBitmap;
import com.google.zxing.PlanarYUVLuminanceSource;
import com.google.zxing.ReaderException;
import com.google.zxing.common.HybridBinarizer;
import com.google.zxing.qrcode.QRCodeReader;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

@SuppressWarnings("deprecation")
public final class ScanActivity extends Activity implements SurfaceHolder.Callback {
    static final String RESULT_INVITE = "cn.ccdesk.android.SCAN_RESULT";
    private final ExecutorService decoder = Executors.newSingleThreadExecutor();
    private Camera camera;
    private SurfaceHolder surface;
    private boolean resumed;
    private boolean openingPermissionDialog;
    private long epoch;

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_SECURE | WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        SurfaceView view = new SurfaceView(this);
        view.getHolder().addCallback(this);
        setContentView(view);
    }

    @Override protected void onResume() {
        super.onResume();
        resumed = true;
        if (checkSelfPermission(Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED) {
            if (!openingPermissionDialog) {
                openingPermissionDialog = true;
                requestPermissions(new String[]{Manifest.permission.CAMERA}, 1);
            }
        } else { start(); }
    }

    @Override protected void onPause() {
        resumed = false;
        stop();
        super.onPause();
    }

    @Override protected void onDestroy() {
        decoder.shutdownNow();
        super.onDestroy();
    }

    @Override public void onRequestPermissionsResult(int code, String[] permissions, int[] result) {
        super.onRequestPermissionsResult(code, permissions, result);
        if (code != 1) return;
        openingPermissionDialog = false;
        if (result.length != 1 || result[0] != PackageManager.PERMISSION_GRANTED) finish();
        else start();
    }

    @Override public void surfaceCreated(SurfaceHolder holder) { surface = holder; start(); }
    @Override public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {}
    @Override public void surfaceDestroyed(SurfaceHolder holder) { surface = null; stop(); }

    private void start() {
        if (!resumed || isFinishing() || surface == null || camera != null ||
            checkSelfPermission(Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED) return;
        try {
            Camera.CameraInfo info = new Camera.CameraInfo();
            int id = -1;
            for (int i = 0; i < Camera.getNumberOfCameras(); i++) {
                Camera.getCameraInfo(i, info);
                if (info.facing == Camera.CameraInfo.CAMERA_FACING_BACK) { id = i; break; }
            }
            if (id < 0) throw new IllegalStateException();
            camera = Camera.open(id);
            camera.setDisplayOrientation((info.orientation - getWindowManager().getDefaultDisplay().getRotation() * 90 + 360) % 360);
            Camera.Parameters params = camera.getParameters();
            params.setPreviewFormat(android.graphics.ImageFormat.NV21);
            Camera.Size size = null;
            for (Camera.Size candidate : params.getSupportedPreviewSizes()) {
                long pixels = (long) candidate.width * candidate.height;
                if (pixels > 1_300_000 || pixels < 100_000) continue;
                if (size == null || pixels > (long) size.width * size.height) size = candidate;
            }
            if (size == null) throw new IllegalStateException();
            params.setPreviewSize(size.width, size.height);
            ListFocus.apply(params);
            camera.setParameters(params);
            camera.setPreviewDisplay(surface);
            camera.startPreview();
            next(++epoch, size.width, size.height);
        } catch (Exception e) {
            stop();
            Toast.makeText(this, "相机不可用，请返回并输入配对码", Toast.LENGTH_LONG).show();
            finish();
        }
    }

    private static final class ListFocus {
        static void apply(Camera.Parameters p) {
            java.util.List<String> modes = p.getSupportedFocusModes();
            if (modes != null && modes.contains(Camera.Parameters.FOCUS_MODE_CONTINUOUS_PICTURE))
                p.setFocusMode(Camera.Parameters.FOCUS_MODE_CONTINUOUS_PICTURE);
        }
    }

    private void next(long request, int width, int height) {
        if (camera == null || !resumed || request != epoch) return;
        camera.setOneShotPreviewCallback((bytes, source) -> {
            if (source != camera || !resumed || request != epoch) return;
            if (bytes == null || bytes.length < width * height) { next(request, width, height); return; }
            byte[] y = java.util.Arrays.copyOf(bytes, width * height);
            decoder.execute(() -> {
                String value = null;
                try {
                    PlanarYUVLuminanceSource luma = new PlanarYUVLuminanceSource(y, width, height,
                        0, 0, width, height, false);
                    String result = new QRCodeReader().decode(new BinaryBitmap(new HybridBinarizer(luma))).getText();
                    if (result != null && !result.isEmpty() && result.length() <= 2048) value = result;
                } catch (ReaderException ignored) {
                    // 非二维码帧不产生提示或日志。
                } finally {
                    java.util.Arrays.fill(y, (byte) 0);
                }
                final String qr = value;
                runOnUiThread(() -> {
                    if (!resumed || request != epoch || isFinishing()) return;
                    if (qr == null) next(request, width, height);
                    else {
                        setResult(RESULT_OK, new Intent().putExtra(RESULT_INVITE, qr));
                        stop();
                        finish();
                    }
                });
            });
        });
    }

    private void stop() {
        epoch++;
        Camera held = camera;
        camera = null;
        if (held == null) return;
        try { held.setPreviewCallback(null); held.stopPreview(); }
        catch (RuntimeException ignored) {}
        finally { held.release(); }
    }
}
