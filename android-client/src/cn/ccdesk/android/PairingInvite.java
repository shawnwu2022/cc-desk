package cn.ccdesk.android;

import android.util.JsonReader;
import android.util.JsonToken;
import java.io.IOException;
import java.io.StringReader;
import java.net.URI;
import java.util.Arrays;

/** Versioned, short-lived input; not an admitted profile or grant. */
final class PairingInvite implements AutoCloseable {
    static final int MAX_QR = 1024;
    static final String ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    final URI relay;
    final String hostEndpointId;
    final String hostFingerprintPrefix;
    private final char[] code;
    private boolean closed;

    private PairingInvite(URI relay, String code, String full, String prefix) {
        this.relay = relay;
        this.code = code.toCharArray();
        this.hostEndpointId = full;
        this.hostFingerprintPrefix = prefix;
    }

    synchronized char[] copyCode() {
        if (closed) throw new IllegalStateException("Invitation closed");
        return code.clone();
    }
    @Override public synchronized void close() { closed = true; Arrays.fill(code, (char)0); }

    byte[] hostCommitment() {
        if (hostEndpointId == null)
            return hostFingerprintPrefix.getBytes(java.nio.charset.StandardCharsets.US_ASCII);
        byte[] digest = new byte[32];
        for (int i = 0; i < digest.length; i++) {
            digest[i] = (byte)((Character.digit(hostEndpointId.charAt(2*i), 16) << 4) |
                               Character.digit(hostEndpointId.charAt(2*i+1), 16));
        }
        return digest;
    }

    boolean matchesHost(String verifiedId) {
        if (verifiedId == null || !verifiedId.matches("[0-9a-f]{64}")) return false;
        return hostEndpointId != null ? hostEndpointId.equals(verifiedId) :
            hostFingerprintPrefix.equals(fingerprint(verifiedId));
    }

    static PairingInvite fromQr(String source) {
        if (source == null || source.isEmpty() || source.length() > MAX_QR) throw invalid();
        String code = null, host = null, relay = null;
        int seen = 0;
        try (JsonReader reader = new JsonReader(new StringReader(source))) {
            reader.setLenient(false);
            reader.beginObject();
            while (reader.hasNext()) {
                switch (reader.nextName()) {
                    case "version":
                        if ((seen & 1) != 0 || reader.peek() != JsonToken.NUMBER ||
                            !"1".equals(reader.nextString())) throw invalid();
                        seen |= 1;
                        break;
                    case "relayOrigin":
                        if ((seen & 2) != 0) throw invalid();
                        relay = readString(reader);
                        seen |= 2;
                        break;
                    case "code":
                        if ((seen & 4) != 0) throw invalid();
                        code = readString(reader);
                        seen |= 4;
                        break;
                    case "hostEndpointId":
                        if ((seen & 8) != 0) throw invalid();
                        host = readString(reader);
                        seen |= 8;
                        break;
                    default: throw invalid();
                }
            }
            reader.endObject();
            if (reader.peek() != JsonToken.END_DOCUMENT || seen != 15 ||
                !canonical(code, 16) || !host.matches("[0-9a-f]{64}")) throw invalid();
            return new PairingInvite(DesktopBackend.Desktop.checkedRelay(relay), code, host, fingerprint(host));
        } catch (IOException | IllegalStateException e) {
            throw invalid();
        }
    }

    static PairingInvite fromPasted(URI relay, CharSequence value) {
        if (value.length() != 45) throw invalid();
        char[] charBuf = new char[45];
        try {
            for (int i = 0; i < charBuf.length; i++) {
                char c = value.charAt(i);
                charBuf[i] = c >= 'a' && c <= 'z' ? (char)(c - 'a' + 'A') : c;
            }
            for (int i = 0; i < 8; i++)
                if (charBuf[i] != "CCDESK1-".charAt(i)) throw invalid();
            if (charBuf[24] != '-') throw invalid();
            for (int i = 8; i < charBuf.length; i++)
                if (i != 24 && ALPHABET.indexOf(charBuf[i]) < 0) throw invalid();
            return new PairingInvite(relay, new String(charBuf, 8, 16), null,
                new String(charBuf, 25, 20));
        } finally {
            Arrays.fill(charBuf, (char)0);
        }
    }

    static String fingerprint(String hex) {
        StringBuilder out = new StringBuilder(20);
        for (int i = 0; i < 100; i += 5) {
            int value = 0;
            for (int bit = i; bit < i + 5; bit++) {
                int nibble = Character.digit(hex.charAt(bit / 4), 16);
                value = (value << 1) | ((nibble >> (3 - bit % 4)) & 1);
            }
            out.append(ALPHABET.charAt(value));
        }
        return out.toString();
    }

    private static boolean canonical(String value, int size) {
        if (value.length() != size) return false;
        for (int i = 0; i < size; i++) if (ALPHABET.indexOf(value.charAt(i)) < 0) return false;
        return true;
    }

    private static String readString(JsonReader reader) throws IOException {
        if (reader.peek() != JsonToken.STRING) throw invalid();
        return reader.nextString();
    }

    private static IllegalArgumentException invalid() {
        return new IllegalArgumentException("Invalid CC Desk invitation");
    }
}
