package cn.juniexd.nexttodo.widget

import android.content.SharedPreferences
import cn.juniexd.nexttodo.Occurrence
import org.json.JSONObject

/** Tasks done here that a list kept still has, being from before, by the change that did each: hidden
 * from it until a list as of that change comes; by none while the change waits to be sent
 * (docs/android.md#原生发出的改动). Kept in the list's preferences apart from the list, which stays
 * what it was as of its change: the “接下来” widget's (`Widget`), and the agenda's (`Agenda`). A
 * repeating task by the time done, `<id> 2026-11-25 3` as its `Occurrence` says, as a list may already
 * have the next, handed over by the app with the change in it before it is sent; one done at no time
 * in particular, by its id alone, every time of it then. */
internal object Done {
    private const val KEY = "done"

    fun of(prefs: SharedPreferences) = JSONObject(prefs.getString(KEY, "{}"))

    private fun key(task: String, occurrence: Occurrence?) = occurrence?.let { "$task ${it.due} ${it.completions}" } ?: task

    /** Whether `done`, as `of` gives it, hides `task` at its time `occurrence`. */
    fun hides(done: JSONObject, task: String, occurrence: Occurrence?) = done.has(task) || done.has(key(task, occurrence))

    /** Hides `task` at its time `occurrence`, done by the change `seq`, or by one not sent yet without
     * it, unless the list is as of the change `kept`, not before it. */
    fun hide(prefs: SharedPreferences, task: String, occurrence: Occurrence?, seq: Long?, kept: Long?) {
        val done = of(prefs)
        val key = key(task, occurrence)
        if (seq != null) done.settle(task)
        when {
            seq == null -> done.put(key, JSONObject.NULL)
            kept != null && kept >= seq -> done.remove(key)
            // The latest of its completions, should it be completed twice, from two widgets say.
            else -> done.put(key, if (done.isNull(key)) seq else maxOf(seq, done.optLong(key, seq)))
        }
        prefs.edit().putString(KEY, done.toString()).commit()
    }

    /** Shows `task` again at its time `occurrence`, the server having turned down its completion;
     * whether it was hidden. */
    fun unhide(prefs: SharedPreferences, task: String, occurrence: Occurrence?): Boolean {
        val done = of(prefs)
        val hidden = done.length()
        done.remove(key(task, occurrence))
        done.settle(task)
        return done.length() != hidden && prefs.edit().putString(KEY, done.toString()).commit()
    }

    /** A completion of `task` sent or turned down: one waiting by its id alone, as versions before
     * kept a repeating task's too, is this one or one before it, as the changes go in order. */
    private fun JSONObject.settle(task: String) {
        if (has(task) && isNull(task)) remove(task)
    }

    /** With a list as of the change `seq` written in `edit`: those it is as of the change of show again;
     * all do with no change, signed out. */
    fun keep(prefs: SharedPreferences, edit: SharedPreferences.Editor, seq: Long?) {
        if (seq == null) {
            edit.remove(KEY)
            return
        }
        val done = of(prefs)
        done.keys().asSequence().toList().filter { !done.isNull(it) && done.getLong(it) <= seq }.forEach(done::remove)
        edit.putString(KEY, done.toString())
    }
}
