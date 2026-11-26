package cn.juniexd.nexttodo.widget

import android.content.Context
import android.content.Intent
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.glance.ColorFilter
import androidx.glance.GlanceId
import androidx.glance.GlanceModifier
import androidx.glance.ImageProvider
import androidx.glance.action.Action
import androidx.glance.action.ActionParameters
import androidx.glance.action.actionParametersOf
import androidx.glance.action.clickable
import androidx.glance.appwidget.GlanceAppWidget
import androidx.glance.appwidget.GlanceAppWidgetReceiver
import androidx.glance.appwidget.SizeMode
import androidx.glance.appwidget.action.ActionCallback
import androidx.glance.appwidget.action.actionRunCallback
import androidx.glance.appwidget.action.actionStartActivity
import androidx.glance.appwidget.lazy.LazyColumn
import androidx.glance.appwidget.lazy.items
import androidx.glance.appwidget.provideContent
import androidx.glance.appwidget.state.updateAppWidgetState
import androidx.glance.background
import androidx.glance.currentState
import androidx.glance.layout.Alignment
import androidx.glance.layout.Box
import androidx.glance.layout.Row
import androidx.glance.layout.fillMaxHeight
import androidx.glance.layout.fillMaxSize
import androidx.glance.layout.fillMaxWidth
import androidx.glance.layout.height
import androidx.glance.layout.padding
import androidx.glance.layout.width
import androidx.glance.text.FontWeight
import androidx.glance.text.Text
import androidx.glance.text.TextAlign
import androidx.glance.text.TextStyle
import androidx.glance.unit.ColorProvider
import cn.juniexd.nexttodo.QuickAddActivity
import cn.juniexd.nexttodo.R
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime

/** The home-screen time grid: the week's seven days side by side, what is due on them by the hour. See
 * docs/android.md#时间轴. */
class TimelineWidget : GlanceAppWidget() {
    // One size, which is laid out to fill: the widget is drawn for each size it is given otherwise.
    override val sizeMode = SizeMode.Single

    override suspend fun provideGlance(context: Context, id: GlanceId) {
        val changes = Agenda.weekChanges(context)
        provideContent {
            val week by changes.collectAsState(Agenda.week(context))
            val picked = currentState(PICKED)?.let(LocalDate::parse)
            CompositionLocalProvider(LocalColors provides Colors(week.look)) { TimelineContent(context, week, picked) }
        }
    }
}

/** How a block looks, by what is in it, as in the app's time grid: also how much it matters, the first
 * most, which a block of a few takes. Also the “月历” widget's labels. */
internal enum class State { OVERDUE, READY, WAITING, LATER }

internal fun stateOf(entry: Entry, now: LocalDateTime) = when {
    entry.later -> State.LATER
    entry.due.overdue(now) -> State.OVERDUE
    entry.ready -> State.READY
    else -> State.WAITING
}

/** A block of `state`: its ground, an image tinted, and the colour of the text on it. */
@Composable
internal fun blockOf(state: State): Triple<ImageProvider, ColorProvider, ColorProvider> {
    val colors = LocalColors.current
    val (ground, text) = when (state) {
        State.OVERDUE -> colors.late to colors.overdue
        State.READY -> colors.ready to colors.onReady
        State.WAITING -> colors.waiting to colors.onWaiting
        State.LATER -> colors.rail to colors.primary
    }
    return Triple(ImageProvider(if (state == State.LATER) R.drawable.widget_block_line else R.drawable.widget_block), ground, text)
}

/** The hours' column, on the left. */
private val GUTTER = 30.dp

/** The quick add card, adding to `day`, at `hour` should there be one. */
private fun add(context: Context, day: LocalDate, hour: Int?): Action {
    val intent = Intent(context, QuickAddActivity::class.java).putExtra(QuickAddActivity.ON, day.toString())
    if (hour != null) intent.putExtra(QuickAddActivity.AT, LocalTime.of(hour, 0).toString())
    return actionStartActivity(intent)
}

