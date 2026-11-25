package cn.ccdesk.android;

import android.content.Context;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyInfo;
import android.security.keystore.KeyProperties;
import android.util.AtomicFile;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.security.GeneralSecurityException;
import java.security.Key;
import java.security.KeyStore;
import java.util.Arrays;
import java.util.UUID;
import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.SecretKey;
import javax.crypto.SecretKeyFactory;
import javax.crypto.spec.GCMParameterSpec;

/** App-private, non-backup and insert-only sealed native receipt storage. */
final class AndroidPairingReceiptStore {
    private static final int MAX_RECORD = 65536;
    private static final Object LOCK = new Object();
    private static final String ALIAS = "cc-desk.v1.native-receipt-seal";
    private final String id;
    private final File dir;

    AndroidPairingReceiptStore(Context context, UUID localId) {
        id = localId.toString().replace("-", "");
        dir = new File(context.getNoBackupFilesDir(), "native-pairing-receipts-v1");
    }

    boolean insertOnce(String lowerHexId, byte[] record) {
        if (!id.equals(lowerHexId) || record == null || record.length == 0 ||
            record.length > MAX_RECORD) return false;
        synchronized (LOCK) {
            if ((!dir.isDirectory() && !dir.mkdirs())) return false;
            File path = new File(dir, id + ".aead");
            if (path.exists() || new File(dir, id + ".aead.bak").exists() ||
                new File(dir, id + ".aead.new").exists()) return false;
            AtomicFile file = new AtomicFile(path);
            byte[] encoded = null;
            try {
                Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
                cipher.init(Cipher.ENCRYPT_MODE, key(true));
                // Per-file immutable AAD binds purpose, storage version and local enrollment owner.
                cipher.updateAAD(("CC-DESK-NATIVE-PAIRING-RECEIPT-v1:" + id)
                    .getBytes(StandardCharsets.US_ASCII));
                byte[] iv = cipher.getIV();
                if (iv == null || iv.length != 12) return false;
                byte[] ciphertext = cipher.doFinal(record);
                encoded = ByteBuffer.allocate(1 + iv.length + ciphertext.length).put((byte)1)
                    .put(iv).put(ciphertext).array();
                FileOutputStream stream = file.startWrite();
                try {
                    stream.write(encoded);
                    stream.getFD().sync();
                } catch (IOException | RuntimeException e) {
                    file.failWrite(stream);
                    return false;
                }
                file.finishWrite(stream);
                return path.length() == encoded.length && Arrays.equals(encoded, file.readFully());
            } catch (GeneralSecurityException | IOException | RuntimeException e) {
                return false;
            } finally {
                if (encoded != null) Arrays.fill(encoded, (byte)0);
                Arrays.fill(record, (byte)0);
            }
        }
    }

    // The returned bytes are opaque and must be fully verified by Rust again.
    byte[] readSealed(String lowerHexId) {
        if (!id.equals(lowerHexId)) return null;
        synchronized (LOCK) {
            File path = new File(dir, id + ".aead");
            if (path.length() < 30 || path.length() > MAX_RECORD + 29) return null;
            try {
                byte[] bytes = new AtomicFile(path).readFully();
                if (bytes.length < 30 || bytes.length > MAX_RECORD + 29 || bytes[0] != 1) return null;
                Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
                cipher.init(Cipher.DECRYPT_MODE, key(false), new GCMParameterSpec(128, bytes, 1, 12));
                cipher.updateAAD(("CC-DESK-NATIVE-PAIRING-RECEIPT-v1:" + id)
                    .getBytes(StandardCharsets.US_ASCII));
                return cipher.doFinal(bytes, 13, bytes.length - 13);
            } catch (GeneralSecurityException | IOException | RuntimeException e) {
                return null;
            }
        }
    }

    private static SecretKey key(boolean create) throws GeneralSecurityException, IOException {
        KeyStore ks = KeyStore.getInstance("AndroidKeyStore");
        ks.load(null);
        if (!ks.containsAlias(ALIAS)) {
            if (!create) throw new GeneralSecurityException("Receipt key unavailable");
            KeyGenerator g = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore");
            g.init(new KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT |
                    KeyProperties.PURPOSE_DECRYPT)
                .setKeySize(256)
                .setRandomizedEncryptionRequired(true)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build());
            g.generateKey();
        }
        Key raw = ks.getKey(ALIAS, null);
        if (!(raw instanceof SecretKey) || raw.getEncoded() != null ||
            !KeyProperties.KEY_ALGORITHM_AES.equals(raw.getAlgorithm())) {
            throw new GeneralSecurityException("Required receipt sealer unavailable");
        }
        SecretKey key = (SecretKey)raw;
        KeyInfo info = (KeyInfo)SecretKeyFactory.getInstance("AES", "AndroidKeyStore")
            .getKeySpec(key, KeyInfo.class);
        if (info.getKeySize() != 256 || (info.getSecurityLevel() != KeyProperties.SECURITY_LEVEL_STRONGBOX &&
            info.getSecurityLevel() != KeyProperties.SECURITY_LEVEL_TRUSTED_ENVIRONMENT))
            throw new GeneralSecurityException("Hardware receipt sealer unavailable");
        return key;
    }
}
