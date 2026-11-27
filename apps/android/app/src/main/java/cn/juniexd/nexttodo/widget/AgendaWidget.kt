package cn.juniexd.nexttodo.widget

import android.content.Context
import android.content.Intent
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.glance.GlanceId
import androidx.glance.GlanceModifier
import androidx.glance.LocalSize
import androidx.glance.action.ActionParameters
import androidx.glance.action.clickable
import androidx.glance.appwidget.GlanceAppWidget
import androidx.glance.appwidget.GlanceAppWidgetReceiver
import androidx.glance.appwidget.SizeMode
import androidx.glance.appwidget.action.ActionCallback
import androidx.glance.appwidget.action.actionRunCallback
import androidx.glance.appwidget.lazy.LazyColumn
import androidx.glance.appwidget.lazy.itemsIndexed
import androidx.glance.appwidget.provideContent
import androidx.glance.background
import androidx.glance.layout.Alignment
import androidx.glance.layout.Box
import androidx.glance.layout.Column
import androidx.glance.layout.Row
import androidx.glance.layout.Spacer
import androidx.glance.layout.fillMaxSize
import androidx.glance.layout.fillMaxWidth
import androidx.glance.layout.height
import androidx.glance.layout.padding
import androidx.glance.text.FontWeight
import androidx.glance.text.Text
import androidx.glance.text.TextStyle
import java.time.LocalDate
import java.time.LocalDateTime
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

/** The home-screen agenda: what is due each of the next days. See docs/android/widgets.md#日程. */
class AgendaWidget : GlanceAppWidget() {
    /** Laid out for the size it is, as the “接下来” widget is. */
    override val sizeMode = SizeMode.Exact

    override val previewSizeMode = previewAt(300.dp, 170.dp)

    override suspend fun provideGlance(context: Context, id: GlanceId) {
        val changes = Agenda.changes(context)
        provideContent {
            val shown by changes.collectAsState(Agenda.shown(context))
            CompositionLocalProvider(LocalColors provides Colors(shown.look)) { AgendaContent(context, shown) }
        }
    }

    override suspend fun providePreview(context: Context, widgetCategory: Int) = provideContent {
        val shown = Sample.agendas(context)
        CompositionLocalProvider(LocalColors provides Colors(shown.look)) { AgendaContent(context, shown) }
    }
}

@Composable
private fun AgendaContent(context: Context, shown: Agendas) {
    val now = shown.now
    val look = shown.look
    val compact = LocalSize.current.height < COMPACT_BELOW
    val narrow = LocalSize.current.width < NARROW_BELOW
    val calendar = open(context, "nexttodo://calendar")
    Card {
        Header(look, "日程", calendar, actionRunCallback<AgendaRefreshAction>(), null)
        when {
            !shown.signedIn -> Note("打开 NextTodo 登录", open(context, "nexttodo://home"))
            shown.days.isEmpty() && shown.more == 0 -> Note("这 ${Agenda.DAYS} 天没有截止的事项", calendar)
            // Under the list, all the rest of the widget, as in the “接下来” widget.
            else -> Box(GlanceModifier.defaultWeight().fillMaxWidth()) {
                Box(GlanceModifier.fillMaxSize().clickable(calendar)) {}
                LazyColumn(GlanceModifier.fillMaxWidth()) {
                    for (day in shown.days) {
                        item { DayTitle(context, day.date, now.toLocalDate(), look.scale) }
                        itemsIndexed(day.entries) { i, entry -> EntryItem(context, entry, i == 0, day.date == null, now, compact, narrow, look.scale) }
                    }
                    if (shown.more > 0) item { AgendaMore(context, shown.more, look.scale) }
                }
            }
        }
    }
}

/** `已逾期` over what is overdue, `今天`, `明天`, then `周日 11月22日`; it opens the calendar. */
@Composable
internal fun DayTitle(context: Context, date: LocalDate?, today: LocalDate, scale: Float) {
    val title = when (date) {
        null -> "已逾期"
        today -> "今天"
        today.plusDays(1) -> "明天"
        else -> "周${"一二三四五六日"[date.dayOfWeek.value - 1]} ${date.monthValue}月${date.dayOfMonth}日"
    }
    val colors = LocalColors.current
    val color = if (date == null) colors.overdue else colors.secondary
    Text(
        title,
        GlanceModifier.fillMaxWidth().padding(start = EDGE, end = EDGE, top = 10.dp * scale, bottom = 2.dp).clickable(open(context, "nexttodo://calendar")),
        style = TextStyle(color = color, fontSize = 12.sp * scale, fontWeight = FontWeight.Bold),
        maxLines = 1,
    )
}