@Composable
private fun TimelineContent(context: Context, week: Week, picked: LocalDate?) {
    val now = week.now
    val today = now.toLocalDate()
    // Past midnight, the day picked may be one of the seven no more.
    val day = picked?.takeIf { it in week.days } ?: today
    val look = week.look
    val scale = look.scale
    val grid = open(context, "nexttodo://calendar/time/$day")
    Card {
        Header(look, "时间轴", grid, actionRunCallback<AgendaRefreshAction>(), add(context, day, null))
        if (!week.signedIn) {
            Note("打开 NextTodo 登录", open(context, "nexttodo://home"))
            return@Card
        }
        val gutter = GUTTER * scale
        Days(week, day, today, scale, { actionRunCallback<PickTimeDayAction>(actionParametersOf(DayKey to it.toString())) }, dots = false, gutter = gutter to grid)
        val (allDay, timed) = week.days.map { d -> week.due[d].orEmpty().partition { it.due.time == null } }.unzip()
        // A widget's list opens at its top, and can't be scrolled to now as the app's: from the morning, or
        // earlier for what is due then, to the evening, or later; with the hour now.
        val hours = timed.flatten().mapNotNull { it.due.time?.hour } + now.hour
        val first = minOf(8, hours.min())
        val last = maxOf(20, hours.max())
        val line = GlanceModifier.background(ImageProvider(R.drawable.widget_hour), colorFilter = ColorFilter.tint(LocalColors.current.divider))
        // Under the list, all the rest of the widget, as in the “接下来” widget.
        Box(GlanceModifier.defaultWeight().fillMaxWidth()) {
            Box(GlanceModifier.fillMaxSize().clickable(grid)) {}
            LazyColumn(GlanceModifier.fillMaxWidth()) {
                if (allDay.any { it.isNotEmpty() }) {
                    item {
                        Row(GlanceModifier.fillMaxWidth().height(22.dp * scale)) {
                            Hour("全天", false, gutter, grid, scale)
                            week.days.forEachIndexed { i, d -> Cell(GlanceModifier.defaultWeight(), context, d, null, allDay[i], d == today, now, scale) }
                        }
                    }
                }
                items((first..last).toList()) { hour ->
                    Row(line.fillMaxWidth().height(40.dp * scale)) {
                        Hour("$hour:00", hour == now.hour, gutter, grid, scale)
                        week.days.forEachIndexed { i, d ->
                            Cell(GlanceModifier.defaultWeight(), context, d, hour, timed[i].filter { it.due.time?.hour == hour }, d == today, now, scale)
                        }
                    }
                }
            }
        }
    }
}

/** An hour's label, `now` in the primary colour, on the left of its row; it opens the time grid, as
 * the title does. */
@Composable
private fun Hour(label: String, now: Boolean, width: Dp, grid: Action, scale: Float) {
    val colors = LocalColors.current
    Text(
        label,
        GlanceModifier.width(width).fillMaxHeight().padding(end = 4.dp, top = 2.dp).clickable(grid),
        style = TextStyle(
            color = if (now) colors.primary else colors.secondary,
            fontSize = 10.sp * scale,
            fontWeight = if (now) FontWeight.Bold else FontWeight.Normal,
            textAlign = TextAlign.End,
        ),
        maxLines = 1,
    )
}

/** A day's `hour`, or its row of what is due that day with no time: empty, it adds to the hour, or to
 * the day; with what is due in it, a block of one opens it, of a few, the time grid on that day. */
@Composable
private fun Cell(modifier: GlanceModifier, context: Context, day: LocalDate, hour: Int?, entries: List<Entry>, today: Boolean, now: LocalDateTime, scale: Float) {
    val colors = LocalColors.current
    // One element each, few as they are: the widget's views go to the launcher at once, which takes so much.
    if (entries.isEmpty()) {
        Box((if (today) modifier.background(colors.todayColumn) else modifier).fillMaxHeight().clickable(add(context, day, hour))) {}
        return
    }
    val one = entries.singleOrNull()
    val (block, ground, text) = blockOf(entries.minOf { stateOf(it, now) })
    Box(
        modifier.fillMaxHeight().background(block, colorFilter = ColorFilter.tint(ground)).padding(horizontal = 4.dp)
            .clickable(if (one != null) open(context, "nexttodo://task/${one.id}") else open(context, "nexttodo://calendar/time/$day")),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            one?.title ?: "${entries.size} 项",
            style = TextStyle(color = text, fontSize = 10.sp * scale, fontWeight = if (one == null) FontWeight.Bold else FontWeight.Medium, textAlign = TextAlign.Center),
            // With no time, a label of a line, as the app's.
            maxLines = if (hour == null) 1 else 2,
        )
    }
}

class PickTimeDayAction : ActionCallback {
    override suspend fun onAction(context: Context, glanceId: GlanceId, parameters: ActionParameters) {
        updateAppWidgetState(context, glanceId) { it[PICKED] = parameters[DayKey]!! }
        TimelineWidget().update(context, glanceId)
    }
}

class TimelineWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget = TimelineWidget()

    override fun onEnabled(context: Context) {
        super.onEnabled(context)
        Widget.schedule(context)
        Agenda.placedChanged(context)
        // The list now, which has when the next task starts, for the alarm: there may have been none.
        Widget.started(context)
    }

    override fun onDisabled(context: Context) {
        super.onDisabled(context)
        Widget.unschedule(context)
        Agenda.placedChanged(context)
    }
}
