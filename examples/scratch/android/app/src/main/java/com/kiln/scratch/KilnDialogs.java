package com.kiln.scratch;

import android.app.Activity;
import android.content.ActivityNotFoundException;
import android.content.ClipData;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Environment;
import android.provider.DocumentsContract;
import android.provider.OpenableColumns;
import android.util.Log;
import android.webkit.MimeTypeMap;

import androidx.activity.result.ActivityResult;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;
import java.util.List;

import kotlin.Unit;
import kotlin.jvm.functions.Function1;

/**
 * The file picker, standing in for {@code rfd}.
 *
 * <p>On Android a picker is a system intent answered by whichever app owns the
 * document, so what comes back is a {@code content://} URI and not a path. Rust
 * opens paths, so every answer is translated here: provider documents that name
 * real storage become their filesystem path, and anything else is copied into
 * this app's cache directory, because the alternative is a path that names
 * nothing.
 *
 * <p>Everything except the static helpers runs on the UI thread; the caller in
 * {@link MainActivity} hops before calling in.
 */
final class KilnDialogs {
    private static final String TAG = MainActivity.TAG;

    private final MainActivity activity;

    /**
     * The id of the picker that is up, or 0 when none is.
     *
     * <p>wry's {@code activityListener} is a single field: a second
     * {@code launchActivityForResult} silently replaces the first listener, and
     * the first picker's result is then delivered to the second waiter. Tracking
     * it here is what lets a second request be refused instead of crossing the
     * two answers.
     */
    private long pending;

    KilnDialogs(MainActivity activity) {
        this.activity = activity;
    }

    /**
     * Show the picker described by {@code requestJson}.
     *
     * <p>The payload is the command plus a {@code kind} of {@code open},
     * {@code open_many}, {@code save}, or {@code folder}. The reply is the same
     * JSON shape the desktop dialogs return: {@code {"path": ...}},
     * {@code {"paths": [...]}}, or null for a cancel.
     */
    void show(long id, String requestJson) {
        if (pending != 0) {
            // Overwriting wry's single activity listener would deliver this
            // picker's result to the one still waiting.
            MainActivity.kilnFailDialog(id, "another file picker is already open");
            return;
        }

        String kind;
        String title;
        String fileName;
        JSONArray filters;
        try {
            JSONObject request = new JSONObject(requestJson);
            kind = text(request, "kind");
            title = text(request, "title");
            fileName = text(request, "file_name");
            filters = request.optJSONArray("filters");
        } catch (JSONException e) {
            MainActivity.kilnFailDialog(id, "the dialog request was not JSON: " + e.getMessage());
            return;
        }
        if (kind == null) {
            kind = "open";
        }

        Intent intent = intentFor(kind);
        if (intent == null) {
            MainActivity.kilnFailDialog(id, "unknown dialog kind " + kind);
            return;
        }

        if (title != null && !title.isEmpty()) {
            intent.putExtra(Intent.EXTRA_TITLE, title);
        }
        if (fileName != null && !fileName.isEmpty()
                && Intent.ACTION_CREATE_DOCUMENT.equals(intent.getAction())) {
            intent.putExtra(Intent.EXTRA_TITLE, fileName);
        }
        if (!Intent.ACTION_OPEN_DOCUMENT_TREE.equals(intent.getAction())) {
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType("*/*");
            String[] types = mimeTypes(filters);
            if (types != null) {
                intent.putExtra(Intent.EXTRA_MIME_TYPES, types);
            }
        }
        // The payload's `directory` is deliberately ignored: a filesystem path is
        // not a document URI, and `EXTRA_INITIAL_URI` only accepts the latter.

        final String answered = kind;
        pending = id;
        try {
            activity.launchActivityForResult(intent, new Function1<ActivityResult, Unit>() {
                @Override
                public Unit invoke(ActivityResult result) {
                    finish(id, answered, result);
                    return Unit.INSTANCE;
                }
            });
        } catch (ActivityNotFoundException e) {
            pending = 0;
            MainActivity.kilnFailDialog(
                    id, "no app on this device can answer " + intent.getAction());
        }
    }

