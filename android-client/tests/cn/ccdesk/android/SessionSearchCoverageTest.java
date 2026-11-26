package cn.ccdesk.android;

public final class SessionSearchCoverageTest {
    public static void main(String[] args) {
        SessionSearchCoverage current = new SessionSearchCoverage();
        current.add(SessionSearchCoverage.Device.LOADED);
        if (current.isPartial()) throw new AssertionError("loaded titles not considered incomplete");
        for (SessionSearchCoverage.Device state : new SessionSearchCoverage.Device[] {
            SessionSearchCoverage.Device.LOADING, SessionSearchCoverage.Device.NOT_CONNECTED,
            SessionSearchCoverage.Device.UNVERIFIED, SessionSearchCoverage.Device.ACTIVE_ONLY
        }) {
            SessionSearchCoverage mixed = new SessionSearchCoverage();
            mixed.add(SessionSearchCoverage.Device.LOADED);
            mixed.add(state);
            if (!mixed.isPartial() || mixed.count(state) != 1)
                throw new AssertionError("missing coverage state");
        }
        System.out.println("SessionSearchCoverageTest: 5 checks passed");
    }
}
