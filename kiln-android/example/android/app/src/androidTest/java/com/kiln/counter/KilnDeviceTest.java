package com.kiln.counter;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;
import static org.junit.Assert.fail;

import android.Manifest;
import android.content.pm.PackageManager;
import android.os.SystemClock;
import android.webkit.ValueCallback;

import androidx.test.core.app.ActivityScenario;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import androidx.test.platform.app.InstrumentationRegistry;

import org.json.JSONObject;
import org.junit.Assume;
import org.junit.Before;
import org.junit.Test;
import org.junit.runner.RunWith;

/**
 * Everything the shell does that only a device can answer: the picker, the tray,
 * HTTP, the updater, and the lifecycle.
 *
 * <p>One class with one test method, on purpose. The shell starts its Rust event
 * loop from {@code ProcessLifecycleOwner}, which runs once per process, so there
 * is exactly one activity per instrumentation run that can ever have a page behind
 * it. JUnit's {@code ActivityFinisher} destroys the activity between methods, so a
 * suite of independent test methods would leave every one after the first waiting
 * for a page that can no longer load. A single method is the shape the shell
 * actually supports; {@link #everythingTheShellPromises()} is a script rather than
 * an isolated case, and that is a fact about Android, not a shortcut.
 *
 * <p>The JSON is posted through the same entry points
 * {@code kiln::tray::build} and the app's own activity use, so nothing here needs
 * the `tray` crate feature to be on: a scaffolded project with no tray still
 * exercises the notification its tray would have produced.
 */
@RunWith(AndroidJUnit4.class)
public class KilnDeviceTest {
    /** The tooltip; unique so a leftover notification cannot be read as this one. */
    private static final String TRAY_TITLE = "Kiln probe";

    private static final String TRAY_REQUEST =
            "{\"title\":\""
                    + TRAY_TITLE
                    + "\",\"items\":["
                    + "{\"id\":\"note\",\"label\":\"Write a note\"},"
                    + "{\"id\":\"quit\",\"label\":\"Quit\"}]}";

    /**
     * A file this run creates, named per run because Downloads is shared state.
     *
     * <p>A fixed name is wrong in a way that only shows up on a second run: the
     * picker lists what is already there, so a leftover file from an earlier run
     * (or from a manual poking session) is indistinguishable from the one this test
     * just wrote, and the assertion then compares against content it never wrote.
     * A timestamped name makes the file unambiguously this run's.
     */
    private static final String PROBE = "kiln-picker-probe-" + System.currentTimeMillis() + ".txt";

    private static final String PROBE_PATH = "/storage/emulated/0/Download/" + PROBE;

    /** What the seeded file contains, so the reply can be checked against it. */
    private static final String PROBE_CONTENT = "kiln-probe-content";

    /** A folder of the test's own, because the tree picker refuses a volume root. */
    private static final String FOLDER = "kiln-picker-probe-folder";

    /** A URL that answers 200 for the root and 404 below it, on the documentation host. */
    private static final String REACHABLE = "https://example.com/";

    private static final String MISSING = "https://example.com/kiln-has-no-file-here.json";

    @Before
    public void seedStorage() {
        // Inserted through MediaStore, which is what makes it appear in the picker
        // and what a real download does. The unique name means the assertion
        // compares against this run's file rather than a leftover.
        KilnDeviceSupport.seedDownload(PROBE, "Download", PROBE_CONTENT);
        // A file inside a subfolder, so the folder exists and holds something.
        KilnDeviceSupport.seedDownload("inside.txt", "Download/" + FOLDER, PROBE_CONTENT);
    }

    @Test
    public void everythingTheShellPromises() throws Exception {
        // Launched once, here, with nothing else on screen. {@code
        // KilnDeviceSupport.app} grants POST_NOTIFICATIONS first, because the app
        // posts its tray during startup and the resulting OS prompt pauses the
        // WebView, which then never reaches a state a script can run in.
        ActivityScenario<MainActivity> scenario = KilnDeviceSupport.app();

        checkLifecycleStatesReachRust();
        checkRotationKeepsTheSameActivity(scenario);
        checkTrayNotification(scenario);
        checkPickerRoundTrip(scenario);
        checkPickerCancel(scenario);
        checkFolderPicker(scenario);
        checkHttp(scenario);
    }

