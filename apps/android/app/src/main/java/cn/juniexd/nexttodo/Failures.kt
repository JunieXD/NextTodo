package cn.juniexd.nexttodo

import android.util.Log
import cn.juniexd.nexttodo.widget.HttpError
import java.net.ConnectException
import java.net.NoRouteToHostException
import java.net.SocketTimeoutException
import java.net.UnknownHostException
import javax.net.ssl.SSLException

/** The kind of failure `e` is, for the page: `offline`, a network out of reach; `unavailable`, an
 * update not to be had for now; else none. */
fun kind(e: Exception): String? = when (e) {
    is UnknownHostException, is ConnectException, is NoRouteToHostException, is SocketTimeoutException, is SSLException -> "offline"
    is Unavailable -> "unavailable"
    else -> null
}

/** What to tell the user of `e`, which made `what` fail, as docs/ui.md#文案 has it: that `what` failed,
 * and why in words for them. The server's reason follows; a message of the native library's or this
 * app's, written for the user in Chinese as all such are, says it all. The system's words go to the
 * log, where they help: to the user they mean little. */
fun forUser(what: String, e: Exception): String {
    Log.w(NextTodoApp.TAG, what, e)
    val message = e.message.orEmpty()
    return when {
        kind(e) == "offline" -> "${what}失败：无法连接，请检查网络后重试"
        kind(e) == "unavailable" -> "${what}失败：暂时无法获取，请稍后重试"
        e is HttpError -> "${what}失败：$message"
        message.any { it in '一'..'鿿' } -> message
        else -> "${what}失败，请稍后重试"
    }
}