/** A task due on its day, and when; an `overdue` one says which day. It opens the task, the line above
 * it too. */
@Composable
private fun EntryItem(context: Context, entry: Entry, first: Boolean, overdue: Boolean, now: LocalDateTime, compact: Boolean, narrow: Boolean, scale: Float) {
    val colors = LocalColors.current
    Column(GlanceModifier.fillMaxWidth().padding(horizontal = EDGE).clickable(open(context, "nexttodo://task/${entry.id}"))) {
        if (!first) Spacer(GlanceModifier.fillMaxWidth().height(1.dp).background(colors.divider))
        val title = TextStyle(color = colors.text, fontSize = 15.sp * scale)
        val faint = TextStyle(color = colors.secondary, fontSize = 12.sp * scale)
        // A later time is due when the one now is done: it is late only by that one, which is not its own.
        val time = if (overdue) entry.due.label(now) else entry.due.time?.let { "%02d:%02d".format(it.hour, it.minute) }
        val timeStyle = TextStyle(color = if (!entry.later && entry.due.overdue(now)) colors.overdue else colors.secondary, fontSize = 12.sp * scale)
        Column(GlanceModifier.fillMaxWidth().padding(top = (if (compact) 6.dp else 8.dp) * scale, bottom = (if (compact) 6.dp else 8.dp) * scale)) {
            Row(GlanceModifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(entry.title, GlanceModifier.defaultWeight(), style = title, maxLines = 1)
                if (entry.later && !narrow) Text("以后的一次", GlanceModifier.padding(start = 6.dp), style = faint, maxLines = 1)
                if (time != null && !narrow) Text(time, GlanceModifier.padding(start = 8.dp), style = timeStyle, maxLines = 1)
            }
            if (!compact && entry.folder.isNotEmpty()) Text(entry.folder, style = faint, maxLines = 1)
        }
    }
}

/** Past the tasks listed, how many more there are, which opens the calendar. */
@Composable
internal fun AgendaMore(context: Context, more: Int, scale: Float) {
    Text(
        "还有 $more 项",
        GlanceModifier.fillMaxWidth().padding(horizontal = EDGE, vertical = 8.dp * scale).clickable(open(context, "nexttodo://calendar")),
        style = TextStyle(color = LocalColors.current.secondary, fontSize = 13.sp * scale),
        maxLines = 1,
    )
}

/** With the “接下来” widget's list too while there is a widget that shows which tasks can be done, for
 * the next start in it (`Widget.showsReady`). */
class AgendaRefreshAction : ActionCallback {
    override suspend fun onAction(context: Context, glanceId: GlanceId, parameters: ActionParameters) = refreshing(context) {
        Agenda.refresh(context)
        if (Widget.showsReady(context)) Widget.refresh(context)
    }
}

class AgendaWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget = AgendaWidget()

    override fun onEnabled(context: Context) {
        super.onEnabled(context)
        Widget.schedule(context)
        Agenda.placedChanged(context)
    }

    override fun onDisabled(context: Context) {
        super.onDisabled(context)
        Widget.unschedule(context)
        Agenda.placedChanged(context)
    }

    /** The alarm `Agenda.redrawAt` set; the phone starting or the app updated, which take it away; and
     * the clock or the time zone changed, which move what it was set for. */
    override fun onReceive(context: Context, intent: Intent) {
        val alarm = setOf(REDRAW, Intent.ACTION_BOOT_COMPLETED, Intent.ACTION_MY_PACKAGE_REPLACED, Intent.ACTION_TIME_CHANGED, Intent.ACTION_TIMEZONE_CHANGED)
        if (intent.action !in alarm) return super.onReceive(context, intent)
        val pending = goAsync()
        CoroutineScope(Dispatchers.IO).launch {
            try {
                // Turned back across midnight or a deadline, what is drawn is wrong, though the time it holds
                // until is still to come.
                val clock = intent.action == Intent.ACTION_TIME_CHANGED || intent.action == Intent.ACTION_TIMEZONE_CHANGED
                Agenda.redraw(context, anyway = clock)
                // What has started is told by the clock here too, and the start's alarm was set by it.
                if (clock && Widget.showsReady(context)) Widget.started(context)
            } finally {
                pending.finish()
            }
        }
    }

    companion object {
        /** For the “日程” and “一周” widgets both. */
        const val REDRAW = "cn.juniexd.nexttodo.AGENDA_REDRAW"
    }
}
