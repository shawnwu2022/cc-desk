package cn.ccdesk.android;

import java.net.URI;
import java.util.UUID;

public final class DesktopAliasTest {
    public static void main(String[] args) {
        DesktopBackend.Desktop original = new DesktopBackend.Desktop(
            new UUID(1, 2), new String(new char[64]).replace('\0', 'a'),
            "Office", URI.create("wss://relay.example/relay"));
        DesktopBackend.Desktop renamed = original.withAlias("  工作电脑  ");
        if (!renamed.name.equals("工作电脑") || !original.name.equals("Office")
            || !renamed.localId.equals(original.localId) || !renamed.hostId.equals(original.hostId)
            || !renamed.relay.equals(original.relay)) throw new AssertionError("Alias changed identity");
        reject(original, "   ");
        reject(original, "foo\nbar");
        reject(original, "abc\u202Edef");
        reject(original, new String(new char[101]).replace('\0', 'a'));
        System.out.println("DesktopAlias: 5 cases passed");
    }

    private static void reject(DesktopBackend.Desktop original, String value) {
        try { original.withAlias(value); }
        catch (IllegalArgumentException expected) { return; }
        throw new AssertionError("Accepted invalid alias");
    }
}
