package cn.ccdesk.android;

import android.content.Context;
import android.util.AtomicFile;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.UUID;
import org.json.JSONException;
import org.json.JSONObject;

/** 保存配对意图，不保存配对码。只有完成原生收据验证后才能恢复设备。 */
final class PairingRecoveryJournal {
    private static final Object LOCK = new Object();
    private static final String SUFFIX = ".json";
    private static final int LIMIT = 64;
    private static final int MAX_RECORD = 2048;
    private static final long STALE_MS = 24 * 60 * 60 * 1000L;
    private final File directory;
    private final File receipts;

    PairingRecoveryJournal(Context context) {
        File root = context.getNoBackupFilesDir();
        directory = new File(root, "pairing-recovery-v1");
        receipts = new File(root, "native-pairing-receipts-v1");
    }

    void stage(PairingRecoveryLink link) throws IOException {
        synchronized (LOCK) {
            if (!directory.isDirectory() && !directory.mkdirs()) throw new IOException("Pairing journal unavailable");
            File[] entries = entries();
            long now = System.currentTimeMillis();
            int count = 0;
            for (File entry : entries) {
                UUID id = idOf(entry);
                if (id == null) continue;
                String receiptName = id.toString().replace("-", "") + ".aead";
                long modified = entry.lastModified();
                if (modified > 0 && now - modified > STALE_MS
                    && !new File(receipts, receiptName).exists()
                    && !new File(receipts, receiptName + ".new").exists()
                    && !new File(receipts, receiptName + ".bak").exists()) {
                    file(id).delete();
                } else count++;
            }
            if (count >= LIMIT) throw new IOException("Pairing journal full");
            AtomicFile target = file(link.id);
            if (target.getBaseFile().exists()) throw new IOException("Pairing request already staged");
            byte[] bytes;
            try {
                bytes = new JSONObject().put("relay", link.relay.toString()).put("commitment", link.commitment)
                    .toString().getBytes(StandardCharsets.UTF_8);
            } catch (JSONException invalid) {
                throw new IOException("Invalid pairing journal", invalid);
            }
            FileOutputStream stream = target.startWrite();
            try {
                stream.write(bytes);
                stream.getFD().sync();
                target.finishWrite(stream);
            } catch (IOException | RuntimeException failure) {
                target.failWrite(stream);
                throw failure;
            }
            if (!java.util.Arrays.equals(bytes, target.readFully()))
                throw new IOException("Pairing journal save unconfirmed");
        }
    }

    List<PairingRecoveryLink> read() throws IOException {
        synchronized (LOCK) {
            List<PairingRecoveryLink> out = new ArrayList<>();
            if (!directory.exists()) return out;
            int visited = 0;
            for (File entry : entries()) {
                UUID id = idOf(entry);
                if (id == null) continue;
                if (++visited > LIMIT) throw new IOException("Pairing journal too large");
                try {
                    if (entry.length() > MAX_RECORD) continue;
                    byte[] bytes = file(id).readFully();
                    if (bytes.length > MAX_RECORD) continue;
                    JSONObject value = new JSONObject(new String(bytes, StandardCharsets.UTF_8));
                    if (value.length() != 2) continue;
                    out.add(new PairingRecoveryLink(id, URI.create(value.getString("relay")),
                        value.getString("commitment")));
                } catch (IOException | JSONException | IllegalArgumentException invalid) {
                    // 一条损坏的本机提示不影响已配对设备。
                }
            }
            return out;
        }
    }

    boolean promote(PairingRecoveryLink link, DesktopBackend.Desktop approved, DesktopStore store)
            throws IOException {
        synchronized (LOCK) {
            // 用户取消后，已经读到提示的另一个 Activity 也不能补写设备。
            if (!file(link.id).getBaseFile().exists()) return false;
            store.addApproved(approved);
            remove(link.id);
            return true;
        }
    }

    void remove(UUID id) {
        synchronized (LOCK) {
            AtomicFile target = file(id);
            target.delete();

        }
    }

    private File[] entries() throws IOException {
        File[] result = directory.listFiles();
        if (result == null || result.length > LIMIT * 3) throw new IOException("Pairing journal unavailable");
        return result;
    }

    private static UUID idOf(File entry) {
        String value = entry.getName();
        if (!value.matches("[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\\.json"))
            return null;
        return UUID.fromString(value.substring(0, value.length() - SUFFIX.length()));
    }

    private AtomicFile file(UUID id) { return new AtomicFile(new File(directory, id + SUFFIX)); }
}