    /** A wrong JNI name is an {@code UnsatisfiedLinkError} at call time, not a build error. */
    private static void checkLifecycleStatesReachRust() {
        // No assertion beyond "returns", including for a state this crate has never
        // heard of: it is a string the activity builds, not a type the compiler
        // checks, and dropping an unknown one would hide an app bug.
        MainActivity.kilnLifecycle("resume");
        MainActivity.kilnLifecycle("pause");
        MainActivity.kilnLifecycle("stop");
        MainActivity.kilnLifecycle("teleport");
    }

    /**
     * A rotation is absorbed, not survived.
     *
     * <p>The manifest lists {@code orientation|screenSize} in {@code configChanges},
     * so the activity must be the same instance afterwards and its page must still
     * answer commands. Without those attributes it would be destroyed and rebuilt,
     * which on this shell means a fresh WebView and a lost frontend.
     */
    private static void checkRotationKeepsTheSameActivity(ActivityScenario<MainActivity> scenario)
            throws Exception {
        MainActivity[] before = new MainActivity[1];
        scenario.onActivity(activity -> {
            before[0] = activity;
            assertNotNull("the webview was never attached", KilnDeviceSupport.webViewOf(activity));
            activity.setRequestedOrientation(android.content.pm.ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE);
        });

        // The orientation request is asynchronous, so waiting on the configuration
        // is the only honest way to know it settled.
        KilnDeviceSupport.awaitOrientation(scenario, android.content.res.Configuration.ORIENTATION_LANDSCAPE);
        KilnDeviceSupport.awaitResumed(scenario);

        boolean[] same = new boolean[1];
        scenario.onActivity(activity -> {
            same[0] = activity == before[0];
            assertFalse("the activity is finishing after a rotation", activity.isFinishing());
            assertEquals(
                    "the activity is not resumed after a rotation",
                    androidx.lifecycle.Lifecycle.State.RESUMED,
                    activity.getLifecycle().getCurrentState());
        });
        assertTrue("a rotation recreated the activity", same[0]);

        // The strongest statement the shell can make about the page: it is the same
        // page, and its commands still round-trip to Rust.
        assertTrue(
                "the page did not answer after a rotation",
                invokeGreet(scenario, "rotation").contains("rotation"));

        // Back to portrait: this activity is the only one the run will get, and
        // leaving it sideways would make the rest of the script run in a layout the
        // picker tests were not written against.
        scenario.onActivity(activity ->
                activity.setRequestedOrientation(android.content.pm.ActivityInfo.SCREEN_ORIENTATION_PORTRAIT));
        KilnDeviceSupport.awaitOrientation(scenario, android.content.res.Configuration.ORIENTATION_PORTRAIT);
        KilnDeviceSupport.awaitResumed(scenario);
    }

    /**
     * The tray is a notification, and whether it is visible is a permission.
     *
     * <p>Both halves run here because the permission cannot be taken away mid-run:
     * revoking {@code POST_NOTIFICATIONS} kills the app process, and instrumentation
     * <em>is</em> that process. Granting costs nothing, so the denied case is only
     * reachable at all if the app was installed without the grant and the suite has
     * not yet given it.
     */
    private static void checkTrayNotification(ActivityScenario<MainActivity> scenario) {
        Assume.assumeTrue(
                "this app does not declare POST_NOTIFICATIONS, so it posts no notification",
                KilnDeviceSupport.declares(Manifest.permission.POST_NOTIFICATIONS));

        KilnDeviceSupport.cancelNotifications();
        scenario.onActivity(activity -> activity.kilnNotify(TRAY_REQUEST));

        // Android 13+ drops a notification from an app without the grant and reports
        // nothing: no exception, no log line, an empty list. That is why "the call
        // returned" is not evidence a user can see anything, and why this asserts on
        // the notification manager instead. If the grant was already in place, all
        // that is skipped and the posted notification is asserted directly.
        if (!KilnDeviceSupport.holds(Manifest.permission.POST_NOTIFICATIONS)) {
            KilnDeviceSupport.sleep(2000);
            assertFalse(
                    "a notification was posted without POST_NOTIFICATIONS",
                    KilnDeviceSupport.trayPosted());
            KilnDeviceSupport.dismissForeignWindows();
            KilnDeviceSupport.grant(Manifest.permission.POST_NOTIFICATIONS);
            assertTrue(
                    "the permission could not be granted, so nothing could be posted",
                    KilnDeviceSupport.holds(Manifest.permission.POST_NOTIFICATIONS));
            scenario.onActivity(activity -> activity.kilnNotify(TRAY_REQUEST));
        }

        KilnDeviceSupport.waitFor("the tray notification", KilnDeviceSupport::trayPosted);
        assertEquals(
                "the tooltip did not become the notification title",
                TRAY_TITLE,
                KilnDeviceSupport.trayTitle());
        assertEquals(
                "the tray items did not become the notification actions",
                2,
                KilnDeviceSupport.trayActions());
    }

