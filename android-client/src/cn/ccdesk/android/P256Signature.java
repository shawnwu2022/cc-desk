package cn.ccdesk.android;

import java.math.BigInteger;
import java.util.Arrays;

final class P256Signature {
    private static final BigInteger ORDER = new BigInteger(
        "ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551", 16);

    private P256Signature() {}

    static byte[] toFixed(byte[] der) {
        if (der.length < 8 || der.length > 72 || der[0] != 0x30 ||
            (der[1] & 255) != der.length - 2) throw invalid();
        byte[] fixed = new byte[64];
        int end = readScalar(der, 2, fixed, 0);
        if (readScalar(der, end, fixed, 32) != der.length) throw invalid();
        return fixed;
    }

    private static int readScalar(byte[] der, int pos, byte[] out, int offset) {
        if (pos + 2 > der.length || der[pos++] != 2) throw invalid();
        int len = der[pos++] & 255;
        if (len < 1 || len > 33 || pos + len > der.length || (der[pos] & 128) != 0 ||
            (len > 1 && der[pos] == 0 && (der[pos + 1] & 128) == 0)) throw invalid();
        int end = pos + len;
        if (der[pos] == 0) { pos++; len--; }
        if (len == 0 || len > 32 ||
            new BigInteger(1, Arrays.copyOfRange(der, pos, end)).compareTo(ORDER) >= 0) throw invalid();
        System.arraycopy(der, pos, out, offset + 32 - len, len);
        return end;
    }

    private static IllegalArgumentException invalid() {
        return new IllegalArgumentException("Invalid P-256 ECDSA DER signature");
    }
}
