package cn.ccdesk.android;

final class ScanPreviewGeometry {
    private ScanPreviewGeometry() {}

    static int[] fit(int viewportWidth, int viewportHeight, int frameWidth, int frameHeight, int rotation) {
        if (viewportWidth < 1 || viewportHeight < 1 || frameWidth < 1 || frameHeight < 1) {
            return new int[]{0, 0};
        }
        boolean quarterTurn = rotation == 90 || rotation == 270;
        int width = quarterTurn ? frameHeight : frameWidth;
        int height = quarterTurn ? frameWidth : frameHeight;
        double scale = Math.min((double) viewportWidth / width, (double) viewportHeight / height);
        return new int[]{Math.max(1, (int) Math.round(width * scale)),
                         Math.max(1, (int) Math.round(height * scale))};
    }
}