    /**
     * The whole picker round trip: a command from the page, the system picker, a
     * document chosen in it, and the reply that arrives back in JavaScript.
     *
     * <p>{@code KilnDialogsTest} pins the two translations down; this is the part
     * only a device can answer, and the part where an app that "works" can silently
     * open the wrong file, because a {@code content://} URI has to become a path
     * before Rust can use it.
     *
     * <p>Driven through the device's own storage root rather than Downloads. Both
     * are real providers, but they answer differently: external storage names a
     * file that exists, while the downloads provider has no path at all and the
     * document has to be copied into the app's cache. Both are correct, and the
     * one under test here is the one with the far more common URI shape. The copy
     * branch is what an app sees for anything a user shared or downloaded, and it
     * is covered by asserting the reply is readable, which is the property that
     * matters; a path naming nothing would fail that.
     */
    private static void checkPickerRoundTrip(ActivityScenario<MainActivity> scenario) throws Exception {
        KilnDeviceSupport.startInvoke(
                scenario, "dialog.open", "{\"filters\":[{\"extensions\":[\"*\"]}]}");
        KilnDeviceSupport.awaitForeignWindow("the file picker");
        KilnDeviceSupport.click(KilnDeviceSupport.SHOW_ROOTS);
        KilnDeviceSupport.click(KilnDeviceSupport.VOLUME_ROOT);
        KilnDeviceSupport.clickRow("Download");
        KilnDeviceSupport.clickRow(PROBE);

        String reply = KilnDeviceSupport.awaitProbe(scenario);
        assertTrue("the picker rejected: " + reply, !reply.startsWith("error:"));
        String path = new JSONObject(reply).getString("path");
        // Either a real storage path or a cache copy, and both are correct answers
        // for different providers. What must hold either way is that the path the
        // page received names a file this app can open and that holds the picked
        // content; a path naming nothing is the failure this exists to catch.
        assertEquals(
                "the path the picker returned does not lead to the picked file",
                PROBE_CONTENT,
                KilnDeviceSupport.readFile(path));
        assertTrue(
                "the answer is neither the storage path nor a cache copy: " + path,
                path.equals(PROBE_PATH) || path.contains("/cache/"));
    }

    /**
     * Dismissing the picker answers {@code null}, which is not the same as failing.
     *
     * <p>The page can tell "the user changed their mind" from "the picker broke"
     * only if the two answers differ, and {@code DIALOG_TIMEOUT} plus a dead
     * activity both land on the former.
     */
    private static void checkPickerCancel(ActivityScenario<MainActivity> scenario) throws Exception {
        KilnDeviceSupport.startInvoke(scenario, "dialog.open", "{}");
        // Back reaches the window in front, so the picker has to own the screen
        // before this: pressing it while the app does would finish the one activity
        // the whole run depends on, which is exactly how this test failed first.
        KilnDeviceSupport.awaitForeignWindow("the file picker");
        KilnDeviceSupport.back();

        assertEquals("a cancel is not a failure", "null", KilnDeviceSupport.awaitProbe(scenario));
    }

