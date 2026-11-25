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

    synchronized List<DesktopBackend.Desktop> read() throws IOException {
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

    synchronized void addApproved(DesktopBackend.Desktop desktop) throws IOException {
        List<DesktopBackend.Desktop> existing = read();
        if (existing.size() >= LIMIT) throw new IOException("Desktop index full");
        for (DesktopBackend.Desktop row : existing) {
            if (row.hostId.equals(desktop.hostId) || row.localId.equals(desktop.localId))
                throw new IOException("Desktop already indexed; re-authorization required");
        }
        existing.add(desktop);
        JSONArray array = new JSONArray();
        try {
            for (DesktopBackend.Desktop row : existing) {
                array.put(new JSONObject().put("localId", row.localId.toString())
                    .put("hostId", row.hostId).put("name", row.name)
                    .put("relay", row.relay.toString()));
            }
        } catch (JSONException e) { throw new IOException("Could not serialize desktop index", e); }

        FileOutputStream stream = file.startWrite();
        try {
            stream.write(array.toString().getBytes(StandardCharsets.UTF_8));
            file.finishWrite(stream);
        } catch (IOException | RuntimeException e) {
            file.failWrite(stream);
            throw e;
        }
    }
}
