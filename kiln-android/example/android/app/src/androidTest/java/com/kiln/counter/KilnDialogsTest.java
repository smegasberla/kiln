package com.kiln.counter;

import static org.junit.Assert.assertArrayEquals;
import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;

import android.content.Intent;

import org.json.JSONArray;
import org.junit.Test;

/**
 * The picker's two translations: a request's {@code kind} into an intent, and
 * the request's extension filters into MIME types.
 *
 * <p>Neither launches anything, so both run without a document provider and
 * without a window. That is the point of the split: this class pins the translation
 * down where a failure names the mapping, and {@code KilnDialogsDeviceTest} drives
 * the other half on a device, where the intent is answered for real by whatever
 * provider is installed. Answering it is not the shell's job and is not claimed
 * here.
 */
public class KilnDialogsTest {
    private static JSONArray json(String text) throws Exception {
        return new JSONArray(text);
    }

    @Test
    public void eachKindMapsToItsIntentAction() {
        assertEquals(Intent.ACTION_OPEN_DOCUMENT, KilnDialogs.intentFor("open").getAction());
        assertEquals(Intent.ACTION_CREATE_DOCUMENT, KilnDialogs.intentFor("save").getAction());
        assertEquals(
                Intent.ACTION_OPEN_DOCUMENT_TREE, KilnDialogs.intentFor("folder").getAction());

        Intent many = KilnDialogs.intentFor("open_many");
        assertEquals(Intent.ACTION_OPEN_DOCUMENT, many.getAction());
        assertTrue(many.getBooleanExtra(Intent.EXTRA_ALLOW_MULTIPLE, false));
    }

    @Test
    public void anUnknownKindHasNoIntent() {
        // The caller turns null into a failure rather than answering with a
        // cancel, which would look like the user dismissed a picker.
        assertNull(KilnDialogs.intentFor("teleport"));
    }

    @Test
    public void extensionsBecomeMimeTypes() throws Exception {
        String[] types = KilnDialogs.mimeTypes(
                json("[{\"extensions\":[\"json\",\".txt\"]}]"));
        assertArrayEquals(new String[] { "application/json", "text/plain" }, types);
    }

    @Test
    public void aWildcardMeansNoRestriction() throws Exception {
        // The same meaning the desktop filters give `*`, and also what an intent
        // with no EXTRA_MIME_TYPES does.
        assertNull(KilnDialogs.mimeTypes(json("[{\"extensions\":[\"*\"]}]")));
    }

    @Test
    public void unknownExtensionsAreDropped() throws Exception {
        assertNull(KilnDialogs.mimeTypes(json("[{\"extensions\":[\"kiln-not-a-type\"]}]")));
    }

    @Test
    public void duplicateTypesAppearOnce() throws Exception {
        String[] types = KilnDialogs.mimeTypes(
                json("[{\"extensions\":[\"json\"]},{\"extensions\":[\"JSON\"]}]"));
        assertArrayEquals(new String[] { "application/json" }, types);
    }

    @Test
    public void noFiltersMeansNoRestriction() {
        assertNull(KilnDialogs.mimeTypes(null));
        assertNull(KilnDialogs.mimeTypes(new JSONArray()));
    }

    @Test
    public void aFilterWithoutExtensionsContributesNothing() throws Exception {
        assertNull(KilnDialogs.mimeTypes(json("[{\"name\":\"Images\"},null]")));
    }
}
