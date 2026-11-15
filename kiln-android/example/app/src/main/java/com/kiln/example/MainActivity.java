package com.kiln.example;

import android.os.Bundle;
import android.util.Log;

/**
 * The activity the OS launches.
 *
 * <p>wry's {@code WryActivity} owns the WebView and the lifecycle forwarding, so
 * this subclass adds only what Kiln needs: loading the Rust library and calling
 * the native entry point once.
 */
public class MainActivity extends WryActivity {
    private static final String TAG = "Kiln";

    static {
        System.loadLibrary("kiln_example");
    }

    /** Native entry point, exported by kiln_android::register_jni!. */
    public static native void android_main();

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        Log.i(TAG, "MainActivity.onCreate");
        super.onCreate(savedInstanceState);
        Log.i(TAG, "calling android_main");
        android_main();
        Log.i(TAG, "android_main returned");
    }
}
