package cn.ccdesk.android;

import android.os.Build;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyInfo;
import android.security.keystore.KeyProperties;
import android.security.keystore.StrongBoxUnavailableException;
import java.io.IOException;
import java.security.GeneralSecurityException;
import java.security.Key;
import java.security.KeyFactory;
import java.security.KeyPairGenerator;
import java.security.KeyStore;
import java.security.PrivateKey;
import java.security.Signature;
import java.security.cert.Certificate;
import java.security.spec.ECGenParameterSpec;
import java.util.UUID;

/** Android API 31+ platform adapter; call only on a dedicated platform worker. */
final class HardwareIdentity {
    enum Usage { ENDPOINT, RELAY, APPROVAL }

    private final PrivateKey key;
    private final Certificate[] chain;
    private final Usage usage;

    private HardwareIdentity(PrivateKey key, Certificate[] chain, Usage usage) {
        this.key = key;
        this.chain = chain;
        this.usage = usage;
    }

    // localId is locally generated; raw QR/intent bytes never select a key alias.
    static synchronized HardwareIdentity provision(UUID localId, Usage usage,
            byte[] hostChallenge, boolean preferStrongBox) throws GeneralSecurityException, IOException {
        if (hostChallenge.length != 32) throw new GeneralSecurityException("Invalid attestation challenge");
        requirePlatform();
        byte[] challenge = hostChallenge.clone();
        String alias = alias(localId, usage);
        KeyStore store = store();
        if (store.containsAlias(alias)) throw new GeneralSecurityException("Identity already provisioned");
        if (preferStrongBox) {
            try { generate(alias, usage, challenge, true); }
            catch (StrongBoxUnavailableException e) {
                if (store().containsAlias(alias)) throw e;
                generate(alias, usage, challenge, false);
            }
        } else {
            generate(alias, usage, challenge, false);
        }
        return open(localId, usage);
    }

    static HardwareIdentity open(UUID localId, Usage usage) throws GeneralSecurityException, IOException {
        requirePlatform();
        KeyStore store = store();
        String alias = alias(localId, usage);
        Key key = store.getKey(alias, null);
        Certificate[] chain = store.getCertificateChain(alias);
        if (!(key instanceof PrivateKey) || chain == null || chain.length < 2 ||
            !"EC".equals(key.getAlgorithm()) || key.getEncoded() != null)
            throw new GeneralSecurityException("Attestable Android identity unavailable");
        KeyInfo info = KeyFactory.getInstance("EC", "AndroidKeyStore")
            .getKeySpec(key, KeyInfo.class);
        boolean secure = info.getSecurityLevel() == KeyProperties.SECURITY_LEVEL_TRUSTED_ENVIRONMENT ||
            info.getSecurityLevel() == KeyProperties.SECURITY_LEVEL_STRONGBOX;
        if (!secure || info.getKeySize() != 256 ||
            info.isUserAuthenticationRequired() != (usage == Usage.APPROVAL) ||
            (usage == Usage.APPROVAL && !info.isUserAuthenticationRequirementEnforcedBySecureHardware()))
            throw new GeneralSecurityException("Hardware key policy unavailable");
        return new HardwareIdentity((PrivateKey) key, chain, usage);
    }

    Certificate[] attestationChain() { return chain.clone(); }
    byte[] spki() { return chain[0].getPublicKey().getEncoded().clone(); }

    byte[] signEndpointTls(byte[] verifiedTlsTranscript) throws GeneralSecurityException {
        return sign(Usage.ENDPOINT, verifiedTlsTranscript).sign();
    }

    byte[] signRelay(byte[] verifiedRelayTranscript) throws GeneralSecurityException {
        return P256Signature.toFixed(sign(Usage.RELAY, verifiedRelayTranscript).sign());
    }

    // Owner 将此 Signature 包入 BiometricPrompt.CryptoObject；先验签/核 scope，
    // 再发用户确认，确认后在原请求 owner 内完成 sign 并编码成 COSE 定长签名。
    PendingApproval prepareApproval(byte[] verifiedGrantTbs) throws GeneralSecurityException {
        return new PendingApproval(newSignature(Usage.APPROVAL, verifiedGrantTbs), verifiedGrantTbs);
    }

    static final class PendingApproval {
        private final Signature signature;
        private final byte[] tbs;
        private boolean finished;

        private PendingApproval(Signature signature, byte[] tbs) {
            this.signature = signature;
            this.tbs = tbs.clone();
        }

        android.hardware.biometrics.BiometricPrompt.CryptoObject cryptoObject() {
            return new android.hardware.biometrics.BiometricPrompt.CryptoObject(signature);
        }

        synchronized byte[] finish(android.hardware.biometrics.BiometricPrompt.AuthenticationResult result)
                throws GeneralSecurityException {
            if (finished || result == null || result.getCryptoObject() == null ||
                result.getCryptoObject().getSignature() != signature)
                throw new GeneralSecurityException("Original biometric operation required");
            finished = true;
            signature.update(tbs);
            return P256Signature.toFixed(signature.sign());
        }
    }

    private Signature sign(Usage expected, byte[] transcript) throws GeneralSecurityException {
        Signature signature = newSignature(expected, transcript);
        signature.update(transcript);
        return signature;
    }

    private Signature newSignature(Usage expected, byte[] transcript) throws GeneralSecurityException {
        if (usage != expected || transcript.length == 0 || transcript.length > 65536)
            throw new GeneralSecurityException("Signing request outside adapter contract");
        Signature signature = Signature.getInstance("SHA256withECDSA");
        signature.initSign(key);
        return signature;
    }

    private static void generate(String alias, Usage usage, byte[] challenge, boolean strongBox)
            throws GeneralSecurityException {
        KeyGenParameterSpec.Builder spec = new KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_SIGN)
            .setAlgorithmParameterSpec(new ECGenParameterSpec("secp256r1"))
            .setDigests(KeyProperties.DIGEST_SHA256)
            .setAttestationChallenge(challenge)
            .setIsStrongBoxBacked(strongBox);
        if (usage == Usage.APPROVAL) spec.setUserAuthenticationRequired(true)
            .setUserAuthenticationParameters(0, KeyProperties.AUTH_BIOMETRIC_STRONG);
        KeyPairGenerator generator = KeyPairGenerator.getInstance("EC", "AndroidKeyStore");
        generator.initialize(spec.build());
        generator.generateKeyPair();
    }

    private static KeyStore store() throws GeneralSecurityException, IOException {
        KeyStore store = KeyStore.getInstance("AndroidKeyStore");
        store.load(null);
        return store;
    }

    private static String alias(UUID localId, Usage usage) {
        return "cc-desk.v1." + usage.name() + "." + localId;
    }

    private static void requirePlatform() throws GeneralSecurityException {
        if (Build.VERSION.SDK_INT < 31) throw new GeneralSecurityException("Android API 31+ required");
    }
}
