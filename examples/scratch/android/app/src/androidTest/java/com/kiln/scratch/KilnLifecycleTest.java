package com.kiln.scratch;

import androidx.test.ext.junit.runners.AndroidJUnit4;

import org.junit.Test;
import org.junit.runner.RunWith;

/**
 * The lifecycle native, called directly, with no activity involved.
 *
 * <p>Deliberately small: a wrong JNI name is an {@code UnsatisfiedLinkError} at
 * call time and never a build error, so reaching Rust at all is the assertion. The
 * states are strings the activity builds rather than values the compiler checks, so
 * an unknown one must not throw either; the crate passes it through for the page to
 * interpret, because dropping it would hide an app-side bug.
 *
 * <p>What an activity does with these, including the rotation that must keep the
 * same instance and leave its page able to answer commands, is in
 * {@code KilnDeviceTest}: it needs a live activity, and the shell only ever starts
 * its event loop for one per process, so it cannot live in a second test method
 * here.
 */
@RunWith(AndroidJUnit4.class)
public class KilnLifecycleTest {
    @Test
    public void everyStateReachesTheNativeSide() {
        MainActivity.kilnLifecycle("resume");
        MainActivity.kilnLifecycle("pause");
        MainActivity.kilnLifecycle("stop");
        MainActivity.kilnLifecycle("teleport");
    }
}
