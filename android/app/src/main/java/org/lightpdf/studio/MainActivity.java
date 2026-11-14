package org.lightpdf.studio;

import android.content.ContentResolver;
import android.content.Intent;
import android.database.Cursor;
import android.graphics.Color;
import android.net.Uri;
import android.os.Bundle;
import android.provider.OpenableColumns;
import android.util.Base64;
import android.util.Log;
import android.webkit.JavascriptInterface;
import android.webkit.WebView;

import androidx.activity.OnBackPressedCallback;
import androidx.core.graphics.Insets;
import androidx.core.view.ViewCompat;
import androidx.core.view.WindowCompat;
import androidx.core.view.WindowInsetsCompat;
import androidx.core.view.WindowInsetsControllerCompat;

import com.getcapacitor.BridgeActivity;

import org.json.JSONObject;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;

/**
 * LightPDF Studio – Android
 * - empfängt PDFs über „Öffnen mit“ und „Teilen“ (Intent VIEW/SEND) und reicht sie an die Web-App weiter
 * - randlose Darstellung: Höhe von Status- und Navigationsleiste wird als CSS-Variable übergeben
 * - Zurück-Taste schließt zuerst Dialoge / den Reader
 */
public class MainActivity extends BridgeActivity {
    private static final String TAG = "LightPDF";
    private String pendingFileJson = null;
    private float insetTop = 0, insetBottom = 0;

    @Override
    public void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        WindowCompat.setDecorFitsSystemWindows(getWindow(), false);
        getWindow().setStatusBarColor(Color.TRANSPARENT);
        getWindow().setNavigationBarColor(Color.TRANSPARENT);
        setSystemBarsDark(false);

        ViewCompat.setOnApplyWindowInsetsListener(getWindow().getDecorView(), (v, insets) -> {
            Insets bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() | WindowInsetsCompat.Type.displayCutout());
            float d = getResources().getDisplayMetrics().density;
            insetTop = bars.top / d;
            insetBottom = bars.bottom / d;
            pushInsets();
            return ViewCompat.onApplyWindowInsets(v, insets);
        });

        setupBridge();
        handleIntent(getIntent());

        getOnBackPressedDispatcher().addCallback(this, new OnBackPressedCallback(true) {
            @Override
            public void handleOnBackPressed() {
                WebView wv = getBridge() != null ? getBridge().getWebView() : null;
                if (wv == null) {
                    finish();
                    return;
                }
                wv.evaluateJavascript(
                    "(function(){return window.handleAndroidHardwareBack ? !!window.handleAndroidHardwareBack() : false})()",
                    value -> {
                        if (!"true".equals(value)) {
                            setEnabled(false);
                            getOnBackPressedDispatcher().onBackPressed();
                            setEnabled(true);
                        }
                    });
            }
        });
    }

    @Override
    public void onResume() {
        super.onResume();
        pushInsets();
        if (pendingFileJson != null) dispatchToWebView();
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        handleIntent(intent);
    }

    /** Hell/Dunkel der Symbole in Status- und Navigationsleiste */
    private void setSystemBarsDark(boolean dark) {
        WindowInsetsControllerCompat c = WindowCompat.getInsetsController(getWindow(), getWindow().getDecorView());
        c.setAppearanceLightStatusBars(!dark);
        c.setAppearanceLightNavigationBars(!dark);
    }

    private void pushInsets() {
        WebView wv = getBridge() != null ? getBridge().getWebView() : null;
        if (wv == null) return;
        final String js = "document.documentElement.style.setProperty('--sat','" + insetTop + "px');"
            + "document.documentElement.style.setProperty('--sab','" + insetBottom + "px');";
        wv.post(() -> wv.evaluateJavascript(js, null));
    }

    private void setupBridge() {
        try {
            WebView wv = getBridge().getWebView();
            wv.setBackgroundColor(Color.parseColor("#fbf9f4"));
            wv.addJavascriptInterface(new Object() {
                @JavascriptInterface
                public String getPendingFile() {
                    String p = pendingFileJson;
                    pendingFileJson = null;
                    return p != null ? p : "";
                }

                @JavascriptInterface
                public String getInsets() {
                    return "{\"top\":" + insetTop + ",\"bottom\":" + insetBottom + "}";
                }

                @JavascriptInterface
                public void setDarkTheme(final boolean dark) {
                    runOnUiThread(() -> {
                        setSystemBarsDark(dark);
                        getBridge().getWebView().setBackgroundColor(Color.parseColor(dark ? "#171a16" : "#fbf9f4"));
                    });
                }
            }, "LightPdfNative");
        } catch (Exception e) {
            Log.e(TAG, "Bridge setup failed", e);
        }
    }

    private void handleIntent(Intent intent) {
        if (intent == null) return;
        Uri uri = null;
        if (Intent.ACTION_VIEW.equals(intent.getAction())) uri = intent.getData();
        else if (Intent.ACTION_SEND.equals(intent.getAction())) uri = intent.getParcelableExtra(Intent.EXTRA_STREAM);
        if (uri != null) readFile(uri);
    }

    private void readFile(final Uri uri) {
        new Thread(() -> {
            try {
                ContentResolver r = getContentResolver();
                String name = "Dokument.pdf";
                if ("content".equals(uri.getScheme())) {
                    try (Cursor c = r.query(uri, null, null, null, null)) {
                        if (c != null && c.moveToFirst()) {
                            int i = c.getColumnIndex(OpenableColumns.DISPLAY_NAME);
                            if (i != -1 && c.getString(i) != null && !c.getString(i).trim().isEmpty()) name = c.getString(i);
                        }
                    } catch (Exception ignored) {
                    }
                } else if (uri.getLastPathSegment() != null) {
                    name = uri.getLastPathSegment();
                }
                if (!name.toLowerCase().endsWith(".pdf")) name = name + ".pdf";
                try (InputStream in = r.openInputStream(uri)) {
                    if (in == null) return;
                    ByteArrayOutputStream out = new ByteArrayOutputStream();
                    byte[] buf = new byte[65536];
                    int n;
                    while ((n = in.read(buf)) != -1) out.write(buf, 0, n);
                    JSONObject json = new JSONObject();
                    json.put("name", name);
                    json.put("base64", Base64.encodeToString(out.toByteArray(), Base64.NO_WRAP));
                    pendingFileJson = json.toString();
                    runOnUiThread(this::dispatchToWebView);
                }
            } catch (Exception e) {
                Log.e(TAG, "Could not read " + uri, e);
            }
        }).start();
    }

    private void dispatchToWebView() {
        if (pendingFileJson == null || getBridge() == null) return;
        final String payload = pendingFileJson;
        WebView wv = getBridge().getWebView();
        wv.post(() -> wv.evaluateJavascript(
            "if(window.__lpReady){window.dispatchEvent(new CustomEvent('lightpdf_native_open_file',{detail:" + payload + "}));'ok'}else{''}",
            v -> {
                // Erst als zugestellt markieren, wenn die App bereit war – sonst holt sie es beim Start selbst ab
                if (v != null && v.contains("ok")) pendingFileJson = null;
            }));
    }
}
