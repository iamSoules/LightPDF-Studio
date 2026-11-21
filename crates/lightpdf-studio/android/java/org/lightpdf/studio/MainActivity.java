package org.lightpdf.studio;

import android.app.NativeActivity;
import android.content.ClipData;
import android.content.Intent;
import android.net.Uri;
import java.util.ArrayList;

/**
 * NativeActivity plus the system file picker (Storage Access Framework).
 * A plain NativeActivity never receives onActivityResult, so the picked URIs are kept
 * here until the Rust side collects them with takePicked().
 */
public class MainActivity extends NativeActivity {
    private static final int PICK = 4711;
    private final Object lock = new Object();
    private String[] picked; // null = nothing to report

    /** Opens the system picker. Called from Rust on the native thread. */
    public void pickFiles(final String[] mimeTypes, final boolean multiple) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                Intent i = new Intent(Intent.ACTION_OPEN_DOCUMENT);
                i.addCategory(Intent.CATEGORY_OPENABLE);
                i.setType(mimeTypes.length == 1 ? mimeTypes[0] : "*/*");
                if (mimeTypes.length > 1) i.putExtra(Intent.EXTRA_MIME_TYPES, mimeTypes);
                i.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, multiple);
                try {
                    startActivityForResult(i, PICK);
                } catch (Exception e) {
                    synchronized (lock) { picked = new String[0]; }
                }
            }
        });
    }

    /** null while the picker is open (or nothing was requested), empty array = cancelled. */
    public String[] takePicked() {
        synchronized (lock) {
            String[] p = picked;
            picked = null;
            return p;
        }
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != PICK) return;
        ArrayList<String> out = new ArrayList<>();
        if (resultCode == RESULT_OK && data != null) {
            ClipData clip = data.getClipData();
            if (clip != null) {
                for (int k = 0; k < clip.getItemCount(); k++) add(out, clip.getItemAt(k).getUri(), data);
            } else {
                add(out, data.getData(), data);
            }
        }
        synchronized (lock) { picked = out.toArray(new String[0]); }
    }

    private void add(ArrayList<String> out, Uri uri, Intent data) {
        if (uri == null) return;
        // Keep access across restarts so "Save" can write back to the picked file.
        int flags = data.getFlags() & (Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
        try {
            getContentResolver().takePersistableUriPermission(uri, flags);
        } catch (Exception ignored) {
        }
        out.add(uri.toString());
    }
}
