package cn.ccdesk.android;

import android.content.Context;
import android.util.AtomicFile;
import java.io.File;
import java.io.FileNotFoundException;
import java.io.FileOutputStream;
import java.io.IOException;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import java.util.UUID;
import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

/** App-private, excluded from backup. Metadata only: no grants, invitations or private keys. */
final class DesktopStore {
    private static final int LIMIT = 32;
    private final AtomicFile file;
    DesktopStore(Context context) {
        file = new AtomicFile(new File(context.getNoBackupFilesDir(), "approved-desktops-v1.json"));
    }

    List<DesktopBackend.Desktop> read() throws IOException {
        synchronized (DesktopStore.class) { return readLocked(); }
    }

    private List<DesktopBackend.Desktop> readLocked() throws IOException {
        try {
            if (file.getBaseFile().length() > 65536) throw new IOException("Desktop index too large");
            byte[] bytes = file.readFully();
            if (bytes.length > 65536) throw new IOException("Desktop index too large");
            JSONArray array = new JSONArray(new String(bytes, StandardCharsets.UTF_8));
            if (array.length() > LIMIT) throw new IOException("Desktop index too large");
            List<DesktopBackend.Desktop> rows = new ArrayList<>();
            Set<String> identities = new HashSet<>();
            Set<UUID> localIds = new HashSet<>();
            for (int i = 0; i < array.length(); i++) {
                JSONObject o = array.getJSONObject(i);
                DesktopBackend.Desktop desktop = new DesktopBackend.Desktop(
                    UUID.fromString(o.getString("localId")), o.getString("hostId"),
                    o.getString("name"), URI.create(o.getString("relay")));
                if (!identities.add(desktop.hostId) || !localIds.add(desktop.localId)) {
                    throw new IOException("Conflicting desktop identity");
                }
                rows.add(desktop);
            }
            return rows;
        } catch (FileNotFoundException e) {
            if (file.getBaseFile().exists()) throw e;
            return new ArrayList<>();
        } catch (JSONException | IllegalArgumentException e) {
            throw new IOException("Invalid desktop index", e);
        }
    }

    void addApproved(DesktopBackend.Desktop desktop) throws IOException {
        synchronized (DesktopStore.class) { addApprovedLocked(desktop); }
    }

    private void addApprovedLocked(DesktopBackend.Desktop desktop) throws IOException {
        List<DesktopBackend.Desktop> existing = readLocked();
        for (DesktopBackend.Desktop row : existing) {
            if (row.localId.equals(desktop.localId) && row.hostId.equals(desktop.hostId)
                && row.relay.equals(desktop.relay)) return;
            if (row.hostId.equals(desktop.hostId) || row.localId.equals(desktop.localId))
                throw new IOException("Desktop already indexed; re-authorization required");
        }
        if (existing.size() >= LIMIT) throw new IOException("Desktop index full");
        existing.add(desktop);
        write(existing);
    }

    void renameAlias(DesktopBackend.Desktop expected, String alias) throws IOException {
        synchronized (DesktopStore.class) { renameAliasLocked(expected, alias); }
    }

    private void renameAliasLocked(DesktopBackend.Desktop expected, String alias) throws IOException {
        List<DesktopBackend.Desktop> existing = readLocked();
        for (int i = 0; i < existing.size(); i++) {
            DesktopBackend.Desktop row = existing.get(i);
            if (row.localId.equals(expected.localId) && row.hostId.equals(expected.hostId)
                && row.relay.equals(expected.relay)) {
                DesktopBackend.Desktop renamed = row.withAlias(alias);
                if (!renamed.name.equals(row.name)) {
                    existing.set(i, renamed);
                    write(existing);
                }
                return;
            }
        }
        throw new IOException("Desktop index changed");
    }

    private void write(List<DesktopBackend.Desktop> existing) throws IOException {
        JSONArray array = new JSONArray();
        try {
            for (DesktopBackend.Desktop row : existing) {
                array.put(new JSONObject().put("localId", row.localId.toString())
                    .put("hostId", row.hostId).put("name", row.name)
                    .put("relay", row.relay.toString()));
            }
        } catch (JSONException e) { throw new IOException("Could not serialize desktop index", e); }

        byte[] bytes = array.toString().getBytes(StandardCharsets.UTF_8);
        if (bytes.length > 65536) throw new IOException("Desktop index too large");
        FileOutputStream stream = file.startWrite();
        try {
            stream.write(bytes);
            stream.getFD().sync();
            file.finishWrite(stream);
        } catch (IOException | RuntimeException e) {
            file.failWrite(stream);
            throw e;
        }
        if (!java.util.Arrays.equals(bytes, file.readFully()))
            throw new IOException("Desktop index save unconfirmed");
    }
}
