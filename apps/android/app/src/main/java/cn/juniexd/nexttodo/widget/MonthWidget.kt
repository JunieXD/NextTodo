package cn.juniexd.nexttodo.widget

import android.appwidget.AppWidgetManager
import android.content.Context
import android.content.Intent
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.glance.ColorFilter
import androidx.glance.GlanceId
import androidx.glance.GlanceModifier
import androidx.glance.ImageProvider
import androidx.glance.LocalContext
import androidx.glance.LocalSize
import androidx.glance.action.Action
import androidx.glance.action.ActionParameters
import androidx.glance.action.actionParametersOf
import androidx.glance.action.clickable
import androidx.glance.appwidget.GlanceAppWidget
import androidx.glance.appwidget.GlanceAppWidgetReceiver
import androidx.glance.appwidget.LocalAppWidgetOptions
import androidx.glance.appwidget.SizeMode
import androidx.glance.appwidget.cornerRadius
import androidx.glance.appwidget.action.ActionCallback
import androidx.glance.appwidget.action.actionRunCallback
import androidx.glance.appwidget.action.actionStartActivity
import androidx.glance.appwidget.provideContent
import androidx.glance.appwidget.state.updateAppWidgetState
import androidx.glance.background
import androidx.glance.currentState
import androidx.glance.layout.Alignment
import androidx.glance.layout.Box
import androidx.glance.layout.Column
import androidx.glance.layout.Row
import androidx.glance.layout.fillMaxHeight
import androidx.glance.layout.fillMaxWidth
import androidx.glance.layout.height
import androidx.glance.layout.padding
import androidx.glance.layout.width
import androidx.glance.text.FontWeight
import androidx.glance.text.Text
import androidx.glance.text.TextAlign
import androidx.glance.text.TextStyle
import cn.juniexd.nexttodo.QuickAddActivity
import cn.juniexd.nexttodo.R
import java.time.LocalDate
import java.time.LocalDateTime

/** The home-screen month: six weeks of days and the titles of what is due on them, this month's or the
 * next. See docs/android/widgets.md#月历. */
class MonthWidget : GlanceAppWidget() {
    // One copy, as the time grid's: the launcher takes the whole widget at once, and this one has much in
    // it. Laid out by the height the options give (`height`).
    override val sizeMode = SizeMode.Single

    override suspend fun provideGlance(context: Context, id: GlanceId) {
        val changes = Agenda.weekChanges(context)
        provideContent {
            val week by changes.collectAsState(Agenda.week(context))
            // Turned to next month today, by the arrows in the title: past midnight, or with the title turned
            // off, it is this month again.
            val next = week.look.title && currentState(NEXT_ON) == week.now.toLocalDate().toString()
            CompositionLocalProvider(LocalColors provides Colors(week.look)) { MonthContent(context, week, next) }
        }
    }
}

/** The day next month was turned to, in the widget's own state. */
private val NEXT_ON = stringPreferencesKey("next_on")

private val NextKey = ActionParameters.Key<Boolean>("next")

/** Below the six weeks. */
private val BOTTOM = 6.dp

