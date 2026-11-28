package cn.ccdesk.android;

import android.Manifest;
import android.app.Activity;
import android.content.pm.PackageManager;
import android.hardware.Camera;
import android.os.Bundle;
import android.view.Gravity;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.WindowManager;
import android.widget.FrameLayout;
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
    static final String SCAN_OWNER = "cn.ccdesk.android.SCAN_OWNER";
    private java.util.UUID owner;
    private final ExecutorService decoder = Executors.newSingleThreadExecutor();
    private Camera camera;
    private SurfaceHolder surface;
    private FrameLayout previewContainer;
    private SurfaceView preview;
    private int frameWidth;
    private int frameHeight;
    private int displayRotation;
    private boolean resumed;
    private boolean openingPermissionDialog;
    private long epoch;

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_SECURE | WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        try { owner = java.util.UUID.fromString(getIntent().getStringExtra(SCAN_OWNER)); }
        catch (IllegalArgumentException | NullPointerException e) { finish(); return; }
        previewContainer = new FrameLayout(this);
        previewContainer.setBackgroundColor(android.graphics.Color.BLACK);
        preview = new SurfaceView(this);
        previewContainer.addView(preview, new FrameLayout.LayoutParams(
            FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT, Gravity.CENTER));
        previewContainer.addOnLayoutChangeListener(
            (v, left, top, right, bottom, oldLeft, oldTop, oldRight, oldBottom) -> fitPreview());
        preview.getHolder().addCallback(this);
        setContentView(previewContainer);
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
            displayRotation = (info.orientation - getWindowManager().getDefaultDisplay().getRotation() * 90 + 360) % 360;
            camera.setDisplayOrientation(displayRotation);
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
            frameWidth = size.width;
            frameHeight = size.height;
            fitPreview();
            camera.setPreviewDisplay(surface);
            camera.startPreview();
            next(++epoch, size.width, size.height);
        } catch (Exception e) {
            stop();
            Toast.makeText(this, "相机不可用，请返回并输入配对码", Toast.LENGTH_LONG).show();
            finish();
        }
    }

    private void fitPreview() {
        int[] size = ScanPreviewGeometry.fit(previewContainer.getWidth(), previewContainer.getHeight(),
            frameWidth, frameHeight, displayRotation);
        if (size[0] == 0 || size[1] == 0) return;
        FrameLayout.LayoutParams layout = (FrameLayout.LayoutParams) preview.getLayoutParams();
        if (layout.width == size[0] && layout.height == size[1]) return;
        layout.width = size[0];
        layout.height = size[1];
        preview.setLayoutParams(layout);
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
                PairingInvite value = null;
                try {
                    PlanarYUVLuminanceSource luma = new PlanarYUVLuminanceSource(y, width, height,
                        0, 0, width, height, false);
                    String result = new QRCodeReader().decode(new BinaryBitmap(new HybridBinarizer(luma))).getText();
                    value = PairingInvite.fromQr(result);
                } catch (ReaderException | IllegalArgumentException ignored) {
                    // 非二维码帧不产生提示或日志。
                } finally {
                    java.util.Arrays.fill(y, (byte) 0);
                }
                final PairingInvite invite = value;
                runOnUiThread(() -> {
                    if (!resumed || request != epoch || isFinishing()) {
                        if (invite != null) invite.close();
                        return;
                    }
                    if (invite == null) next(request, width, height);
                    else {
                        if (ScanMailbox.complete(owner, invite)) setResult(RESULT_OK);
                        else invite.close();
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
