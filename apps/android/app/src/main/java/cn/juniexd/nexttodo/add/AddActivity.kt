package cn.juniexd.nexttodo.add

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.view.Gravity
import android.view.WindowManager
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import cn.juniexd.nexttodo.Changes
import cn.juniexd.nexttodo.MainActivity
import cn.juniexd.nexttodo.R
import cn.juniexd.nexttodo.Server
import cn.juniexd.nexttodo.forUser
import cn.juniexd.nexttodo.ui.LocalPalette
import cn.juniexd.nexttodo.ui.NextTodoTheme
import cn.juniexd.nexttodo.widget.Widget
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * Adding from the widget's `+` without opening the app (docs/android.md#添加弹窗): a card at the bottom,
 * over the keyboard, as the app's quick add bar is, AI organizing with ✦ included. Tasks go to the
 * root. Closed by a tap outside or back, the text stays for next time.
 */
class AddActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Signed out: the app asks to sign in.
        if (Server.token(this) == null) {
            open(this, "nexttodo://home")
            return finish()
        }
        setContent { NextTodoTheme { AddBar(::finish) } }
        // After the content: a floating window takes its size from the theme until then.
        window.setLayout(WindowManager.LayoutParams.MATCH_PARENT, WindowManager.LayoutParams.WRAP_CONTENT)
        window.setGravity(Gravity.BOTTOM)
    }

    /** Gone with the screen it was on: it is a popup, not somewhere to come back to. */
    override fun onStop() {
        super.onStop()
        finish()
    }

    companion object {
        private fun prefs(context: Context) = context.getSharedPreferences("add", Context.MODE_PRIVATE)

        /** What was typed and not added yet, apart from the app's drafts. */
        private const val DRAFT = "draft"

        fun draft(context: Context): String = prefs(context).getString(DRAFT, "")!!

        fun keepDraft(context: Context, text: String) {
            prefs(context).edit().putString(DRAFT, text).apply()
        }

        /** Signed out: the draft goes. */
        fun forget(context: Context) {
            prefs(context).edit().clear().commit()
        }

        /** Whether AI organizing was the mode last used, here or in the app: in Capacitor Preferences,
         * which the app uses for it too. And whether a model is set up, as the app last said. */
        private const val AI_MODE = "aiMode"
        private const val AI_READY = "aiReady"

        fun open(context: Context, url: String) {
            context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url), context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        }

        fun aiMode(context: Context) = Server.prefs(context).getString(AI_MODE, null) == "on"

        fun setAiMode(context: Context, on: Boolean) {
            Server.prefs(context).edit().putString(AI_MODE, if (on) "on" else "off").apply()
        }

        fun aiReady(context: Context) = Server.prefs(context).getString(AI_READY, null) == "true"
    }
}

/** Changes go on after the popup has closed. */
private val background = CoroutineScope(Dispatchers.IO)

/** Control characters but line breaks and tabs, which the server refuses: dropped, as the app does
 * (`apps/web/src/domain/text.ts`). */
private val control = Regex("[\\p{Cc}&&[^\\n\\r\\t]]")

/** The title typed, cleaned as the app cleans it, or what is wrong with it. */
private fun title(text: String): Result<String> {
    val value = text.replace(control, "").replace('\t', ' ').trim()
    return when {
        value.isEmpty() -> Result.failure(Exception("标题不能为空"))
        value.codePointCount(0, value.length) > 200 -> Result.failure(Exception("标题不能超过 200 个字；长段文字可以点 ✦ 交给 AI 整理"))
        else -> Result.success(value)
    }
}

/** The text for AI organizing, cleaned, or what is wrong with it. */
private fun jobText(text: String): Result<String> {
    val value = text.replace(control, "")
    return when {
        value.isBlank() -> Result.failure(Exception("内容不能为空"))
        value.codePointCount(0, value.length) > 10_000 -> Result.failure(Exception("内容不能超过 10000 个字"))
        else -> Result.success(value)
    }
}

