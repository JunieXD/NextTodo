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
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.glance.ColorFilter
import androidx.glance.GlanceId
import androidx.glance.GlanceModifier
import androidx.glance.Image
import androidx.glance.ImageProvider
import androidx.glance.LocalSize
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
import androidx.glance.appwidget.lazy.itemsIndexed
import androidx.glance.appwidget.provideContent
import androidx.glance.appwidget.state.updateAppWidgetState
import androidx.glance.currentState
import androidx.glance.layout.Alignment
import androidx.glance.layout.Box
import androidx.glance.layout.Column
import androidx.glance.layout.Row
import androidx.glance.layout.Spacer
import androidx.glance.layout.fillMaxSize
import androidx.glance.layout.fillMaxWidth
import androidx.glance.layout.height
import androidx.glance.layout.padding
import androidx.glance.layout.size
import androidx.glance.layout.width
import androidx.glance.text.FontWeight
import androidx.glance.text.Text
import androidx.glance.text.TextDecoration
import androidx.glance.text.TextStyle
import androidx.glance.unit.ColorProvider
import cn.juniexd.nexttodo.QuickAddActivity
import cn.juniexd.nexttodo.R
import java.time.LocalDate
import java.time.LocalDateTime

/** The home-screen week: seven days to pick one of, and what is due on it, to tick off. See
 * docs/android/widgets.md#一周. */
class WeekWidget : GlanceAppWidget() {
    override val sizeMode = SizeMode.Exact

    override suspend fun provideGlance(context: Context, id: GlanceId) {
        val changes = Agenda.weekChanges(context)
        provideContent {
            val week by changes.collectAsState(Agenda.week(context))
            val picked = currentState(PICKED)?.let(LocalDate::parse)
            CompositionLocalProvider(LocalColors provides Colors(week.look)) { WeekContent(context, week, picked) }
        }
    }
}

/** The day picked, in the widget's own state. */
internal val PICKED = stringPreferencesKey("picked")

internal val DayKey = ActionParameters.Key<String>("day")

@Composable
private fun WeekContent(context: Context, week: Week, picked: LocalDate?) {
    val now = week.now
    val today = now.toLocalDate()
    // Past midnight, the day picked may be one of the seven no more.
    val day = picked?.takeIf { it in week.days } ?: today
    val look = week.look
    val calendar = open(context, "nexttodo://calendar")
    val add = actionStartActivity(Intent(context, QuickAddActivity::class.java).putExtra(QuickAddActivity.ON, day.toString()))
    Card {
        Header(look, "一周", calendar, actionRunCallback<AgendaRefreshAction>(), add)
        if (!week.signedIn) {
            Note("打开 NextTodo 登录", open(context, "nexttodo://home"))
            return@Card
        }
        Days(week, day, today, look.scale, { actionRunCallback<PickDayAction>(actionParametersOf(DayKey to it.toString())) })
        // Today's list starts with what is overdue, in a group of its own, as the agenda's does.
        val overdue = if (day == today) week.overdue else emptyList()
        val entries = overdue + week.due[day].orEmpty()
        val compact = LocalSize.current.height < COMPACT_BELOW
        if (entries.isEmpty()) {
            DayTitle(context, day, today, look.scale)
            Note(if (day.isBefore(today)) "这天没有没完成的事项" else "这天没有截止的事项", calendar)
            return@Card
        }
        val shown = entries.take(Agenda.MOST)
        val late = shown.take(overdue.size)
        val due = shown.drop(overdue.size)
        // Under the list, all the rest of the widget, as in the “接下来” widget.
        Box(GlanceModifier.defaultWeight().fillMaxWidth()) {
            Box(GlanceModifier.fillMaxSize().clickable(calendar)) {}
            LazyColumn(GlanceModifier.fillMaxWidth()) {
                if (late.isNotEmpty()) item { DayTitle(context, null, today, look.scale) }
                itemsIndexed(late) { i, entry -> WeekEntry(context, entry, i == 0, week.ticked.any(entry::same), day, now, compact, look.scale) }
                if (due.isNotEmpty()) item { DayTitle(context, day, today, look.scale) }
                itemsIndexed(due) { i, entry -> WeekEntry(context, entry, i == 0, week.ticked.any(entry::same), day, now, compact, look.scale) }
                if (entries.size > Agenda.MOST) item { AgendaMore(context, entries.size - Agenda.MOST, look.scale) }
            }
        }
    }
}

/** The seven days, side by side, each doing what `pick` gives for it: what day of the week, `今`
 * today; its number, filled in the day `picked`, should there be one; and, with `dots`, a dot for each of up to three tasks due
 * on it, red for one overdue. After a `gutter` as wide as the time grid's hours, should there be one,
 * which opens what its action does. */