    /** The intent that asks for one {@code kind} of document, or null for an unknown kind. */
    static Intent intentFor(String kind) {
        Intent intent = new Intent();
        switch (kind) {
            case "save":
                intent.setAction(Intent.ACTION_CREATE_DOCUMENT);
                break;
            case "folder":
                intent.setAction(Intent.ACTION_OPEN_DOCUMENT_TREE);
                break;
            case "open_many":
                intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true);
                intent.setAction(Intent.ACTION_OPEN_DOCUMENT);
                break;
            case "open":
                intent.setAction(Intent.ACTION_OPEN_DOCUMENT);
                break;
            default:
                return null;
        }
        return intent;
    }

    private void finish(long id, String kind, ActivityResult result) {
        pending = 0;
        if (result == null
                || result.getResultCode() != Activity.RESULT_OK
                || result.getData() == null) {
            MainActivity.kilnCompleteDialog(id, null);
            return;
        }

        List<Uri> picked = picked(kind, result);
        if (picked.isEmpty()) {
            MainActivity.kilnCompleteDialog(id, null);
            return;
        }

        try {
            if ("open_many".equals(kind)) {
                JSONArray paths = new JSONArray();
                for (Uri uri : picked) {
                    paths.put(pathOf(uri, false));
                }
                MainActivity.kilnCompleteDialog(
                        id, new JSONObject().put("paths", paths).toString());
            } else {
                boolean tree = "folder".equals(kind);
                String path = pathOf(picked.get(0), tree);
                MainActivity.kilnCompleteDialog(id, new JSONObject().put("path", path).toString());
            }
        } catch (IOException | JSONException e) {
            MainActivity.kilnFailDialog(id, "the picked file could not be read: " + e.getMessage());
        }
    }

    /** The URIs a result carries: a clip for a multi-select, the data URI otherwise. */
    static List<Uri> picked(String kind, ActivityResult result) {
        List<Uri> picked = new ArrayList<>();
        if (result == null || result.getData() == null) {
            return picked;
        }
        ClipData clip = result.getData().getClipData();
        Uri single = result.getData().getData();
        if ("open_many".equals(kind) && clip != null) {
            for (int i = 0; i < clip.getItemCount(); i++) {
                picked.add(clip.getItemAt(i).getUri());
            }
        } else if (single != null) {
            picked.add(single);
        }
        return picked;
    }

    /**
     * Turn a picker result into a path Rust can open.
     *
     * <p>Three things have to be true of the answer, and only the first is obvious:
     * the URI has to name something, a storage provider's document id often spells
     * out a filesystem path, and this process has to be able to <em>open</em> it.
     * The third is the one that bites. Under scoped storage the picker will happily
     * return {@code primary:Download/report.pdf}, and that path exists, but an app
     * that has not asked for broad storage access cannot read it. Answering with it
     * would hand Rust a path that names a real file it may not open, which is worse
     * than copying, because it fails later and somewhere else.
     *
     * <p>So a path is only returned when it can be verified by opening it.
     * Everything else is copied into this app's own cache, which is always
     * readable. A folder cannot be verified that way and cannot be copied either,
     * so a tree that is not readable is reported as a failure rather than answered
     * with a path that leads nowhere.
     */
    String pathOf(Uri uri, boolean tree) throws IOException {
        if ("file".equals(uri.getScheme())) {
            String path = uri.getPath();
            if (path != null && readable(new File(path))) {
                return path;
            }
        }
        String path = pathForDocument(documentId(uri, tree));
        if (path != null && readable(new File(path))) {
            return path;
        }
        if (tree) {
            throw new IOException(
                    "this app may not read " + (path == null ? "that folder" : path)
                            + "; granting it storage access would, but scoped storage does not"
                            + " let an app open another app's shared files by path");
        }
        return copyToCache(uri);
    }

    /** Whether this process can actually open {@code file} for reading. */
    private static boolean readable(File file) {
        // Opening, not {@code canRead()}: the latter reports the permission bit and
        // ignores the SELinux and scoped-storage rules that are what refuse the
        // open. A stream that closes again immediately is the only real answer.
        try (InputStream probe = new FileInputStream(file)) {
            return true;
        } catch (IOException e) {
            return false;
        }
    }

    private static String documentId(Uri uri, boolean tree) {
        if (!"content".equals(uri.getScheme())
                || !"com.android.externalstorage.documents".equals(uri.getAuthority())) {
            return null;
        }
        try {
            return tree
                    ? DocumentsContract.getTreeDocumentId(uri)
                    : DocumentsContract.getDocumentId(uri);
        } catch (IllegalArgumentException e) {
            return null;
        }
    }

    private static String pathForDocument(String document) {
        if (document == null) {
            return null;
        }
        if (document.startsWith("raw:")) {
            return document.substring(4);
        }
        int colon = document.indexOf(':');
        if (colon < 0) {
            return null;
        }
        String volume = document.substring(0, colon);
        String rest = document.substring(colon + 1);
        File root = "primary".equals(volume)
                ? Environment.getExternalStorageDirectory()
                : new File("/storage", volume);
        return new File(root, rest).getAbsolutePath();
    }

    private String copyToCache(Uri uri) throws IOException {
        File dir = new File(activity.getCacheDir(), "picked");
        if (!dir.exists() && !dir.mkdirs()) {
            throw new IOException("could not create " + dir);
        }
        String name = displayName(uri).replace('/', '_');
        File target = new File(dir, name);
        try (InputStream input = activity.getContentResolver().openInputStream(uri);
                OutputStream output = new FileOutputStream(target)) {
            if (input == null) {
                throw new IOException("this app may not read the picked file");
            }
            byte[] buffer = new byte[8192];
            int read;
            while ((read = input.read(buffer)) > 0) {
                output.write(buffer, 0, read);
            }
        }
        return target.getAbsolutePath();
    }

    private String displayName(Uri uri) {
        try (Cursor cursor = activity.getContentResolver().query(uri, null, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) {
                int column = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME);
                if (column >= 0) {
                    String name = cursor.getString(column);
                    if (name != null && !name.isEmpty()) {
                        return name;
                    }
                }
            }
        }
        return "picked-" + System.currentTimeMillis();
    }

    /**
     * MIME types for the payload's filters, or null to accept anything.
     *
     * <p>Android filters by MIME type, not by extension, so this is a translation
     * and not a copy of the desktop filter list. An unknown extension is dropped
     * rather than turned into a type that matches nothing, and a {@code *} filter
     * means the same thing on both platforms: no restriction.
     */
    static String[] mimeTypes(JSONArray filters) {
        if (filters == null) {
            return null;
        }
        List<String> types = new ArrayList<>();
        MimeTypeMap map = MimeTypeMap.getSingleton();
        for (int i = 0; i < filters.length(); i++) {
            JSONObject filter = filters.optJSONObject(i);
            if (filter == null) {
                continue;
            }
            JSONArray extensions = filter.optJSONArray("extensions");
            if (extensions == null) {
                continue;
            }
            for (int j = 0; j < extensions.length(); j++) {
                String extension = text(extensions, j);
                if (extension == null) {
                    continue;
                }
                extension = extension.startsWith(".")
                        ? extension.substring(1)
                        : extension;
                if (extension.isEmpty()) {
                    continue;
                }
                if ("*".equals(extension)) {
                    return null;
                }
                String type = map.getMimeTypeFromExtension(extension);
                if (type != null && !types.contains(type)) {
                    types.add(type);
                }
            }
        }
        return types.isEmpty() ? null : types.toArray(new String[0]);
    }

    private static String text(JSONObject object, String key) {
        Object value = object.opt(key);
        return value instanceof String ? (String) value : null;
    }

    private static String text(JSONArray array, int index) {
        Object value = array.opt(index);
        return value instanceof String ? (String) value : null;
    }
}
