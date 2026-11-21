package com.voussoir.counter;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;

import android.content.Context;
import android.view.ContextThemeWrapper;
import android.view.Menu;
import android.view.MenuItem;
import android.view.View;
import android.widget.PopupMenu;

import androidx.test.platform.app.InstrumentationRegistry;

import org.json.JSONArray;
import org.json.JSONObject;
import org.junit.Test;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;

/**
 * The shell hands the menu over as JSON and expects a click back as one of the
 * ids it sent. Both directions are data translation, so they run here without an
 * activity, a window, or any of the shell being up.
 *
 * <p>A {@code PopupMenu} is used for its {@code Menu}, not for its popup: the
 * menu object is allocated eagerly, so building one never shows anything and
 * needs no layout.
 */
public class VoussoirMenuTest {
    private static JSONArray groups(String json) throws Exception {
        return new JSONArray(json);
    }

    private static Menu freshMenu() {
        Context context = new ContextThemeWrapper(
                InstrumentationRegistry.getInstrumentation().getTargetContext(),
                android.R.style.Theme_Material_Light);
        return new PopupMenu(context, new View(context)).getMenu();
    }

    private static List<String> labels(Menu menu) {
        List<String> labels = new ArrayList<>();
        for (int i = 0; i < menu.size(); i++) {
            MenuItem item = menu.getItem(i);
            labels.add(item.getTitle().toString());
        }
        return labels;
    }

    @Test
    public void flatGroupsBecomeItemsInOrder() throws Exception {
        Menu menu = freshMenu();
        VoussoirMenu.PopupMenuBuilder built = VoussoirMenu.build(groups(
                "[{\"title\":\"\",\"items\":[{\"id\":\"a\",\"label\":\"A\"},"
                        + "{\"id\":\"b\",\"label\":\"B\"}]}]"), menu);

        assertEquals(Arrays.asList("a", "b"), built.ids);
        assertEquals(Arrays.asList("A", "B"), labels(menu));
    }

    @Test
    public void idsAreDenseAndStartAtOne() throws Exception {
        // MenuItem uses 0 for "no id", so the mapping back to an index has to be
        // 1-based and dense or a click lands on the wrong item.
        Menu menu = freshMenu();
        VoussoirMenu.PopupMenuBuilder built = VoussoirMenu.build(groups(
                "[{\"items\":[{\"id\":\"a\",\"label\":\"A\"}]},"
                        + "{\"items\":[{\"id\":\"b\",\"label\":\"B\"},"
                        + "{\"id\":\"c\",\"label\":\"C\"}]}]"), menu);

        assertEquals(Arrays.asList("a", "b", "c"), built.ids);
        for (int i = 0; i < built.ids.size(); i++) {
            assertEquals(i + 1, menu.getItem(i).getItemId());
        }
    }

    @Test
    public void aTitledGroupBecomesASubmenu() throws Exception {
        Menu menu = freshMenu();
        VoussoirMenu.PopupMenuBuilder built = VoussoirMenu.build(groups(
                "[{\"title\":\"File\",\"items\":[{\"id\":\"quit\",\"label\":\"Quit\"}]}]"), menu);

        assertEquals(Arrays.asList("quit"), built.ids);
        assertEquals(1, menu.size());
        MenuItem item = menu.getItem(0);
        assertEquals("File", item.getTitle().toString());
        assertTrue(item.hasSubMenu());
        assertEquals(Arrays.asList("Quit"), labels(item.getSubMenu()));
    }

    @Test
    public void anItemWithNoLabelFallsBackToItsId() throws Exception {
        Menu menu = freshMenu();
        VoussoirMenu.build(groups("[{\"items\":[{\"id\":\"quit\"}]}]"), menu);
        assertEquals(Arrays.asList("quit"), labels(menu));
    }

    @Test
    public void entriesWithoutAnIdAreDropped() throws Exception {
        // Without an id there is nothing to send back to the shell, so showing
        // the item would be a click that goes nowhere.
        Menu menu = freshMenu();
        VoussoirMenu.PopupMenuBuilder built = VoussoirMenu.build(groups(
                "[{\"items\":[{\"label\":\"orphan\"},{\"id\":\"a\",\"label\":\"A\"},null]}]"), menu);

        assertEquals(Arrays.asList("a"), built.ids);
        assertEquals(Arrays.asList("A"), labels(menu));
    }

    @Test
    public void anEmptyMenuBuildsNothing() throws Exception {
        Menu menu = freshMenu();
        assertEquals(Arrays.asList(), VoussoirMenu.build(new JSONArray(), menu).ids);
        assertEquals(Arrays.asList(), VoussoirMenu.build(groups("[{}]"), menu).ids);
    }

    @Test
    public void textReadsStringsOnly() throws Exception {
        JSONObject object = new JSONObject();
        object.put("name", "value");
        object.put("number", 7);

        assertEquals("value", VoussoirMenu.text(object, "name"));
        assertNull(VoussoirMenu.text(object, "number"));
        assertNull(VoussoirMenu.text(object, "absent"));
    }
}
