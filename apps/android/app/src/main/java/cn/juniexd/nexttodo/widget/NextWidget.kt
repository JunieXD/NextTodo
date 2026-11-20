package cn.juniexd.nexttodo.widget

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.SystemClock
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
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
import androidx.glance.appwidget.cornerRadius
import androidx.glance.appwidget.lazy.LazyColumn
import androidx.glance.appwidget.lazy.itemsIndexed
import androidx.glance.appwidget.provideContent
import androidx.glance.background
import androidx.glance.color.ColorProvider
import androidx.glance.layout.Alignment
import androidx.glance.layout.Box
import androidx.glance.layout.Column
import androidx.glance.layout.ColumnScope
import androidx.glance.layout.Row
import androidx.glance.layout.Spacer
import androidx.glance.layout.fillMaxHeight
import androidx.glance.layout.fillMaxSize
import androidx.glance.layout.fillMaxWidth
import androidx.glance.layout.height
import androidx.glance.layout.padding
import androidx.glance.layout.size
import androidx.glance.layout.width
import androidx.glance.text.FontWeight
import androidx.glance.text.TextDecoration
import androidx.glance.text.Text
import androidx.glance.text.TextStyle
import cn.juniexd.nexttodo.Changes
import cn.juniexd.nexttodo.MainActivity
import cn.juniexd.nexttodo.Palette
import cn.juniexd.nexttodo.Server
import cn.juniexd.nexttodo.R
import cn.juniexd.nexttodo.forUser
import java.time.LocalDateTime
import kotlinx.coroutines.delay

/** The palette's colours, light and dark. */
internal object Colors {
    private fun of(color: Palette.() -> Color) = ColorProvider(day = Palette.light.color(), night = Palette.dark.color())

    val background = of { surface }
    val text = of { onSurface }
    val secondary = of { onSurfaceVariant }
    val primary = of { primary }
    val overdue = of { error }
    val today = of { warn }
    val onPrimary = of { onPrimary }
    val done = of { outline }
    val divider = of { divider }
}

private val TaskKey = ActionParameters.Key<String>("task")
private val TitleKey = ActionParameters.Key<String>("title")

/** Opens the app at `nexttodo://…`, which the web app reads (`apps/web/src/mobile.ts`). */
internal fun open(context: Context, url: String) =
    actionStartActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url), context, MainActivity::class.java))

/** The home-screen widget: what can be done now. See docs/android.md#小组件. */
class NextWidget : GlanceAppWidget() {
    /** Laid out for the size it is, to fit more tasks in a low one. */
    override val sizeMode = SizeMode.Exact

    override suspend fun provideGlance(context: Context, id: GlanceId) {
        val changes = Widget.changes(context)
        // Glance keeps a session going for a while after an update or a tap, and updates during it
        // only recompose: what to show is read here, again on every change.
        provideContent {
            val shown by changes.collectAsState(Widget.shown(context))
            Content(context, shown, LocalDateTime.now())
        }
    }
}

/** Lower than this, a task takes a line, its folder left out: on most home screens, under three rows,
 * which the 4×2 the widget starts as is. */
internal val COMPACT_BELOW = 260.dp

/** How far the content keeps from the widget's sides, and from its top. Inside what is tapped there:
 * the widget leaves no gap, which HyperOS's launcher would give to whichever action it picks
 * (docs/android.md#小组件). */
internal val EDGE = 14.dp
internal val TOP = 10.dp

/** Below the header, and how tall it is with that and `TOP`. */
internal val UNDER_HEADER = 4.dp
internal val HEADER = TOP + 32.dp + UNDER_HEADER

@Composable
private fun Content(context: Context, shown: Shown, now: LocalDateTime) {
    val compact = LocalSize.current.height < COMPACT_BELOW
    Column(GlanceModifier.fillMaxSize().background(Colors.background).cornerRadius(20.dp)) {
        Row(GlanceModifier.fillMaxWidth().height(HEADER), verticalAlignment = Alignment.CenterVertically) {
            Box(
                GlanceModifier.defaultWeight().fillMaxHeight().padding(start = EDGE, top = TOP, bottom = UNDER_HEADER).clickable(open(context, "nexttodo://home")),
                contentAlignment = Alignment.CenterStart,
            ) {
                Text("接下来", style = TextStyle(color = Colors.text, fontSize = 17.sp * shown.scale, fontWeight = FontWeight.Bold))
            }
            Icon(R.drawable.ic_widget_refresh, "刷新", actionRunCallback<RefreshAction>(), 4.dp)
            Icon(R.drawable.ic_add, "添加事项", open(context, "nexttodo://add"), EDGE)
        }
        when {
            !shown.signedIn -> Note("打开 NextTodo 登录", open(context, "nexttodo://home"))
            shown.items.isEmpty() && shown.more == 0 -> Note("暂无可做的事项", open(context, "nexttodo://home"))
            // Under the list, all the rest of the widget, so that a tap below the last task opens the app:
            // beside the list rather than around it, whose actions it would take.
            else -> Box(GlanceModifier.defaultWeight().fillMaxWidth()) {
                Box(GlanceModifier.fillMaxSize().clickable(open(context, "nexttodo://home"))) {}
                LazyColumn(GlanceModifier.fillMaxWidth()) {
                    itemsIndexed(shown.items) { i, item -> TaskItem(context, item, i == 0, item.id in shown.ticked, now, compact, shown.scale) }
                    if (shown.more > 0) item { More(context, shown.more, shown.items.isEmpty(), shown.scale) }
                }
            }
        }
    }
}

