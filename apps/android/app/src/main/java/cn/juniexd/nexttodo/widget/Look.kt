package cn.juniexd.nexttodo.widget

import android.content.SharedPreferences
import androidx.compose.runtime.Composable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.glance.ColorFilter
import androidx.glance.GlanceModifier
import androidx.glance.Image
import androidx.glance.ImageProvider
import androidx.glance.LocalSize
import androidx.glance.action.Action
import androidx.glance.action.clickable
import androidx.glance.color.ColorProvider
import androidx.glance.layout.Alignment
import androidx.glance.layout.Box
import androidx.glance.layout.Row
import androidx.glance.layout.fillMaxHeight
import androidx.glance.layout.fillMaxWidth
import androidx.glance.layout.height
import androidx.glance.layout.padding
import androidx.glance.layout.width
import androidx.glance.text.FontWeight
import androidx.glance.text.Text
import androidx.glance.text.TextStyle
import cn.juniexd.nexttodo.Palette
import cn.juniexd.nexttodo.R
import org.json.JSONObject

/** How the widgets look, as chosen in the app's settings, the same for all of them
 * (docs/android/widgets.md#小组件的设置). A colour is `white`, `black` or `#rrggbb`. */
data class Look(
    /** `system`, or a colour. */
    val background: String = "system",
    /** The background's, 0–100. */
    val opacity: Int = 100,
    /** `auto`, or a colour. */
    val text: String = "auto",
    /** What the header has. */
    val title: Boolean = true,
    val refresh: Boolean = true,
    val add: Boolean = true,
    /** `small`, `standard` or `large`. */
    val size: String = "standard",
    /** The “一周” widget's days: `today` and the six after, or `monday` to Sunday of this week. */
    val week: String = "today",
) {
    /** How much larger or smaller than standard the text is. */
    val scale get() = SCALES.getValue(size)

    fun json(): JSONObject = JSONObject()
        .put("background", background)
        .put("opacity", opacity)
        .put("text", text)
        .put("title", title)
        .put("refresh", refresh)
        .put("add", add)
        .put("size", size)
        .put("week", week)

    /** Kept in `edit`, a key each. */
    fun write(edit: SharedPreferences.Editor): SharedPreferences.Editor = edit
        .putString(BACKGROUND, background)
        .putInt(OPACITY, opacity)
        .putString(TEXT, text)
        .putBoolean(TITLE, title)
        .putBoolean(REFRESH, refresh)
        .putBoolean(ADD, add)
        .putString(SIZE, size)
        .putString(WEEK, week)

    companion object {
        private val SCALES = mapOf("small" to 0.88f, "standard" to 1f, "large" to 1.15f)
        private const val BACKGROUND = "background"
        private const val OPACITY = "opacity"
        private const val TEXT = "text_color"
        private const val TITLE = "show_title"
        private const val REFRESH = "show_refresh"
        private const val ADD = "show_add"
        /** The key the text size has had since before the rest. */
        private const val SIZE = "text_size"
        private const val WEEK = "week"

        /** As the app gives it. */
        fun from(o: JSONObject) = Look(
            o.getString("background").also { if (it != "system") color(it) },
            o.getInt("opacity").also { require(it in 0..100) { "不透明度应在 0–100 之间" } },
            o.getString("text").also { if (it != "auto") color(it) },
            o.getBoolean("title"),
            o.getBoolean("refresh"),
            o.getBoolean("add"),
            o.getString("size").also { require(it in SCALES) { "文字大小只能是 small、standard 或 large" } },
            o.getString("week").also { require(it == "today" || it == "monday") { "一周的日子只能是 today 或 monday" } },
        )

        /** As kept in `prefs`; as at first where nothing is. */
        fun read(prefs: SharedPreferences): Look {
            val d = Look()
            return Look(
                prefs.getString(BACKGROUND, d.background)!!,
                prefs.getInt(OPACITY, d.opacity),
                prefs.getString(TEXT, d.text)!!,
                prefs.getBoolean(TITLE, d.title),
                prefs.getBoolean(REFRESH, d.refresh),
                prefs.getBoolean(ADD, d.add),
                prefs.getString(SIZE, d.size)!!,
                prefs.getString(WEEK, d.week)!!,
            )
        }
    }
}

/** `white`, `black` or `#rrggbb`. */
private fun color(s: String): Color = when (s) {
    "white" -> Color.White
    "black" -> Color.Black
    else -> Color(android.graphics.Color.parseColor(s.also { require(Regex("#[0-9a-fA-F]{6}").matches(it)) { "颜色“$it”应为 #rrggbb" } }))
}

private fun fixed(c: Color) = ColorProvider(day = c, night = c)

/** The colours the widgets are drawn in, as `look` has them. The text and the marks, those of the
 * palette that go on a light background or on a dark one; one's own text colour, fainter for what is
 * less to the fore. */
internal class Colors(look: Look) {
    /** The background, by day and by night. */
    private val ground = if (look.background == "system") Palette.light.surface to Palette.dark.surface else color(look.background).let { it to it }

    /** The palette the text and the marks are of, by day and by night. */
    private val tones: Pair<Palette, Palette> = when {
        look.text != "auto" -> onto(!light(color(look.text))).let { it to it }
        // Mostly the wallpaper shows through, light or dark no one knows: as the system is.
        look.background == "system" || look.opacity < 50 -> Palette.light to Palette.dark
        else -> onto(light(ground.first)).let { it to it }
    }

    private val own = look.text.takeIf { it != "auto" }?.let(::color)

    private fun of(c: Palette.() -> Color) = ColorProvider(day = tones.first.c(), night = tones.second.c())

