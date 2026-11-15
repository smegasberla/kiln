package com.kiln.example;

import android.util.Log;
import android.view.ViewGroup;
import android.webkit.WebView;

/**
 * The activity the OS launches.
 *
 * <p>Two things belong here and nowhere else:
 *
 * <ol>
 *   <li>Loading the Rust library. It has to happen before the activity's super
 *       {@code onCreate} reaches wry's {@code Rust.onCreate}, so it is a static
 *       initialiser.
 *   <li>Attaching the WebView. wry creates the view and hands it to
 *       {@code onWebViewCreate}, but deliberately does not install it: the app owns
 *       its layout. A WebView that is created and never attached paints nothing
 *       and looks exactly like a page that failed to load.
 * </ol>
 *
 * <p>Note what is <em>not</em> here: calling {@code android_main}. tao invokes the
 * app's entry point itself, from {@code onFirstActivityCreate}, which runs when the
 * process lifecycle starts (after {@code onCreate} returns). Calling it here too
 * would start the event loop twice and before stdout is wired to logcat.
 */
public class MainActivity extends WryActivity {
    private static final String TAG = "Kiln";

    static {
        System.loadLibrary("kiln_example");
    }

    @Override
    public void onWebViewCreate(WebView webView) {
        Log.i(TAG, "onWebViewCreate: attaching the webview");
        setContentView(webView);
        webView.setLayoutParams(new ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT));
    }
}
