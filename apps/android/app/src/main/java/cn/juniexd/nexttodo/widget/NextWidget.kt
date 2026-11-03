package cn.juniexd.nexttodo.widget

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.glance.ColorFilter
import androidx.glance.GlanceId
import androidx.glance.GlanceModifier
import androidx.glance.Image
import androidx.glance.ImageProvider
import androidx.glance.action.ActionParameters
import androidx.glance.action.actionParametersOf
import androidx.glance.action.clickable
import androidx.glance.appwidget.GlanceAppWidget
import androidx.glance.appwidget.GlanceAppWidgetReceiver
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
import androidx.glance.layout.Row
import androidx.glance.layout.Spacer
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
import cn.juniexd.nexttodo.MainActivity
import cn.juniexd.nexttodo.R
import java.time.LocalDateTime

/** The colours of docs/ui.md, light and dark. */
private object Colors {
    val background = ColorProvider(day = Color(0xFFF5FBF6), night = Color(0xFF0F1512))
    val text = ColorProvider(day = Color(0xFF171D1A), night = Color(0xFFDEE4DF))
    val secondary = ColorProvider(day = Color(0xFF404944), night = Color(0xFFBFC9C2))
    val primary = ColorProvider(day = Color(0xFF1B6B51), night = Color(0xFF8BD6B6))
    val overdue = ColorProvider(day = Color(0xFFBA1A1A), night = Color(0xFFFFB4AB))
    val today = ColorProvider(day = Color(0xFF8A5100), night = Color(0xFFFFB870))
    val onPrimary = ColorProvider(day = Color(0xFFFFFFFF), night = Color(0xFF003827))
    val done = ColorProvider(day = Color(0xFF707973), night = Color(0xFF89938D))
    /** Fainter than outline-variant: the widget can't draw the app's half-pixel lines. */
    val divider = ColorProvider(day = Color(0xFFDDE4DF), night = Color(0xFF2A302D))
}

private val TaskKey = ActionParameters.Key<String>("task")
private val NumberKey = ActionParameters.Key<Int>("number")

/** Opens the app at `nexttodo://…`, which the web app reads (`apps/web/src/android.ts`). */
private fun open(context: Context, url: String) =
    actionStartActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url), context, MainActivity::class.java))

/** The home-screen widget: what can be done now. See docs/android.md#小组件. */
class NextWidget : GlanceAppWidget() {
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

@Composable
private fun Content(context: Context, shown: Shown, now: LocalDateTime) {
    Column(GlanceModifier.fillMaxSize().background(Colors.background).cornerRadius(20.dp).padding(horizontal = 14.dp, vertical = 10.dp)) {
        Row(GlanceModifier.fillMaxWidth().padding(bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(
                "接下来",
                GlanceModifier.defaultWeight().clickable(open(context, "nexttodo://home")),
                style = TextStyle(color = Colors.text, fontSize = 17.sp, fontWeight = FontWeight.Bold),
            )
            Icon(R.drawable.ic_widget_refresh, "刷新", actionRunCallback<RefreshAction>())
            Spacer(GlanceModifier.width(4.dp))
            Icon(R.drawable.ic_widget_add, "添加事项", open(context, "nexttodo://add"))
        }
        when {
            !shown.signedIn -> Note("打开 NextTodo 登录", GlanceModifier.clickable(open(context, "nexttodo://home")))
            shown.items.isEmpty() -> Note("暂无可做的事项")
            else -> LazyColumn {
                itemsIndexed(shown.items) { i, item ->
                    Column {
                        if (i > 0) Divider()
                        TaskRow(context, item, item.id in shown.ticked, now)
                    }
                }
            }
        }
    }
}

/** A line between tasks, from where the titles start, as in the app's lists. */
@Composable
private fun Divider() {
    Box(GlanceModifier.fillMaxWidth().padding(start = 36.dp)) {
        Spacer(GlanceModifier.fillMaxWidth().height(1.dp).background(Colors.divider))
    }
}

/** A task; one `ticked` off is shown done, while it is being completed. */
@Composable
private fun TaskRow(context: Context, item: Item, ticked: Boolean, now: LocalDateTime) {
    Row(GlanceModifier.fillMaxWidth().padding(vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        if (ticked) {
            Box(GlanceModifier.size(26.dp).padding(2.dp), contentAlignment = Alignment.Center) {
                Image(ImageProvider(R.drawable.ic_widget_dot), "已完成", GlanceModifier.fillMaxSize(), colorFilter = ColorFilter.tint(Colors.primary))
                Image(ImageProvider(R.drawable.ic_widget_tick), null, GlanceModifier.fillMaxSize(), colorFilter = ColorFilter.tint(Colors.onPrimary))
            }
        } else {
            Image(
                ImageProvider(R.drawable.ic_widget_check),
                "标为完成",
                GlanceModifier.size(26.dp).padding(2.dp).clickable(actionRunCallback<CompleteAction>(actionParametersOf(TaskKey to item.id, NumberKey to item.number))),
                colorFilter = ColorFilter.tint(Colors.primary),
            )
        }
        Column(GlanceModifier.defaultWeight().padding(start = 10.dp).clickable(open(context, "nexttodo://task/${item.id}"))) {
            val title = if (ticked) TextStyle(color = Colors.done, fontSize = 15.sp, textDecoration = TextDecoration.LineThrough) else TextStyle(color = Colors.text, fontSize = 15.sp)
            Text(item.title, style = title, maxLines = 1)
            val meta = listOfNotNull(item.folder.takeIf { it.isNotEmpty() }, item.due?.label(now)).joinToString(" · ")
            if (meta.isNotEmpty()) {
                val color = when {
                    item.due?.overdue(now) == true -> Colors.overdue
                    item.due?.today(now) == true -> Colors.today
                    else -> Colors.secondary
                }
                Text(meta, style = TextStyle(color = color, fontSize = 12.sp), maxLines = 1)
            }
        }
    }
}

@Composable
private fun Icon(drawable: Int, label: String, onClick: androidx.glance.action.Action) {
    Image(ImageProvider(drawable), label, GlanceModifier.size(32.dp).padding(5.dp).clickable(onClick), colorFilter = ColorFilter.tint(Colors.secondary))
}

@Composable
private fun Note(text: String, modifier: GlanceModifier = GlanceModifier) {
    Text(text, modifier.padding(vertical = 12.dp), style = TextStyle(color = Colors.secondary, fontSize = 14.sp))
}

class CompleteAction : ActionCallback {
    override suspend fun onAction(context: Context, glanceId: GlanceId, parameters: ActionParameters) {
        Widget.complete(context, parameters[TaskKey]!!, parameters[NumberKey]!!)
    }
}

class RefreshAction : ActionCallback {
    override suspend fun onAction(context: Context, glanceId: GlanceId, parameters: ActionParameters) {
        try {
            Widget.refresh(context)
        } catch (e: Exception) {
            Widget.toast(context, "刷新失败：${e.message}")
        }
    }
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
}