@Composable
internal fun Days(week: Week, picked: LocalDate?, today: LocalDate, scale: Float, pick: (LocalDate) -> Action, dots: Boolean = true, gutter: Pair<Dp, Action>? = null) {
    val colors = LocalColors.current
    // Each day from edge to edge of its share of the width, the first and last to the widget's: no gap
    // (see `EDGE`).
    val box = (if (dots) 40.dp else 32.dp) * scale
    Row(GlanceModifier.fillMaxWidth()) {
        gutter?.let { (width, action) ->
            // Laid out as a day with nothing in it, as tall: what fills the row's height takes all there is.
            Column(GlanceModifier.width(width).padding(vertical = 2.dp).clickable(action)) {
                Text("", style = TextStyle(fontSize = 12.sp * scale))
                Spacer(GlanceModifier.height(box))
            }
        }
        for (d in week.days) {
            val chosen = d == picked
            Column(
                GlanceModifier.defaultWeight().padding(horizontal = 2.dp, vertical = 2.dp).clickable(pick(d)),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(
                    if (d == today) "今" else "${"一二三四五六日"[d.dayOfWeek.value - 1]}",
                    style = TextStyle(color = colors.secondary, fontSize = 12.sp * scale),
                )
                Box(GlanceModifier.fillMaxWidth().height(box).padding(top = 2.dp), contentAlignment = Alignment.Center) {
                    if (chosen) Image(ImageProvider(R.drawable.widget_day), null, GlanceModifier.fillMaxSize(), colorFilter = ColorFilter.tint(colors.primary))
                    week.labels[d]?.holiday?.let { HolidayMark(it, if (chosen) colors.onPrimary else null, scale) }
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text(
                            "${d.dayOfMonth}",
                            style = TextStyle(
                                color = if (chosen) colors.onPrimary else if (d == today) colors.primary else colors.text,
                                fontSize = 15.sp * scale,
                                fontWeight = if (d == today) FontWeight.Bold else FontWeight.Normal,
                            ),
                        )
                        if (dots) Row(GlanceModifier.height(7.dp).padding(top = 3.dp)) {
                            week.due[d].orEmpty().take(3).forEachIndexed { i, entry ->
                                if (i > 0) Spacer(GlanceModifier.width(3.dp))
                                val dot = when {
                                    chosen -> colors.onPrimary
                                    !entry.later && entry.due.overdue(week.now) -> colors.overdue
                                    else -> colors.primary
                                }
                                Image(ImageProvider(R.drawable.ic_widget_dot), null, GlanceModifier.size(4.dp), colorFilter = ColorFilter.tint(dot))
                            }
                        }
                    }
                }
            }
        }
    }
}

/** 休 or 班 in the top corner of a day, over what the day's box has. */
@Composable
internal fun HolidayMark(holiday: String, color: ColorProvider?, scale: Float) {
    Box(GlanceModifier.fillMaxSize().padding(top = 1.dp, end = 3.dp), contentAlignment = Alignment.TopEnd) { HolidayText(holiday, color, scale) }
}

/** 休 or 班, as `holiday` has it (docs/ui/calendar.md#日历), in `color` or else in the primary colour or the amber. */
@Composable
internal fun HolidayText(holiday: String, color: ColorProvider?, scale: Float) {
    val colors = LocalColors.current
    Text(
        if (holiday == "off") "休" else "班",
        style = TextStyle(color = color ?: if (holiday == "off") colors.primary else colors.today, fontSize = 8.sp * scale, fontWeight = FontWeight.Bold),
    )
}

/** A task on the day picked, as the agenda's, with a circle: one to do ticks it off, as in the “接下来”
 * widget, `ticked` showing it done meanwhile; one waiting, or a later time, can't be, and is open to see
 * why, as the app's are. The rest of it, the line above it too, opens the task. */
@Composable
private fun WeekEntry(context: Context, entry: Entry, first: Boolean, ticked: Boolean, day: LocalDate, now: LocalDateTime, compact: Boolean, scale: Float) {
    val colors = LocalColors.current
    Box(GlanceModifier.fillMaxWidth(), contentAlignment = Alignment.CenterStart) {
        Column(GlanceModifier.fillMaxWidth().padding(horizontal = EDGE).clickable(open(context, "nexttodo://task/${entry.id}"))) {
            if (!first) Divider()
            val title = TextStyle(
                color = if (ticked) colors.done else colors.text,
                fontSize = 15.sp * scale,
                textDecoration = if (ticked) TextDecoration.LineThrough else null,
            )
            val faint = TextStyle(color = colors.secondary, fontSize = 12.sp * scale)
            // Overdue from a day before the one it is listed on, it says which; a later time is on its day.
            val time = if (!entry.later && entry.due.date.isBefore(day)) entry.due.label(now) else entry.due.time?.let { "%02d:%02d".format(it.hour, it.minute) }
            // A later time is due when the one now is done: it is late only by that one, which is not its own.
            val timeStyle = TextStyle(color = if (!entry.later && entry.due.overdue(now)) colors.overdue else colors.secondary, fontSize = 12.sp * scale)
            val space = (if (compact) 6.dp else 8.dp) * scale
            Column(GlanceModifier.fillMaxWidth().padding(start = CIRCLE, top = space, bottom = space)) {
                Row(GlanceModifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Text(entry.title, GlanceModifier.defaultWeight(), style = title, maxLines = 1)
                    if (entry.later) Text("以后的一次", GlanceModifier.padding(start = 6.dp), style = faint, maxLines = 1)
                    if (time != null) Text(time, GlanceModifier.padding(start = 8.dp), style = timeStyle, maxLines = 1)
                }
                if (!compact && entry.folder.isNotEmpty()) Text(entry.folder, style = faint, maxLines = 1)
            }
        }
        if (entry.ready && !entry.later) {
            Tick(entry.id, entry.title, entry.occurrence, ticked)
        } else {
            // Not to be tapped: a tap here opens the task, as on the rest of the line.
            Box(GlanceModifier.width(EDGE + CIRCLE).height(CIRCLE).padding(start = EDGE), contentAlignment = Alignment.CenterStart) {
                Image(ImageProvider(R.drawable.ic_widget_dashed), null, GlanceModifier.size(26.dp).padding(2.dp), colorFilter = ColorFilter.tint(colors.rail))
            }
        }
    }
}

class PickDayAction : ActionCallback {
    override suspend fun onAction(context: Context, glanceId: GlanceId, parameters: ActionParameters) {
        updateAppWidgetState(context, glanceId) { it[PICKED] = parameters[DayKey]!! }
        WeekWidget().update(context, glanceId)
    }
}

class WeekWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget = WeekWidget()

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
