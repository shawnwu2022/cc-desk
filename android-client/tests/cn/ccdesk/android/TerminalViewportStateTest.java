package cn.ccdesk.android;

public final class TerminalViewportStateTest {
    public static void main(String[] args) {
        TerminalViewportState view = new TerminalViewportState();
        long first = view.nextLayout();
        require(view.follow(first), "initial output must follow");
        long second = view.nextLayout();
        require(!view.follow(first) && view.follow(second), "superseded layout cannot scroll");
        view.scrolled(false);
        require(!view.follow(second), "scrolling away cancels queued snap");
        long whileReading = view.nextLayout();
        require(!view.follow(whileReading), "new output cannot steal manual reading");
        long explicit = view.returnToLatest();
        require(!view.follow(whileReading) && view.follow(explicit), "latest resumes following explicitly");
        view.scrolled(false);
        view.scrolled(true);
        require(view.follow(view.nextLayout()), "scrolling to bottom resumes following");
        view.close();
        require(!view.follow(view.nextLayout()), "retired surface rejects late scroll work");
        System.out.println("TerminalViewportState: 7 cases passed");
    }

    private static void require(boolean condition, String message) {
        if (!condition) throw new AssertionError(message);
    }
}
