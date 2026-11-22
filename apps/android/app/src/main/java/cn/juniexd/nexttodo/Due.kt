package cn.juniexd.nexttodo

import java.time.DayOfWeek
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
import java.time.temporal.ChronoUnit
import org.json.JSONObject

/** A due date with an optional time, read the way the web app reads it (`apps/web/src/domain/dates.ts`). */
data class Due(val date: LocalDate, val time: LocalTime?) {
    /** A date alone is due at the end of that day. */
    val deadline: LocalDateTime get() = time?.let(date::atTime) ?: date.plusDays(1).atStartOfDay()

    fun overdue(now: LocalDateTime) = !deadline.isAfter(now)

    fun today(now: LocalDateTime) = date == now.toLocalDate()

    /** `今天 18:00`, `明天`, `周五`, `11月20日 18:00`, `2027年3月15日`. */
    fun label(now: LocalDateTime): String {
        val at = time?.let { " %02d:%02d".format(it.hour, it.minute) } ?: ""
        val today = now.toLocalDate()
        val days = ChronoUnit.DAYS.between(today, date)
        val leftThisWeek = 8 - today.dayOfWeek.value
        return when {
            days == 0L -> "今天$at"
            days == -1L -> "昨天$at"
            days == 1L -> "明天$at"
            days in 2 until leftThisWeek -> "周${WEEK[date.dayOfWeek]}$at"
            date.year == today.year -> "${date.monthValue}月${date.dayOfMonth}日$at"
            else -> "${date.year}年${date.monthValue}月${date.dayOfMonth}日$at"
        }
    }

    companion object {
        /** From the web app's `Due`, as JSON. */
        fun from(d: JSONObject) = Due(LocalDate.parse(d.getString("date")), d.optString("time").takeIf { it.isNotEmpty() }?.let(LocalTime::parse))

        private val WEEK = mapOf(
            DayOfWeek.MONDAY to "一", DayOfWeek.TUESDAY to "二", DayOfWeek.WEDNESDAY to "三", DayOfWeek.THURSDAY to "四",
            DayOfWeek.FRIDAY to "五", DayOfWeek.SATURDAY to "六", DayOfWeek.SUNDAY to "日",
        )
    }
}

/** A time of a repeating task, as `Occurrence` in `apps/web/src/api/types`: the one due on `due` after
 * it was done `completions` times (docs/api.md#改动). */
data class Occurrence(val due: String, val completions: Int) {
    fun json(): JSONObject = JSONObject().put("due", due).put("completions", completions)

    companion object {
        fun from(o: JSONObject?) = o?.let { Occurrence(it.getString("due"), it.getInt("completions")) }

        /** The time `task`, as JSON in the shape of `Task`, is due now, should it repeat. */
        fun of(task: JSONObject) = task.optJSONObject("due")?.takeIf { it.optJSONObject("repeat") != null }
            ?.let { Occurrence(it.getString("date"), task.optJSONObject("completions")?.getInt("count") ?: 0) }
    }
}

/** Whether a time marked `this`, every time of the task without one, is time `o`. */
fun Occurrence?.covers(o: Occurrence?) = this == null || this == o