/** A line between tasks, from where the titles start, as in the app's lists. */
@Composable
private fun Divider() {
    Box(GlanceModifier.fillMaxWidth().padding(start = CIRCLE)) {
        Spacer(GlanceModifier.fillMaxWidth().height(1.dp).background(Colors.divider))
    }
}

/** A task; one `ticked` off is shown done, while it is being completed. The circle completes it; the
 * rest of the item, the line above it too, opens it. The two are side by side in the tree, the circle
 * drawn over the start of the other, which comes first and leaves no gap: HyperOS's launcher takes a
 * tap on a clickable with another inside as a tap on the inner one, and a tap on none as a tap on
 * the first in the item. */
@Composable
private fun TaskItem(context: Context, item: Item, first: Boolean, ticked: Boolean, now: LocalDateTime, compact: Boolean, scale: Float) {
    Box(GlanceModifier.fillMaxWidth(), contentAlignment = Alignment.CenterStart) {
        Column(GlanceModifier.fillMaxWidth().padding(horizontal = EDGE).clickable(open(context, "nexttodo://task/${item.id}"))) {
            if (!first) Divider()
            val title = TextStyle(
                color = if (ticked) Colors.done else Colors.text,
                fontSize = 15.sp * scale,
                textDecoration = if (ticked) TextDecoration.LineThrough else null,
            )
            val doneToday = "今天已完成".takeIf { item.lastDone == now.toLocalDate() }
            // In a line, the folder gives way to the title.
            val meta = listOfNotNull(item.folder.takeIf { it.isNotEmpty() && !compact }, doneToday, item.due?.label(now)).joinToString(" · ")
            val color = when {
                item.due?.overdue(now) == true -> Colors.overdue
                item.due?.today(now) == true -> Colors.today
                else -> Colors.secondary
            }
            val metaStyle = TextStyle(color = color, fontSize = 12.sp * scale)
            val stepStyle = TextStyle(color = Colors.secondary, fontSize = 12.sp * scale)
            if (compact) {
                Row(GlanceModifier.fillMaxWidth().padding(start = CIRCLE, top = 7.dp * scale, bottom = 7.dp * scale), verticalAlignment = Alignment.CenterVertically) {
                    Text(item.title, GlanceModifier.defaultWeight(), style = title, maxLines = 1)
                    if (item.step != null) Text(item.step, GlanceModifier.padding(start = 6.dp), style = stepStyle, maxLines = 1)
                    if (meta.isNotEmpty()) Text(meta, GlanceModifier.padding(start = 8.dp), style = metaStyle, maxLines = 1)
                }
            } else {
                Column(GlanceModifier.fillMaxWidth().padding(start = CIRCLE, top = 8.dp * scale, bottom = 8.dp * scale)) {
                    // The step whole, however long the title.
                    Row(GlanceModifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                        Text(item.title, GlanceModifier.defaultWeight(), style = title, maxLines = 1)
                        if (item.step != null) Text(item.step, GlanceModifier.padding(start = 6.dp), style = stepStyle, maxLines = 1)
                    }
                    if (meta.isNotEmpty()) Text(meta, style = metaStyle, maxLines = 1)
                }
            }
        }
        if (ticked) {
            Box(GlanceModifier.width(EDGE + 26.dp).height(26.dp).padding(start = EDGE + 2.dp, top = 2.dp, end = 2.dp, bottom = 2.dp)) {
                Image(ImageProvider(R.drawable.ic_widget_dot), "已完成", GlanceModifier.fillMaxSize(), colorFilter = ColorFilter.tint(Colors.primary))
                Image(ImageProvider(R.drawable.ic_widget_tick), null, GlanceModifier.fillMaxSize(), colorFilter = ColorFilter.tint(Colors.onPrimary))
            }
        } else {
            // Larger than the circle, from the widget's side up to where the title starts, to be easy to
            // hit; round at the ends, for its ripple.
            Box(
                GlanceModifier.width(EDGE + CIRCLE).height(CIRCLE).padding(start = EDGE).cornerRadius(CIRCLE / 2).clickable(actionRunCallback<CompleteAction>(actionParametersOf(TaskKey to item.id, TitleKey to item.title))),
                contentAlignment = Alignment.CenterStart,
            ) {
                Image(ImageProvider(R.drawable.ic_widget_check), "标为完成", GlanceModifier.size(26.dp).padding(2.dp), colorFilter = ColorFilter.tint(Colors.primary))
            }
        }
    }
}

