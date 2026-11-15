package com.kiln.counter;

import android.util.Log;
import android.view.Menu;
import android.view.View;
import android.widget.PopupMenu;

import org.json.JSONArray;
import org.json.JSONObject;

import java.util.ArrayList;
import java.util.List;

/**
 * The menu, standing in for a desktop menu bar.
 *
 * <p>Android has no menu bar, so the shell's menu only exists while a
 * {@code PopupMenu} is up. That is why installing the menu and showing it are
 * two separate calls: the shell builds the menu once at startup and then asks
 * for it to appear on the user's request, and there is nowhere to put it in
 * between.
 *
 * <p>Both entry points run on the UI thread; {@link MainActivity} hops before
 * calling in.
 */
final class KilnMenu {
    private static final String TAG = MainActivity.TAG;

    private final MainActivity activity;

    /** The menu groups Rust handed over, verbatim. */
    private JSONArray groups = new JSONArray();

    KilnMenu(MainActivity activity) {
        this.activity = activity;
    }

    /** Keep the menu until {@link #show()} can put it on screen. */
    void install(String json) {
        activity.runOnUiThread(() -> {
            try {
                groups = new JSONArray(json == null ? "[]" : json);
            } catch (Exception e) {
                Log.w(TAG, "the menu could not be read: " + e.getMessage());
                groups = new JSONArray();
            }
        });
    }

    /** Show the installed menu as a popup anchored on the content view. */
    void show() {
        activity.runOnUiThread(this::popUp);
    }

    private void popUp() {
        if (groups.length() == 0) {
            Log.w(TAG, "the menu was shown before it was installed");
            return;
        }
        View anchor = activity.findViewById(android.R.id.content);
        if (anchor == null) {
            return;
        }

        PopupMenu popup = new PopupMenu(activity, anchor);
        PopupMenuBuilder built = build(groups, popup.getMenu());
        if (built.ids.isEmpty()) {
            return;
        }

        popup.setOnMenuItemClickListener(item -> {
            MainActivity.kilnMenuClicked(built.ids.get(item.getItemId() - 1));
            return true;
        });
        popup.show();
    }

    /** A populated menu plus the item ids in the order they were added. */
    static final class PopupMenuBuilder {
        final List<String> ids = new ArrayList<>();
    }

    /**
     * Copy the shell's groups into {@code menu}.
     *
     * <p>Item ids have to be dense and start at 1, because {@code MenuItem}'s id
     * is how the click finds its way back to a shell id, and 0 means "no id".
     */
    static PopupMenuBuilder build(JSONArray groups, Menu menu) {
        PopupMenuBuilder built = new PopupMenuBuilder();
        for (int i = 0; i < groups.length(); i++) {
            JSONObject group = groups.optJSONObject(i);
            if (group == null) {
                continue;
            }
            // A group with a title becomes a submenu, which is the closest a
            // PopupMenu has to the grouping a menu bar shows side by side.
            String groupTitle = text(group, "title");
            Menu target = menu;
            if (groupTitle != null && !groupTitle.isEmpty()) {
                target = menu.addSubMenu(groupTitle);
            }
            JSONArray items = group.optJSONArray("items");
            if (items == null) {
                continue;
            }
            for (int j = 0; j < items.length(); j++) {
                JSONObject item = items.optJSONObject(j);
                if (item == null) {
                    continue;
                }
                String itemId = text(item, "id");
                if (itemId == null) {
                    continue;
                }
                String label = text(item, "label");
                built.ids.add(itemId);
                target.add(Menu.NONE, built.ids.size(), built.ids.size(),
                        label == null ? itemId : label);
            }
        }
        return built;
    }

    /** A JSON string field, or null when it is absent or not a string. */
    static String text(JSONObject object, String key) {
        Object value = object.opt(key);
        return value instanceof String ? (String) value : null;
    }
}