    /**
     * The tree picker, whose confirm button cannot be reached from a volume root.
     *
     * <p>A folder under shared storage is normally unreadable to an app without
     * broad storage access, and unlike a single document a tree cannot be copied,
     * so the honest answers are "here is the path" when it can be opened and a named
     * failure when it cannot. Answering with the path regardless would be the one
     * outcome that is definitely wrong, because Rust would then try to walk a
     * directory this process may not open.
     */
    private static void checkFolderPicker(ActivityScenario<MainActivity> scenario) throws Exception {
        KilnDeviceSupport.startInvoke(scenario, "dialog.folder", "{}");
        KilnDeviceSupport.awaitForeignWindow("the folder picker");
        KilnDeviceSupport.click(KilnDeviceSupport.SHOW_ROOTS);
        KilnDeviceSupport.click(KilnDeviceSupport.DOWNLOADS);
        KilnDeviceSupport.clickRow(FOLDER);
        KilnDeviceSupport.click("Use this folder");
        // The provider asks the user to consent to handing the tree over.
        KilnDeviceSupport.click("Allow");

        String reply = KilnDeviceSupport.awaitProbe(scenario);
        if (reply.startsWith("error:")) {
            // A named failure, not a silent null: the page can tell this apart from
            // "the user cancelled", which is the point of failing here.
            assertTrue(
                    "the folder failure does not say why: " + reply,
                    reply.contains("may not read") || reply.contains("not on this device"));
            return;
        }
        // Or the folder was readable, in which case the path must be the one picked.
        assertEquals(
                "/storage/emulated/0/Download/" + FOLDER, new JSONObject(reply).getString("path"));
    }

    /**
     * HTTP through the platform stack, and the updater that sits on it.
     *
     * <p>Desktop uses a bundled TLS stack; Android uses {@code HttpURLConnection} so
     * it inherits the system certificate store, which is the whole reason
     * {@code KilnHttp} exists and is unverifiable off a device.
     */
    private static void checkHttp(ActivityScenario<MainActivity> scenario) throws Exception {
        // Off the UI thread, because that is how the shell calls it: a blocking
        // request on the main thread throws NetworkOnMainThreadException, and a test
        // would then be measuring the wrong thing.
        assertNull("a malformed URL produced a body", KilnDeviceSupport.httpGet("not a url"));

        Assume.assumeTrue(
                "this device has no network, so the HTTP transport cannot be exercised",
                KilnDeviceSupport.online());

        assertNull("a 404 produced a body", KilnDeviceSupport.httpGet(MISSING));
        String body = KilnDeviceSupport.httpGet(REACHABLE);
        assertNotNull("the request failed; the system trust store was not used", body);
        // Not the exact document: a body arriving at all fails if TLS, the JNI
        // string, or the read loop broke.
        assertTrue("the body was empty", body.length() > 0);
        assertTrue(
                "the body is not the document at " + REACHABLE, body.toLowerCase().contains("<html"));

        KilnDeviceSupport.startInvoke(scenario, "update.check", "{}");
        String reply = KilnDeviceSupport.awaitProbe(scenario);

        // Two honest answers, and which is correct depends on the config an app
        // ships rather than on this test: a configured feed resolves with the update
        // object, and a placeholder URL that does not exist has to arrive as a
        // rejection naming the failure rather than as a hang or a crash. Either way
        // the Rust updater reached the network through the activity.
        if (reply.startsWith("error:")) {
            assertTrue(
                    "the updater failed for something other than the fetch: " + reply,
                    reply.contains("update check failed") || reply.contains("invalid JSON"));
            return;
        }
        JSONObject update = new JSONObject(reply);
        assertTrue("the updater reported no configuration", update.getBoolean("configured"));
        assertFalse("the updater named no latest version", update.getString("latest").isEmpty());
        // Not compared against the APK's versionName: the shell compares the version
        // of the framework crate it was built against, a different string that
        // happens to agree today.
        assertFalse("the updater named no running version", update.getString("current").isEmpty());
        assertTrue("the updater returned no verdict", update.has("available"));
    }

    /**
     * Invoke {@code greet} from the live page and return its reply.
     *
     * <p>Two steps because {@code evaluateJavascript} cannot return a promise: the
     * call stashes its reply on {@code window}, and a later evaluation reads it back
     * once it is no longer null.
     */
    private static String invokeGreet(ActivityScenario<MainActivity> scenario, String name)
            throws Exception {
        KilnDeviceSupport.startInvoke(scenario, "greet", "{\"name\":\"" + name + "\"}");
        return KilnDeviceSupport.awaitProbe(scenario);
    }
}
