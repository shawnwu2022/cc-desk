package cn.ccdesk.android;

import java.security.KeyPair;
import java.security.KeyPairGenerator;
import java.security.Signature;
import java.security.spec.ECGenParameterSpec;
import java.util.Arrays;
import java.util.HexFormat;

public final class P256SignatureTest {
    public static void main(String[] args) throws Exception {
        HexFormat hex = HexFormat.of();
        byte[] fixed = P256Signature.toFixed(hex.parseHex("300702020080020101"));
        if (fixed[31] != (byte) 128 || fixed[63] != 1) throw new AssertionError();
        for (String malformed : new String[] {
             "", "3006020100020101", "3006020180020101", "300702020001020101",
             "300602010102010100", "308106020101020101", "3006020101030101",
             "3026022100ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551020101"
        }) {
            try {
                P256Signature.toFixed(hex.parseHex(malformed));
            } catch (IllegalArgumentException expected) {
                continue;
            }
            throw new AssertionError(malformed);
        }

        KeyPairGenerator generator = KeyPairGenerator.getInstance("EC");
        generator.initialize(new ECGenParameterSpec("secp256r1"));
        KeyPair pair = generator.generateKeyPair(); // JVM fixture only.
        Signature signer = Signature.getInstance("SHA256withECDSA");
        signer.initSign(pair.getPrivate());
        Signature verifier = Signature.getInstance("SHA256withECDSAinP1363Format");
        verifier.initVerify(pair.getPublic());
        for (int i = 0; i < 128; i++) {
            byte[] message = new byte[] {(byte) i, (byte) 0x7f};
            signer.update(message);
            verifier.update(message);
            byte[] converted = P256Signature.toFixed(signer.sign());
            if (!verifier.verify(converted)) throw new AssertionError(i);
            verifier.update(Arrays.copyOf(message, 1));
            if (verifier.verify(converted)) throw new AssertionError("wrong message");
        }
        System.out.println("P256Signature: 9 format cases and 128 real JDK P-256 roundtrips passed");
    }
}