    /** One's own text colour, `alpha` of it, or else the palette's `c`. */
    private fun faint(alpha: Float, c: Palette.() -> Color) = own?.let { fixed(it.copy(alpha = alpha)) } ?: of(c)

    /** The background, by day and by night, and how opaque it is. */
    val background = ColorProvider(day = ground.first, night = ground.second)
    val opacity = look.opacity / 100f
    val text = own?.let(::fixed) ?: of { onSurface }
    val secondary = faint(SECONDARY) { onSurfaceVariant }

    /** `secondary` for an image, which takes a tint's colour but not its alpha: that apart. */
    val icon = own?.let(::fixed) ?: of { onSurfaceVariant }
    val iconAlpha = if (own == null) 1f else SECONDARY
    val done = faint(0.5f) { outline }

    /** The circle of a task that can't be ticked off here, as the app's waiting ones. */
    val rail = faint(0.4f) { rail }
    val divider = faint(0.15f) { divider }
    val primary = of { primary }
    val onPrimary = of { onPrimary }
    val overdue = of { error }
    val today = of { warn }

    /** The time grid's blocks, by state, as the app's: the ground of one to do and the text on it, of one
     * overdue (in `overdue`), and of one waiting; a later time is in the primary colour, `rail` around it. */
    val ready = of { primaryContainer }
    val onReady = of { onPrimaryContainer }
    val late = of { errorContainer }
    val waiting = of { surfaceHighest }
    val onWaiting = of { onSurfaceVariant }

    /** Today's column of the time grid. */
    val todayColumn = of { primary.copy(alpha = 0.06f) }

    private companion object {
        /** How much of one's own text colour what is less to the fore has. */
        const val SECONDARY = 0.7f

        /** Whether dark text reads better on `c` than light: it is nearer white than black by contrast. */
        fun light(c: Color) = c.luminance() > 0.179f

        /** The palette for a light background, or a dark one. */
        fun onto(light: Boolean) = if (light) Palette.light else Palette.dark
    }
}

/** The colours to draw in, as provided where a widget's content is. */
internal val LocalColors = staticCompositionLocalOf { Colors(Look()) }

/** The header as `look` has it: the title, which `onTitle` opens, the refresh button, and the add
 * button where the widget has `add`. Lower with the buttons alone, which keep to the right; the space
 * before them still opens what the title does, as the widget leaves no gap (see `EDGE`). With nothing
 * in it, only the space above the list, which does too: the list's own padding would take a tap there
 * and do nothing. */
@Composable
internal fun Header(look: Look, title: String, onTitle: Action, refresh: Action, add: Action?) {
    val height = headerHeight(look, add != null && look.add)
    if (height == TOP) {
        Box(GlanceModifier.fillMaxWidth().height(TOP).clickable(onTitle)) {}
        return
    }
    val top = if (look.title) TOP else LOW_TOP
    Row(GlanceModifier.fillMaxWidth().height(height), verticalAlignment = Alignment.CenterVertically) {
        Box(
            GlanceModifier.defaultWeight().fillMaxHeight().padding(start = EDGE, top = top, bottom = if (look.title) UNDER_HEADER else 0.dp).clickable(onTitle),
            contentAlignment = Alignment.CenterStart,
        ) {
            if (look.title) Text(title, style = TextStyle(color = LocalColors.current.text, fontSize = 17.sp * look.scale, fontWeight = FontWeight.Bold))
        }
        val bottom = if (look.title) UNDER_HEADER else 0.dp
        Buttons(look, refresh, add, top, bottom)
    }
}

/** How high `Header` is, `adding` should it have the add button: only the room above the list, `TOP`,
 * with nothing in it or in a widget too small for it. */
@Composable
internal fun headerHeight(look: Look, adding: Boolean): Dp = when {
    LocalSize.current.height < NO_HEADER_BELOW || LocalSize.current.width < TINY_BELOW || (!look.title && !look.refresh && !adding) -> TOP
    look.title -> TOP + 32.dp + UNDER_HEADER
    else -> LOW_TOP + 32.dp
}

/** The header's refresh button and add button, should `look` have them and the widget `add`; at its
 * end, `top` and `bottom` its own. */
@Composable
internal fun Buttons(look: Look, refresh: Action, add: Action?, top: Dp, bottom: Dp) {
    val adding = add != null && look.add
    if (look.refresh) Icon(R.drawable.ic_widget_refresh, "刷新", refresh, top, bottom, if (adding) 4.dp else EDGE)
    if (adding) Icon(R.drawable.ic_add, "添加事项", add!!, top, bottom, EDGE)
}

/** A button of the header, 22dp across, tapped anywhere in the header's height, `top` and `bottom`
 * the header's own, and `end` beyond it. */
@Composable
private fun Icon(drawable: Int, label: String, onClick: Action, top: Dp, bottom: Dp, end: Dp) {
    val colors = LocalColors.current
    Image(
        ImageProvider(drawable),
        label,
        alpha = colors.iconAlpha,
        modifier = GlanceModifier.width(32.dp + end).fillMaxHeight().padding(start = 5.dp, top = top + 5.dp, end = 5.dp + end, bottom = bottom + 5.dp).clickable(onClick),
        colorFilter = ColorFilter.tint(colors.icon),
    )
}

/** How far the buttons keep from the top in a header without the title. */
private val LOW_TOP = 4.dp

/** Lower than this, about a cell, there is no header, whatever the settings say: no room for it; nor
 * in a widget `TINY_BELOW` wide. */
private val NO_HEADER_BELOW = 110.dp
