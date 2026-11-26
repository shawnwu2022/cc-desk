package cn.ccdesk.android;

/** Number of device lists the current search was actually able to cover. */
final class SessionSearchCoverage {
    enum Device { LOADED, LOADING, NOT_CONNECTED, UNVERIFIED, ACTIVE_ONLY }
    private final int[] counts = new int[Device.values().length];

    void add(Device device) { counts[device.ordinal()]++; }
    int count(Device device) { return counts[device.ordinal()]; }
    boolean isPartial() {
        return count(Device.LOADING) + count(Device.NOT_CONNECTED)
            + count(Device.UNVERIFIED) + count(Device.ACTIVE_ONLY) > 0;
    }
}
