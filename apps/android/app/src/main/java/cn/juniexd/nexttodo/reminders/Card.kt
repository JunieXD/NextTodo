package cn.juniexd.nexttodo.reminders

import android.content.Context
import android.content.Intent
import android.graphics.PixelFormat
import android.net.Uri
import android.os.Bundle
import android.view.Gravity
import android.view.WindowManager
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.platform.ComposeView
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleRegistry
import androidx.lifecycle.setViewTreeLifecycleOwner
import androidx.savedstate.SavedStateRegistry
import androidx.savedstate.SavedStateRegistryController
import androidx.savedstate.SavedStateRegistryOwner
import androidx.savedstate.setViewTreeSavedStateRegistryOwner
import cn.juniexd.nexttodo.Changes
import cn.juniexd.nexttodo.MainActivity
import cn.juniexd.nexttodo.R
import cn.juniexd.nexttodo.ui.LocalPalette
import cn.juniexd.nexttodo.ui.NextTodoTheme
import java.time.LocalDateTime
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * The reminder card (docs/android.md#弹窗提醒): a window over whatever is on screen, which needs only
 * the permission to draw over other apps; an activity started from the background would need HyperOS's
 * own permission too. It takes no focus, so touches around it and the back key go to the app below.
 * One reminder at a time, the others queued behind it. On the main thread.
 */
object Card {
    private val queue = mutableStateListOf<Reminder>()

    /** The card's window while it is open, what it composes in, and how to stop watching the reminders. */
    private class Open(val view: ComposeView, val owner: Owner, val unwatch: () -> Unit)

    private var window: Open? = null

    /** The task being done on the card: it stays its moment there, whatever comes meanwhile. */
    private var completing: String? = null

    /** When the card shows the reminders as of, which may wait for a start date: as of each new list,
     * past midnight say, and each time it opens. */
    private var now by mutableStateOf(LocalDateTime.now())

    /** Changes go on after the card has gone. */
    private val background = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    /** Shows `reminders` on the card, behind those on it already. */
    fun show(context: Context, reminders: List<Reminder>) {
        now = LocalDateTime.now()
        for (r in reminders) if (queue.none { it.task == r.task }) queue.add(r)
        if (queue.isNotEmpty() && window == null) open(context.applicationContext)
    }

    private fun open(context: Context) {
        val owner = Owner()
        val view = ComposeView(context)
        view.setViewTreeLifecycleOwner(owner)
        view.setViewTreeSavedStateRegistryOwner(owner)
        view.setContent {
            NextTodoTheme {
                val r = queue.firstOrNull() ?: return@NextTodoTheme
                key(r.task) {
                    ReminderCard(
                        r,
                        now,
                        more = queue.size - 1,
                        snoozeFirst = false,
                        close = ::close,
                        open = {
                            openTask(context, r.task)
                            close()
                        },
                        complete = {
                            completing = r.task
                            complete(context, r)
                        },
                        snooze = {
                            Reminders.snooze(context, r.task, it)
                            next(r.task)
                        },
                        next = { next(r.task) },
                    )
                }
            }
        }
        val params = WindowManager.LayoutParams(
            WindowManager.LayoutParams.MATCH_PARENT,
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL,
            PixelFormat.TRANSLUCENT,
        )
        params.gravity = Gravity.BOTTOM
        params.windowAnimations = android.R.style.Animation_InputMethod
        context.getSystemService(WindowManager::class.java).addView(view, params)
        window = Open(view, owner, Reminders.watch(context) { retain(context) })
    }

    private fun next(task: String) {
        queue.removeAll { it.task == task }
        completing = null
        if (queue.isEmpty()) close()
    }

    /** Keeps the card as the reminders are: takes off what is done, deleted, moved or put off since,
     * from the notification, the widget or the app, and shows the rest as they are now, waiting for
     * another task say. Done once more from the card, a repeating task would move on twice; one that
     * has come to wait would skip what the app asks first. */
    private fun retain(context: Context) {
        now = LocalDateTime.now()
        val current = Reminders.current(context).associateBy { it.task to it.at }
        queue.removeAll { (it.task to it.at) !in current && it.task != completing }
        for (i in queue.indices) current[queue[i].task to queue[i].at]?.let { queue[i] = it }
        if (queue.isEmpty()) close()
    }

    /** Takes the card away, and every reminder on it. */
    fun close() {
        val open = window ?: return
        open.view.context.getSystemService(WindowManager::class.java).removeView(open.view)
        open.owner.destroy()
        open.unwatch()
        window = null
        queue.clear()
        completing = null
    }

    /** Marks `r`'s task done, as the notification's “完成” does. */
    fun complete(context: Context, r: Reminder) {
        Reminders.withdraw(context, r.task)
        background.launch {
            val outcome = Changes.complete(context, r.task, r.title)
            if (outcome is Changes.Outcome.Refused) withContext(Dispatchers.Main) { Toast.makeText(context, outcome.message, Toast.LENGTH_LONG).show() }
        }
    }

    fun openTask(context: Context, task: String) {
        context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("nexttodo://task/$task"), context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    /** What a window outside any activity needs to compose in. */
    private class Owner : SavedStateRegistryOwner {
        private val registry = LifecycleRegistry(this)
        private val savedState = SavedStateRegistryController.create(this)
        override val lifecycle: Lifecycle get() = registry
        override val savedStateRegistry: SavedStateRegistry get() = savedState.savedStateRegistry

        init {
            savedState.performRestore(null)
            registry.currentState = Lifecycle.State.RESUMED
        }

        fun destroy() {
            registry.currentState = Lifecycle.State.DESTROYED
        }
    }
}

/** The reminder card opened from “稍后提醒” on a notification, at its options: by the user's tap, so
 * it needs no permission. Over the app in use; tapping outside or back closes it, putting nothing off. */
class SnoozeActivity : ComponentActivity() {
    private var unwatch = {}

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        var r by mutableStateOf(Reminders.find(this, intent.data!!.lastPathSegment!!) ?: return finish())
        var now by mutableStateOf(LocalDateTime.now())
        var completing = false
        // As the card: gone once done, deleted, moved or put off meanwhile, from the notification say;
        // shown as it is now otherwise.
        unwatch = Reminders.watch(this) {
            now = LocalDateTime.now()
            val current = Reminders.current(this).find { it.task == r.task && it.at == r.at }
            if (current != null) r = current else if (!completing) finish()
        }
        setContent {
            NextTodoTheme {
                ReminderCard(
                    r,
                    now,
                    more = 0,
                    snoozeFirst = true,
                    close = ::finish,
                    open = {
                        Card.openTask(this, r.task)
                        finish()
                    },
                    complete = {
                        completing = true
                        Card.complete(applicationContext, r)
                    },
                    snooze = {
                        Reminders.snooze(this, r.task, it)
                        finish()
                    },
                    next = ::finish,
                )
            }
        }
        // After the content: a floating window takes its size from the theme until then.
        window.setLayout(WindowManager.LayoutParams.MATCH_PARENT, WindowManager.LayoutParams.WRAP_CONTENT)
        window.setGravity(Gravity.BOTTOM)
    }

    override fun onDestroy() {
        unwatch()
        super.onDestroy()
    }

    /** “稍后提醒” on another notification while this is open: that reminder instead. */
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        recreate()
    }
}