@Composable
private fun MonthContent(context: Context, week: Week, next: Boolean) {
    val now = week.now
    val today = now.toLocalDate()
    val look = week.look
    val scale = look.scale
    val first = today.withDayOfMonth(1).let { if (next) it.plusMonths(1) else it }
    // The calendar on its day: today in this month, else the first.
    val calendar = open(context, "nexttodo://calendar/month/${if (next) first else today}")
    val add = actionStartActivity(Intent(context, QuickAddActivity::class.java).putExtra(QuickAddActivity.ON, today.toString()))
    val refresh = actionRunCallback<AgendaRefreshAction>()
    Card {
        // Signing out, the list is emptied after the token is gone.
        val overdue = if (week.signedIn) week.overdue.size else 0
        if (look.title) MonthHeader(context, look, first, next, overdue, today, calendar, refresh, add) else Header(look, "", calendar, refresh, add)
        if (!week.signedIn) {
            Note("打开 NextTodo 登录", open(context, "nexttodo://home"))
            return@Card
        }
        // Heights in dp as the text in them grows, with the system's font size too.
        val grow = scale * LocalContext.current.resources.configuration.fontScale
        Row(GlanceModifier.fillMaxWidth().height(WEEKDAYS * grow).clickable(calendar), verticalAlignment = Alignment.CenterVertically) {
            for (w in "一二三四五六日") {
                Text("$w", GlanceModifier.defaultWeight(), style = TextStyle(color = LocalColors.current.secondary, fontSize = 11.sp * scale, textAlign = TextAlign.Center))
            }
        }
        // As many lines as there is room for under a day's number, up to three, as many views as there are;
        // none, a dot each, as the app's month, should there be room for them.
        val row = (height() - headerHeight(look, look.add) - WEEKDAYS * grow - BOTTOM) / 6
        val room = row - (DAY_LINE + 4.dp) * grow
        val lines = (room / (LINE * grow)).toInt().coerceIn(0, 3)
        val dots = room >= DOTS * grow
        val line = GlanceModifier.background(ImageProvider(R.drawable.widget_hour), colorFilter = ColorFilter.tint(LocalColors.current.divider))
        val start = first.minusDays(first.dayOfWeek.value - 1L)
        Column(GlanceModifier.defaultWeight().fillMaxWidth().padding(bottom = BOTTOM)) {
            for (w in 0L..5L) {
                Row(line.defaultWeight().fillMaxWidth()) {
                    for (d in 0L..6L) {
                        val day = start.plusDays(7 * w + d)
                        Cell(GlanceModifier.defaultWeight(), context, day, day.month == first.month, day == today, week.due[day].orEmpty(), week.labels[day], lines, dots, now, scale)
                    }
                }
            }
        }
    }
}

/** The weekdays' row. */
private val WEEKDAYS = 18.dp

/** The line of a day's number. */
private val DAY_LINE = 15.dp

/** A line under a day's number, a title. */
private val LINE = 16.dp

/** The dots under a day's number. */
private val DOTS = 8.dp

/** The widget's height on the home screen: the options' largest, which it has upright; before the
 * launcher has given any, the least it may be. */
@Composable
private fun height() = LocalAppWidgetOptions.current.getInt(AppWidgetManager.OPTION_APPWIDGET_MAX_HEIGHT).takeIf { it > 0 }?.dp ?: LocalSize.current.height

/** `‹ 11月 ›`, which turn between this month and the next and open the calendar on it, red `逾期 3`
 * when anything is, which opens it on today, and the buttons; as `Header` is, edge to edge. */
@Composable
private fun MonthHeader(context: Context, look: Look, first: LocalDate, next: Boolean, overdue: Int, today: LocalDate, calendar: Action, refresh: Action, add: Action) {
    val colors = LocalColors.current
    val scale = look.scale
    val height = TOP + 32.dp + UNDER_HEADER
    val text = GlanceModifier.fillMaxHeight().padding(top = TOP, bottom = UNDER_HEADER)
    // The arrow that goes nowhere, faint, opens the calendar as the title does: no gap is left.
    val arrow = @Composable { label: String, to: Boolean, start: Boolean ->
        val turns = to != next
        Box(
            text.width(if (start) EDGE + 14.dp else 28.dp).padding(start = if (start) EDGE - 4.dp else 0.dp)
                .clickable(if (turns) actionRunCallback<TurnMonthAction>(actionParametersOf(NextKey to to)) else calendar),
            contentAlignment = Alignment.Center,
        ) {
            Text(label, style = TextStyle(color = if (turns) colors.text else colors.done, fontSize = 20.sp * scale))
        }
    }
    Row(GlanceModifier.fillMaxWidth().height(height), verticalAlignment = Alignment.CenterVertically) {
        arrow("‹", false, true)
        Box(text.clickable(calendar), contentAlignment = Alignment.CenterStart) {
            // No year, this month's or the next: four cells may be as narrow as 250dp.
            Text("${first.monthValue}月", style = TextStyle(color = colors.text, fontSize = 17.sp * scale, fontWeight = FontWeight.Bold), maxLines = 1)
        }
        arrow("›", true, false)
        // The rest of the row, the arrows and the buttons keeping their room: `逾期 3`, cut short should
        // there be too little, or nothing, which opens the calendar as the title does.
        val late = overdue > 0
        Box(
            text.defaultWeight().padding(start = 4.dp, end = 4.dp).clickable(if (late) open(context, "nexttodo://calendar/month/$today") else calendar),
            contentAlignment = Alignment.CenterStart,
        ) {
            if (late) Text("逾期 $overdue", style = TextStyle(color = colors.overdue, fontSize = 12.sp * scale, fontWeight = FontWeight.Medium), maxLines = 1)
        }
        Buttons(look, refresh, add, TOP, UNDER_HEADER)
    }
}

