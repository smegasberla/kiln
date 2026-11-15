package com.kiln.counter;

import android.Manifest;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.os.Build;
import android.util.Log;

import androidx.core.app.ActivityCompat;
import androidx.core.app.NotificationCompat;
import androidx.core.content.ContextCompat;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

/**
 * The tray, standing in for a desktop status area.
 *
 * <p>Android has no status area either, so the tray becomes one notification
 * whose actions are PendingIntents back into the activity. A click arrives as an
 * intent extra, which {@link MainActivity} hands to {@link #deliverClick()} on
 * resume: a notification that starts the process resumes the activity before the
 * shell has published a handle, so the first delivery can be too early and the
 * extra has to stay until one lands.
 *
 * <p>Both entry points run on the UI thread; {@link MainActivity} hops before
 * calling in.
 */
final class KilnTray {
    private static final String TAG = MainActivity.TAG;

    private static final String CHANNEL_ID = "kiln-tray";
    private static final int NOTIFICATION = 1;
    private static final int REQUEST_BASE = 100;
    private static final int NOTIFICATION_PERMISSION_REQUEST = 1;

    private final MainActivity activity;

    private boolean askedForNotifications;

    KilnTray(MainActivity activity) {
        this.activity = activity;
    }

    /** Post the tray configuration as a notification. */
    void post(String json) {
        activity.runOnUiThread(() -> publish(json));
    }

    private void publish(String json) {
        String title;
        JSONArray items;
        try {
            JSONObject request = new JSONObject(json == null ? "{}" : json);
            title = KilnMenu.text(request, "title");
            items = request.optJSONArray("items");
        } catch (JSONException e) {
            Log.w(TAG, "the tray could not be read: " + e.getMessage());
            return;
        }

        NotificationManager manager = activity.getSystemService(NotificationManager.class);
        if (manager == null) {
            Log.w(TAG, "the tray could not be posted: no notification manager");
            return;
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            manager.createNotificationChannel(new NotificationChannel(
                    CHANNEL_ID, "Tray", NotificationManager.IMPORTANCE_DEFAULT));
        }

        NotificationCompat.Builder builder =
                new NotificationCompat.Builder(activity, CHANNEL_ID)
                        .setSmallIcon(android.R.drawable.ic_dialog_info)
                        .setContentTitle(title == null || title.isEmpty() ? "Kiln" : title)
                        .setAutoCancel(true);
        if (items != null) {
            for (int i = 0; i < items.length(); i++) {
                JSONObject item = items.optJSONObject(i);
                if (item == null) {
                    continue;
                }
                String itemId = KilnMenu.text(item, "id");
                if (itemId == null) {
                    continue;
                }
                String label = KilnMenu.text(item, "label");
                builder.addAction(
                        0, label == null ? itemId : label, intent(i, itemId));
            }
        }

        // Asking here means the first post can be dropped while the user is still
        // answering, which is the OS's decision to make: without the permission
        // `notify` returns and shows nothing. Rust is told the notification was
        // accepted, which is all it can observe.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
                && ContextCompat.checkSelfPermission(
                                activity, Manifest.permission.POST_NOTIFICATIONS)
                        != PackageManager.PERMISSION_GRANTED) {
            askForPermission();
        }
        manager.notify(NOTIFICATION, builder.build());
    }

    private void askForPermission() {
        if (askedForNotifications) {
            return;
        }
        askedForNotifications = true;
        ActivityCompat.requestPermissions(
                activity,
                new String[] { Manifest.permission.POST_NOTIFICATIONS },
                NOTIFICATION_PERMISSION_REQUEST);
    }

    private PendingIntent intent(int index, String id) {
        Intent intent = new Intent(activity, MainActivity.class);
        intent.addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP);
        intent.putExtra(MainActivity.TRAY_EXTRA, id);
        return PendingIntent.getActivity(
                activity,
                REQUEST_BASE + index,
                intent,
                PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
    }

    /**
     * Forward a tray click carried by the current intent, if there is one.
     *
     * <p>Called from {@code onResume} and {@code onNewIntent}. The extra is
     * cleared only once the click actually reached the event loop, so a tap that
     * arrives before the shell is ready is retried on the next resume rather than
     * lost.
     */
    void deliverClick() {
        Intent intent = activity.getIntent();
        if (intent == null) {
            return;
        }
        String id = intent.getStringExtra(MainActivity.TRAY_EXTRA);
        if (id == null) {
            return;
        }
        if (MainActivity.kilnTrayClicked(id)) {
            intent.removeExtra(MainActivity.TRAY_EXTRA);
        }
    }
}