/** How long a task done stays on the card, as in the app (`apps/web/src/views/marks.tsx`). */
private const val LINGER_MS = 900L

/** The card for `r` as of `now`, `more` behind it: “完成” or, for one that waits, “打开”, and “稍后提醒”,
 * whose options it can open at (`snoozeFirst`). `close` takes the card away, `next` goes on to the next. */
@Composable
private fun ReminderCard(
    r: Reminder,
    now: LocalDateTime,
    more: Int,
    snoozeFirst: Boolean,
    close: () -> Unit,
    open: () -> Unit,
    complete: () -> Unit,
    snooze: (minutes: Int) -> Unit,
    next: () -> Unit,
) {
    var snoozing by remember { mutableStateOf(snoozeFirst) }
    var done by remember { mutableStateOf(false) }
    val ready = r.ready(now.toLocalDate())
    LaunchedEffect(done) {
        if (!done) return@LaunchedEffect
        delay(LINGER_MS)
        next()
    }
    Surface(
        Modifier.fillMaxWidth().padding(12.dp),
        shape = RoundedCornerShape(28.dp),
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        shadowElevation = 6.dp,
    ) {
        Column(Modifier.padding(start = 12.dp, end = 8.dp, top = 8.dp, bottom = 16.dp)) {
            Row(verticalAlignment = Alignment.Top) {
                if (snoozing) {
                    IconButton({ snoozing = false }) { Icon(painterResource(R.drawable.ic_back), "返回") }
                } else {
                    Check(ready, done, Modifier.padding(12.dp))
                }
                Column(Modifier.weight(1f).clip(RoundedCornerShape(12.dp)).clickable(onClick = open).padding(horizontal = 4.dp, vertical = 10.dp)) {
                    Text(
                        r.title,
                        fontSize = 17.sp,
                        fontWeight = FontWeight.SemiBold,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                        color = if (done) MaterialTheme.colorScheme.outline else MaterialTheme.colorScheme.onSurface,
                        textDecoration = if (done) TextDecoration.LineThrough else null,
                    )
                    Text(r.meta(now), fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
                if (more > 0) Text("还有 $more 项", Modifier.padding(top = 14.dp), fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                IconButton(close) { Icon(painterResource(R.drawable.ic_close), "关闭") }
            }
            Spacer(Modifier.height(12.dp))
            if (snoozing) {
                SnoozeOptions(snooze)
            } else {
                Row(Modifier.fillMaxWidth().padding(end = 8.dp), Arrangement.spacedBy(8.dp, Alignment.End)) {
                    FilledTonalButton({ snoozing = true }, enabled = !done) { Text("稍后提醒") }
                    if (ready) {
                        Button(
                            {
                                done = true
                                complete()
                            },
                            enabled = !done,
                        ) { Text("完成") }
                    } else {
                        Button(open) { Text("打开") }
                    }
                }
            }
        }
    }
}

/** The circle of a task, as in the app's lists: done, a tick on a filled circle; waiting, in grey. */
@Composable
private fun Check(ready: Boolean, done: Boolean, modifier: Modifier) {
    val primary = MaterialTheme.colorScheme.primary
    Box(modifier.size(24.dp)) {
        if (done) {
            Image(painterResource(R.drawable.ic_widget_dot), "已完成", colorFilter = ColorFilter.tint(primary))
            Image(painterResource(R.drawable.ic_widget_tick), null, colorFilter = ColorFilter.tint(MaterialTheme.colorScheme.onPrimary))
        } else {
            Image(painterResource(R.drawable.ic_widget_check), null, colorFilter = ColorFilter.tint(if (ready) primary else LocalPalette.current.rail))
        }
    }
}

/** How long to put a reminder off: the usual choices, or a number of minutes or hours, up to a day. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun SnoozeOptions(snooze: (minutes: Int) -> Unit) {
    var custom by remember { mutableStateOf(false) }
    if (!custom) {
        FlowRow(Modifier.padding(horizontal = 12.dp), Arrangement.spacedBy(8.dp), Arrangement.spacedBy(8.dp)) {
            for ((minutes, label) in listOf(15 to "15 分钟", 30 to "30 分钟", 60 to "1 小时", 180 to "3 小时")) {
                FilledTonalButton({ snooze(minutes) }) { Text(label) }
            }
            FilledTonalButton({ custom = true }) { Text("自定义") }
        }
        return
    }
    var hours by remember { mutableStateOf(false) }
    var minutes by remember { mutableIntStateOf(10) }
    var hourCount by remember { mutableIntStateOf(2) }
    Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        if (hours) Stepper(hourCount, 1..24, 1) { hourCount = it } else Stepper(minutes, 5..55, 5) { minutes = it }
        Spacer(Modifier.weight(1f))
        Segmented(listOf("分钟", "小时"), if (hours) 1 else 0) { hours = it == 1 }
    }
    Spacer(Modifier.height(12.dp))
    Row(Modifier.fillMaxWidth().padding(end = 8.dp), Arrangement.End) {
        Button({ snooze(if (hours) hourCount * 60 else minutes) }) { Text("确定") }
    }
}

/** A number with − and + beside it, each greyed at its end of `range`. */
@Composable
private fun Stepper(value: Int, range: IntRange, step: Int, change: (Int) -> Unit) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        IconButton({ change(value - step) }, enabled = value - step >= range.first) { Icon(painterResource(R.drawable.ic_minus), "减少") }
        Text("$value", Modifier.width(40.dp), fontSize = 17.sp, textAlign = TextAlign.Center, color = MaterialTheme.colorScheme.onSurface)
        IconButton({ change(value + step) }, enabled = value + step <= range.last) { Icon(painterResource(R.drawable.ic_add), "增加") }
    }
}

/** A segmented control of a few `items`, `picked` one of them. */
@Composable
private fun Segmented(items: List<String>, picked: Int, pick: (Int) -> Unit) {
    Row(Modifier.clip(RoundedCornerShape(10.dp)).background(MaterialTheme.colorScheme.surfaceContainer).padding(2.dp)) {
        items.forEachIndexed { i, item ->
            Text(
                item,
                Modifier.clip(RoundedCornerShape(8.dp))
                    .background(if (i == picked) LocalPalette.current.segOn else MaterialTheme.colorScheme.surfaceContainer)
                    .clickable { pick(i) }
                    .padding(horizontal = 14.dp, vertical = 6.dp),
                fontSize = 14.sp,
                fontWeight = FontWeight.Medium,
                color = MaterialTheme.colorScheme.onSurface,
            )
        }
    }
}