/** Past the tasks listed, how many more there are, which opens the app; `first` when none are listed,
 * all done here say, with no line above it then. */
@Composable
private fun More(context: Context, more: Int, first: Boolean, scale: Float) {
    Column(GlanceModifier.fillMaxWidth().padding(horizontal = EDGE).clickable(open(context, "nexttodo://home"))) {
        if (!first) Divider()
        Text("还有 $more 项", GlanceModifier.padding(start = CIRCLE, top = 8.dp * scale, bottom = 8.dp * scale), style = TextStyle(color = Colors.secondary, fontSize = 13.sp * scale))
    }
}

/** Where the title starts: the circle, and the space after it. */
private val CIRCLE = 36.dp

/** An icon of the header, 22dp across, tapped anywhere in the header's height and `end` beyond it. */
@Composable
internal fun Icon(drawable: Int, label: String, onClick: Action, end: Dp) {
    Image(
        ImageProvider(drawable),
        label,
        GlanceModifier.width(32.dp + end).fillMaxHeight().padding(start = 5.dp, top = TOP + 5.dp, end = 5.dp + end, bottom = UNDER_HEADER + 5.dp).clickable(onClick),
        colorFilter = ColorFilter.tint(Colors.secondary),
    )
}

/** In place of the list, all the rest of the widget: tapped, it opens the app. */
@Composable
internal fun ColumnScope.Note(text: String, onClick: Action) {
    Text(text, GlanceModifier.defaultWeight().fillMaxWidth().padding(horizontal = EDGE, vertical = 12.dp).clickable(onClick), style = TextStyle(color = Colors.secondary, fontSize = 14.sp))
}

/** Marks a task done (docs/android.md#原生发出的改动), and fetches the list again. The task shows as
 * done at once, and stays a moment before it goes, as in the app. */
class CompleteAction : ActionCallback {
    override suspend fun onAction(context: Context, glanceId: GlanceId, parameters: ActionParameters) {
        val task = parameters[TaskKey]!!
        val token = Server.token(context) ?: return
        val start = SystemClock.elapsedRealtime()
        Widget.tick(context, task, token)
        val outcome = try {
            Changes.complete(context, task, parameters[TitleKey]!!).also {
                if (it !is Changes.Outcome.Refused) delay(LINGER_MS - (SystemClock.elapsedRealtime() - start))
            }
        } finally {
            Widget.untick(context, task, token)
        }
        when (outcome) {
            is Changes.Outcome.Refused -> Widget.toast(context, outcome.message)
            // The agenda too, where the task moves on or goes.
            is Changes.Outcome.Sent -> refreshing(context) {
                Widget.refresh(context)
                if (Agenda.placed(context)) Agenda.refresh(context)
            }
            // Offline: it goes later, and the list with it.
            Changes.Outcome.Kept -> {}
        }
    }
}

/** How long a task ticked off stays in the list, as in the app (`apps/web/src/views/marks.tsx`). */
private const val LINGER_MS = 900L

/** Runs `fetch`, telling the user should it fail. */
internal suspend fun refreshing(context: Context, fetch: suspend () -> Unit) {
    try {
        fetch()
    } catch (e: Exception) {
        Widget.toast(context, forUser("刷新", e))
    }
}

class RefreshAction : ActionCallback {
    override suspend fun onAction(context: Context, glanceId: GlanceId, parameters: ActionParameters) = refreshing(context) { Widget.refresh(context) }
}

class NextWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget = NextWidget()

    override fun onEnabled(context: Context) {
        super.onEnabled(context)
        Widget.schedule(context)
    }

    override fun onDisabled(context: Context) {
        super.onDisabled(context)
        Widget.unschedule(context)
    }

    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action == STARTED) Widget.started(context) else super.onReceive(context, intent)
    }

    companion object {
        /** A task in the list has started: see `Widget.refreshAt`. */
        const val STARTED = "cn.juniexd.nexttodo.WIDGET_STARTED"
    }
}