@Composable
private fun AddBar(close: () -> Unit) {
    val context = LocalContext.current
    var text by remember { mutableStateOf(AddActivity.draft(context).let { TextFieldValue(it, TextRange(it.length)) }) }
    var ai by remember { mutableStateOf(AddActivity.aiMode(context)) }
    var sending by remember { mutableStateOf(false) }
    val focus = remember { FocusRequester() }
    val edit = { value: TextFieldValue ->
        text = value
        AddActivity.keepDraft(context, value.text)
    }
    val send = send@{
        if (sending || text.text.isBlank()) return@send
        // The mode it is sent in, which cannot change while it goes.
        val organizing = ai
        val checked = if (organizing) jobText(text.text) else title(text.text)
        val value = checked.getOrElse {
            Toast.makeText(context, it.message, Toast.LENGTH_SHORT).show()
            return@send
        }
        sending = true
        val sent = text.text
        // On after the popup has gone, should it go meanwhile.
        val app = context.applicationContext
        background.launch {
            val outcome = if (organizing) Changes.organize(app, value) else Changes.add(app, value)
            withContext(Dispatchers.Main) {
                if (outcome is Changes.Outcome.Refused) {
                    Toast.makeText(app, outcome.message, Toast.LENGTH_LONG).show()
                    sending = false
                    return@withContext
                }
                // Unless typed into since, in the popup opened again meanwhile.
                if (AddActivity.draft(app) == sent) AddActivity.keepDraft(app, "")
                Toast.makeText(app, if (organizing) "已交给 AI 整理" else "已添加", Toast.LENGTH_SHORT).show()
                close()
            }
            if (outcome !is Changes.Outcome.Sent) return@launch
            try {
                Widget.refresh(app)
            } catch (e: Exception) {
                Widget.toast(app, forUser("刷新", e))
            }
        }
    }
    val switchMode = {
        if (!ai && !AddActivity.aiReady(context)) {
            Toast.makeText(context, "先选好 AI 整理用的模型", Toast.LENGTH_SHORT).show()
            AddActivity.open(context, "nexttodo://ai")
            close()
        } else {
            ai = !ai
            AddActivity.setAiMode(context, ai)
        }
    }
    val colors = MaterialTheme.colorScheme
    Surface(
        Modifier.fillMaxWidth().padding(12.dp),
        shape = RoundedCornerShape(25.dp),
        color = LocalPalette.current.group,
        shadowElevation = 6.dp,
    ) {
        Row(Modifier.padding(5.dp), Arrangement.spacedBy(6.dp), Alignment.Bottom) {
            BasicTextField(
                text,
                { value ->
                    when {
                        ai || '\n' !in value.text && '\r' !in value.text -> edit(value)
                        // Enter where the keyboard has no done key: add.
                        value.text == text.text.substring(0, text.selection.start) + "\n" + text.text.substring(text.selection.end) -> send()
                        // A title has one line: what is pasted joins up, as in the app.
                        else -> edit(value.copy(text = value.text.replace(Regex("[\\r\\n]"), "")))
                    }
                },
                Modifier.weight(1f).focusRequester(focus),
                enabled = !sending,
                textStyle = TextStyle(fontSize = 16.sp, lineHeight = 22.sp, color = colors.onSurface),
                keyboardOptions = KeyboardOptions(imeAction = if (ai) ImeAction.Default else ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { send() }),
                maxLines = 5,
                cursorBrush = SolidColor(colors.primary),
                decorationBox = { field ->
                    Box(Modifier.background(colors.surfaceContainer, RoundedCornerShape(20.dp)).padding(horizontal = 14.dp, vertical = 9.dp)) {
                        if (text.text.isEmpty()) {
                            Text(if (ai) "交给 AI 整理" else "添加事项", fontSize = 16.sp, color = colors.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                        field()
                    }
                },
            )
            Row(Modifier.padding(bottom = 2.dp), Arrangement.spacedBy(4.dp)) {
                val container = if (ai) colors.primary else colors.surfaceContainer
                val content = if (ai) colors.onPrimary else colors.onSurfaceVariant
                IconButton(
                    switchMode,
                    Modifier.size(36.dp),
                    enabled = !sending,
                    colors = IconButtonDefaults.iconButtonColors(
                        containerColor = container,
                        contentColor = content,
                        disabledContainerColor = container.copy(alpha = 0.4f),
                        disabledContentColor = content.copy(alpha = 0.4f),
                    ),
                ) { Icon(painterResource(R.drawable.ic_sparkles), if (ai) "改为直接添加" else "改为 AI 整理", Modifier.size(16.dp)) }
                IconButton(
                    send,
                    Modifier.size(36.dp),
                    enabled = !sending && text.text.isNotBlank(),
                    colors = IconButtonDefaults.iconButtonColors(
                        containerColor = colors.primary,
                        contentColor = colors.onPrimary,
                        disabledContainerColor = colors.primary.copy(alpha = 0.4f),
                        disabledContentColor = colors.onPrimary.copy(alpha = 0.4f),
                    ),
                ) { Icon(painterResource(R.drawable.ic_send), if (ai) "交给 AI 整理" else "添加", Modifier.size(18.dp)) }
            }
        }
    }
    LaunchedEffect(Unit) { focus.requestFocus() }
}