/** A day: its number, what is written with it and 休 or 班; what is due on it in `lines` lines, titles
 * and how many more, or with none, `dots`. All of it opens the calendar on that day. Faint in another month; today
 * tinted. In few views: Glance gives the whole widget 500, a day takes up to ten, and a clickable or an
 * image ground takes two more each, a box around it and its ripple or the image; before Android 12, so
 * does a fixed size. */
@Composable
private fun Cell(modifier: GlanceModifier, context: Context, day: LocalDate, inMonth: Boolean, today: Boolean, entries: List<Entry>, label: DayLabel?, lines: Int, dots: Boolean, now: LocalDateTime, scale: Float) {
    val colors = LocalColors.current
    val cell = modifier.fillMaxHeight().padding(start = 1.dp, end = 1.dp, top = 3.dp).clickable(open(context, "nexttodo://calendar/month/$day"))
    Column(if (today) cell.background(colors.todayColumn) else cell) {
        Row(GlanceModifier.fillMaxWidth().padding(start = 2.dp, end = 2.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(
                "${day.dayOfMonth}",
                style = TextStyle(
                    color = when {
                        today -> colors.primary
                        inMonth -> colors.text
                        else -> colors.done
                    },
                    fontSize = 12.sp * scale,
                    fontWeight = if (today) FontWeight.Bold else FontWeight.Medium,
                ),
            )
            if (label != null) {
                Text(
                    label.text,
                    GlanceModifier.defaultWeight().padding(start = 2.dp),
                    style = TextStyle(color = if (!inMonth) colors.done else if (label.note) colors.primary else colors.secondary, fontSize = 8.sp * scale),
                    maxLines = 1,
                )
                label.holiday?.let { HolidayText(it, if (inMonth) null else colors.done, scale) }
            }
        }
        val states = entries.map { stateOf(it, now) }
        when {
            lines == 0 -> if (dots) Dots(states)
            lines == 1 && entries.size > 1 -> Label("${entries.size} 项", states.min(), scale)
            else -> {
                // The last line says how many more there are.
                val shown = if (entries.size <= lines) entries.size else lines - 1
                for (i in 0 until shown) Label(entries[i].title, states[i], scale)
                if (shown < entries.size) {
                    Text("+${entries.size - shown}", GlanceModifier.padding(start = 3.dp, top = 1.dp), style = TextStyle(color = colors.secondary, fontSize = 9.sp * scale), maxLines = 1)
                }
            }
        }
    }
}

/** A title on a day, a line, on the ground of its `state` as the time grid's blocks; that of a later time,
 * a ring there, on a faint one here: a ground of an image takes two views more. None between two: a view
 * each, the rounded corners part them. */
@Composable
private fun Label(title: String, state: State, scale: Float) {
    val colors = LocalColors.current
    val (_, ground, text) = blockOf(state)
    Text(
        title,
        GlanceModifier.fillMaxWidth().background(if (state == State.LATER) colors.divider else ground).cornerRadius(4.dp).padding(horizontal = 3.dp, vertical = 1.dp),
        style = TextStyle(color = text, fontSize = 10.sp * scale, fontWeight = FontWeight.Medium),
        maxLines = 1,
    )
}

/** A dot for each of the first three of `states`, those overdue in the error colour, as the app's month. */
@Composable
private fun Dots(states: List<State>) {
    val colors = LocalColors.current
    val first = states.take(3)
    val late = first.count { it == State.OVERDUE }
    val dots = { n: Int -> List(n) { "●" }.joinToString("\u2009") }
    Row(GlanceModifier.fillMaxWidth().padding(top = 2.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        if (late > 0) Text(dots(late), style = TextStyle(color = colors.overdue, fontSize = 5.sp))
        if (late < first.size) Text((if (late > 0) "\u2009" else "") + dots(first.size - late), style = TextStyle(color = colors.primary, fontSize = 5.sp))
    }
}

class TurnMonthAction : ActionCallback {
    override suspend fun onAction(context: Context, glanceId: GlanceId, parameters: ActionParameters) {
        updateAppWidgetState(context, glanceId) { if (parameters[NextKey]!!) it[NEXT_ON] = LocalDate.now().toString() else it.remove(NEXT_ON) }
        MonthWidget().update(context, glanceId)
    }
}

class MonthWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget = MonthWidget()

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
