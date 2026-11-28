package cn.ccdesk.android;

import java.util.Arrays;

public final class ScanPreviewGeometryTest {
    public static void main(String[] args) {
        check(1080, 1920, 1280, 960, 90, 1080, 1440);
        check(1920, 1080, 1280, 960, 0, 1440, 1080);
        check(1000, 800, 1280, 720, 180, 1000, 563);
        check(1000, 800, 1280, 720, 270, 450, 800);
        check(0, 0, 1280, 960, 90, 0, 0);
        System.out.println("ScanPreviewGeometry: 5 cases passed");
    }

    private static void check(int vw, int vh, int fw, int fh, int rotation, int w, int h) {
        int[] actual = ScanPreviewGeometry.fit(vw, vh, fw, fh, rotation);
        if (!Arrays.equals(actual, new int[]{w, h})) {
            throw new AssertionError(Arrays.toString(actual) + " != " + w + "x" + h);
        }
    }
}
